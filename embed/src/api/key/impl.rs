use rapidhash::{RapidHashSet as HashSet, v3::rapidhash_v3};

use crate::{
  api::key::{ALL_COMPOSITE_META_TAGS, cleanup_composite_data, find_active_composite_meta},
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key_composer::{KeyTag, matches_glob_bytes},
  meta::{KeyMeta, current_now_ms},
  string::{
    compose_string_key as raw, compose_string_prefix as string_key_prefix, decode_string_value,
    encode_string_value, is_string_expired,
  },
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn del_one<K: AsRef<[u8]>>(&self, key: K) -> Result<bool> {
    Ok(self.del(&[key])? > 0)
  }

  #[inline]
  pub fn del<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<usize> {
    del_impl(self, keys)
  }

  #[inline]
  pub fn exists_one<K: AsRef<[u8]>>(&self, key: K) -> Result<bool> {
    Ok(self.exists(&[key])? > 0)
  }

  #[inline]
  pub fn exists<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<usize> {
    exists_impl(self, keys)
  }

  #[inline]
  pub fn keys<P: AsRef<[u8]>>(&self, pattern: P) -> Result<Vec<Vec<u8>>> {
    keys_impl(self, pattern.as_ref())
  }

  #[inline]
  pub fn key_count(&self) -> Result<usize> {
    key_count_impl(self)
  }

  #[inline]
  pub fn persist<K: AsRef<[u8]>>(&self, key: K) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), 0)
  }

  #[inline]
  pub fn expire<K: AsRef<[u8]>>(&self, key: K, ttl_sec: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), current_now_ms() + ttl_sec * 1000)
  }

  #[inline]
  pub fn pexpire<K: AsRef<[u8]>>(&self, key: K, ttl_ms: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), current_now_ms() + ttl_ms)
  }

  #[inline]
  pub fn expireat<K: AsRef<[u8]>>(&self, key: K, unix_sec: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), unix_sec * 1000)
  }

  #[inline]
  pub fn pexpireat<K: AsRef<[u8]>>(&self, key: K, unix_ms: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), unix_ms)
  }

  #[inline]
  fn query_key_expire_info<K: AsRef<[u8]>, M: Fn(u64, u64) -> i64>(
    &self,
    key: K,
    map_live_ttl: M,
  ) -> Result<i64> {
    match get_key_expire_at_impl(self, key.as_ref())? {
      Some(0) => Ok(-1),
      Some(exp) => {
        let now = current_now_ms();
        if now >= exp {
          Ok(-2)
        } else {
          Ok(map_live_ttl(exp, now))
        }
      }
      None => Ok(-2),
    }
  }

  #[inline]
  pub fn ttl<K: AsRef<[u8]>>(&self, key: K) -> Result<i64> {
    self.query_key_expire_info(key, |exp, now| (exp - now).div_ceil(1000) as i64)
  }

  #[inline]
  pub fn pttl<K: AsRef<[u8]>>(&self, key: K) -> Result<i64> {
    self.query_key_expire_info(key, |exp, now| (exp - now) as i64)
  }

  #[inline]
  pub fn expiretime<K: AsRef<[u8]>>(&self, key: K) -> Result<i64> {
    self.query_key_expire_info(key, |exp, _| (exp / 1000) as i64)
  }

  #[inline]
  pub fn pexpiretime<K: AsRef<[u8]>>(&self, key: K) -> Result<i64> {
    self.query_key_expire_info(key, |exp, _| exp as i64)
  }

  #[inline]
  pub fn type_of<K: AsRef<[u8]>>(&self, key: K) -> Result<&'static str> {
    key_type_impl(self, key.as_ref())
  }

  #[inline]
  pub fn get_key_expire_at<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<u64>> {
    get_key_expire_at_impl(self, key.as_ref())
  }
}

/// Deletes one or more keys (DEL).
/// 删除一个或多个键（DEL）
pub fn del_impl<E: Engine, K: AsRef<[u8]>>(db: &Db<E>, keys: &[K]) -> Result<usize>
where
  Error: From<E::Error>,
{
  let mut deleted = 0;
  let mut batch = db.batch_with_capacity(keys.len());
  let now_ms = current_now_ms();
  let mut buf = Vec::with_capacity(64);
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  for k in keys {
    let k_bytes = k.as_ref();
    let mut hit = false;

    // 1. 检查并删除原生 String 键
    let raw_k = raw(&kc, k_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, _) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        hit = true;
      }
      batch.rm_data(&raw_k);
    }

    // 2. 检查并级联删除复合数据结构子键与元数据
    if !meta_ks.is_empty()? {
      for &meta_tag in ALL_COMPOSITE_META_TAGS {
        kc.compose_meta_key_into(meta_tag, k_bytes, &mut buf);
        if let Some(m_bytes) = meta_ks.get(&buf)? {
          if let Some(base_meta) = KeyMeta::decode(&m_bytes)
            && !base_meta.is_expired(now_ms)
          {
            hit = true;
          }
          batch.rm_meta(buf.as_slice());
          cleanup_composite_data(db, meta_tag, k_bytes, &mut batch, &mut buf)?;
        }
      }
    }

    if hit {
      deleted += 1;
    }
  }

  batch.commit()?;
  Ok(deleted)
}

/// Checks whether a key exists (EXISTS).
/// 检查键是否存在（EXISTS）
pub fn exists_impl<E: Engine, K: AsRef<[u8]>>(db: &Db<E>, keys: &[K]) -> Result<usize>
where
  Error: From<E::Error>,
{
  let mut count = 0;
  let now_ms = current_now_ms();
  let mut buf = Vec::with_capacity(64);
  let kc = db.kc();
  let data_ks = db.data();

  for k in keys {
    let k_bytes = k.as_ref();
    let raw_k = raw(&kc, k_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, _) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        count += 1;
        continue;
      }
    }
    if find_active_composite_meta(db, k_bytes, now_ms, &mut buf)?.is_some() {
      count += 1;
    }
  }
  Ok(count)
}

/// Queries all user keys in current namespace matching a wildcard pattern.
/// 按通配符模式匹配查询当前命名空间下的所有用户 Key
pub fn keys_impl<E: Engine>(db: &Db<E>, pattern: impl AsRef<[u8]>) -> Result<Vec<Vec<u8>>>
where
  Error: From<E::Error>,
{
  let mut result = Vec::new();
  let mut seen = HashSet::default();
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();
  let pat_bytes = pattern.as_ref();

  // 1. 扫描 data_ks 中的 String 键（仅匹配 String 前缀，零子键开销）
  let str_prefix = string_key_prefix(&kc);
  for item in data_ks.prefix(&str_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(&str_prefix) {
      break;
    }
    let (expire_at, _) = decode_string_value(entry.value());
    if is_string_expired(expire_at, now_ms) {
      continue;
    }
    let user_k = &k[str_prefix.len()..];
    if matches_glob_bytes(pat_bytes, user_k) && seen.insert(user_k.to_vec()) {
      result.push(user_k.to_vec());
    }
  }

  // 2. 扫描 meta_ks 中的复合数据结构元数据键（单次遍历无子键）
  let meta_prefix = kc.namespace_prefix();
  let scope_prefix_len = kc.scope_prefix_len();
  for item in meta_ks.prefix(&meta_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(&meta_prefix) {
      break;
    }
    let remain = &k[scope_prefix_len..];
    if remain.is_empty() {
      continue;
    }
    let Some(tag) = KeyTag::from_u8(remain[0]) else {
      continue;
    };
    if !tag.is_meta() {
      continue;
    }
    if let Some(meta) = KeyMeta::decode(entry.value()) {
      if meta.is_expired(now_ms) {
        continue;
      }
      let user_k = &remain[1..];
      if matches_glob_bytes(pat_bytes, user_k) && seen.insert(user_k.to_vec()) {
        result.push(user_k.to_vec());
      }
    }
  }

  result.sort();
  Ok(result)
}

/// Counts the total number of user keys in the current namespace.
/// 统计当前命名空间下的 Key 总数
pub fn key_count_impl<E: Engine>(db: &Db<E>) -> Result<usize>
where
  Error: From<E::Error>,
{
  let mut seen = HashSet::<u64>::default();
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  let str_prefix = string_key_prefix(&kc);
  for item in data_ks.prefix(&str_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(&str_prefix) {
      break;
    }
    let (expire_at, _) = decode_string_value(entry.value());
    if is_string_expired(expire_at, now_ms) {
      continue;
    }
    let user_k = &k[str_prefix.len()..];
    seen.insert(rapidhash_v3(user_k));
  }

  let meta_prefix = kc.namespace_prefix();
  let scope_prefix_len = kc.scope_prefix_len();
  for item in meta_ks.prefix(&meta_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(&meta_prefix) {
      break;
    }
    let remain = &k[scope_prefix_len..];
    if remain.is_empty() {
      continue;
    }
    let Some(tag) = KeyTag::from_u8(remain[0]) else {
      continue;
    };
    if !tag.is_meta() {
      continue;
    }
    if let Some(meta) = KeyMeta::decode(entry.value()) {
      if meta.is_expired(now_ms) {
        continue;
      }
      let user_k = &remain[1..];
      seen.insert(rapidhash_v3(user_k));
    }
  }

  Ok(seen.len())
}

/// Retrieves data type name for any given key (TYPE).
/// 查询任意键的数据类型名称
pub fn key_type_impl<E: Engine>(db: &Db<E>, key: &[u8]) -> Result<&'static str>
where
  Error: From<E::Error>,
{
  let now_ms = current_now_ms();
  let kc = db.kc();
  let raw_k = raw(&kc, key);
  if let Some(raw) = db.data().get(&raw_k)? {
    let (exp, _) = decode_string_value(&raw);
    if !is_string_expired(exp, now_ms) {
      return Ok("string");
    }
  }

  let mut buf = Vec::with_capacity(32 + key.len());
  if let Some((tag, ..)) = find_active_composite_meta(db, key, now_ms, &mut buf)? {
    return Ok(
      KeyTag::from_u8(tag)
        .map(|t| t.type_name())
        .unwrap_or("unknown"),
    );
  }

  Ok("none")
}

/// Retrieves absolute expiration timestamp in milliseconds for any key.
/// 获取任意键的绝对过期毫秒时间戳（不存在返回 None，无过期返回 Some(0)，有过期返回 Some(expire_at_ms)）
pub fn get_key_expire_at_impl<E: Engine>(db: &Db<E>, key: &[u8]) -> Result<Option<u64>>
where
  Error: From<E::Error>,
{
  let now_ms = current_now_ms();
  let kc = db.kc();
  let raw_k = raw(&kc, key);
  if let Some(raw) = db.data().get(&raw_k)? {
    let (exp, _) = decode_string_value(&raw);
    if !is_string_expired(exp, now_ms) {
      return Ok(Some(exp));
    }
  }

  let mut buf = Vec::with_capacity(32 + key.len());
  if let Some((_, meta, _)) = find_active_composite_meta(db, key, now_ms, &mut buf)? {
    return Ok(Some(meta.expire_at));
  }

  Ok(None)
}

/// Sets absolute expiration timestamp in milliseconds for any key (expire_at_ms = 0 persists key).
/// 为任意键设置绝对过期毫秒时间戳（expire_at_ms = 0 表示 PERSIST）
pub fn set_key_expire_at_impl<E: Engine>(db: &Db<E>, key: &[u8], expire_at_ms: u64) -> Result<bool>
where
  Error: From<E::Error>,
{
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  // 1. 检查原生 String
  let raw_k = raw(&kc, key);
  if let Some(raw) = data_ks.get(&raw_k)? {
    let (exp, payload) = decode_string_value(&raw);
    if !is_string_expired(exp, now_ms) {
      if expire_at_ms == 0 && exp == 0 {
        return Ok(false);
      }
      let new_raw = encode_string_value(payload, expire_at_ms);
      let mut batch = db.batch();
      batch.insert_data(&raw_k, &new_raw);
      batch.commit()?;
      return Ok(true);
    }
  }

  if meta_ks.is_empty()? {
    return Ok(false);
  }

  // 2. 检查复合类型元数据
  let mut buf = Vec::with_capacity(32 + key.len());
  for &tag in ALL_COMPOSITE_META_TAGS {
    kc.compose_meta_key_into(tag, key, &mut buf);
    if let Some(m_bytes) = meta_ks.get(&buf)?
      && let Some(mut base_meta) = KeyMeta::decode(&m_bytes)
      && !base_meta.is_expired(now_ms)
    {
      if expire_at_ms == 0 && base_meta.expire_at == 0 {
        return Ok(false);
      }
      base_meta.expire_at = expire_at_ms;
      let mut batch = db.batch();
      let enc_hdr = base_meta.encode();
      if m_bytes.len() <= KeyMeta::ENCODED_SIZE {
        batch.insert_meta(&buf, &enc_hdr[..]);
      } else {
        let mut encoded = Vec::with_capacity(m_bytes.len());
        encoded.extend_from_slice(&enc_hdr);
        encoded.extend_from_slice(&m_bytes[KeyMeta::ENCODED_SIZE..]);
        batch.insert_meta(&buf, &encoded);
      }
      batch.commit()?;
      return Ok(true);
    }
  }

  Ok(false)
}
