use crate::{
  api::string::{
    MAX_STRING_SIZE,
    r#const::{ERR_DIGEST_INVALID_LEN, ERR_STRING_EXCEEDS_MAX_SIZE},
    get_string_raw, key,
    meta::{encode_string_value, with_encoded_string_value},
    opt::{DelEx, GetEx, Set, StringSet, StringSetType},
    string_digest_bytes,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::cleanup_all_composite_data,
  meta::current_now_ms,
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => Ok(Some(raw[offset..].to_vec())),
      None => Ok(None),
    }
  }

  #[inline]
  pub fn with_get<K: AsRef<[u8]>, R>(
    &self,
    key: K,
    f: impl FnOnce(&[u8]) -> R,
  ) -> Result<Option<R>> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => Ok(Some(f(&raw[offset..]))),
      None => Ok(None),
    }
  }

  #[inline]
  pub fn get_with_expire<K: AsRef<[u8]>>(&self, key: K) -> Result<(Option<Vec<u8>>, u64)> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, exp, offset)) => Ok((Some(raw[offset..].to_vec()), exp)),
      None => Ok((None, 0)),
    }
  }

  #[inline]
  pub fn set_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<Option<Vec<u8>>> {
    self.set(key, val, [])
  }

  #[inline]
  pub fn set<'a, K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    opt_li: impl IntoIterator<Item = Set<'a>>,
  ) -> Result<Option<Vec<u8>>> {
    let now_ms = current_now_ms();
    let args = Set::parse_options(opt_li, now_ms);
    self.set_internal(key, val, &args)
  }

  #[inline]
  pub(crate) fn set_internal<'a, K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    args: &StringSet<'a>,
  ) -> Result<Option<Vec<u8>>> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();

    if val_bytes.len() > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    // 校验 IFDEQ / IFDNE 摘要格式（16位十六进制字符）
    if let Some(expected_digest) = args.cmp_value
      && matches!(args.set_type, StringSetType::IfDeq | StringSetType::IfDne)
      && (expected_digest.len() != 16 || !expected_digest.iter().all(|b| b.is_ascii_hexdigit()))
    {
      return Err(Error::invalid_data(ERR_DIGEST_INVALID_LEN));
    }

    let raw_k = key::raw(&kc, key_bytes);
    let data_ks = self.data();
    // 极速直写快速通道 (Fast Path)
    if args.is_fast_path() {
      let meta_ks = self.meta();
      let meta_is_empty = meta_ks.is_empty()?;
      let mut dyn_buf = Vec::new();
      with_encoded_string_value(
        val_bytes,
        args.expire,
        &mut dyn_buf,
        |enc_val| -> Result<()> {
          if meta_is_empty {
            data_ks.insert(&raw_k, enc_val)?;
          } else {
            let mut batch = self.batch();
            batch.insert_data(&raw_k, enc_val);
            cleanup_all_composite_data(self, key_bytes, &mut batch)?;
            batch.commit()?;
          }
          Ok(())
        },
      )?;
      return Ok(Some(Vec::new()));
    }

    let need_old_value = args.set_type != StringSetType::None || args.get || args.keep_ttl;
    let old_raw_res = if need_old_value {
      Some(get_string_raw(self, key_bytes))
    } else {
      None
    };

    let (old_raw, old_is_wrong_type) = match old_raw_res {
      Some(Ok(v)) => (v, false),
      Some(Err(e)) => {
        if args.set_type == StringSetType::IfEq
          || args.set_type == StringSetType::IfNe
          || args.set_type == StringSetType::IfDeq
          || args.set_type == StringSetType::IfDne
          || args.get
        {
          return Err(e);
        }
        if args.set_type == StringSetType::Nx {
          return Ok(None);
        }
        (None, true)
      }
      None => (None, false),
    };

    let condition_met = match args.set_type {
      StringSetType::None => true,
      StringSetType::Nx => old_raw.is_none() && !old_is_wrong_type,
      StringSetType::Xx => old_raw.is_some() || old_is_wrong_type,
      StringSetType::IfEq => {
        if let Some(expected) = args.cmp_value
          && let Some((ref r, _, offset)) = old_raw
        {
          &r[offset..] == expected
        } else {
          false
        }
      }
      StringSetType::IfNe => {
        if let Some(expected) = args.cmp_value {
          if let Some((ref r, _, offset)) = old_raw {
            &r[offset..] != expected
          } else {
            true
          }
        } else {
          true
        }
      }
      StringSetType::IfDeq => {
        if let Some(expected) = args.cmp_value
          && let Some((ref r, _, offset)) = old_raw
        {
          string_digest_bytes(&r[offset..]).eq_ignore_ascii_case(expected)
        } else {
          false
        }
      }
      StringSetType::IfDne => {
        if let Some(expected) = args.cmp_value {
          if let Some((ref r, _, offset)) = old_raw {
            !string_digest_bytes(&r[offset..]).eq_ignore_ascii_case(expected)
          } else {
            true
          }
        } else {
          true
        }
      }
    };

    let old_val = if args.get {
      old_raw.as_ref().map(|(r, _, offset)| r[*offset..].to_vec())
    } else {
      None
    };

    if !condition_met {
      return Ok(old_val);
    }

    let expire = if args.keep_ttl {
      old_raw.as_ref().map(|(_, exp, _)| *exp).unwrap_or(0)
    } else {
      args.expire
    };

    let mut dyn_buf = Vec::new();
    with_encoded_string_value(val_bytes, expire, &mut dyn_buf, |enc_val| -> Result<()> {
      if old_is_wrong_type {
        let mut batch = self.batch();
        batch.insert_data(&raw_k, enc_val);
        cleanup_all_composite_data(self, key_bytes, &mut batch)?;
        batch.commit()?;
      } else {
        data_ks.insert(&raw_k, enc_val)?;
      }
      Ok(())
    })?;

    if args.get {
      Ok(old_val)
    } else {
      Ok(Some(Vec::new()))
    }
  }

  #[inline]
  pub fn set_with<'a, K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    args: &StringSet<'a>,
  ) -> Result<Option<Vec<u8>>> {
    self.set_internal(key, val, args)
  }

  #[inline]
  pub fn setex<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    expire_at_ms: u64,
  ) -> Result<()> {
    self.set(key, val, [Set::PxAt(expire_at_ms)])?;
    Ok(())
  }

  #[inline]
  pub fn setex_ttl<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    ttl_sec: u64,
  ) -> Result<()> {
    self.set(key, val, [Set::Ex(ttl_sec)])?;
    Ok(())
  }

  #[inline]
  pub fn psetex<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V, ttl_ms: u64) -> Result<()> {
    self.set(key, val, [Set::Px(ttl_ms)])?;
    Ok(())
  }

  #[inline]
  pub fn getex<K: AsRef<[u8]>>(
    &self,
    key: K,
    opt_li: impl IntoIterator<Item = GetEx>,
  ) -> Result<Option<Vec<u8>>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);
    let data_ks = self.data();

    let old_raw = match get_string_raw(self, key_bytes)? {
      Some(v) => v,
      None => return Ok(None),
    };

    let (raw, _, offset) = old_raw;
    let payload = &raw[offset..];
    let res_vec = payload.to_vec();

    let mut opt_iter = opt_li.into_iter();
    if let Some(opt_val) = opt_iter.next() {
      let now_ms = current_now_ms();
      let new_expire = opt_val.compute_expire(now_ms);
      let enc_val = encode_string_value(payload, new_expire);
      data_ks.insert(&raw_k, &enc_val)?;
    }

    Ok(Some(res_vec))
  }

  #[inline]
  pub fn delex<'a, K: AsRef<[u8]>>(
    &self,
    key: K,
    opt_li: impl IntoIterator<Item = DelEx<'a>>,
  ) -> Result<bool> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    let old_raw = match get_string_raw(self, key_bytes)? {
      Some(v) => v,
      None => return Ok(false),
    };

    let (raw, _, offset) = old_raw;
    let val_slice = &raw[offset..];

    for opt in opt_li {
      let matched = match opt {
        DelEx::None => true,
        DelEx::IfEq(expected) => val_slice == expected,
        DelEx::IfNe(expected) => val_slice != expected,
        DelEx::IfDeq(expected) => {
          if expected.len() != 16 || !expected.iter().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::invalid_data(ERR_DIGEST_INVALID_LEN));
          }
          string_digest_bytes(val_slice).eq_ignore_ascii_case(expected)
        }
        DelEx::IfDne(expected) => {
          if expected.len() != 16 || !expected.iter().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::invalid_data(ERR_DIGEST_INVALID_LEN));
          }
          !string_digest_bytes(val_slice).eq_ignore_ascii_case(expected)
        }
      };
      if !matched {
        return Ok(false);
      }
    }

    self.data().rm(&raw_k)?;
    Ok(true)
  }

  #[inline]
  pub fn getset<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<Option<Vec<u8>>> {
    self.set(key, val, [Set::Get])
  }

  #[inline]
  pub fn getdel<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let key_bytes = key.as_ref();
    if let Some((raw, _, offset)) = get_string_raw(self, key_bytes)? {
      let kc = self.kc();
      let raw_k = key::raw(&kc, key_bytes);
      let val = raw[offset..].to_vec();
      self.data().rm(&raw_k)?;
      Ok(Some(val))
    } else {
      Ok(None)
    }
  }

  #[inline]
  pub fn setnx<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<bool> {
    Ok(self.set(key, val, [Set::Nx])?.is_some())
  }

  #[inline]
  pub fn setnx_ex<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    expire_at_ms: u64,
  ) -> Result<bool> {
    if expire_at_ms > 0 {
      Ok(
        self
          .set(key, val, [Set::Nx, Set::PxAt(expire_at_ms)])?
          .is_some(),
      )
    } else {
      Ok(self.set(key, val, [Set::Nx])?.is_some())
    }
  }

  #[inline]
  pub fn setxx<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    val: V,
    expire_at_ms: u64,
  ) -> Result<bool> {
    if expire_at_ms > 0 {
      Ok(
        self
          .set(key, val, [Set::Xx, Set::PxAt(expire_at_ms)])?
          .is_some(),
      )
    } else {
      Ok(self.set(key, val, [Set::Xx])?.is_some())
    }
  }
}
