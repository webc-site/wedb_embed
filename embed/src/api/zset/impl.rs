use std::{ops::Bound, str};

use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::zset::{
    ZScanResult, ZSetKeyMemberScore, ZSetMemberScore, ZSetPopResult, ZSetScanByMemberResult,
    key::{compose_zset_key, compose_zset_score_key},
    meta::ZSetMeta,
    opt::{IntoRangeLex, IntoRangeRank, IntoRangeScore, RangeLex, RangeScore, ZAdd, ZRange},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{check_key_not_other_type, clear_prefix_in_batch, get_meta_checked},
  key_composer::{KeyComposer, KeyTag, SmallKey, SubkeyComposer, matches_glob_bytes},
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
    let mut b = [0u8; 8];
    b.copy_from_slice(&sub[..8]);
    Some((decode_sortable_f64(b), &sub[8..]))
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
    let mut k = Vec::with_capacity(score_prefix.len() + SCORE_LEN);
    k.extend_from_slice(score_prefix);
    k.extend_from_slice(&min_enc);
    if spec.minex {
      prefix_upper_bound(&k)
    } else {
      Bound::Included(k)
    }
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
    let mut k = Vec::with_capacity(score_prefix.len() + SCORE_LEN);
    k.extend_from_slice(score_prefix);
    k.extend_from_slice(&max_enc);
    if spec.maxex {
      Bound::Excluded(k)
    } else {
      prefix_upper_bound(&k)
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
    let mut k = Vec::with_capacity(member_prefix.len() + spec.min.len());
    k.extend_from_slice(member_prefix);
    k.extend_from_slice(&spec.min);
    if spec.minex {
      Bound::Excluded(k)
    } else {
      Bound::Included(k)
    }
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
    let mut batch = self.batch_with_capacity(score_members.len() * 2 + 1);
    let (mut meta, metadata_existed) =
      prepare_zset_meta_for_write(self, k_bytes, &meta_k, now_ms, &mut batch)?;

    let mut added = 0;
    let mut changed = 0;

    let is_single = score_members.len() == 1;

    if is_single {
      let (input_score, member) = &score_members[0];
      let m_bytes = member.as_ref();
      let m_key = compose_zset_key(&kc, k_bytes, m_bytes);

      let old_score_bytes = if metadata_existed {
        data_ks.get(m_key.as_slice())?
      } else {
        None
      };

      if let Some(old_sb) = old_score_bytes {
        if !nx {
          let mut sb = [0u8; 8];
          if old_sb.len() >= 8 {
            sb.copy_from_slice(&old_sb[..8]);
          }
          let old_score = decode_sortable_f64(sb);

          let final_score = if incr {
            if (lt && *input_score >= 0.0) || (gt && *input_score <= 0.0) {
              return Ok(0);
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

          if !((gt && final_score <= old_score) || (lt && final_score >= old_score))
            && final_score != old_score
          {
            changed = 1;
            let old_s_key = compose_zset_score_key(&kc, k_bytes, old_score, m_bytes);
            batch.rm_weak_data(old_s_key.as_slice());

            let new_enc = encode_sortable_f64(final_score);
            let new_s_key = compose_zset_score_key(&kc, k_bytes, final_score, m_bytes);
            batch.insert_data(new_s_key.as_slice(), b"");
            batch.insert_data(m_key.as_slice(), &new_enc);
          }
        }
      } else if !xx {
        let final_score = *input_score;
        added = 1;
        meta.base.size = meta.base.size.saturating_add(1);

        let new_enc = encode_sortable_f64(final_score);
        let new_s_key = compose_zset_score_key(&kc, k_bytes, final_score, m_bytes);

        batch.insert_data(new_s_key.as_slice(), b"");
        batch.insert_data(m_key.as_slice(), &new_enc);
      }
    } else {
      let prefix = compose_zset_prefix(&kc, k_bytes);
      let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
      let mut m_key = Vec::with_capacity(prefix.len() + 32);
      let mut s_key = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 32);
      let mut seen = HashSet::with_capacity(score_members.len());

      for (input_score, member) in score_members.iter().rev() {
        let m_bytes = member.as_ref();
        if !seen.insert(m_bytes) {
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

          let final_score = *input_score;

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
            batch.rm_weak_data(&s_key);

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

    let mut meta = match get_zset_meta(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let mut deleted = 0;
    let mut batch = self.batch_with_capacity(members.len() * 2 + 1);

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      let m_key = compose_zset_key(&kc, k_bytes, m_bytes);

      if let Some(sb) = data_ks.get(m_key.as_slice())? {
        deleted = 1;
        meta.base.size = meta.base.size.saturating_sub(1);
        let mut b = [0u8; 8];
        if sb.len() >= 8 {
          b.copy_from_slice(&sb[..8]);
        }
        let score = decode_sortable_f64(b);
        let s_key = compose_zset_score_key(&kc, k_bytes, score, m_bytes);

        batch.rm_weak_data(s_key.as_slice());
        batch.rm_weak_data(m_key.as_slice());
      }
    } else {
      let prefix = compose_zset_prefix(&kc, k_bytes);
      let score_prefix = compose_zset_score_prefix(&kc, k_bytes);
      let mut m_key = Vec::with_capacity(prefix.len() + 32);
      let mut s_key = Vec::with_capacity(score_prefix.len() + SCORE_LEN + 32);
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

          batch.rm_weak_data(&s_key);
          batch.rm_weak_data(&m_key);
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

    let m_key = compose_zset_key(&kc, k_bytes, member.as_ref());

    match self.data().get(m_key.as_slice())? {
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

    let data_ks = self.data();

    if members.len() == 1 {
      let m_key = compose_zset_key(&kc, k_bytes, members[0].as_ref());
      let score = match data_ks.get(m_key.as_slice())? {
        Some(sb) if sb.len() >= 8 => {
          let mut b = [0u8; 8];
          b.copy_from_slice(&sb[..8]);
          Some(decode_sortable_f64(b))
        }
        _ => None,
      };
      scores.push(score);
      return Ok(scores);
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let mut composer = SubkeyComposer::from_slice(&prefix);

    for m in members {
      let m_key = composer.compose_sub(m.as_ref());
      let score = match data_ks.get(m_key)? {
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

    let data_ks = self.data();

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      let m_key = compose_zset_key(&kc, k_bytes, m_bytes);
      if let Some(sb) = data_ks.get(m_key.as_slice())?
        && sb.len() >= 8
      {
        let mut b = [0u8; 8];
        b.copy_from_slice(&sb[..8]);
        mscores.insert(m_bytes.to_vec(), decode_sortable_f64(b));
      }
      return Ok(mscores);
    }

    let prefix = compose_zset_prefix(&kc, k_bytes);
    let mut composer = SubkeyComposer::from_slice(&prefix);

    for m in members {
      let m_bytes = m.as_ref();
      let m_key = composer.compose_sub(m_bytes);
      if let Some(sb) = data_ks.get(m_key)?
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

    let m_bytes = member.as_ref();
    let m_key = compose_zset_key(&kc, k_bytes, m_bytes);
    let data_ks = self.data();

    let old_score_bytes = if metadata_existed {
      data_ks.get(m_key.as_slice())?
    } else {
      None
    };

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
        let old_s_key = compose_zset_score_key(&kc, k_bytes, old_score, m_bytes);
        batch.rm_weak_data(old_s_key.as_slice());

        let new_enc = encode_sortable_f64(score);
        let new_s_key = compose_zset_score_key(&kc, k_bytes, score, m_bytes);
        batch.insert_data(new_s_key.as_slice(), b"");
        batch.insert_data(m_key.as_slice(), &new_enc);
        batch.insert_meta(&meta_k, &meta.encode());
        batch.commit()?;
      }
      score
    } else {
      let score = increment;
      meta.base.size = meta.base.size.saturating_add(1);
      let new_enc = encode_sortable_f64(score);
      let new_s_key = compose_zset_score_key(&kc, k_bytes, score, m_bytes);
      batch.insert_data(new_s_key.as_slice(), b"");
      batch.insert_data(m_key.as_slice(), &new_enc);
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
      score
    };

    Ok(final_score)
  }

  #[inline]
  pub fn zrank<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<Option<u64>> {
    Ok(self.zrank_with_score(key, member)?.map(|(r, _)| r))
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
      } else if score > target_score || (score == target_score && m > m_ref) {
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
    Ok(self.zrevrank_with_score(key, member)?.map(|(r, _)| r))
  }

  /// ZREVRANK key member [WITHSCORE] (reverse rank and score retrieval).
  /// 逆序获取有序集合成员排名与分数
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
      } else if score < target_score || (score == target_score && m < m_ref) {
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
    self
      .zrangebylex_with_scores(key, spec)
      .map(|items| items.into_iter().map(|(m, _)| m).collect())
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
    self
      .zrevrangebylex_with_scores(key, spec)
      .map(|items| items.into_iter().map(|(m, _)| m).collect())
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

  /// ZRANDMEMBER key (single random element extraction with zero full-scan memory).
  /// 随机获取单个元素（零全量扫描内存开销，针对单元素随机访问优化）
  #[inline]
  pub fn zrandmember_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<ZSetMemberScore>> {
    let card = self.zcard(&key)? as usize;
    if card == 0 {
      return Ok(None);
    }
    let target = fastrand::usize(0..card);
    let mut current_idx = 0usize;
    let mut chosen = None;

    self.ziter_members(key, |m, score| {
      if current_idx == target {
        chosen = Some((m.to_vec(), score));
        return false;
      }
      current_idx += 1;
      true
    })?;

    Ok(chosen)
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
    let member_prefix = compose_zset_prefix(&kc, k_bytes);

    if s == 0 && e + 1 >= card {
      let mut batch = self.batch();
      clear_prefix_in_batch(self.data(), &prefix, &mut batch)?;
      clear_prefix_in_batch(self.data(), &member_prefix, &mut batch)?;
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(card);
    }

    let mut deleted = 0usize;
    let mut current_idx = 0usize;
    let mut batch = self.batch_with_capacity(count * 2 + 1);
    let mut m_key = Vec::with_capacity(member_prefix.len() + 32);
    let data_ks = self.data();

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
          batch.rm_weak_data(k);
          batch.rm_weak_data(&m_key);
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
          let mut sb = [0u8; 8];
          sb.copy_from_slice(&v[..8]);

          s_key.clear();
          s_key.extend_from_slice(&score_prefix);
          s_key.extend_from_slice(&sb);
          s_key.extend_from_slice(member);

          batch.rm_weak_data(&s_key);
        }
        batch.rm_weak_data(k);
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

    if let Some(cursor_bytes) = cursor {
      let start_k = compose_zset_key(&kc, k_bytes, cursor_bytes);
      let upper = prefix_upper_bound(prefix_bytes);
      let upper_ref = Bound::as_ref(&upper).map(|v| v.as_slice());
      for g in data_ks.range((Bound::Excluded(start_k.as_slice()), upper_ref)) {
        let entry = g?;
        let (k, v) = (entry.key(), entry.value());
        if !k.starts_with(prefix_bytes) {
          break;
        }
        let member = &k[prefix_len..];
        if is_match_all || matches_glob_bytes(pat_bytes, member) {
          let mut sb = [0u8; 8];
          if v.len() >= 8 {
            sb.copy_from_slice(&v[..8]);
          }
          let score = decode_sortable_f64(sb);
          results.push((member.to_vec(), score));
          if results.len() >= limit {
            next_cursor = Some(member.to_vec());
            break;
          }
        }
      }
    } else {
      for g in data_ks.prefix(prefix_bytes) {
        let entry = g?;
        let (k, v) = (entry.key(), entry.value());
        if !k.starts_with(prefix_bytes) {
          break;
        }
        let member = &k[prefix_len..];
        if is_match_all || matches_glob_bytes(pat_bytes, member) {
          let mut sb = [0u8; 8];
          if v.len() >= 8 {
            sb.copy_from_slice(&v[..8]);
          }
          let score = decode_sortable_f64(sb);
          results.push((member.to_vec(), score));
          if results.len() >= limit {
            next_cursor = Some(member.to_vec());
            break;
          }
        }
      }
    }

    Ok((next_cursor, results))
  }
}
