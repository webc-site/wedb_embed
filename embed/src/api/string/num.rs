use crate::{
  api::string::{
    r#const::{ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW},
    format_float_bytes, get_string_raw, key,
    meta::{with_encoded_string_value, write_string_val},
    parse_redis_float, parse_redis_integer,
  },
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};

/// Numeric arithmetic string operations (INCR, DECR, INCRBY, INCRBYFLOAT, etc.).
/// 数值增减与浮点算术字符串操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn incrby_ex<K: AsRef<[u8]>>(
    &self,
    key: K,
    increment: i64,
    expire_ms: u64,
    keep_ttl: bool,
  ) -> Result<i64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    let old_raw = get_string_raw(self, key_bytes)?;
    let (cur_num, old_expire) = match old_raw {
      Some((ref raw, exp, offset)) => (parse_redis_integer(&raw[offset..])?, exp),
      None => (0i64, 0u64),
    };

    let new_num = cur_num
      .checked_add(increment)
      .ok_or_else(|| Error::invalid_data(ERR_INCREMENT_OVERFLOW))?;

    let mut itoa_buf = itoa::Buffer::new();
    let formatted_bytes = itoa_buf.format(new_num).as_bytes();

    let target_expire = if keep_ttl { old_expire } else { expire_ms };

    let mut dyn_buf = Vec::new();
    with_encoded_string_value(formatted_bytes, target_expire, &mut dyn_buf, |enc_val| {
      write_string_val(self, &raw_k, key_bytes, enc_val, old_raw.is_none())
    })?;
    Ok(new_num)
  }

  #[inline]
  pub fn incr<K: AsRef<[u8]>>(&self, key: K) -> Result<i64> {
    self.incrby_ex(key, 1, 0, true)
  }

  #[inline]
  pub fn decr<K: AsRef<[u8]>>(&self, key: K) -> Result<i64> {
    self.incrby_ex(key, -1, 0, true)
  }

  #[inline]
  pub fn incrby<K: AsRef<[u8]>>(&self, key: K, increment: i64) -> Result<i64> {
    self.incrby_ex(key, increment, 0, true)
  }

  #[inline]
  pub fn decrby<K: AsRef<[u8]>>(&self, key: K, decrement: i64) -> Result<i64> {
    self.incrby_ex(key, -decrement, 0, true)
  }

  #[inline]
  pub fn incrbyfloat_ex<K: AsRef<[u8]>>(
    &self,
    key: K,
    increment: f64,
    expire_ms: u64,
    keep_ttl: bool,
  ) -> Result<f64> {
    if increment.is_nan() || increment.is_infinite() {
      return Err(Error::invalid_data(ERR_INCREMENT_NAN_OR_INFINITY));
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    let old_raw = get_string_raw(self, key_bytes)?;
    let (cur_num, old_expire) = match old_raw {
      Some((ref raw, exp, offset)) => (parse_redis_float(&raw[offset..])?, exp),
      None => (0.0f64, 0u64),
    };

    let mut new_num = cur_num + increment;
    if new_num.is_nan() || new_num.is_infinite() {
      return Err(Error::invalid_data(ERR_INCREMENT_NAN_OR_INFINITY));
    }
    if new_num == 0.0 {
      new_num = 0.0;
    }

    let target_expire = if keep_ttl { old_expire } else { expire_ms };

    let mut num_buf = zmij::Buffer::new();
    let formatted_bytes = format_float_bytes(new_num, &mut num_buf);
    let mut dyn_buf = Vec::new();
    with_encoded_string_value(formatted_bytes, target_expire, &mut dyn_buf, |enc_val| {
      write_string_val(self, &raw_k, key_bytes, enc_val, old_raw.is_none())
    })?;
    Ok(new_num)
  }

  #[inline]
  pub fn incrbyfloat<K: AsRef<[u8]>>(&self, key: K, increment: f64) -> Result<f64> {
    self.incrbyfloat_ex(key, increment, 0, true)
  }
}
