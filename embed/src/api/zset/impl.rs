use std::{ops::Bound, str};

use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::zset::{
    ZScanResult, ZSetKeyMemberScore, ZSetMemberScore,
    meta::ZSetMeta,
    opt::{
      Aggregate, IntoRangeLex, IntoRangeRank, IntoRangeScore, RangeLex, RangeScore, ZAdd, ZRange,
    },
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{check_key_not_other_type, clear_prefix_in_batch, get_meta_checked},
  key_composer::{KeyComposer, KeyTag, SmallKey, matches_glob_bytes},
  meta::{
    current_now_ms, decode_sortable_f64, encode_sortable_f64, generate_version,
    normalize_range as meta_normalize_range,
  },
  wedb::{Db, DbBatch},
};

const SCORE_LEN: usize = 8;

/// Zero-copy parses (score, member) pair from big-endian 8-byte score index key.
/// 零拷贝解析 Score 索引子键中的 (score, member) 切片（大端序紧凑保序 8 字节）
#[inline(always)]
fn parse_score_sub(sub: &[u8]) -> Option<(f64, &[u8])> {
  if sub.len() >= SCORE_LEN {
    let score_bytes: [u8; 8] = sub[..SCORE_LEN].try_into().ok()?;
    let member = &sub[SCORE_LEN..];
    Some((decode_sortable_f64(score_bytes), member))
  } else {
    None
  }
}

/// Stack-allocated ZSet metadata key without heap allocation.
/// 构造 ZSet 元数据键字节序列（栈上定长，零堆分配）
#[inline]
fn compose_zset_meta_key(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::ZSetMeta.as_slice(), key)
}

/// Stack-allocated ZSet member prefix without heap allocation.
/// 构造 ZSet 成员前缀字节序列（栈上定长，零堆分配）
#[inline]
fn compose_zset_prefix(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ZSetData.as_slice(), key)
}

/// Stack-allocated ZSet score index prefix without heap allocation.
/// 构造 ZSet 分数索引前缀字节序列（栈上定长，零堆分配）
#[inline]
fn compose_zset_score_prefix(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ZSetScore.as_slice(), key)
}

/// Normalizes Redis index range supporting negative indices with lower bound clamped to 0.
/// 标准 Redis 索引范围规整化（支持负索引，下界 0 对齐，超范围终止）
#[inline]
fn normalize_range(card: usize, start: i64, stop: i64) -> Option<(usize, usize)> {
  let (s, e) = meta_normalize_range(start, stop, card as i64);
  if s > e {
    None
  } else {
    Some((s as usize, e as usize))
  }
}

/// Calculates exclusive prefix upper bound safely without overflow or panic.
/// 安全计算前缀排他上界（单次反向迭代，彻底杜绝 0xFF 溢出与 panic 隐患，支持任意二进制键）
#[inline]
fn prefix_upper_bound(prefix: &[u8]) -> Bound<Vec<u8>> {
  let mut bound = prefix.to_vec();
  while let Some(last) = bound.pop() {
    if last < 0xFF {
      bound.push(last + 1);
      return Bound::Excluded(bound);
    }
  }
  Bound::Unbounded
}

/// Constructs start and end bounds from RangeScore using 8-byte big-endian encoding.
/// 根据 RangeScore 构造基于保序大端序 8 字节 Score 前缀的起止边界
#[inline]
fn score_range_bounds(score_prefix: &[u8], spec: &RangeScore) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
  let start = if spec.min == f64::NEG_INFINITY {
    Bound::Included(score_prefix.to_vec())
  } else {
    let min_val = if spec.min == 0.0 {
      if spec.minex { 0.0 } else { -0.0 }
    } else {
      spec.min
    };
    let min_enc = encode_sortable_f64(min_val);
    let mut k = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 1);
    k.extend_from_slice(score_prefix);
    k.extend_from_slice(&min_enc);
    if spec.minex {
      k.push(0xFF);
    }
    Bound::Included(k)
  };

  let end = if spec.max == f64::INFINITY {
    prefix_upper_bound(score_prefix)
  } else {
    let max_val = if spec.max == 0.0 {
      if spec.maxex { -0.0 } else { 0.0 }
    } else {
      spec.max
    };
    let max_enc = encode_sortable_f64(max_val);
    let mut k = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 1);
    k.extend_from_slice(score_prefix);
    k.extend_from_slice(&max_enc);
    if spec.maxex {
      Bound::Excluded(k)
    } else {
      k.push(0xFF);
      Bound::Included(k)
    }
  };

  (start, end)
}

/// Constructs start and end bounds from RangeLex using member prefixes.
/// 根据 RangeLex 构造基于 Member 前缀的起止边界
#[inline]
fn lex_range_bounds(member_prefix: &[u8], spec: &RangeLex) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
  let start = if spec.min_infinite {
    Bound::Included(member_prefix.to_vec())
  } else {
    let mut k = Vec::with_capacity(member_prefix.len() + spec.min.len() + 1);
    k.extend_from_slice(member_prefix);
    k.extend_from_slice(&spec.min);
    if spec.minex {
      k.push(0x00);
    }
    Bound::Included(k)
  };

  let end = if spec.max_infinite {
    prefix_upper_bound(member_prefix)
  } else {
    let mut k = Vec::with_capacity(member_prefix.len() + spec.max.len());
    k.extend_from_slice(member_prefix);
    k.extend_from_slice(&spec.max);
    if spec.maxex {
      Bound::Excluded(k)
    } else {
      Bound::Included(k)
    }
  };

  (start, end)
}

#[inline]
fn get_zset_meta<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
) -> Result<Option<ZSetMeta>>
where
  Error: From<E::Error>,
{
  match get_meta_checked::<ZSetMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) if !meta.is_empty() => Ok(Some(meta)),
    _ => Ok(None),
  }
}

/// Prepares ZSet metadata for write operations with automated purging of expired subkeys.
/// 准备写入时的有序集合元数据（自动清理已过期残留子键，保证数据隔离无脏数据）
#[inline]
pub fn prepare_zset_meta_for_write<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
  batch: &mut DbBatch<E>,
) -> Result<(ZSetMeta, bool)>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  match get_meta_checked::<ZSetMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) => Ok((meta, true)),
    None => {
      let prefix = compose_zset_prefix(&kc, k_bytes);
      clear_prefix_in_batch(db.data(), &prefix, batch)?;
      let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
      clear_prefix_in_batch(db.data(), &score_prefix, batch)?;
      Ok((ZSetMeta::new(0, generate_version(), 0), false))
    }
  }
}

/// Sorted Set structure operations interface (Sorted Sets).
/// 有序集合结构操作接口 (Sorted Sets)
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
      let (k, _) = (entry.key(), entry.value());
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
      let (k, _) = (entry.key(), entry.value());
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
      let mut sb = [0u8; 8];
      if v.len() >= 8 {
        sb.copy_from_slice(&v[..8]);
      }
      let score = decode_sortable_f64(sb);
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
      let mut sb = [0u8; 8];
      if v.len() >= 8 {
        sb.copy_from_slice(&v[..8]);
      }
      let score = decode_sortable_f64(sb);
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
      let (k, _) = (entry.key(), entry.value());
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
      let (k, _) = (entry.key(), entry.value());
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
        if spec.maxex && member >= spec.max.as_slice() {
          break;
        }
        if !spec.maxex && member > spec.max.as_slice() {
          break;
        }
      }
      if spec.check(member) {
        let mut sb = [0u8; 8];
        if v.len() >= 8 {
          sb.copy_from_slice(&v[..8]);
        }
        let score = decode_sortable_f64(sb);
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
        if spec.minex && member <= spec.min.as_slice() {
          break;
        }
        if !spec.minex && member < spec.min.as_slice() {
          break;
        }
      }
      if spec.check(member) {
        let mut sb = [0u8; 8];
        if v.len() >= 8 {
          sb.copy_from_slice(&v[..8]);
        }
        let score = decode_sortable_f64(sb);
        if !f(member, score) {
          break;
        }
      }
    }
    Ok(())
  }

  /// Retrieves all members and scores from sorted set ordered by score ascending.
  /// 获取有序集合全部成员与分数（按分数由小到大排序，同分数按 member 字典序升序）
  #[inline]
  pub fn zget_all<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<ZSetMemberScore>> {
    let card = self.zcard(&key)? as usize;
    let mut items = Vec::with_capacity(card);
    self.ziter(key, |member, score| {
      items.push((member.to_vec(), score));
      true
    })?;
    Ok(items)
  }

  #[inline]
  pub fn zadd_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    score: f64,
    member: M,
    opt_li: impl IntoIterator<Item = ZAdd>,
  ) -> Result<usize> {
    self.zadd(key, &[(score, member)], opt_li)
  }

  #[inline]
  pub fn zadd<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    score_members: &[(f64, M)],
    opt_li: impl IntoIterator<Item = ZAdd>,
  ) -> Result<usize> {
    let kc = self.kc();
    if score_members.is_empty() {
      return Ok(0);
    }
    for (score, _) in score_members {
      if score.is_nan() {
        return Err(Error::invalid_data("ERR score is not a valid float"));
      }
    }

    let mut nx = false;
    let mut xx = false;
    let mut gt = false;
    let mut lt = false;
    let mut ch = false;
    let mut incr = false;

    for opt in opt_li {
      match opt {
        ZAdd::Nx => nx = true,
        ZAdd::Xx => xx = true,
        ZAdd::Gt => gt = true,
        ZAdd::Lt => lt = true,
        ZAdd::Ch => ch = true,
        ZAdd::Incr => incr = true,
      }
    }

    if nx && xx {
      return Err(Error::invalid_data(
        "ERR XX and NX options at the same time are not compatible",
      ));
    }
    if (gt && lt) || (nx && gt) || (nx && lt) {
      return Err(Error::invalid_data(
        "ERR GT, LT, and/or NX options at the same time are not compatible",
      ));
    }
    if incr && score_members.len() > 1 {
      return Err(Error::invalid_data(
        "ERR INCR option supports a single increment-element pair",
      ));
    }
    let k_bytes = key.as_ref();
    let now_ms = current_now_ms();

    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let data_ks = self.data();
    let _meta_ks = self.meta();
    let mut batch = self.batch_with_capacity(score_members.len() * 2 + 1);
    let (mut meta, metadata_existed) =
      prepare_zset_meta_for_write(self, k_bytes, &meta_k, now_ms, &mut batch)?;

    let mut added = 0;
    let mut changed = 0;

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
    let mut m_key = Vec::with_capacity(prefix.len() + 32);
    let mut s_key = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 32);

    let is_single = score_members.len() == 1;
    let mut seen = if is_single {
      HashSet::default()
    } else {
      HashSet::with_capacity(score_members.len())
    };

    for (input_score, member) in score_members.iter().rev() {
      let m_bytes = member.as_ref();
      if !is_single && !seen.insert(m_bytes) {
        continue;
      }

      m_key.clear();
      m_key.extend_from_slice(&prefix);
      m_key.extend_from_slice(m_bytes);

      let old_score_bytes = if metadata_existed {
        data_ks.get(&m_key)?
      } else {
        None
      };

      if let Some(old_sb) = old_score_bytes {
        if nx {
          continue;
        }
        let mut sb = [0u8; 8];
        if old_sb.len() >= 8 {
          sb.copy_from_slice(&old_sb[..8]);
        }
        let old_score = decode_sortable_f64(sb);

        let final_score = if incr {
          if (lt && *input_score >= 0.0) || (gt && *input_score <= 0.0) {
            continue;
          }
          old_score + *input_score
        } else {
          *input_score
        };

        if final_score.is_nan() {
          return Err(Error::invalid_data(
            "ERR resulting score is not a number (NaN)",
          ));
        }

        if (gt && final_score <= old_score) || (lt && final_score >= old_score) {
          continue;
        }

        if final_score != old_score {
          changed += 1;
          s_key.clear();
          s_key.extend_from_slice(&score_prefix);
          s_key.extend_from_slice(&sb);
          s_key.extend_from_slice(m_bytes);
          batch.rm_data(&s_key);

          let new_enc = encode_sortable_f64(final_score);
          s_key.clear();
          s_key.extend_from_slice(&score_prefix);
          s_key.extend_from_slice(&new_enc);
          s_key.extend_from_slice(m_bytes);
          batch.insert_data(&s_key, b"");
          batch.insert_data(&m_key, &new_enc);
        }
      } else {
        if xx {
          continue;
        }
        let final_score = *input_score;
        added += 1;
        meta.base.size = meta.base.size.saturating_add(1);

        let new_enc = encode_sortable_f64(final_score);
        s_key.clear();
        s_key.extend_from_slice(&score_prefix);
        s_key.extend_from_slice(&new_enc);
        s_key.extend_from_slice(m_bytes);

        batch.insert_data(&s_key, b"");
        batch.insert_data(&m_key, &new_enc);
      }
    }

    if added > 0 || changed > 0 {
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
    }

    Ok(if ch { added + changed } else { added })
  }

  #[inline]
  pub fn zrem_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<usize> {
    self.zrem(key, &[member])
  }

  #[inline]
  pub fn zrem<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, members: &[M]) -> Result<usize> {
    if members.is_empty() {
      return Ok(0);
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let _meta_ks = self.meta();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let mut deleted = 0;
    let mut batch = self.batch_with_capacity(members.len() * 2 + 1);

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
    let mut m_key = Vec::with_capacity(prefix.len() + 32);
    let mut s_key = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 32);

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      m_key.clear();
      m_key.extend_from_slice(&prefix);
      m_key.extend_from_slice(m_bytes);

      if let Some(sb) = data_ks.get(&m_key)? {
        deleted = 1;
        meta.base.size = meta.base.size.saturating_sub(1);
        let mut b = [0u8; 8];
        if sb.len() >= 8 {
          b.copy_from_slice(&sb[..8]);
        }

        s_key.clear();
        s_key.extend_from_slice(&score_prefix);
        s_key.extend_from_slice(&b);
        s_key.extend_from_slice(m_bytes);

        batch.rm_data(&s_key);
        batch.rm_data(&m_key);
      }
    } else {
      let mut seen = HashSet::with_capacity(members.len());
      for member in members {
        let m_bytes = member.as_ref();
        if !seen.insert(m_bytes) {
          continue;
        }
        m_key.clear();
        m_key.extend_from_slice(&prefix);
        m_key.extend_from_slice(m_bytes);

        if let Some(sb) = data_ks.get(&m_key)? {
          deleted += 1;
          meta.base.size = meta.base.size.saturating_sub(1);
          let mut b = [0u8; 8];
          if sb.len() >= 8 {
            b.copy_from_slice(&sb[..8]);
          }

          s_key.clear();
          s_key.extend_from_slice(&score_prefix);
          s_key.extend_from_slice(&b);
          s_key.extend_from_slice(m_bytes);

          batch.rm_data(&s_key);
          batch.rm_data(&m_key);
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
  pub fn zscore<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<Option<f64>> {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(None);
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let mut m_key = Vec::with_capacity(prefix.len() + member.as_ref().len());
    m_key.extend_from_slice(&prefix);
    m_key.extend_from_slice(member.as_ref());

    match self.data().get(&m_key)? {
      Some(sb) if sb.len() >= 8 => {
        let mut b = [0u8; 8];
        b.copy_from_slice(&sb[..8]);
        Ok(Some(decode_sortable_f64(b)))
      }
      _ => Ok(None),
    }
  }

  /// ZMSCORE key member [member ...] (multi-score lookup with single metadata check).
  /// ZMSCORE key member [member ...] (单次元数据检查与缓冲池点查，极致性能)
  #[inline]
  pub fn zmscore<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<Vec<Option<f64>>> {
    let mut scores = Vec::with_capacity(members.len());
    if members.is_empty() {
      return Ok(scores);
    }

    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      scores.resize(members.len(), None);
      return Ok(scores);
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let mut m_key = Vec::with_capacity(prefix.len() + 32);
    let data_ks = self.data();

    for m in members {
      m_key.clear();
      m_key.extend_from_slice(&prefix);
      m_key.extend_from_slice(m.as_ref());
      let score = match data_ks.get(&m_key)? {
        Some(sb) if sb.len() >= 8 => {
          let mut b = [0u8; 8];
          b.copy_from_slice(&sb[..8]);
          Some(decode_sortable_f64(b))
        }
        _ => None,
      };
      scores.push(score);
    }
    Ok(scores)
  }

  /// ZMGET key member [member ...] (multi-score retrieval aligned with Kvrocks ZSet::MGet).
  /// ZMGET key member [member ...] (对标 Apache Kvrocks ZSet::MGet)
  #[inline]
  pub fn zmget<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<HashMap<Vec<u8>, f64>> {
    let mut mscores = HashMap::with_capacity(members.len());
    if members.is_empty() {
      return Ok(mscores);
    }

    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_zset_meta(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(mscores);
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let mut m_key = Vec::with_capacity(prefix.len() + 32);
    let data_ks = self.data();

    for m in members {
      let m_bytes = m.as_ref();
      m_key.clear();
      m_key.extend_from_slice(&prefix);
      m_key.extend_from_slice(m_bytes);
      if let Some(sb) = data_ks.get(&m_key)?
        && sb.len() >= 8
      {
        let mut b = [0u8; 8];
        b.copy_from_slice(&sb[..8]);
        mscores.insert(m_bytes.to_vec(), decode_sortable_f64(b));
      }
    }
    Ok(mscores)
  }

  #[inline]
  pub fn zcard<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(meta) => Ok(meta.base.size),
      None => Ok(0),
    }
  }

  #[inline]
  pub fn zcount<K: AsRef<[u8]>>(&self, key: K, spec: impl IntoRangeScore) -> Result<u64> {
    let spec = spec.into_range_score();
    if spec.is_empty() {
      return Ok(0);
    }
    let mut count = 0u64;
    self.ziter_range_byscore(key, &spec, |_, _| {
      count += 1;
      true
    })?;
    Ok(count)
  }

  /// ZLEXCOUNT key min max (lexicographical range count with precise prefix seek).
  /// ZLEXCOUNT key min max（基于字典序精准范围遍历与计数，零全量慢扫）
  #[inline]
  pub fn zlexcount<K: AsRef<[u8]>>(&self, key: K, spec: impl IntoRangeLex) -> Result<u64> {
    let spec = spec.into_range_lex();
    if spec.is_empty() {
      return Ok(0);
    }
    let mut count = 0u64;
    self.ziter_range_bylex(key, &spec, |_, _| {
      count += 1;
      true
    })?;
    Ok(count)
  }

  #[inline]
  pub fn zincrby<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    increment: f64,
    member: M,
  ) -> Result<f64> {
    let kc = self.kc();
    if increment.is_nan() {
      return Err(Error::invalid_data("ERR increment is not a valid float"));
    }
    let k_bytes = key.as_ref();
    let now_ms = current_now_ms();

    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let mut batch = self.batch();
    let (mut meta, metadata_existed) =
      prepare_zset_meta_for_write(self, k_bytes, &meta_k, now_ms, &mut batch)?;

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
    let m_bytes = member.as_ref();
    let mut m_key = Vec::with_capacity(prefix.len() + m_bytes.len());
    m_key.extend_from_slice(&prefix);
    m_key.extend_from_slice(m_bytes);

    let data_ks = self.data();
    let _meta_ks = self.meta();

    let old_score_bytes = if metadata_existed {
      data_ks.get(&m_key)?
    } else {
      None
    };

    let mut s_key = Vec::with_capacity(score_prefix.len() + SCORE_LEN + m_bytes.len());

    let final_score = if let Some(old_sb) = old_score_bytes {
      let mut sb = [0u8; 8];
      if old_sb.len() >= 8 {
        sb.copy_from_slice(&old_sb[..8]);
      }
      let old_score = decode_sortable_f64(sb);
      let score = old_score + increment;
      if score.is_nan() {
        return Err(Error::invalid_data(
          "ERR resulting score is not a number (NaN)",
        ));
      }
      if score != old_score {
        s_key.clear();
        s_key.extend_from_slice(&score_prefix);
        s_key.extend_from_slice(&sb);
        s_key.extend_from_slice(m_bytes);
        batch.rm_data(&s_key);

        let new_enc = encode_sortable_f64(score);
        s_key.clear();
        s_key.extend_from_slice(&score_prefix);
        s_key.extend_from_slice(&new_enc);
        s_key.extend_from_slice(m_bytes);
        batch.insert_data(&s_key, b"");
        batch.insert_data(&m_key, &new_enc);
        batch.insert_meta(&meta_k, &meta.encode());
        batch.commit()?;
      }
      score
    } else {
      let score = increment;
      meta.base.size = meta.base.size.saturating_add(1);
      let new_enc = encode_sortable_f64(score);
      s_key.clear();
      s_key.extend_from_slice(&score_prefix);
      s_key.extend_from_slice(&new_enc);
      s_key.extend_from_slice(m_bytes);
      batch.insert_data(&s_key, b"");
      batch.insert_data(&m_key, &new_enc);
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
      score
    };

    Ok(final_score)
  }

  #[inline]
  pub fn zrank<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<Option<u64>> {
    let _kc = self.kc();
    let target_score = match self.zscore(&key, &member)? {
      Some(score) => score,
      None => return Ok(None),
    };

    let m_ref = member.as_ref();
    let mut rank = 0u64;
    let mut found = false;

    self.ziter(key, |m, score| {
      if score == target_score && m == m_ref {
        found = true;
        false
      } else if score > target_score {
        false
      } else {
        rank += 1;
        true
      }
    })?;

    Ok(if found { Some(rank) } else { None })
  }

  /// ZRANK key member [WITHSCORE] (single-pass rank and score retrieval).
  /// ZRANK key member [WITHSCORE] (对标 Redis 7.2 / Kvrocks ZSet::Rank，单遍迭代获取排名与分数)
  #[inline]
  pub fn zrank_with_score<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    member: M,
  ) -> Result<Option<(u64, f64)>> {
    let target_score = match self.zscore(&key, &member)? {
      Some(score) => score,
      None => return Ok(None),
    };

    let m_ref = member.as_ref();
    let mut rank = 0u64;
    let mut found = false;

    self.ziter(key, |m, score| {
      if score == target_score && m == m_ref {
        found = true;
        false
      } else if score > target_score {
        false
      } else {
        rank += 1;
        true
      }
    })?;

    Ok(if found {
      Some((rank, target_score))
    } else {
      None
    })
  }

  #[inline]
  pub fn zrevrank<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<Option<u64>> {
    let _kc = self.kc();
    let target_score = match self.zscore(&key, &member)? {
      Some(score) => score,
      None => return Ok(None),
    };

    let m_ref = member.as_ref();
    let mut rank = 0u64;
    let mut found = false;

    self.ziter_rev(key, |m, score| {
      if score == target_score && m == m_ref {
        found = true;
        false
      } else if score < target_score {
        false
      } else {
        rank += 1;
        true
      }
    })?;

    Ok(if found { Some(rank) } else { None })
  }

  /// ZREVRANK key member [WITHSCORE] (reverse rank and score retrieval).
  /// ZREVRANK key member [WITHSCORE]
  #[inline]
  pub fn zrevrank_with_score<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    member: M,
  ) -> Result<Option<(u64, f64)>> {
    let target_score = match self.zscore(&key, &member)? {
      Some(score) => score,
      None => return Ok(None),
    };

    let m_ref = member.as_ref();
    let mut rank = 0u64;
    let mut found = false;

    self.ziter_rev(key, |m, score| {
      if score == target_score && m == m_ref {
        found = true;
        false
      } else if score < target_score {
        false
      } else {
        rank += 1;
        true
      }
    })?;

    Ok(if found {
      Some((rank, target_score))
    } else {
      None
    })
  }

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

    if spec.reversed {
      self.ziter_rev(key, |member, score| {
        if current_idx >= s {
          items.push((member.to_vec(), score));
          if items.len() >= count {
            return false;
          }
        }
        current_idx += 1;
        current_idx <= e
      })?;
    } else {
      self.ziter(key, |member, score| {
        if current_idx >= s {
          items.push((member.to_vec(), score));
          if items.len() >= count {
            return false;
          }
        }
        current_idx += 1;
        current_idx <= e
      })?;
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
    let mut items = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut skipped = 0usize;

    self.ziter_range_byscore(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(limit) = spec.count
        && items.len() >= limit
      {
        return false;
      }
      true
    })?;
    Ok(items)
  }

  /// ZREVRANGEBYSCORE key max min [LIMIT offset count] (reverse score range query).
  /// ZREVRANGEBYSCORE key max min [LIMIT offset count]（基于保序十六进制分数前缀逆序精准范围遍历，零全量慢扫）
  #[inline]
  pub fn zrevrangebyscore<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: impl IntoRangeScore,
  ) -> Result<Vec<ZSetMemberScore>> {
    let spec = spec.into_range_score();
    if spec.count == Some(0) || spec.is_empty() {
      return Ok(Vec::new());
    }
    let mut items = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut skipped = 0usize;

    self.ziter_range_byscore_rev(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(limit) = spec.count
        && items.len() >= limit
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZRANGEBYLEX key min max [LIMIT offset count] (lexicographical range query).
  /// ZRANGEBYLEX key min max [LIMIT offset count]（基于字典序精准范围遍历，零全量慢扫）
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
    let mut items = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut skipped = 0usize;

    self.ziter_range_bylex(key, &spec, |member, _| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push(member.to_vec());
      if let Some(limit) = spec.count
        && items.len() >= limit
      {
        return false;
      }
      true
    })?;
    Ok(items)
  }

  /// ZRANGEBYLEX key min max [LIMIT offset count] with scores (lexicographical range query).
  /// ZRANGEBYLEX key min max [LIMIT offset count]（返回 member 与 score，基于字典序精准范围遍历）
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
    let mut items = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut skipped = 0usize;

    self.ziter_range_bylex(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(limit) = spec.count
        && items.len() >= limit
      {
        return false;
      }
      true
    })?;
    Ok(items)
  }

  /// ZREVRANGEBYLEX key max min [LIMIT offset count] (reverse lexicographical range query).
  /// ZREVRANGEBYLEX key max min [LIMIT offset count]（基于字典序逆序精准范围遍历，零全量慢扫）
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
    let mut items = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut skipped = 0usize;

    self.ziter_range_bylex_rev(key, &spec, |member, _| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push(member.to_vec());
      if let Some(limit) = spec.count
        && items.len() >= limit
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// ZREVRANGEBYLEX key max min [LIMIT offset count] with scores (reverse lexicographical query).
  /// ZREVRANGEBYLEX key max min [LIMIT offset count]（逆序返回 member 与 score，基于字典序逆序精准范围遍历）
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
    let mut items = Vec::with_capacity(spec.count.unwrap_or(16).min(1024));
    let mut skipped = 0usize;

    self.ziter_range_bylex_rev(key, &spec, |member, score| {
      if skipped < spec.offset {
        skipped += 1;
        return true;
      }
      items.push((member.to_vec(), score));
      if let Some(limit) = spec.count
        && items.len() >= limit
      {
        return false;
      }
      true
    })?;

    Ok(items)
  }

  /// Unified ZRANGE query supporting BYSCORE / BYLEX / REV / LIMIT / WITHSCORES.
  /// 统一 ZRANGE 范围查询（支持 BYSCORE / BYLEX / REV / LIMIT / WITHSCORES，对标 Redis 6.2+ / Kvrocks）
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

  /// ZPOPMIN key [count] (pops lowest score members atomically).
  /// ZPOPMIN key [count]（单批次读取并删除极小值，原子高效）
  #[inline]
  pub fn zpopmin_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<ZSetMemberScore>> {
    let mut res = self.zpopmin(key, 1)?;
    Ok(res.pop())
  }

  #[inline]
  pub fn zpopmin<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<ZSetMemberScore>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(Vec::new()),
    };

    let prefix = compose_zset_score_prefix(&kc, k_bytes);
    let member_prefix = compose_zset_prefix(&kc, k_bytes);
    let actual_count = count.min(meta.size() as usize);
    let mut popped = Vec::with_capacity(actual_count);
    let mut batch = self.batch_with_capacity(actual_count * 2 + 1);
    let mut m_key = Vec::with_capacity(member_prefix.len() + 32);
    let data_ks = self.data();
    let _meta_ks = self.meta();

    for g in data_ks.prefix(&prefix) {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        m_key.clear();
        m_key.extend_from_slice(&member_prefix);
        m_key.extend_from_slice(member);
        batch.rm_weak_data(k);
        batch.rm_weak_data(&m_key);
        popped.push((member.to_vec(), score));
        if popped.len() >= count {
          break;
        }
      }
    }

    if !popped.is_empty() {
      meta.base.size = meta.base.size.saturating_sub(popped.len() as u64);
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }
    Ok(popped)
  }

  /// ZPOPMAX key [count] (pops highest score members atomically).
  /// ZPOPMAX key [count]（基于逆序单遍流式精准截取，零冗余内存分配，原子高效）
  #[inline]
  pub fn zpopmax_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<ZSetMemberScore>> {
    let mut res = self.zpopmax(key, 1)?;
    Ok(res.pop())
  }

  #[inline]
  pub fn zpopmax<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<ZSetMemberScore>> {
    if count == 0 {
      return Ok(Vec::new());
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
    let member_prefix = compose_zset_prefix(&kc, k_bytes);
    let mut popped = Vec::with_capacity(num_pop);
    let mut batch = self.batch_with_capacity(num_pop * 2 + 1);
    let mut m_key = Vec::with_capacity(member_prefix.len() + 32);
    let data_ks = self.data();
    let _meta_ks = self.meta();

    for g in data_ks.prefix(&prefix).rev() {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        m_key.clear();
        m_key.extend_from_slice(&member_prefix);
        m_key.extend_from_slice(member);
        batch.rm_weak_data(k);
        batch.rm_weak_data(&m_key);
        popped.push((member.to_vec(), score));
        if popped.len() >= num_pop {
          break;
        }
      }
    }

    if !popped.is_empty() {
      meta.base.size = meta.base.size.saturating_sub(popped.len() as u64);
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }
    Ok(popped)
  }

  /// BZPOPMIN key [key ...] (checks keys and pops lowest score member from first non-empty set).
  /// BZPOPMIN key [key ...] (检查多键并弹出第一个非空的最小值)
  #[inline]
  pub fn bzpopmin<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Option<ZSetKeyMemberScore>> {
    for k in keys {
      let popped = self.zpopmin(k, 1)?;
      if let Some((member, score)) = popped.into_iter().next() {
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
      let popped = self.zpopmax(k, 1)?;
      if let Some((member, score)) = popped.into_iter().next() {
        return Ok(Some((k.as_ref().to_vec(), member, score)));
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
    let member_prefix = compose_zset_prefix(&kc, k_bytes);
    let mut deleted = 0usize;
    let mut current_idx = 0usize;
    let mut batch = self.batch_with_capacity(count * 2 + 1);
    let mut m_key = Vec::with_capacity(member_prefix.len() + 32);
    let data_ks = self.data();
    let _meta_ks = self.meta();

    for g in data_ks.prefix(&prefix) {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((_, member)) = parse_score_sub(&k[prefix.len()..]) {
        if current_idx >= s {
          m_key.clear();
          m_key.extend_from_slice(&member_prefix);
          m_key.extend_from_slice(member);
          batch.rm_weak_data(k);
          batch.rm_weak_data(&m_key);
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
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
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
    let member_prefix = compose_zset_prefix(&kc, k_bytes);
    let (start, end) = score_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    let mut deleted = 0usize;
    let mut batch = self.batch();
    let mut m_key = Vec::with_capacity(member_prefix.len() + 32);
    let data_ks = self.data();
    let _meta_ks = self.meta();

    for g in data_ks.range((start_ref, end_ref)) {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some((score, member)) = parse_score_sub(&k[prefix.len()..]) {
        if (spec.maxex && score >= spec.max) || score > spec.max {
          break;
        }
        if spec.check(score) {
          m_key.clear();
          m_key.extend_from_slice(&member_prefix);
          m_key.extend_from_slice(member);
          batch.rm_data(k);
          batch.rm_data(&m_key);
          deleted += 1;
        }
      }
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
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
    let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
    let (start, end) = lex_range_bounds(&prefix, spec);
    let start_ref = Bound::as_ref(&start).map(|v| v.as_slice());
    let end_ref = Bound::as_ref(&end).map(|v| v.as_slice());

    let mut deleted = 0usize;
    let mut batch = self.batch();
    let mut s_key = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 1 + 32);
    let data_ks = self.data();
    let _meta_ks = self.meta();

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
        let mut sb = [0u8; 8];
        if v.len() >= 8 {
          sb.copy_from_slice(&v[..8]);
        }

        s_key.clear();
        s_key.extend_from_slice(&score_prefix);
        s_key.extend_from_slice(&sb);
        s_key.extend_from_slice(member);

        batch.rm_data(&s_key);
        batch.rm_data(k);
        deleted += 1;
      }
    }

    if deleted > 0 {
      meta.base.size = meta.base.size.saturating_sub(deleted as u64);
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(deleted)
  }

  /// Overwrites sorted set data (aligned with Apache Kvrocks ZSet::Overwrite).
  /// 覆盖写入有序集合数据（对标 Apache Kvrocks ZSet::Overwrite）
  #[inline]
  pub fn overwrite_zset<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    score_members: &[(M, f64)],
  ) -> Result<usize> {
    let kc = self.kc();
    let k_bytes = key.as_ref();
    let now_ms = current_now_ms();

    check_key_not_other_type(self, k_bytes, KeyTag::ZSetMeta.as_slice(), now_ms)?;

    let z_prefix = compose_zset_prefix(&kc, k_bytes);
    let zs_prefix = compose_zset_score_prefix(&kc, k_bytes);
    let _data_ks = self.data();
    let _meta_ks = self.meta();

    let mut batch = self.batch();
    clear_prefix_in_batch(self.data(), &z_prefix, &mut batch)?;
    clear_prefix_in_batch(self.data(), &zs_prefix, &mut batch)?;
    let meta_k = compose_zset_meta_key(&kc, k_bytes);
    batch.rm_meta(&meta_k);

    let mut seen = HashSet::with_capacity(score_members.len());
    let mut count = 0u64;

    let mut m_key = Vec::with_capacity(z_prefix.len() + 32);
    let mut s_key = Vec::with_capacity(zs_prefix.len() + SCORE_LEN + 32);

    for (member, score) in score_members {
      let m_bytes = member.as_ref();
      if !seen.insert(m_bytes) {
        continue;
      }
      let enc = encode_sortable_f64(*score);

      s_key.clear();
      s_key.extend_from_slice(&zs_prefix);
      s_key.extend_from_slice(&enc);
      s_key.extend_from_slice(m_bytes);

      m_key.clear();
      m_key.extend_from_slice(&z_prefix);
      m_key.extend_from_slice(m_bytes);

      batch.insert_data(&s_key, b"");
      batch.insert_data(&m_key, &enc);
      count += 1;
    }

    if count > 0 {
      let meta = ZSetMeta::new_with_version(0, count);
      batch.insert_meta(&meta_k, &meta.encode());
    }

    batch.commit()?;
    Ok(count as usize)
  }

  /// ZDIFF numkeys key [key ...] [WITHSCORES] (computes set difference).
  /// ZDIFF numkeys key [key ...] [WITHSCORES]
  #[inline]
  pub fn zdiff<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Vec<ZSetMemberScore>> {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let first_card = self.zcard(&keys[0])?;
    if first_card == 0 {
      return Ok(Vec::new());
    }
    if keys.len() == 1 {
      return self.zget_all(&keys[0]);
    }

    let mut base_items = self.zget_all(&keys[0])?;
    if base_items.is_empty() {
      return Ok(Vec::new());
    }

    for k in &keys[1..] {
      if base_items.is_empty() {
        return Ok(Vec::new());
      }
      let card = self.zcard(k)?;
      if (base_items.len() as u64).saturating_mul(4) < card {
        base_items.retain(|(member, _)| self.zscore(k, member).unwrap_or(None).is_none());
      } else {
        let mut exclude: HashSet<Vec<u8>> = HashSet::with_capacity(card as usize);
        self.ziter(k, |m, _| {
          exclude.insert(m.to_vec());
          true
        })?;
        base_items.retain(|(member, _)| !exclude.contains(member));
      }
    }

    Ok(base_items)
  }

  /// ZDIFFSTORE destination numkeys key [key ...] (stores set difference).
  /// ZDIFFSTORE destination numkeys key [key ...]
  #[inline]
  pub fn zdiffstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(&self, dst: D, keys: &[K]) -> Result<usize> {
    let diff = self.zdiff(keys)?;
    self.overwrite_zset(dst, &diff)
  }

  /// ZUNION numkeys key [key ...] (computes union with weights and aggregation).
  /// ZUNION numkeys key [key ...] [WEIGHTS weight [weight ...]] [AGGREGATE <SUM | MIN | MAX>]
  #[inline]
  pub fn zunion<K: AsRef<[u8]>>(
    &self,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<Vec<ZSetMemberScore>> {
    if keys_weights.is_empty() {
      return Ok(Vec::new());
    }
    if keys_weights.len() == 1 {
      let (k, weight) = &keys_weights[0];
      let mut items = self.zget_all(k)?;
      if *weight != 1.0 {
        for (_, score) in &mut items {
          *score *= weight;
          if score.is_nan() {
            *score = 0.0;
          }
        }
        items.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
      }
      return Ok(items);
    }

    let mut map: HashMap<Vec<u8>, f64> = HashMap::default();

    for (k, weight) in keys_weights {
      self.ziter(k, |member, score| {
        let mut weighted_score = score * weight;
        if weighted_score.is_nan() {
          weighted_score = 0.0;
        }
        match map.get_mut(member) {
          Some(cur_score) => {
            *cur_score = aggregate.apply(*cur_score, weighted_score);
          }
          None => {
            map.insert(member.to_vec(), weighted_score);
          }
        }
        true
      })?;
    }

    let mut results: Vec<ZSetMemberScore> = map.into_iter().collect();
    results.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(results)
  }

  /// ZUNIONSTORE destination numkeys key [key ...] (stores union result).
  /// ZUNIONSTORE destination numkeys key [key ...] [WEIGHTS weight [weight ...]] [AGGREGATE <SUM | MIN | MAX>]
  #[inline]
  pub fn zunionstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(
    &self,
    dst: D,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<usize> {
    let union_res = self.zunion(keys_weights, aggregate)?;
    self.overwrite_zset(dst, &union_res)
  }

  /// ZINTER numkeys key [key ...] (computes intersection with weights and aggregation).
  /// ZINTER numkeys key [key ...] [WEIGHTS weight [weight ...]] [AGGREGATE <SUM | MIN | MAX>]
  #[inline]
  pub fn zinter<K: AsRef<[u8]>>(
    &self,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<Vec<ZSetMemberScore>> {
    if keys_weights.is_empty() {
      return Ok(Vec::new());
    }

    let mut min_idx = 0;
    let mut min_card = u64::MAX;
    for (i, (k, _)) in keys_weights.iter().enumerate() {
      let card = self.zcard(k)?;
      if card == 0 {
        return Ok(Vec::new());
      }
      if card < min_card {
        min_card = card;
        min_idx = i;
      }
    }

    let (base_k, base_w) = &keys_weights[min_idx];
    let base_items = self.zget_all(base_k)?;
    if base_items.is_empty() {
      return Ok(Vec::new());
    }

    let mut current_map: HashMap<Vec<u8>, f64> = HashMap::with_capacity(base_items.len());

    for (m, s) in base_items {
      let mut score = s * base_w;
      if score.is_nan() {
        score = 0.0;
      }
      current_map.insert(m, score);
    }

    for (i, (k, weight)) in keys_weights.iter().enumerate() {
      if i == min_idx {
        continue;
      }
      current_map.retain(|member, cur_score| match self.zscore(k, member) {
        Ok(Some(score)) => {
          let mut weighted = score * weight;
          if weighted.is_nan() {
            weighted = 0.0;
          }
          *cur_score = aggregate.apply(*cur_score, weighted);
          true
        }
        _ => false,
      });
      if current_map.is_empty() {
        return Ok(Vec::new());
      }
    }

    let mut results: Vec<ZSetMemberScore> = current_map.into_iter().collect();
    results.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(results)
  }

  /// ZINTERSTORE destination numkeys key [key ...] (stores intersection result).
  /// ZINTERSTORE destination numkeys key [key ...] [WEIGHTS weight [weight ...]] [AGGREGATE <SUM | MIN | MAX>]
  #[inline]
  pub fn zinterstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(
    &self,
    dst: D,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<usize> {
    let inter_res = self.zinter(keys_weights, aggregate)?;
    self.overwrite_zset(dst, &inter_res)
  }

  /// ZINTERCARD numkeys key [key ...] [LIMIT limit] (computes intersection cardinality with early termination).
  /// ZINTERCARD numkeys key [key ...] [LIMIT limit]（基于基数升序优先扫描与 O(1) 存在性探针，提前中断）
  #[inline]
  pub fn zintercard<K: AsRef<[u8]>>(&self, keys: &[K], limit: usize) -> Result<usize> {
    if keys.is_empty() {
      return Ok(0);
    }
    let mut key_cards: Vec<(&K, u64)> = Vec::with_capacity(keys.len());
    for k in keys {
      let card = self.zcard(k)?;
      if card == 0 {
        return Ok(0);
      }
      key_cards.push((k, card));
    }

    key_cards.sort_by_key(|(_, card)| *card);

    let (smallest_key, _) = key_cards[0];
    let kc = self.kc();

    let other_prefixes: Vec<_> = key_cards[1..]
      .iter()
      .map(|(k, _)| compose_zset_prefix(&kc, k.as_ref()))
      .collect();

    let data_ks = self.data();
    let mut cardinality = 0;
    let mut probe_buf = Vec::with_capacity(64);

    self.ziter(smallest_key, |member, _| {
      let in_all = other_prefixes.iter().all(|prefix| {
        probe_buf.clear();
        probe_buf.extend_from_slice(prefix);
        probe_buf.extend_from_slice(member);
        data_ks.contains_key(&probe_buf).unwrap_or(false)
      });

      if in_all {
        cardinality += 1;
        if limit > 0 && cardinality >= limit {
          return false;
        }
      }
      true
    })?;

    Ok(cardinality)
  }

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
}
