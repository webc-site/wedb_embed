use std::ops::Bound;

use rapidhash::{RapidHashSet as HashSet, v3::rapidhash_v3};

use crate::{
  api::key::{
    ALL_COMPOSITE_META_TAGS, cleanup_all_composite_data_with_buf, cleanup_composite_data,
    find_active_composite_meta, opt::ExpireCondition,
  },
  engine::{Engine, KvEntry, Partition},
  error::{ERR_NO_SUCH_KEY, Error, Result},
  key_composer::{KeyTag, matches_glob_bytes},
  meta::{KeyMeta, RedisType, current_now_ms, generate_version},
  string::{
    compose_string_key as raw, compose_string_prefix as string_key_prefix, decode_string_value,
    encode_string_value, is_string_expired,
  },
  wedb::{
    Db,
    core::{activate_db_impl, db_rm_impl},
  },
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
  pub fn dbsize(&self) -> Result<usize> {
    self.key_count()
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
  pub fn expire_with_condition<K: AsRef<[u8]>>(
    &self,
    key: K,
    ttl_sec: u64,
    cond: ExpireCondition,
  ) -> Result<bool> {
    set_key_expire_at_impl_with_condition(
      self,
      key.as_ref(),
      current_now_ms() + ttl_sec * 1000,
      cond,
    )
  }

  #[inline]
  pub fn pexpire<K: AsRef<[u8]>>(&self, key: K, ttl_ms: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), current_now_ms() + ttl_ms)
  }

  #[inline]
  pub fn pexpire_with_condition<K: AsRef<[u8]>>(
    &self,
    key: K,
    ttl_ms: u64,
    cond: ExpireCondition,
  ) -> Result<bool> {
    set_key_expire_at_impl_with_condition(self, key.as_ref(), current_now_ms() + ttl_ms, cond)
  }

  #[inline]
  pub fn expireat<K: AsRef<[u8]>>(&self, key: K, unix_sec: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), unix_sec * 1000)
  }

  #[inline]
  pub fn expireat_with_condition<K: AsRef<[u8]>>(
    &self,
    key: K,
    unix_sec: u64,
    cond: ExpireCondition,
  ) -> Result<bool> {
    set_key_expire_at_impl_with_condition(self, key.as_ref(), unix_sec * 1000, cond)
  }

  #[inline]
  pub fn pexpireat<K: AsRef<[u8]>>(&self, key: K, unix_ms: u64) -> Result<bool> {
    set_key_expire_at_impl(self, key.as_ref(), unix_ms)
  }

  #[inline]
  pub fn pexpireat_with_condition<K: AsRef<[u8]>>(
    &self,
    key: K,
    unix_ms: u64,
    cond: ExpireCondition,
  ) -> Result<bool> {
    set_key_expire_at_impl_with_condition(self, key.as_ref(), unix_ms, cond)
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

  #[inline]
  pub fn flushdb(&self) -> Result<u64> {
    let count = db_rm_impl(
      self.data(),
      self.meta(),
      self.engine(),
      self.ns_id(),
      self.id(),
    )?;
    activate_db_impl::<E>(self.meta(), self.ns_id(), self.id())?;
    Ok(count)
  }

  #[inline]
  pub fn scan(
    &self,
    cursor: &[u8],
    count: Option<usize>,
    pattern: Option<&[u8]>,
    rtype: Option<RedisType>,
  ) -> Result<(Vec<u8>, Vec<Vec<u8>>)> {
    scan_impl(self, cursor, count, pattern, rtype)
  }

  #[inline]
  pub fn randomkey(&self) -> Result<Option<Vec<u8>>> {
    randomkey_impl(self)
  }

  #[inline]
  pub fn copy<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2, nx: bool) -> Result<bool> {
    copy_impl(self, src.as_ref(), dst.as_ref(), nx, false)
  }

  #[inline]
  pub fn rename<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2) -> Result<()> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    if !self.exists_one(src_bytes)? {
      return Err(Error::not_found(ERR_NO_SUCH_KEY));
    }
    if src_bytes == dst_bytes {
      return Ok(());
    }
    copy_impl(self, src_bytes, dst_bytes, false, true)?;
    Ok(())
  }

  #[inline]
  pub fn renamenx<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2) -> Result<bool> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    if !self.exists_one(src_bytes)? {
      return Err(Error::not_found(ERR_NO_SUCH_KEY));
    }
    if src_bytes == dst_bytes {
      return Ok(false);
    }
    copy_impl(self, src_bytes, dst_bytes, true, true)
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
#[inline]
pub fn set_key_expire_at_impl<E: Engine>(db: &Db<E>, key: &[u8], expire_at_ms: u64) -> Result<bool>
where
  Error: From<E::Error>,
{
  set_key_expire_at_impl_with_condition(db, key, expire_at_ms, ExpireCondition::None)
}

/// Sets absolute expiration timestamp in milliseconds with condition (NX, XX, GT, LT).
/// 按条件为任意键设置绝对过期毫秒时间戳（支持 NX, XX, GT, LT 语义，对标 Redis 7.0+）
pub fn set_key_expire_at_impl_with_condition<E: Engine>(
  db: &Db<E>,
  key: &[u8],
  expire_at_ms: u64,
  cond: ExpireCondition,
) -> Result<bool>
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
      if !cond.should_update(exp, expire_at_ms) {
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
      && let Some(base_meta) = KeyMeta::decode(&m_bytes)
      && !base_meta.is_expired(now_ms)
    {
      if expire_at_ms == 0 && base_meta.expire_at == 0 {
        return Ok(false);
      }
      if !cond.should_update(base_meta.expire_at, expire_at_ms) {
        return Ok(false);
      }
      let mut new_m_bytes = m_bytes.to_vec();
      let is_kvrocks = (new_m_bytes[0] & KeyMeta::META_64BIT_ENCODING_MASK) != 0
        || (new_m_bytes.len() >= KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE && new_m_bytes[0] > 14);
      let exp_offset = if is_kvrocks { 1 } else { 2 };
      if new_m_bytes.len() >= exp_offset + 8 {
        new_m_bytes[exp_offset..exp_offset + 8].copy_from_slice(&expire_at_ms.to_be_bytes());
        let mut batch = db.batch();
        batch.insert_meta(&buf, &new_m_bytes);
        batch.commit()?;
        return Ok(true);
      }
    }
  }

  Ok(false)
}

/// Scans database keys matching pattern and type using cursor-based pagination (SCAN).
/// 数据库键游标分页遍历（对标 Redis SCAN / Kvrocks Database::Scan）
pub fn scan_impl<E: Engine>(
  db: &Db<E>,
  cursor: &[u8],
  count: Option<usize>,
  pattern: Option<&[u8]>,
  rtype: Option<RedisType>,
) -> Result<(Vec<u8>, Vec<Vec<u8>>)>
where
  Error: From<E::Error>,
{
  let limit = count.unwrap_or(10).max(1);
  let mut keys = Vec::with_capacity(limit);
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  let is_init = cursor.is_empty() || cursor == b"0";
  let in_meta_phase = !is_init && cursor.starts_with(b"m:");

  // Phase 1: Scan String keys from data_ks if rtype is compatible
  let should_scan_string = rtype.is_none() || rtype == Some(RedisType::String);
  if should_scan_string && !in_meta_phase {
    let str_prefix = string_key_prefix(&kc);
    let seek_key = if !is_init && cursor.starts_with(b"s:") {
      let mut k = str_prefix.clone();
      k.extend_from_slice(&cursor[2..]);
      Some(k)
    } else {
      None
    };

    let start_bound = match seek_key.as_ref() {
      Some(sk) => Bound::Included(sk.as_slice()),
      None => Bound::Included(str_prefix.as_slice()),
    };
    let iter = data_ks.range((start_bound, Bound::Unbounded));

    let seek_user_k = if !is_init && cursor.starts_with(b"s:") {
      Some(&cursor[2..])
    } else {
      None
    };

    for item in iter {
      let entry = item?;
      let k = entry.key();
      if !k.starts_with(&str_prefix) {
        break;
      }
      let user_k = &k[str_prefix.len()..];
      if let Some(sk) = seek_user_k
        && user_k == sk
      {
        continue;
      }

      let (expire_at, _) = decode_string_value(entry.value());
      if is_string_expired(expire_at, now_ms) {
        continue;
      }

      if pattern
        .map(|p| matches_glob_bytes(p, user_k))
        .unwrap_or(true)
      {
        keys.push(user_k.to_vec());
        if keys.len() >= limit {
          let mut next_cursor = Vec::with_capacity(2 + user_k.len());
          next_cursor.extend_from_slice(b"s:");
          next_cursor.extend_from_slice(user_k);
          return Ok((next_cursor, keys));
        }
      }
    }

    if rtype == Some(RedisType::String) {
      return Ok((b"0".to_vec(), keys));
    }
  }

  // Phase 2: Scan Composite keys from meta_ks
  let should_scan_meta = rtype.map(|t| t != RedisType::String).unwrap_or(true);
  if should_scan_meta {
    let meta_prefix = kc.namespace_prefix();
    let scope_prefix_len = kc.scope_prefix_len();

    let seek_key = if in_meta_phase {
      let mut k = meta_prefix.clone();
      k.extend_from_slice(&cursor[2..]);
      Some(k)
    } else {
      None
    };

    let start_bound = match seek_key.as_ref() {
      Some(sk) => Bound::Included(sk.as_slice()),
      None => Bound::Included(meta_prefix.as_slice()),
    };
    let iter = meta_ks.range((start_bound, Bound::Unbounded));

    let seek_meta_k = if in_meta_phase {
      Some(&cursor[2..])
    } else {
      None
    };

    for item in iter {
      let entry = item?;
      let k = entry.key();
      if !k.starts_with(&meta_prefix) {
        break;
      }
      let remain = &k[scope_prefix_len..];
      if remain.is_empty() {
        continue;
      }

      if let Some(smk) = seek_meta_k
        && remain == smk
      {
        continue;
      }

      let Some(tag) = KeyTag::from_u8(remain[0]) else {
        continue;
      };
      if !tag.is_meta() {
        continue;
      }

      let Some(meta) = KeyMeta::decode(entry.value()) else {
        continue;
      };
      if meta.is_expired(now_ms) {
        continue;
      }

      if let Some(expected_type) = rtype
        && meta.rtype != expected_type
      {
        continue;
      }

      let user_k = &remain[1..];
      if pattern
        .map(|p| matches_glob_bytes(p, user_k))
        .unwrap_or(true)
      {
        keys.push(user_k.to_vec());
        if keys.len() >= limit {
          let mut next_cursor = Vec::with_capacity(2 + remain.len());
          next_cursor.extend_from_slice(b"m:");
          next_cursor.extend_from_slice(remain);
          return Ok((next_cursor, keys));
        }
      }
    }
  }

  Ok((b"0".to_vec(), keys))
}

/// Returns a random active key from current database (RANDOMKEY, aligned with Kvrocks Database::RandomKey).
/// 返回当前数据库中的一个随机活跃键（对标 Kvrocks RANDOM_KEY_SCAN_LIMIT = 60 算法）
pub fn randomkey_impl<E: Engine>(db: &Db<E>) -> Result<Option<Vec<u8>>>
where
  Error: From<E::Error>,
{
  let (_, keys) = scan_impl(db, b"0", Some(60), None, None)?;
  if keys.is_empty() {
    return Ok(None);
  }
  let idx = fastrand::usize(..keys.len());
  Ok(Some(keys[idx].clone()))
}

/// Copies or moves a key and all its associated subkeys (COPY / RENAME / RENAMENX).
/// 复制或原子移动键及其所有关联子键（对标 Kvrocks Database::Copy / Rename）
pub fn copy_impl<E: Engine>(
  db: &Db<E>,
  src: &[u8],
  dst: &[u8],
  nx: bool,
  delete_old: bool,
) -> Result<bool>
where
  Error: From<E::Error>,
{
  if src == dst {
    let exists = exists_impl(db, &[src])? > 0;
    if !exists {
      return Ok(false);
    }
    return Ok(!nx);
  }

  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  if nx && exists_impl(db, &[dst])? > 0 {
    return Ok(false);
  }

  let src_raw_k = raw(&kc, src);

  // 1. 检查原生 String
  if let Some(src_val) = data_ks.get(&src_raw_k)? {
    let (exp, _) = decode_string_value(&src_val);
    if !is_string_expired(exp, now_ms) {
      let mut batch = db.batch();
      if !nx {
        let dst_raw_k = raw(&kc, dst);
        batch.rm_data(&dst_raw_k);
        let mut buf = Vec::new();
        cleanup_all_composite_data_with_buf(db, dst, &mut batch, &mut buf)?;
      }
      let dst_raw_k = raw(&kc, dst);
      batch.insert_data(&dst_raw_k, &src_val);
      if delete_old {
        batch.rm_data(&src_raw_k);
      }
      batch.commit()?;
      return Ok(true);
    }
  }

  if meta_ks.is_empty()? {
    return Ok(false);
  }

  // 2. 检查复合类型元数据
  let mut buf = Vec::new();
  if let Some((tag_u8, _base_meta, raw_guard)) =
    find_active_composite_meta(db, src, now_ms, &mut buf)?
  {
    let mut batch = db.batch();
    if !nx {
      let dst_raw_k = raw(&kc, dst);
      batch.rm_data(&dst_raw_k);
      cleanup_all_composite_data_with_buf(db, dst, &mut batch, &mut buf)?;
    }

    let new_version = generate_version();
    let mut dst_meta_val = raw_guard.to_vec();
    let is_kvrocks = (dst_meta_val[0] & KeyMeta::META_64BIT_ENCODING_MASK) != 0
      || (dst_meta_val.len() >= KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE && dst_meta_val[0] > 14);
    let ver_offset = if is_kvrocks { 9 } else { 10 };
    if dst_meta_val.len() >= ver_offset + 8 {
      dst_meta_val[ver_offset..ver_offset + 8].copy_from_slice(&new_version.to_be_bytes());
    }

    kc.compose_meta_key_into(&[tag_u8], dst, &mut buf);
    batch.insert_meta(&buf, &dst_meta_val);

    let Some(key_tag) = KeyTag::from_u8(tag_u8) else {
      return Ok(false);
    };

    let data_tags = match key_tag {
      KeyTag::HashMeta => vec![KeyTag::HashData],
      KeyTag::ListMeta => vec![KeyTag::ListData],
      KeyTag::SetMeta => vec![KeyTag::SetData],
      KeyTag::ZSetMeta => vec![KeyTag::ZSetData, KeyTag::ZSetScore],
      KeyTag::BitmapMeta => vec![KeyTag::BitmapData],
      KeyTag::BloomMeta => vec![KeyTag::BloomData],
      KeyTag::CuckooMeta => vec![KeyTag::CuckooData],
      KeyTag::SortedIntMeta => vec![KeyTag::SortedIntData],
      KeyTag::TimeSeriesMeta => vec![KeyTag::TimeSeriesData],
      KeyTag::StreamMeta => vec![
        KeyTag::StreamData,
        KeyTag::StreamGroup,
        KeyTag::StreamConsumer,
        KeyTag::StreamPel,
      ],
      KeyTag::JsonMeta => vec![KeyTag::JsonData],
      KeyTag::TDigestMeta => vec![KeyTag::TDigestData],
      KeyTag::HllMeta => vec![KeyTag::HllRaw],
      _ => vec![],
    };

    for dtag in data_tags {
      let mut src_prefix = Vec::new();
      let mut dst_prefix = Vec::new();

      if dtag == KeyTag::HllRaw {
        kc.compose_meta_key_into(dtag.as_slice(), src, &mut src_prefix);
        kc.compose_meta_key_into(dtag.as_slice(), dst, &mut dst_prefix);
        if let Some(val) = data_ks.get(&src_prefix)? {
          batch.insert_data(&dst_prefix, &val);
          if delete_old {
            batch.rm_data(&src_prefix);
          }
        }
      } else {
        kc.compose_prefix_into(dtag.as_slice(), src, &mut src_prefix);
        kc.compose_prefix_into(dtag.as_slice(), dst, &mut dst_prefix);

        for item in data_ks.prefix(&src_prefix) {
          let entry = item?;
          let k = entry.key();
          if !k.starts_with(&src_prefix) {
            break;
          }
          let remain = &k[src_prefix.len()..];
          let mut new_sub_k = dst_prefix.clone();
          new_sub_k.extend_from_slice(remain);

          batch.insert_data(&new_sub_k, entry.value());
          if delete_old {
            batch.rm_data(k);
          }
        }
      }
    }

    if delete_old {
      kc.compose_meta_key_into(&[tag_u8], src, &mut buf);
      batch.rm_meta(&buf);
    }

    batch.commit()?;
    return Ok(true);
  }

  Ok(false)
}
