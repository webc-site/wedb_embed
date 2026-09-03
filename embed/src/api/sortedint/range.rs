use std::ops::Bound;

use super::{
  compose_si_item_key, compose_si_meta_key, compose_si_prefix_stack, extract_id,
  meta::SortedintMeta, opt::IntoSortedintRange,
};
use crate::{
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

/// Range query and rank query operations for Sortedint.
/// 有序整数集合范围与排位检索实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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

    let mut results = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut pos = 0usize;
    let mut scan_entry = |k: &[u8]| -> bool {
      if let Some(id) = extract_id(k, prefix_len) {
        if pos < spec.offset {
          pos += 1;
          return true;
        }
        results.push(id);
        if let Some(cnt) = spec.count
          && results.len() >= cnt
        {
          return false;
        }
      }
      true
    };

    let range = self.data().range((start_bound, end_bound));
    if !spec.reversed {
      for g in range {
        let entry = g?;
        if !scan_entry(entry.key()) {
          break;
        }
      }
    } else {
      for g in range.rev() {
        let entry = g?;
        if !scan_entry(entry.key()) {
          break;
        }
      }
    }
    Ok(results)
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

    // 1. O(1) 点查：若元素不存在，直接返回 None，避免 O(N) 盲目扫描
    if !self.data().contains_key(end_k.as_slice())? {
      return Ok(None);
    }

    // 2. 元素已确认存在：排位即严格小于 end_k 的元素个数，区间采用 Excluded(end_k)
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
