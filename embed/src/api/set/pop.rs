use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::set::{
    SetItemKeyComposer, compose_set_key, compose_set_meta_key, compose_set_prefix_stack,
    r#impl::prepare_set_meta_for_write, meta::SetMeta,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{check_key_not_other_type, clear_prefix_in_batch, get_meta_checked},
  key_composer::KeyTag,
  meta::current_now_ms,
  wedb::Db,
};

/// Set random sampling and element relocation operations (SPOP, SRANDMEMBER, SMOVE, OVERWRITE_SET).
/// 集合随机采样与元素迁移操作实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn spop_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let mut res = self.spop(key, 1)?;
    Ok(res.pop())
  }

  #[inline]
  pub fn spop<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<Vec<u8>>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if count == 1 {
      let card = meta.base.size as usize;
      let target = fastrand::usize(0..card);
      let mut current_idx = 0usize;
      let mut popped_item = None;

      self.siter(k_bytes, |m| {
        if current_idx == target {
          popped_item = Some(m.to_vec());
          return false;
        }
        current_idx += 1;
        true
      })?;

      if let Some(item) = popped_item {
        let mut batch = self.batch_with_capacity(2);
        let item_k = compose_set_key(&kc, k_bytes, &item);
        batch.rm_weak_data(item_k.as_slice());
        meta.base.size = meta.base.size.saturating_sub(1);
        if meta.base.size == 0 {
          batch.rm_meta(&meta_k);
        } else {
          batch.insert_meta(&meta_k, &meta.encode());
        }
        batch.commit()?;
        return Ok(vec![item]);
      } else {
        return Ok(Vec::new());
      }
    }

    let mut members = Vec::with_capacity((meta.base.size as usize).min(4096));
    self.siter(k_bytes, |m| {
      members.push(m.to_vec());
      true
    })?;

    if members.is_empty() {
      return Ok(Vec::new());
    }

    let pop_count = count.min(members.len());
    let mut batch = self.batch_with_capacity(pop_count + 1);

    if pop_count == members.len() {
      let prefix = compose_set_prefix_stack(&kc, k_bytes);
      clear_prefix_in_batch(self.data(), &prefix, &mut batch)?;
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(members);
    }

    let popped: Vec<Vec<u8>> = if pop_count == 1 {
      let idx = fastrand::usize(0..members.len());
      vec![members.swap_remove(idx)]
    } else {
      fastrand::shuffle(&mut members);
      members.into_iter().take(pop_count).collect()
    };

    let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
    for m in &popped {
      let item_k = composer.key_for_member(m);
      batch.rm_weak_data(item_k);
    }

    meta.base.size = meta.base.size.saturating_sub(pop_count as u64);
    if meta.base.size == 0 {
      batch.rm_meta(&meta_k);
    } else {
      batch.insert_meta(&meta_k, &meta.encode());
    }
    batch.commit()?;

    Ok(popped)
  }

  #[inline]
  pub fn srandmember_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let card = self.scard(&key)? as usize;
    if card == 0 {
      return Ok(None);
    }
    let target = fastrand::usize(0..card);
    let mut current_idx = 0usize;
    let mut result = None;

    self.siter(key, |m| {
      if current_idx == target {
        result = Some(m.to_vec());
        return false;
      }
      current_idx += 1;
      true
    })?;

    Ok(result)
  }

  #[inline]
  pub fn srandmember<K: AsRef<[u8]>>(&self, key: K, count: i64) -> Result<Vec<Vec<u8>>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    if count == 1 {
      return match self.srandmember_one(&key)? {
        Some(m) => Ok(vec![m]),
        None => Ok(Vec::new()),
      };
    }
    let mut all = self.smembers(key)?;
    let total = all.len();
    if total == 0 {
      return Ok(Vec::new());
    }

    if count > 0 {
      let sample_cnt = (count as usize).min(total);
      if sample_cnt == total {
        return Ok(all);
      }
      for i in 0..sample_cnt {
        let j = fastrand::usize(i..total);
        all.swap(i, j);
      }
      all.truncate(sample_cnt);
      Ok(all)
    } else {
      let total_sample = count.unsigned_abs() as usize;
      let mut out = Vec::with_capacity(total_sample);
      for _ in 0..total_sample {
        let idx = fastrand::usize(0..total);
        out.push(all[idx].clone());
      }
      Ok(out)
    }
  }

  #[inline]
  pub fn smove<SK: AsRef<[u8]>, DK: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    src: SK,
    dst: DK,
    member: M,
  ) -> Result<bool> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    let m_bytes = member.as_ref();
    let kc = self.kc();

    if src_bytes == dst_bytes {
      return self.sismember(src, member);
    }

    let src_meta_k = compose_set_meta_key(&kc, src_bytes);
    let now_ms = current_now_ms();

    let mut src_meta = match get_meta_checked::<SetMeta, _>(self, src_bytes, &src_meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(false),
    };

    let src_item_k = compose_set_key(&kc, src_bytes, m_bytes);
    let data_ks = self.data();

    if !data_ks.contains_key(src_item_k.as_slice())? {
      return Ok(false);
    }

    let dst_meta_k = compose_set_meta_key(&kc, dst_bytes);
    let dst_prefix = compose_set_prefix_stack(&kc, dst_bytes);
    let mut batch = self.batch();

    let (mut dst_meta, dst_is_new) = prepare_set_meta_for_write(
      self,
      dst_bytes,
      &dst_prefix,
      &dst_meta_k,
      now_ms,
      &mut batch,
    )?;

    batch.rm_weak_data(src_item_k.as_slice());
    src_meta.base.size = src_meta.base.size.saturating_sub(1);
    if src_meta.base.size == 0 {
      batch.rm_meta(&src_meta_k);
    } else {
      batch.insert_meta(&src_meta_k, &src_meta.encode());
    }

    let dst_item_k = compose_set_key(&kc, dst_bytes, m_bytes);
    if dst_is_new || !data_ks.contains_key(dst_item_k.as_slice())? {
      batch.insert_data(dst_item_k.as_slice(), b"");
      dst_meta.base.size += 1;
      batch.insert_meta(&dst_meta_k, &dst_meta.encode());
    }

    batch.commit()?;
    Ok(true)
  }

  #[inline]
  pub fn overwrite_set<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<usize> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, key_bytes);
    let prefix = compose_set_prefix_stack(&kc, key_bytes);
    let now_ms = current_now_ms();

    check_key_not_other_type(self, key_bytes, KeyTag::SetMeta.as_slice(), now_ms)?;

    let data_ks = self.data();

    let mut batch = self.batch();
    clear_prefix_in_batch(data_ks, &prefix, &mut batch)?;
    batch.rm_meta(&meta_k);

    let mut seen = HashSet::with_capacity(members.len());
    let mut count = 0u64;
    let mut composer = SetItemKeyComposer::new(&kc, key_bytes);

    for m in members {
      let m_bytes = m.as_ref();
      if !seen.insert(m_bytes) {
        continue;
      }
      let item_k = composer.key_for_member(m_bytes);
      batch.insert_data(item_k, b"");
      count += 1;
    }

    if count > 0 {
      let mut meta = SetMeta::new_with_version(0, 0);
      meta.base.size = count;
      batch.insert_meta(&meta_k, &meta.encode());
    }

    batch.commit()?;
    Ok(count as usize)
  }
}
