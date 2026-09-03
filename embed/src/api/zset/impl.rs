use std::ops::Bound;

use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::zset::{
    ZSetMemberScore,
    key::{compose_zset_key, compose_zset_score_key},
    meta::{ZSetMeta, decode_sortable_f64_slice},
    opt::{RangeLex, RangeScore, ZAdd},
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{check_key_not_other_type, clear_prefix_in_batch, get_meta_checked, prefix_upper_bound},
  key_composer::{KeyComposer, KeyTag, SmallKey},
  meta::{
    current_now_ms, encode_sortable_f64, generate_version, normalize_range as meta_normalize_range,
  },
  wedb::{Db, DbBatch},
};

pub(crate) const SCORE_LEN: usize = 8;

#[inline(always)]
pub(crate) fn parse_score_sub(sub: &[u8]) -> Option<(f64, &[u8])> {
  decode_sortable_f64_slice(sub).map(|score| (score, &sub[SCORE_LEN..]))
}

/// Stack-allocated ZSet metadata key without heap allocation.
/// 构造 ZSet 元数据键字节序列（栈上定长，零堆分配）
#[inline]
pub(crate) fn compose_zset_meta_key(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::ZSetMeta.as_slice(), key)
}

/// Stack-allocated ZSet member prefix without heap allocation.
/// 构造 ZSet 成员前缀字节序列（栈上定长，零堆分配）
#[inline]
pub(crate) fn compose_zset_prefix(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ZSetData.as_slice(), key)
}

/// Stack-allocated ZSet score index prefix without heap allocation.
/// 构造 ZSet 分数索引前缀字节序列（栈上定长，零堆分配）
#[inline]
pub(crate) fn compose_zset_score_prefix(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ZSetScore.as_slice(), key)
}

/// Normalizes Redis index range supporting negative indices with lower bound clamped to 0.
/// 标准 Redis 索引范围规整化（支持负索引，下界 0 对齐，超范围终止）
#[inline]
pub(crate) fn normalize_range(card: usize, start: i64, stop: i64) -> Option<(usize, usize)> {
  let (s, e) = meta_normalize_range(start, stop, card as i64);
  if s > e {
    None
  } else {
    Some((s as usize, e as usize))
  }
}

/// Constructs start and end bounds from RangeScore using 8-byte big-endian encoding.
/// 根据 RangeScore 构造基于保序大端序 8 字节 Score 前缀的起止边界
#[inline]
pub(crate) fn score_range_bounds(
  score_prefix: &[u8],
  spec: &RangeScore,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
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
pub(crate) fn lex_range_bounds(
  member_prefix: &[u8],
  spec: &RangeLex,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
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
pub(crate) fn get_zset_meta<E: Engine>(
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
          let old_score = decode_sortable_f64_slice(&old_sb).unwrap_or(0.0);

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
          let old_score = decode_sortable_f64_slice(&old_sb).unwrap_or(0.0);

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
            s_key.extend_from_slice(&old_sb[..8]);
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
        let score = decode_sortable_f64_slice(&sb).unwrap_or(0.0);
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

          if sb.len() >= 8 {
            s_key.clear();
            s_key.extend_from_slice(&score_prefix);
            s_key.extend_from_slice(&sb[..8]);
            s_key.extend_from_slice(m_bytes);

            batch.rm_weak_data(&s_key);
          }
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
      let old_score = decode_sortable_f64_slice(&old_sb).unwrap_or(0.0);
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
}
