use std::ops::Bound;

use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  IntoIndexRange,
  api::sortedint::{
    compose_si_item_key, compose_si_key, compose_si_meta_key, compose_si_prefix_stack,
    r#const::BE_LEN,
    extract_id,
    meta::SortedintMeta,
    opt::{IntoSortedintRange, encode_be_u64},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::{current_now_ms, normalize_range},
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
  pub fn si_iter<K: AsRef<[u8]>, F: FnMut(u64) -> bool>(&self, key: K, mut f: F) -> Result<()> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = prefix.len();

    for g in self.data().prefix(&prefix) {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some(id) = extract_id(k, prefix_len)
        && !f(id)
      {
        break;
      }
    }
    Ok(())
  }

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
    let _meta_ks = self.meta();

    let mut batch = self.batch();
    let (mut meta, is_fresh) =
      prepare_si_meta_for_write(self, k_bytes, &prefix, &meta_k, now_ms, &mut batch)?;

    let mut added = 0usize;
    let mut item_buf = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = item_buf.len();
    item_buf.extend_from_slice(&[0u8; BE_LEN]);

    if ids.len() == 1 {
      let id = ids[0];
      let be_bytes = encode_be_u64(id);
      item_buf[prefix_len..].copy_from_slice(&be_bytes);
      if is_fresh || !data_ks.contains_key(&item_buf)? {
        added = 1;
        meta.base.size = meta.base.size.saturating_add(1);
        batch.insert_data(&item_buf, b"");
      }
    } else {
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
    let _meta_ks = self.meta();

    let mut meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let mut item_buf = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = item_buf.len();
    item_buf.extend_from_slice(&[0u8; BE_LEN]);

    let mut deleted = 0usize;
    let mut batch = self.batch();

    if ids.len() == 1 {
      let id = ids[0];
      let be_bytes = encode_be_u64(id);
      item_buf[prefix_len..].copy_from_slice(&be_bytes);
      if data_ks.contains_key(&item_buf)? {
        deleted = 1;
        batch.rm_weak_data(&item_buf);
        meta.base.size = meta.base.size.saturating_sub(1);
      }
    } else {
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

  #[inline]
  pub fn si_rev_range<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: u64,
    offset: usize,
    limit: usize,
  ) -> Result<Vec<u64>> {
    self.si_range(key, cursor, offset, limit, true)
  }

  #[inline]
  pub fn si_range<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: u64,
    offset: usize,
    limit: usize,
    reversed: bool,
  ) -> Result<Vec<u64>> {
    if limit == 0 {
      return Ok(Vec::new());
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(Vec::new());
    }

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = prefix.len();

    let mut results = Vec::with_capacity(limit.min(1024));
    let mut pos = 0usize;

    let mut process_entry = |k: &[u8]| -> bool {
      if let Some(id) = extract_id(k, prefix_len) {
        if cursor > 0 && id == cursor {
          return true;
        }
        if pos < offset {
          pos += 1;
          return true;
        }
        results.push(id);
        if results.len() >= limit {
          return false;
        }
      }
      true
    };

    if !reversed {
      let start_k = compose_si_item_key(&prefix, cursor);
      let end_k = compose_si_item_key(&prefix, u64::MAX);

      for g in self.data().range((
        Bound::Included(start_k.as_slice()),
        Bound::Included(end_k.as_slice()),
      )) {
        let entry = g?;
        if !process_entry(entry.key()) {
          break;
        }
      }
    } else {
      let start_k = compose_si_item_key(&prefix, 0);
      let end_k = compose_si_item_key(&prefix, if cursor == 0 { u64::MAX } else { cursor });

      for g in self
        .data()
        .range((
          Bound::Included(start_k.as_slice()),
          Bound::Included(end_k.as_slice()),
        ))
        .rev()
      {
        let entry = g?;
        if !process_entry(entry.key()) {
          break;
        }
      }
    }
    Ok(results)
  }

  #[inline]
  pub fn si_rev_range_by_value<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoSortedintRange,
  ) -> Result<Vec<u64>> {
    let mut s = spec.into_sortedint_range();
    s.reversed = true;
    self.si_range_by_value(key, s)
  }

  #[inline]
  pub fn si_range_by_value<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoSortedintRange,
  ) -> Result<Vec<u64>> {
    let spec_obj = spec.into_sortedint_range();
    let spec = &spec_obj;
    if spec.is_empty_range() {
      return Ok(Vec::new());
    }
    if let Some(0) = spec.count {
      return Ok(Vec::new());
    }

    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(Vec::new());
    }

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = prefix.len();
    let start_k = compose_si_item_key(&prefix, spec.min);
    let end_k = compose_si_item_key(&prefix, spec.max);

    let start_bound = if spec.minex {
      Bound::Excluded(start_k.as_slice())
    } else {
      Bound::Included(start_k.as_slice())
    };
    let end_bound = if spec.maxex {
      Bound::Excluded(end_k.as_slice())
    } else {
      Bound::Included(end_k.as_slice())
    };

    if !spec.reversed {
      let mut results = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
      let mut pos = 0usize;
      for g in self.data().range((start_bound, end_bound)) {
        let entry = g?;
        let (k, _) = (entry.key(), entry.value());
        if let Some(id) = extract_id(k, prefix_len) {
          if pos < spec.offset {
            pos += 1;
            continue;
          }
          results.push(id);
          if let Some(cnt) = spec.count
            && results.len() >= cnt
          {
            break;
          }
        }
      }
      Ok(results)
    } else {
      let mut results = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
      let mut pos = 0usize;
      for g in self.data().range((start_bound, end_bound)).rev() {
        let entry = g?;
        let (k, _) = (entry.key(), entry.value());
        if let Some(id) = extract_id(k, prefix_len) {
          if pos < spec.offset {
            pos += 1;
            continue;
          }
          results.push(id);
          if let Some(cnt) = spec.count
            && results.len() >= cnt
          {
            break;
          }
        }
      }
      Ok(results)
    }
  }

  #[inline]
  pub fn si_rem_range_by_value<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoSortedintRange,
  ) -> Result<usize> {
    let spec_obj = spec.into_sortedint_range();
    let spec = &spec_obj;
    if spec.is_empty_range() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    if !spec.minex && !spec.maxex && spec.min == u64::MIN && spec.max == u64::MAX {
      let mut batch = self.batch();
      clear_prefix_in_batch(self.data(), prefix.as_slice(), &mut batch)?;
      batch.rm_meta(meta_k.as_slice());
      batch.commit()?;
      return Ok(meta.base.size as usize);
    }

    let start_k = compose_si_item_key(&prefix, spec.min);
    let end_k = compose_si_item_key(&prefix, spec.max);
    let start_bound = if spec.minex {
      Bound::Excluded(start_k.as_slice())
    } else {
      Bound::Included(start_k.as_slice())
    };
    let end_bound = if spec.maxex {
      Bound::Excluded(end_k.as_slice())
    } else {
      Bound::Included(end_k.as_slice())
    };

    let mut deleted = 0usize;
    let mut batch = self.batch();

    for g in self.data().range((start_bound, end_bound)) {
      let entry = g?;
      deleted += 1;
      batch.rm_weak_data(entry.key());
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      if meta.base.size == 0 {
        batch.rm_meta(meta_k.as_slice());
      } else {
        batch.insert_meta(meta_k.as_slice(), &meta.encode());
      }
      batch.commit()?;
    }

    Ok(deleted)
  }

  #[inline]
  pub fn si_rem_range_by_rank<K: AsRef<[u8]>>(
    &self,
    key: K,
    range: impl IntoIndexRange,
  ) -> Result<usize> {
    let (start, stop) = range.into_index_range();
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    if meta.base.size == 0 {
      return Ok(0);
    }

    let (s, e) = match normalize_range(start, stop, meta.base.size as i64) {
      (s, e) if s <= e => (s as usize, e as usize),
      _ => return Ok(0),
    };

    let prefix = compose_si_prefix_stack(&kc, k_bytes);

    if s == 0 && e + 1 >= meta.base.size as usize {
      let mut batch = self.batch();
      clear_prefix_in_batch(self.data(), prefix.as_slice(), &mut batch)?;
      batch.rm_meta(meta_k.as_slice());
      batch.commit()?;
      return Ok(meta.base.size as usize);
    }

    let mut deleted = 0usize;
    let mut batch = self.batch();

    for (rank, g) in self.data().prefix(&prefix).enumerate() {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if rank > e {
        break;
      }
      if rank >= s {
        deleted += 1;
        batch.rm_weak_data(k);
      }
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      if meta.base.size == 0 {
        batch.rm_meta(meta_k.as_slice());
      } else {
        batch.insert_meta(meta_k.as_slice(), &meta.encode());
      }
      batch.commit()?;
    }

    Ok(deleted)
  }

  #[inline]
  pub fn si_rank<K: AsRef<[u8]>>(&self, key: K, id: u64) -> Result<Option<usize>> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(None);
    }

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let end_k = compose_si_item_key(&prefix, id);

    // 1. O(1) 布隆过滤器点查：若元素不存在，直接返回 None，避免 O(N) 盲目扫描
    if !self.data().contains_key(end_k.as_slice())? {
      return Ok(None);
    }

    // 2. 元素已确认存在：排位即严格小于 end_k 的元素个数，区间采用 Excluded(end_k)
    //    该区间内所有 Key 必严格小于 id，因此前序项完全无需反序列化解码
    let start_k = compose_si_item_key(&prefix, 0);
    let mut rank = 0usize;
    for g in self.data().range((
      Bound::Included(start_k.as_slice()),
      Bound::Excluded(end_k.as_slice()),
    )) {
      let _ = g?;
      rank += 1;
    }
    Ok(Some(rank))
  }

  #[inline]
  pub fn si_revrank<K: AsRef<[u8]>>(&self, key: K, id: u64) -> Result<Option<usize>> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(None),
    };

    if let Some(rank) = self.si_rank(key, id)? {
      Ok(Some(
        (meta.base.size as usize)
          .saturating_sub(1)
          .saturating_sub(rank),
      ))
    } else {
      Ok(None)
    }
  }

  #[inline]
  pub fn si_count<K: AsRef<[u8]>>(&self, key: K, spec: impl IntoSortedintRange) -> Result<usize> {
    let spec_obj = spec.into_sortedint_range();
    let spec = &spec_obj;
    if spec.is_empty_range() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    // 全区间 O(1) 极速短路返回
    if !spec.minex && !spec.maxex && spec.min == u64::MIN && spec.max == u64::MAX {
      return Ok(meta.base.size as usize);
    }

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let start_k = compose_si_item_key(&prefix, spec.min);
    let end_k = compose_si_item_key(&prefix, spec.max);

    let start_bound = if spec.minex {
      Bound::Excluded(start_k.as_slice())
    } else {
      Bound::Included(start_k.as_slice())
    };
    let end_bound = if spec.maxex {
      Bound::Excluded(end_k.as_slice())
    } else {
      Bound::Included(end_k.as_slice())
    };

    let mut count = 0usize;
    for g in self.data().range((start_bound, end_bound)) {
      let _ = g?;
      count += 1;
    }
    Ok(count)
  }
}
