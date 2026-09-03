use rapidhash::{HashMapExt, RapidHashMap as HashMap};

use crate::{
  api::zset::{
    compose_zset_key, compose_zset_prefix,
    meta::decode_sortable_f64,
    r#impl::{compose_zset_meta_key, get_zset_meta},
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key_composer::SubkeyComposer,
  meta::current_now_ms,
  wedb::Db,
};

/// Score retrieval and multi-score operations (ZSCORE, ZMSCORE, ZMGET).
/// 有序集合分数获取与批量查询接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
