use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::sortedint::{
    compose_si_key, compose_si_meta_key, compose_si_prefix_stack, r#const::BE_LEN, extract_id,
    meta::SortedintMeta, opt::encode_be_u64,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::current_now_ms,
  wedb::{Db, DbBatch},
};

/// Sorted integer data structure operations interface (SortedInt).
/// 有序整型数据结构操作接口 (SortedInt)
#[inline]
fn prepare_si_meta_for_write<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  prefix: &[u8],
  meta_k: &[u8],
  now_ms: u64,
  batch: &mut DbBatch<E>,
) -> Result<(SortedintMeta, bool)>
where
  Error: From<E::Error>,
{
  match get_meta_checked::<SortedintMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) => Ok((meta, false)),
    None => {
      clear_prefix_in_batch(db.data(), prefix, batch)?;
      Ok((SortedintMeta::default(), true))
    }
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn si_add_one<K: AsRef<[u8]>>(&self, key: K, id: u64) -> Result<usize> {
    self.si_add(key, &[id])
  }

  #[inline]
  pub fn si_add<K: AsRef<[u8]>>(&self, key: K, ids: &[u64]) -> Result<usize> {
    if ids.is_empty() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let now_ms = current_now_ms();
    let data_ks = self.data();

    let mut batch = self.batch();
    let (mut meta, is_fresh) =
      prepare_si_meta_for_write(self, k_bytes, &prefix, &meta_k, now_ms, &mut batch)?;

    let mut added = 0usize;
    let mut item_buf = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = item_buf.len();
    item_buf.extend_from_slice(&[0u8; BE_LEN]);

    let mut seen = HashSet::with_capacity(ids.len());
    for &id in ids {
      if !seen.insert(id) {
        continue;
      }
      let be_bytes = encode_be_u64(id);
      item_buf[prefix_len..].copy_from_slice(&be_bytes);
      if is_fresh || !data_ks.contains_key(&item_buf)? {
        added += 1;
        meta.base.size = meta.base.size.saturating_add(1);
        batch.insert_data(&item_buf, b"");
      }
    }

    if added > 0 || is_fresh {
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
    }

    Ok(added)
  }

  #[inline]
  pub fn si_rem_one<K: AsRef<[u8]>>(&self, key: K, id: u64) -> Result<usize> {
    self.si_rem(key, &[id])
  }

  #[inline]
  pub fn si_rem<K: AsRef<[u8]>>(&self, key: K, ids: &[u64]) -> Result<usize> {
    if ids.is_empty() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();
    let data_ks = self.data();

    let mut meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let mut item_buf = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = item_buf.len();
    item_buf.extend_from_slice(&[0u8; BE_LEN]);

    let mut deleted = 0usize;
    let mut batch = self.batch();

    let mut seen = HashSet::with_capacity(ids.len());
    for &id in ids {
      if !seen.insert(id) {
        continue;
      }
      let be_bytes = encode_be_u64(id);
      item_buf[prefix_len..].copy_from_slice(&be_bytes);
      if data_ks.contains_key(&item_buf)? {
        deleted += 1;
        batch.rm_weak_data(&item_buf);
        meta.base.size = meta.base.size.saturating_sub(1);
      }
    }

    if deleted > 0 {
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(deleted)
  }

  #[inline]
  pub fn si_card<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();
    Ok(
      get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?
        .map_or(0, |m| m.base.size),
    )
  }

  #[inline]
  pub fn si_exists<K: AsRef<[u8]>>(&self, key: K, id: u64) -> Result<bool> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(false);
    }

    let subkey = compose_si_key(&kc, k_bytes, id);
    Ok(self.data().contains_key(&subkey)?)
  }

  #[inline]
  pub fn si_mexist<K: AsRef<[u8]>>(&self, key: K, ids: &[u64]) -> Result<Vec<bool>> {
    let mut results = Vec::with_capacity(ids.len());
    if ids.is_empty() {
      return Ok(results);
    }

    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      results.resize(ids.len(), false);
      return Ok(results);
    }

    let mut item_buf = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = item_buf.len();
    item_buf.extend_from_slice(&[0u8; BE_LEN]);

    let data_ks = self.data();
    for &id in ids {
      let be_bytes = encode_be_u64(id);
      item_buf[prefix_len..].copy_from_slice(&be_bytes);
      results.push(data_ks.contains_key(&item_buf)?);
    }
    Ok(results)
  }

  #[inline]
  pub fn si_members<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<u64>> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(Vec::new()),
    };

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = prefix.len();
    let mut results = Vec::with_capacity((meta.base.size as usize).min(4096));

    for g in self.data().prefix(&prefix) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some(id) = extract_id(k, prefix_len) {
        results.push(id);
      }
    }
    Ok(results)
  }
}
