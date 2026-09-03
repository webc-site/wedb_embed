use crate::{
  api::zset::{
    r#impl::{compose_zset_meta_key, get_zset_meta},
    opt::{IntoRangeLex, IntoRangeScore},
  },
  engine::Engine,
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

/// Cardinality, counting, and ranking operations (ZCARD, ZCOUNT, ZLEXCOUNT, ZRANK, ZREVRANK).
/// 有序集合元素基数、区间计数与成员排名接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
