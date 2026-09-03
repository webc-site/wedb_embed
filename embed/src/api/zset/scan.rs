use std::ops::Bound;

use crate::{
  api::zset::{
    ZScanResult, ZSetScanByMemberResult, compose_zset_key, compose_zset_prefix,
    meta::decode_sortable_f64_slice,
    r#impl::{compose_zset_meta_key, get_zset_meta},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::prefix_upper_bound,
  key_composer::matches_glob_bytes,
  meta::current_now_ms,
  wedb::Db,
};

/// Streaming pagination and member-based cursor scanning (ZSCAN).
/// 有序集合游标分页与成员区间精准扫描操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  /// ZSCAN key cursor [MATCH pattern] [COUNT count] (streaming cursor pagination).
  /// ZSCAN key cursor [MATCH pattern] [COUNT count] (流式分页扫描，按 member 字典序遍历，低内存占用)
  #[inline]
  pub fn zscan<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: u64,
    pattern: Option<&[u8]>,
    count: Option<usize>,
  ) -> Result<ZScanResult> {
    let total = self.zcard(&key)? as usize;
    let start = cursor as usize;
    if start >= total || total == 0 {
      return Ok((0, Vec::new()));
    }

    let step = count.unwrap_or(10);
    let end = (start + step).min(total);
    let next_cursor = if end >= total { 0 } else { end as u64 };

    let mut results = Vec::with_capacity(step.min(total - start));
    let mut current_idx = 0usize;

    self.ziter_members(key, |item, score| {
      if current_idx >= start && current_idx < end {
        if let Some(pat) = pattern {
          if matches_glob_bytes(pat, item) {
            results.push((item.to_vec(), score));
          }
        } else {
          results.push((item.to_vec(), score));
        }
      }
      current_idx += 1;
      current_idx < end
    })?;

    Ok((next_cursor, results))
  }

  /// ZSCAN key cursor [MATCH pattern] [COUNT count] (range-based pagination by member).
  /// ZSCAN key cursor [MATCH pattern] [COUNT count]（基于成员寻址精准范围遍历，零全量慢查，对标 Kvrocks ZSet::Scan）
  #[inline]
  pub fn zscan_by_member<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: Option<&[u8]>,
    pattern: Option<&[u8]>,
    count: Option<usize>,
  ) -> Result<ZSetScanByMemberResult> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let _meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok((None, Vec::new())),
    };

    let limit = count.unwrap_or(10).max(1);
    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat_bytes = pattern.unwrap_or(b"*");

    let data_ks = self.data();
    let prefix = compose_zset_prefix(&kc, k_bytes);
    let prefix_bytes = prefix.as_slice();
    let prefix_len = prefix_bytes.len();

    let mut results = Vec::with_capacity(limit);
    let mut next_cursor = None;

    let start_bound = cursor
      .map(|c| compose_zset_key(&kc, k_bytes, c))
      .map(|k| Bound::Excluded(k.to_vec()))
      .unwrap_or(Bound::Included(prefix_bytes.to_vec()));
    let start_ref = Bound::as_ref(&start_bound).map(|v| v.as_slice());
    let upper = prefix_upper_bound(prefix_bytes);
    let upper_ref = Bound::as_ref(&upper).map(|v| v.as_slice());

    for g in data_ks.range((start_ref, upper_ref)) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix_bytes) {
        break;
      }
      let member = &k[prefix_len..];
      if is_match_all || matches_glob_bytes(pat_bytes, member) {
        let score = decode_sortable_f64_slice(v).unwrap_or(0.0);
        results.push((member.to_vec(), score));
        if results.len() >= limit {
          next_cursor = Some(member.to_vec());
          break;
        }
      }
    }

    Ok((next_cursor, results))
  }
}
