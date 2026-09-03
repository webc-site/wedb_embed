use crate::{
  api::hash::{
    r#const::{ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW},
    r#impl::prepare_hash_meta_for_write,
    meta::{HashItemKeyComposer, compose_hash_meta_key, is_field_expired},
    parse_hash_float, parse_hash_integer,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  string::format_float_bytes,
  wedb::Db,
};

/// Numeric and arithmetic hash operations (HINCRBY, HINCRBYFLOAT).
/// 哈希数字增减与浮点算术操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn hincrby<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    step: i64,
  ) -> Result<i64> {
    let key_bytes = key.as_ref();
    let field_bytes = field.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut batch = self.batch();
    let (mut meta, metadata_existed) =
      prepare_hash_meta_for_write(self, key_bytes, &meta_k, now_ms, &mut batch)?;

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(field_bytes);

    let data_ks = self.data();
    let _meta_ks = self.meta();

    let (cur_val, is_missing, is_expired_ttl, target_expire) = if metadata_existed {
      match data_ks.get(item_k)? {
        Some(raw) => match meta.decode_subkey_value(&raw) {
          Some((exp, payload)) => {
            if is_field_expired(exp, now_ms) {
              (0i64, false, true, 0u64)
            } else {
              (parse_hash_integer(payload)?, false, false, exp)
            }
          }
          None => (0i64, true, false, 0u64),
        },
        None => (0i64, true, false, 0u64),
      }
    } else {
      (0i64, true, false, 0u64)
    };

    let new_val = cur_val
      .checked_add(step)
      .ok_or_else(|| Error::invalid_data(ERR_INCREMENT_OVERFLOW))?;

    if is_missing {
      meta.apply_missing_to_persistent();
    } else if is_expired_ttl {
      meta.apply_ttl_to_persistent();
    }

    let mut itoa_buf = itoa::Buffer::new();
    let val_bytes = itoa_buf.format(new_val).as_bytes();
    meta.with_encoded_subkey_value(val_bytes, target_expire, |enc| {
      batch.insert_data(item_k, enc)
    });
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;

    Ok(new_val)
  }

  #[inline]
  pub fn hincrbyfloat<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    step: f64,
  ) -> Result<f64> {
    let key_bytes = key.as_ref();
    let field_bytes = field.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut batch = self.batch();
    let (mut meta, metadata_existed) =
      prepare_hash_meta_for_write(self, key_bytes, &meta_k, now_ms, &mut batch)?;

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(field_bytes);

    let data_ks = self.data();
    let _meta_ks = self.meta();

    let (cur_val, is_missing, is_expired_ttl, target_expire) = if metadata_existed {
      match data_ks.get(item_k)? {
        Some(raw) => match meta.decode_subkey_value(&raw) {
          Some((exp, payload)) => {
            if is_field_expired(exp, now_ms) {
              (0.0f64, false, true, 0u64)
            } else {
              (parse_hash_float(payload)?, false, false, exp)
            }
          }
          None => (0.0f64, true, false, 0u64),
        },
        None => (0.0f64, true, false, 0u64),
      }
    } else {
      (0.0f64, true, false, 0u64)
    };

    let new_val = cur_val + step;
    if new_val.is_nan() || new_val.is_infinite() {
      return Err(Error::invalid_data(ERR_INCREMENT_NAN_OR_INFINITY));
    }

    if is_missing {
      meta.apply_missing_to_persistent();
    } else if is_expired_ttl {
      meta.apply_ttl_to_persistent();
    }

    let mut f_buf = zmij::Buffer::new();
    let val_bytes = format_float_bytes(new_val, &mut f_buf);
    meta.with_encoded_subkey_value(val_bytes, target_expire, |enc| {
      batch.insert_data(item_k, enc)
    });
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;

    Ok(new_val)
  }
}
