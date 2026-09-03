use std::ops::Bound;

use crate::{
  api::zset::{
    ZSetKeyMemberScore, ZSetMemberScore, ZSetPopResult, compose_zset_key,
    compose_zset_score_from_bytes_key,
    r#impl::{
      compose_zset_meta_key, compose_zset_prefix, compose_zset_score_prefix, get_zset_meta,
      lex_range_bounds, normalize_range, parse_score_sub, score_range_bounds,
    },
    meta::{ZSetMeta, decode_sortable_f64_slice},
    opt::{IntoRangeLex, IntoRangeRank, IntoRangeScore},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::clear_prefix_in_batch,
  meta::current_now_ms,
  wedb::{Db, DbBatch},
};

#[inline]
pub(crate) fn commit_zset_batch<E: Engine>(
  meta_k: &[u8],
  meta: &ZSetMeta,
  mut batch: DbBatch<E>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  if meta.base.size == 0 {
    batch.rm_meta(meta_k);
  } else {
    batch.insert_meta(meta_k, &meta.encode());
  }
  batch.commit()?;
  Ok(())
}

/// Pop and range removal operations for Sorted Set.
/// 有序集合弹出与范围删除操作
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub(crate) fn zpop_one_internal<K: AsRef<[u8]>>(
    &self,
    key: K,
    reverse: bool,
  ) -> Result<Option<ZSetMemberScore>> {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.size() > 0 => m,
      _ => return Ok(None),
    };

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let data_ks = self.data();

    let target = if reverse {
      data_ks.prefix(&prefix).next_back()
    } else {
      data_ks.prefix(&prefix).next()
    };

    if let Some(g) = target {
      let entry = g?;
      let k = entry.key();
      if k.starts_with(&prefix)
        && let Some((score, member)) = parse_score_sub(&k[prefix.len()..])
      {
        let m_key = compose_zset_key(&kc, k_bytes, member);
        let mut batch = self.batch_with_capacity(3);
        batch.rm_weak_data(k);
        batch.rm_weak_data(m_key.as_slice());
        meta.base.size = meta.base.size.saturating_sub(1);
        commit_zset_batch(&meta_k, &meta, batch)?;
        return Ok(Some((member.to_vec(), score)));
      }
    }
    Ok(None)
  }

  /// ZPOPMIN key [count] (pops lowest score members atomically).
  /// ZPOPMIN key [count]（单批次读取并删除极小值，原子高效）
  #[inline]
  pub fn zpopmin_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<ZSetMemberScore>> {
    self.zpop_one_internal(key, false)
  }

  #[inline]
  pub fn zpopmin<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<ZSetMemberScore>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    if count == 1 {
      return Ok(match self.zpopmin_one(key)? {
        Some(item) => vec![item],
        None => Vec::new(),
      });
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.size() > 0 => m,
      _ => return Ok(Vec::new()),
    };

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let actual_count = count.min(meta.size() as usize);
    let mut popped = Vec::with_capacity(actual_count);
    let mut batch = self.batch_with_capacity(actual_count * 2 + 1);
    let data_ks = self.data();

    for g in data_ks.prefix(&prefix) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        let m_key = compose_zset_key(&kc, k_bytes, member);
        batch.rm_weak_data(k);
        batch.rm_weak_data(m_key.as_slice());
        popped.push((member.to_vec(), score));
        if popped.len() >= actual_count {
          break;
        }
      }
    }

    if !popped.is_empty() {
      meta.base.size = meta.base.size.saturating_sub(popped.len() as u64);
      commit_zset_batch(&meta_k, &meta, batch)?;
    }
    Ok(popped)
  }

  /// ZPOPMAX key [count] (pops highest score members atomically).
  /// ZPOPMAX key [count]（单批次读取并删除极大值，原子高效）
  #[inline]
  pub fn zpopmax_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<ZSetMemberScore>> {
    self.zpop_one_internal(key, true)
  }

  #[inline]
  pub fn zpopmax<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<ZSetMemberScore>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    if count == 1 {
      return Ok(match self.zpopmax_one(key)? {
        Some(item) => vec![item],
        None => Vec::new(),
      });
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(Vec::new()),
    };

    let num_pop = count.min(meta.size() as usize);
    if num_pop == 0 {
      return Ok(Vec::new());
    }

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let mut popped = Vec::with_capacity(num_pop);
    let mut batch = self.batch_with_capacity(num_pop * 2 + 1);
    let data_ks = self.data();

    for g in data_ks.prefix(&prefix).rev() {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        let m_key = compose_zset_key(&kc, k_bytes, member);
        batch.rm_weak_data(k);
        batch.rm_weak_data(m_key.as_slice());
        popped.push((member.to_vec(), score));
        if popped.len() >= num_pop {
          break;
        }
      }
    }

    if !popped.is_empty() {
      meta.base.size = meta.base.size.saturating_sub(popped.len() as u64);
      commit_zset_batch(&meta_k, &meta, batch)?;
    }
    Ok(popped)
  }

  /// ZMPOP numkeys key [key ...] <MIN | MAX> [COUNT count] (Redis 7.0 multi-key pop).
  /// ZMPOP numkeys key [key ...] <MIN | MAX> [COUNT count] (对标 Redis 7.0 多键批量弹出)
  #[inline]
  pub fn zmpop<K: AsRef<[u8]>>(
    &self,
    keys: &[K],
    min: bool,
    count: usize,
  ) -> Result<Option<ZSetPopResult>> {
    if count == 0 {
      return Ok(None);
    }
    for k in keys {
      let popped = if min {
        self.zpopmin(k, count)?
      } else {
        self.zpopmax(k, count)?
      };
      if !popped.is_empty() {
        return Ok(Some((k.as_ref().to_vec(), popped)));
      }
    }
    Ok(None)
  }

  /// BZPOPMIN key [key ...] (checks keys and pops lowest score member from first non-empty set).
  /// BZPOPMIN key [key ...] (检查多键并弹出第一个非空的最小值)
  #[inline]
  pub fn bzpopmin<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Option<ZSetKeyMemberScore>> {
    for k in keys {
      if let Some((member, score)) = self.zpopmin_one(k)? {
        return Ok(Some((k.as_ref().to_vec(), member, score)));
      }
    }
    Ok(None)
  }

  /// BZPOPMAX key [key ...] (checks keys and pops highest score member from first non-empty set).
  /// BZPOPMAX key [key ...] (检查多键并弹出第一个非空的最大值)
  #[inline]
  pub fn bzpopmax<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Option<ZSetKeyMemberScore>> {
    for k in keys {
      if let Some((member, score)) = self.zpopmax_one(k)? {
        return Ok(Some((k.as_ref().to_vec(), member, score)));
      }
    }
    Ok(None)
  }

  /// ZRANDMEMBER key (single random element extraction with zero full-scan memory).
  /// 随机获取单个元素（零全量扫描内存开销，单次元数据点查与惰性分数解码优化）
  #[inline]
  pub fn zrandmember_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<ZSetMemberScore>> {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.size() > 0 => m,
      _ => return Ok(None),
    };

    let card = meta.size() as usize;
    let target = fastrand::usize(0..card);
    let prefix = compose_zset_prefix(&kc, k_bytes);
    for (current_idx, g) in self.data().prefix(&prefix).enumerate() {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if current_idx == target {
        let member = &k[prefix.len()..];
        let score = decode_sortable_f64_slice(v).unwrap_or(0.0);
        return Ok(Some((member.to_vec(), score)));
      }
    }

    Ok(None)
  }

  /// ZRANDMEMBER key [count] (random member extraction aligned with Kvrocks).
  /// ZRANDMEMBER key [count] (对标 Apache Kvrocks ExtractRandMemberFromSet)
  #[inline]
  pub fn zrandmember<K: AsRef<[u8]>>(&self, key: K, count: i64) -> Result<Vec<ZSetMemberScore>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    if count == 1 || count == -1 {
      return match self.zrandmember_one(&key)? {
        Some(item) => Ok(vec![item]),
        None => Ok(Vec::new()),
      };
    }
    let all = self.zget_all(key)?;
    let total = all.len();
    if total == 0 {
      return Ok(Vec::new());
    }

    if count > 0 {
      let num = (count as usize).min(total);
      if num == total {
        return Ok(all);
      }
      let mut all = all;
      for i in 0..num {
        let j = fastrand::usize(i..total);
        all.swap(i, j);
      }
      all.truncate(num);
      Ok(all)
    } else {
      let num = count.unsigned_abs() as usize;
      let mut results = Vec::with_capacity(num);
      for _ in 0..num {
        let idx = fastrand::usize(0..total);
        results.push(all[idx].clone());
      }
      Ok(results)
    }
  }

  /// ZREMRANGEBYRANK key start stop (single-pass rank-based deletion).
  /// ZREMRANGEBYRANK key start stop (单遍流式扫描与删除，零二次点查，对标 Kvrocks RangeByRank with_deletion)
  #[inline]
  pub fn zremrangebyrank<K: AsRef<[u8]>>(&self, key: K, spec: impl IntoRangeRank) -> Result<usize> {
    let spec = spec.into_range_rank();
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let card = meta.size() as usize;
    let (s, e) = match normalize_range(card, spec.start, spec.stop) {
      Some(range) => range,
      None => return Ok(0),
    };

    let count = e - s + 1;
    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    if s == 0 && e + 1 >= card {
      let mut batch = self.batch();
      clear_prefix_in_batch(self.data(), &prefix, &mut batch)?;
      let member_prefix = compose_zset_prefix(&kc, k_bytes);
      clear_prefix_in_batch(self.data(), &member_prefix, &mut batch)?;
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(card);
    }

    let mut deleted = 0usize;
    let mut current_idx = 0usize;
    let mut batch = self.batch_with_capacity(count * 2 + 1);
    let data_ks = self.data();

    for g in data_ks.prefix(&prefix) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((_, member)) = parse_score_sub(&k[prefix.len()..]) {
        if current_idx >= s {
          let m_key = compose_zset_key(&kc, k_bytes, member);
          batch.rm_weak_data(k);
          batch.rm_weak_data(m_key.as_slice());
          deleted += 1;
          if deleted >= count {
            break;
          }
        }
        current_idx += 1;
        if current_idx > e {
          break;
        }
      }
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      commit_zset_batch(&meta_k, &meta, batch)?;
    }

    Ok(deleted)
  }

  /// ZREMRANGEBYSCORE key min max (single-pass score-based deletion).
  /// ZREMRANGEBYSCORE key min max (基于保序十六进制分数前缀精准范围遍历与删除，零二次点查，对标 Kvrocks RangeByScore with_deletion)
  #[inline]
  pub fn zremrangebyscore<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeScore,
  ) -> Result<usize> {
    let spec_obj = spec.into_range_score();
    let spec = &spec_obj;
    if spec.is_empty() {
      return Ok(0);
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let (start, end) = score_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    let mut deleted = 0usize;
    let mut batch = self.batch_with_capacity(32);
    let data_ks = self.data();

    for g in data_ks.range((start_ref, end_ref)) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        if (spec.maxex && score >= spec.max) || score > spec.max {
          break;
        }
        if spec.check(score) {
          let m_key = compose_zset_key(&kc, k_bytes, member);
          batch.rm_weak_data(k);
          batch.rm_weak_data(m_key.as_slice());
          deleted += 1;
        }
      }
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      commit_zset_batch(&meta_k, &meta, batch)?;
    }

    Ok(deleted)
  }

  /// ZREMRANGEBYLEX key min max (single-pass lexicographical deletion).
  /// ZREMRANGEBYLEX key min max (基于字典序精准范围遍历与删除，零二次点查，对标 Kvrocks RangeByLex with_deletion)
  #[inline]
  pub fn zremrangebylex<K: AsRef<[u8]>>(&self, key: K, spec: impl IntoRangeLex) -> Result<usize> {
    let spec_obj = spec.into_range_lex();
    let spec = &spec_obj;
    if spec.is_empty() {
      return Ok(0);
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let (start, end) = lex_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    let mut deleted = 0usize;
    let mut batch = self.batch_with_capacity(32);
    let data_ks = self.data();

    for g in data_ks.range((start_ref, end_ref)) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      let member = &k[prefix.len()..];
      if !spec.max_infinite {
        if spec.maxex && member >= spec.max.as_slice() {
          break;
        }
        if !spec.maxex && member > spec.max.as_slice() {
          break;
        }
      }
      if spec.check(member) {
        if v.len() >= 8 {
          let s_key = compose_zset_score_from_bytes_key(&kc, k_bytes, &v[..8], member);
          batch.rm_weak_data(s_key.as_slice());
        }
        batch.rm_weak_data(k);
        deleted += 1;
      }
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      commit_zset_batch(&meta_k, &meta, batch)?;
    }

    Ok(deleted)
  }
}
