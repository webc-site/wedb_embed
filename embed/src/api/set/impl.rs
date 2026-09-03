use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::set::{
    SetItemKeyComposer, compose_set_key, compose_set_meta_key, compose_set_prefix_stack,
    meta::SetMeta,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::current_now_ms,
  wedb::{Db, DbBatch},
};

/// Set structure operations interface (Sets).
/// 集合结构操作接口 (Sets)
#[inline]
pub fn prepare_set_meta_for_write<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  prefix: &[u8],
  meta_k: &[u8],
  now_ms: u64,
  batch: &mut DbBatch<E>,
) -> Result<(SetMeta, bool)>
where
  Error: From<E::Error>,
{
  match get_meta_checked::<SetMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) => Ok((meta, false)),
    None => {
      clear_prefix_in_batch(db.data(), prefix, batch)?;
      Ok((SetMeta::new_with_version(0, 0), true))
    }
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn sadd_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<usize> {
    self.sadd(key, &[member])
  }

  #[inline]
  pub fn sadd<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, members: &[M]) -> Result<usize> {
    if members.is_empty() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let prefix = compose_set_prefix_stack(&kc, k_bytes);
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let mut batch = self.batch_with_capacity(members.len() + 1);

    let (mut meta, is_new) =
      prepare_set_meta_for_write(self, k_bytes, &prefix, &meta_k, now_ms, &mut batch)?;

    let mut added = 0usize;

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      let item_k = compose_set_key(&kc, k_bytes, m_bytes);
      if is_new || !data_ks.contains_key(item_k.as_slice())? {
        batch.insert_data(item_k.as_slice(), b"");
        added = 1;
      }
    } else {
      let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
      let mut seen = HashSet::with_capacity(members.len());
      for m in members {
        let m_bytes = m.as_ref();
        if !seen.insert(m_bytes) {
          continue;
        }

        let item_k = composer.key_for_member(m_bytes);
        if is_new || !data_ks.contains_key(item_k)? {
          batch.insert_data(item_k, b"");
          added += 1;
        }
      }
    }

    if added > 0 {
      meta.base.size += added as u64;
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
    }

    Ok(added)
  }

  #[inline]
  pub fn srem_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<usize> {
    self.srem(key, &[member])
  }

  #[inline]
  pub fn srem<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, members: &[M]) -> Result<usize> {
    if members.is_empty() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(0),
    };

    let mut removed = 0usize;
    let mut batch = self.batch_with_capacity(members.len() + 1);
    let data_ks = self.data();

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      let item_k = compose_set_key(&kc, k_bytes, m_bytes);
      if data_ks.contains_key(item_k.as_slice())? {
        batch.rm_weak_data(item_k.as_slice());
        removed = 1;
      }
    } else {
      let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
      let mut seen = HashSet::with_capacity(members.len());
      for m in members {
        let m_bytes = m.as_ref();
        if !seen.insert(m_bytes) {
          continue;
        }

        let item_k = composer.key_for_member(m_bytes);
        if data_ks.contains_key(item_k)? {
          batch.rm_weak_data(item_k);
          removed += 1;
        }
      }
    }

    if removed > 0 {
      meta.base.size = meta.base.size.saturating_sub(removed as u64);
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(removed)
  }

  #[inline]
  pub fn smembers<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<Vec<u8>>> {
    let card = self.scard(&key)? as usize;
    let mut results = Vec::with_capacity(card.min(4096));
    self.siter(key, |m| {
      results.push(m.to_vec());
      true
    })?;
    Ok(results)
  }

  #[inline]
  pub fn sismember<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<bool> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(false),
    };

    let _ = meta;
    let item_k = compose_set_key(&kc, k_bytes, member.as_ref());
    Ok(self.data().contains_key(item_k.as_slice())?)
  }

  #[inline]
  pub fn smismember<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<Vec<bool>> {
    if members.is_empty() {
      return Ok(Vec::new());
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(vec![false; members.len()]),
    };
    let _ = meta;

    let mut results = Vec::with_capacity(members.len());
    let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
    let data_ks = self.data();

    for m in members {
      let item_k = composer.key_for_member(m.as_ref());
      results.push(data_ks.contains_key(item_k)?);
    }
    Ok(results)
  }

  #[inline]
  pub fn scard<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => Ok(m.base.size),
      None => Ok(0),
    }
  }
}
