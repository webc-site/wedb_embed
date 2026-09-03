use std::{mem::swap, ops::Bound, str};

use crate::{
  api::zset::{
    ZSetMemberScore,
    r#impl::{
      compose_zset_meta_key, compose_zset_prefix, compose_zset_score_prefix, get_zset_meta,
      lex_range_bounds, normalize_range, parse_score_sub, score_range_bounds,
    },
    meta::decode_sortable_f64_slice,
    opt::{IntoRangeLex, IntoRangeRank, IntoRangeScore, RangeLex, RangeScore, ZRange},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

/// Range queries and streaming iterators for Sorted Set.
/// 有序集合范围查询与流式迭代器
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn ziter<K: AsRef<[u8]>, F>(&self, key: K, mut f: F) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    for g in self.data().prefix(&prefix) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..])
        && !f(member, score)
      {
        break;
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_rev<K: AsRef<[u8]>, F>(&self, key: K, mut f: F) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    for g in self.data().prefix(&prefix).rev() {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..])
        && !f(member, score)
      {
        break;
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_members<K: AsRef<[u8]>, F>(&self, key: K, mut f: F) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    for g in self.data().prefix(&prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      let member = &k[prefix.len()..];
      let score = decode_sortable_f64_slice(v).unwrap_or(0.0);
      if !f(member, score) {
        break;
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_members_rev<K: AsRef<[u8]>, F>(&self, key: K, mut f: F) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    for g in self.data().prefix(&prefix).rev() {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      let member = &k[prefix.len()..];
      let score = decode_sortable_f64_slice(v).unwrap_or(0.0);
      if !f(member, score) {
        break;
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_range_byscore<K: AsRef<[u8]>, F>(
    &self,
    key: K,
    spec: &RangeScore,
    mut f: F,
  ) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    if spec.is_empty() {
      return Ok(());
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let (start, end) = score_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    for g in self.data().range((start_ref, end_ref)) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        if (spec.maxex && score >= spec.max) || score > spec.max {
          break;
        }
        if spec.check(score) && !f(member, score) {
          break;
        }
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_range_byscore_rev<K: AsRef<[u8]>, F>(
    &self,
    key: K,
    spec: &RangeScore,
    mut f: F,
  ) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    if spec.is_empty() {
      return Ok(());
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let (start, end) = score_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    for g in self.data().range((start_ref, end_ref)).rev() {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        if (spec.minex && score <= spec.min) || score < spec.min {
          break;
        }
        if spec.check(score) && !f(member, score) {
          break;
        }
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_range_bylex<K: AsRef<[u8]>, F>(
    &self,
    key: K,
    spec: &RangeLex,
    mut f: F,
  ) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    if spec.is_empty() {
      return Ok(());
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let (start, end) = lex_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    for g in self.data().range((start_ref, end_ref)) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      let member = &k[prefix.len()..];
      if !spec.max_infinite {
        let max_b = spec.max.as_slice();
        if spec.maxex && member >= max_b {
          break;
        }
        if !spec.maxex && member > max_b {
          break;
        }
      }
      if spec.check(member) {
        let score = decode_sortable_f64_slice(v).unwrap_or(0.0);
        if !f(member, score) {
          break;
        }
      }
    }
    Ok(())
  }

  #[inline]
  pub fn ziter_range_bylex_rev<K: AsRef<[u8]>, F>(
    &self,
    key: K,
    spec: &RangeLex,
    mut f: F,
  ) -> Result<()>
  where
    F: FnMut(&[u8], f64) -> bool,
  {
    if spec.is_empty() {
      return Ok(());
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let (start, end) = lex_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    for g in self.data().range((start_ref, end_ref)).rev() {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      let member = &k[prefix.len()..];
      if !spec.min_infinite {
        let min_b = spec.min.as_slice();
        if spec.minex && member <= min_b {
          break;
        }
        if !spec.minex && member < min_b {
          break;
        }
      }
      if spec.check(member) {
        let score = decode_sortable_f64_slice(v).unwrap_or(0.0);
        if !f(member, score) {
          break;
        }
      }
    }
    Ok(())
  }

  /// ZRANGE key min max (by rank index, 0-based, negative indices supported).
  /// ZRANGE key min max (按排名索引检索，0 基底，支持负数索引)
  #[inline]
  pub fn zrangebyrank<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeRank,
  ) -> Result<Vec<ZSetMemberScore>> {
    let spec = spec.into_range_rank();
    let card = self.zcard(&key)? as usize;
    let (s, e) = match normalize_range(card, spec.start, spec.stop) {
      Some(range) => range,
      None => return Ok(Vec::new()),
    };

    let count = e - s + 1;
    let mut items = Vec::with_capacity(count);
    let mut current_idx = 0usize;

    let mut process_item = |member: &[u8], score: f64| -> bool {
      if current_idx >= s {
        items.push((member.to_vec(), score));
        if items.len() >= count {
          return false;
        }
      }
      current_idx += 1;
      current_idx <= e
    };

    if spec.reversed {
      self.ziter_rev(key, &mut process_item)?;
    } else {
      self.ziter(key, &mut process_item)?;
    }

    Ok(items)
  }

  /// ZREVRANGE key start stop (reverse streaming range traversal).
  /// ZREVRANGE key start stop (基于逆序流式截取，达到上限即终止，零冗余内存分配)
  #[inline]
  pub fn zrevrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeRank,
  ) -> Result<Vec<ZSetMemberScore>> {
    let mut spec = spec.into_range_rank();
    spec.reversed = true;
    self.zrangebyrank(key, spec)
  }

  /// ZRANGEBYSCORE key min max [LIMIT offset count] (score range query with precise seek).
  /// ZRANGEBYSCORE key min max [LIMIT offset count]（基于保序十六进制分数前缀精准范围遍历，零全量慢扫）
  #[inline]
  pub fn zrangebyscore<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeScore,
  ) -> Result<Vec<ZSetMemberScore>> {
    let spec = spec.into_range_score();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut skipped = 0usize;

    self.ziter_range_byscore(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(c) = spec.count
        && items.len() >= c
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZREVRANGEBYSCORE key max min [LIMIT offset count] (reverse score range query).
  /// ZREVRANGEBYSCORE key max min [LIMIT offset count]（基于保序十六进制分数前缀反向遍历）
  #[inline]
  pub fn zrevrangebyscore<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeScore,
  ) -> Result<Vec<ZSetMemberScore>> {
    let mut spec = spec.into_range_score();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }
    if spec.min > spec.max {
      swap(&mut spec.min, &mut spec.max);
      swap(&mut spec.minex, &mut spec.maxex);
    }

    let mut items = Vec::new();
    let mut skipped = 0usize;

    self.ziter_range_byscore_rev(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(c) = spec.count
        && items.len() >= c
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZRANGEBYLEX key min max [LIMIT offset count] (lexicographical range query).
  /// ZRANGEBYLEX key min max [LIMIT offset count]（正向字典序范围扫描，零冗余点查）
  #[inline]
  pub fn zrangebylex<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeLex,
  ) -> Result<Vec<Vec<u8>>> {
    let spec = spec.into_range_lex();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut skipped = 0usize;

    self.ziter_range_bylex(key, &spec, |member, _| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push(member.to_vec());
      if let Some(c) = spec.count
        && items.len() >= c
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZRANGEBYLEX key min max WITHSCORES [LIMIT offset count].
  /// ZRANGEBYLEX key min max WITHSCORES（正向字典序范围扫描附带分数，零多余解包）
  #[inline]
  pub fn zrangebylex_with_scores<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeLex,
  ) -> Result<Vec<ZSetMemberScore>> {
    let spec = spec.into_range_lex();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut skipped = 0usize;

    self.ziter_range_bylex(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(c) = spec.count
        && items.len() >= c
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZREVRANGEBYLEX key max min [LIMIT offset count].
  /// ZREVRANGEBYLEX key max min [LIMIT offset count]（反向字典序范围扫描）
  #[inline]
  pub fn zrevrangebylex<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeLex,
  ) -> Result<Vec<Vec<u8>>> {
    let spec = spec.into_range_lex();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut skipped = 0usize;

    self.ziter_range_bylex_rev(key, &spec, |member, _| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push(member.to_vec());
      if let Some(c) = spec.count
        && items.len() >= c
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZREVRANGEBYLEX key max min WITHSCORES [LIMIT offset count].
  /// ZREVRANGEBYLEX key max min WITHSCORES（反向字典序范围扫描附带分数）
  #[inline]
  pub fn zrevrangebylex_with_scores<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeLex,
  ) -> Result<Vec<ZSetMemberScore>> {
    let spec = spec.into_range_lex();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut skipped = 0usize;

    self.ziter_range_bylex_rev(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(c) = spec.count
        && items.len() >= c
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// Universal ZRANGE command supporting BYSCORE, BYLEX, REV, and LIMIT clauses aligned with Redis 6.2+.
  /// 通用 ZRANGE 命令（支持 BYSCORE、BYLEX、REV 与 LIMIT 复合选项，完全对齐 Redis 6.2+ 标准）
  #[inline]
  pub fn zrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    start_or_min: &[u8],
    stop_or_max: &[u8],
    opt_li: impl IntoIterator<Item = ZRange>,
  ) -> Result<Vec<ZSetMemberScore>> {
    let mut by_score = false;
    let mut by_lex = false;
    let mut rev = false;
    let mut offset = 0;
    let mut count = None;

    for opt in opt_li {
      match opt {
        ZRange::ByScore => by_score = true,
        ZRange::ByLex => by_lex = true,
        ZRange::Rev => rev = true,
        ZRange::WithScores => {}
        ZRange::Limit(off, cnt) => {
          offset = off;
          count = Some(cnt);
        }
      }
    }

    if by_score {
      let s1_str = str::from_utf8(start_or_min).unwrap_or("-inf");
      let s2_str = str::from_utf8(stop_or_max).unwrap_or("+inf");
      let (v1, ex1) = RangeScore::parse_bound(s1_str)?;
      let (v2, ex2) = RangeScore::parse_bound(s2_str)?;
      let (min, minex, max, maxex) = if v1 <= v2 {
        (v1, ex1, v2, ex2)
      } else {
        (v2, ex2, v1, ex1)
      };
      let range_spec = RangeScore {
        min,
        max,
        minex,
        maxex,
        offset,
        count,
      };
      if rev {
        self.zrevrangebyscore(key, range_spec)
      } else {
        self.zrangebyscore(key, range_spec)
      }
    } else if by_lex {
      let (v1, ex1, inf1) = RangeLex::parse_bound(start_or_min)?;
      let (v2, ex2, inf2) = RangeLex::parse_bound(stop_or_max)?;
      let (min, minex, min_inf, max, maxex, max_inf) = if (inf1 && start_or_min == b"+")
        || (inf2 && stop_or_max == b"-")
        || (!inf1 && !inf2 && v1 > v2)
      {
        (v2, ex2, inf2, v1, ex1, inf1)
      } else {
        (v1, ex1, inf1, v2, ex2, inf2)
      };
      let range_spec = RangeLex {
        min,
        max,
        minex,
        maxex,
        min_infinite: min_inf,
        max_infinite: max_inf,
        offset,
        count,
        reversed: rev,
      };
      if rev {
        self.zrevrangebylex_with_scores(&key, &range_spec)
      } else {
        self.zrangebylex_with_scores(&key, &range_spec)
      }
    } else {
      let s_start = str::from_utf8(start_or_min)
        .unwrap_or("0")
        .parse::<i64>()
        .unwrap_or(0);
      let s_stop = str::from_utf8(stop_or_max)
        .unwrap_or("-1")
        .parse::<i64>()
        .unwrap_or(-1);
      if rev {
        self.zrevrange(key, (s_start, s_stop))
      } else {
        self.zrangebyrank(key, (s_start, s_stop))
      }
    }
  }
}
