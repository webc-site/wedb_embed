use crate::{
  api::key::{
    ALL_COMPOSITE_META_TAGS, cleanup_composite_data, find_active_composite_meta,
    opt::ExpireCondition,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  meta::{KeyMeta, current_now_ms},
  string::{
    compose_string_key as raw, decode_string_value, encode_string_value, is_string_expired,
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
  pub fn unlink_one<K: AsRef<[u8]>>(&self, key: K) -> Result<bool> {
    self.del_one(key)
  }

  #[inline]
  pub fn unlink<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<usize> {
    self.del(keys)
  }

  #[inline]
  pub fn exists_one<K: AsRef<[u8]>>(&self, key: K) -> Result<bool> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let now_ms = current_now_ms();
    let raw_k = raw(&kc, k_bytes);
    if let Some(raw) = self.data().get(&raw_k)? {
      let (expire_at, _) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        return Ok(true);
      }
    }
    let mut buf = Vec::with_capacity(64);
    Ok(find_active_composite_meta(self, k_bytes, now_ms, &mut buf)?.is_some())
  }

  #[inline]
  pub fn exists<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<usize> {
    exists_impl(self, keys)
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
    self
      .inner
      .db_scan_infos
      .write()
      .remove(&(self.ns_id(), self.id()));
    Ok(count)
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

    // 2. 若非未过期原生 String，检查并级联删除复合数据结构子键与元数据
    // 依据数学防穿透设计，活跃键只归属于单一类型，一旦命中即可 break 终止遍历
    if !hit && !meta_ks.is_empty()? {
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
          break;
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

/// Retrieves data type name for any given key (TYPE).
/// 查询任意键的数据类型名称（对标 Redis 7.0+ 与 Apache Kvrocks RedisTypeNames）
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
  if let Some((_, base_meta, _)) = find_active_composite_meta(db, key, now_ms, &mut buf)? {
    return Ok(base_meta.rtype.name());
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

  // 2. 检查复合类型元数据（单次复用 find_active_composite_meta 定位，消除冗余二次遍历）
  let mut buf = Vec::with_capacity(32 + key.len());
  if let Some((_, base_meta, m_bytes)) = find_active_composite_meta(db, key, now_ms, &mut buf)? {
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

  Ok(false)
}
