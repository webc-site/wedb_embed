use std::ops::Bound;

use super::{
  compose_si_item_key, compose_si_meta_key, compose_si_prefix_stack, meta::SortedintMeta,
  opt::IntoSortedintRange,
};
use crate::{
  IntoIndexRange,
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::current_now_ms,
  normalize_range,
  wedb::Db,
};

/// Range removal operations for Sortedint (ZREMRANGEBYRANK, ZREMRANGEBYSCORE equivalents).
/// 有序整数集合按值和排位范围删除实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
      let k = entry.key();
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
}
