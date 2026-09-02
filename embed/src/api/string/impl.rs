use crate::{
  IntoIndexRange,
  api::string::{
    MAX_STRING_SIZE, compute_lcs_with,
    r#const::{
      ERR_DIGEST_INVALID_LEN, ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW,
      ERR_OFFSET_OUT_OF_RANGE, ERR_STRING_EXCEEDS_MAX_SIZE,
    },
    format_float_bytes, get_string_raw, key,
    meta::{
      STRING_HDR_SIZE, decode_live_string_value, decode_string_value, encode_string_header,
      encode_string_value, is_string_expired, with_encoded_string_value,
    },
    opt::{DelEx, GetEx, Lcs, Set, StringLCSResult, StringMSet, StringSet, StringSetType},
    parse_redis_float, parse_redis_integer, string_digest, string_digest_bytes,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{
    check_composite_meta_not_other_type_with_buf, cleanup_all_composite_data,
    cleanup_all_composite_data_with_buf,
  },
  meta::{current_now_ms, normalize_range},
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
  pub fn with_get<K: AsRef<[u8]>, R>(&self, key: K, f: impl FnOnce(&[u8]) -> R) -> Result<Option<R>> {
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

    if (increment < 0 && cur_num <= 0 && increment < (i64::MIN - cur_num))
      || (increment > 0 && cur_num >= 0 && increment > (i64::MAX - cur_num))
    {
      return Err(Error::invalid_data(ERR_INCREMENT_OVERFLOW));
    }

    let new_num = cur_num
      .checked_add(increment)
      .ok_or_else(|| Error::invalid_data(ERR_INCREMENT_OVERFLOW))?;

    let mut itoa_buf = itoa::Buffer::new();
    let formatted_bytes = itoa_buf.format(new_num).as_bytes();

    let target_expire = if keep_ttl { old_expire } else { expire_ms };

    let enc_val = encode_string_value(formatted_bytes, target_expire);
    if old_raw.is_none() && !self.meta().is_empty()? {
      let mut batch = self.batch();
      batch.insert_data(&raw_k, &enc_val);
      cleanup_all_composite_data(self, key_bytes, &mut batch)?;
      batch.commit()?;
    } else {
      self.data().insert(&raw_k, &enc_val)?;
    }
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
    let enc_val = encode_string_value(formatted_bytes, target_expire);
    if old_raw.is_none() && !self.meta().is_empty()? {
      let mut batch = self.batch();
      batch.insert_data(&raw_k, &enc_val);
      cleanup_all_composite_data(self, key_bytes, &mut batch)?;
      batch.commit()?;
    } else {
      self.data().insert(&raw_k, &enc_val)?;
    }
    Ok(new_num)
  }

  #[inline]
  pub fn incrbyfloat<K: AsRef<[u8]>>(&self, key: K, increment: f64) -> Result<f64> {
    self.incrbyfloat_ex(key, increment, 0, true)
  }

  #[inline]
  pub fn strlen<K: AsRef<[u8]>>(&self, key: K) -> Result<usize> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => Ok(raw.len() - offset),
      None => Ok(0),
    }
  }

  #[inline]
  pub fn append<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<usize> {
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    let old_raw = get_string_raw(self, key_bytes)?;
    let (cur_len, cur_expire) = match old_raw {
      Some((ref raw, exp, offset)) => (raw.len() - offset, exp),
      None => (0, 0),
    };

    let new_len = cur_len
      .checked_add(val_bytes.len())
      .ok_or_else(|| Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE))?;
    if new_len > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    let mut enc_val = Vec::with_capacity(STRING_HDR_SIZE + new_len);
    enc_val.extend_from_slice(&encode_string_header(cur_expire));
    if let Some((ref raw, _, offset)) = old_raw {
      enc_val.extend_from_slice(&raw[offset..]);
    }
    enc_val.extend_from_slice(val_bytes);

    if old_raw.is_none() && !self.meta().is_empty()? {
      let mut batch = self.batch();
      batch.insert_data(&raw_k, &enc_val);
      cleanup_all_composite_data(self, key_bytes, &mut batch)?;
      batch.commit()?;
    } else {
      self.data().insert(&raw_k, &enc_val)?;
    }
    Ok(new_len)
  }

  #[inline]
  pub fn getrange<K: AsRef<[u8]>>(&self, key: K, range: impl IntoIndexRange) -> Result<Vec<u8>> {
    let (start, end) = range.into_index_range();
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => {
        let payload = &raw[offset..];
        let len = payload.len() as i64;
        let (s, e) = normalize_range(start, end, len);
        if s > e || payload.is_empty() {
          Ok(Vec::new())
        } else {
          Ok(payload[s as usize..=e as usize].to_vec())
        }
      }
      None => Ok(Vec::new()),
    }
  }

  #[inline]
  pub fn setrange<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    offset: usize,
    val: V,
  ) -> Result<usize> {
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    if offset > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_OFFSET_OUT_OF_RANGE));
    }

    let required_len = offset
      .checked_add(val_bytes.len())
      .ok_or_else(|| Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE))?;
    if required_len > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    let old_raw = get_string_raw(self, key_bytes)?;
    if old_raw.is_none() && val_bytes.is_empty() {
      return Ok(0);
    }
    let (old_len, cur_expire) = match old_raw {
      Some((ref raw, exp, off)) => (raw.len() - off, exp),
      None => (0, 0),
    };

    let new_total_len = old_len.max(required_len);
    let mut enc_val = Vec::with_capacity(STRING_HDR_SIZE + new_total_len);
    enc_val.extend_from_slice(&encode_string_header(cur_expire));

    if let Some((ref raw, _, off)) = old_raw {
      let old_payload = &raw[off..];
      if offset < old_len {
        enc_val.extend_from_slice(&old_payload[..offset]);
      } else {
        enc_val.extend_from_slice(old_payload);
        enc_val.resize(STRING_HDR_SIZE + offset, 0);
      }
    } else {
      enc_val.resize(STRING_HDR_SIZE + offset, 0);
    }

    enc_val.extend_from_slice(val_bytes);
    if old_len > required_len
      && let Some((ref raw, _, off)) = old_raw
    {
      let old_payload = &raw[off..];
      enc_val.extend_from_slice(&old_payload[required_len..]);
    }

    if old_raw.is_none() && !self.meta().is_empty()? {
      let mut batch = self.batch();
      batch.insert_data(&raw_k, &enc_val);
      cleanup_all_composite_data(self, key_bytes, &mut batch)?;
      batch.commit()?;
    } else {
      self.data().insert(&raw_k, &enc_val)?;
    }
    Ok(new_total_len)
  }

  #[inline]
  pub fn mget<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Vec<Option<Vec<u8>>>> {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let data_ks = self.data();
    let meta_ks = self.meta();
    let meta_is_empty = meta_ks.is_empty()?;
    let kc = self.kc();
    let now_ms = current_now_ms();

    let mut results = Vec::with_capacity(keys.len());
    let mut check_buf = if !meta_is_empty {
      Some(Vec::new())
    } else {
      None
    };

    for k in keys {
      let key_bytes = k.as_ref();
      let raw_k = key::raw(&kc, key_bytes);
      if let Some(raw) = data_ks.get(&raw_k)?
        && let Some(payload) = decode_live_string_value(&raw, now_ms)
      {
        results.push(Some(payload.to_vec()));
        continue;
      }

      if let Some(ref mut buf) = check_buf {
        match check_composite_meta_not_other_type_with_buf::<E>(
          meta_ks, &kc, key_bytes, b"", now_ms, buf,
        ) {
          Ok(()) => results.push(None),
          Err(ref e) if e.is_wrong_type() => results.push(None),
          Err(e) => return Err(e),
        }
      } else {
        results.push(None);
      }
    }
    Ok(results)
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

  #[inline]
  pub fn msetex<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    kvs: &[(K, V)],
    expire_at_ms: u64,
  ) -> Result<()> {
    self.mset_with(
      kvs,
      StringMSet {
        expire: expire_at_ms,
        set_type: StringSetType::None,
        keep_ttl: false,
      },
    )?;
    Ok(())
  }

  #[inline]
  pub fn mset<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, kvs: &[(K, V)]) -> Result<()> {
    let args = StringMSet::default();
    self.mset_internal(kvs, args)?;
    Ok(())
  }

  #[inline]
  pub fn msetnx<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, kvs: &[(K, V)]) -> Result<bool> {
    let args = StringMSet {
      set_type: StringSetType::Nx,
      ..Default::default()
    };
    self.mset_internal(kvs, args)
  }

  #[inline]
  pub fn msetxx<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, kvs: &[(K, V)]) -> Result<bool> {
    let args = StringMSet {
      set_type: StringSetType::Xx,
      ..Default::default()
    };
    self.mset_internal(kvs, args)
  }

  #[inline]
  pub fn mset_with<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    kvs: &[(K, V)],
    args: StringMSet,
  ) -> Result<bool> {
    self.mset_internal(kvs, args)
  }

  #[inline]
  fn mset_internal<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    kvs: &[(K, V)],
    args: StringMSet,
  ) -> Result<bool> {
    if kvs.is_empty() {
      return Ok(true);
    }

    // 1. 处理 NX / XX 条件判断与 KEEPTTL 单遍预提取（单次遍历优化）
    let mut expires = if args.keep_ttl {
      Vec::with_capacity(kvs.len())
    } else {
      Vec::new()
    };

    if args.set_type != StringSetType::None || args.keep_ttl {
      let data_ks = self.data();
      let meta_ks = self.meta();
      let meta_is_empty = meta_ks.is_empty()?;
      let kc = self.kc();
      let now_ms = current_now_ms();
      let mut check_buf = if !meta_is_empty {
        Some(Vec::new())
      } else {
        None
      };

      for (k, _) in kvs {
        let key_bytes = k.as_ref();
        let raw_k = key::raw(&kc, key_bytes);
        let raw_opt = data_ks.get(&raw_k)?;

        let (is_live, live_expire) = match raw_opt {
          Some(ref raw) => {
            let (exp, _) = decode_string_value(raw);
            if !is_string_expired(exp, now_ms) {
              (true, exp)
            } else {
              (false, 0)
            }
          }
          None => (false, 0),
        };

        if is_live {
          if args.set_type == StringSetType::Nx {
            return Ok(false);
          }
          if args.keep_ttl {
            expires.push(live_expire);
          }
        } else {
          // 键不存在或已过期
          if args.set_type == StringSetType::Xx {
            return Ok(false);
          }

          if let Some(ref mut buf) = check_buf {
            match check_composite_meta_not_other_type_with_buf::<E>(
              meta_ks, &kc, key_bytes, b"", now_ms, buf,
            ) {
              Ok(()) => {}
              Err(ref e) if e.is_wrong_type() => {
                if args.set_type == StringSetType::Nx {
                  return Ok(false);
                }
              }
              Err(e) => return Err(e),
            }
          }

          if args.keep_ttl {
            expires.push(0);
          }
        }
      }
    }

    let meta_ks = self.meta();
    let meta_is_empty = meta_ks.is_empty()?;
    let mut batch = self.batch_with_capacity(kvs.len());
    let kc = self.kc();

    // 3. Fast Path：常规无 TTL 保留且无复合元数据的极速单次遍历写入
    if !args.keep_ttl && meta_is_empty && args.expire == 0 {
      let mut dyn_buf = Vec::new();
      for (k, v) in kvs {
        let v_bytes = v.as_ref();
        if v_bytes.len() > MAX_STRING_SIZE {
          return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
        }
        let raw_k = key::raw(&kc, k.as_ref());
        with_encoded_string_value(v_bytes, 0, &mut dyn_buf, |enc_val| {
          batch.insert_data(&raw_k, enc_val);
        });
      }
      batch.commit()?;
      return Ok(true);
    }

    // 4. 标准/带参数通用路径：单次遍历 + 缓冲区复用
    let mut dyn_buf = Vec::new();
    let mut clean_buf = if !meta_is_empty {
      Some(Vec::new())
    } else {
      None
    };

    for (i, (k, v)) in kvs.iter().enumerate() {
      let k_bytes = k.as_ref();
      let v_bytes = v.as_ref();
      if v_bytes.len() > MAX_STRING_SIZE {
        return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
      }

      let raw_k = key::raw(&kc, k_bytes);
      let expire = if args.keep_ttl {
        expires[i]
      } else {
        args.expire
      };

      with_encoded_string_value(v_bytes, expire, &mut dyn_buf, |enc_val| {
        batch.insert_data(&raw_k, enc_val);
      });

      if let Some(ref mut c_buf) = clean_buf {
        cleanup_all_composite_data_with_buf(self, k_bytes, &mut batch, c_buf)?;
      }
    }

    batch.commit()?;
    Ok(true)
  }

  #[inline]
  pub fn cas<K: AsRef<[u8]>, V1: AsRef<[u8]>, V2: AsRef<[u8]>>(
    &self,
    key: K,
    old_val: V1,
    new_val: V2,
    expire_ms: u64,
  ) -> Result<i32> {
    let key_bytes = key.as_ref();
    let old_bytes = old_val.as_ref();
    let new_bytes = new_val.as_ref();

    if new_bytes.len() > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);
    let data_ks = self.data();

    match get_string_raw(self, key_bytes)? {
      Some((raw, _, offset)) => {
        if &raw[offset..] == old_bytes {
          let enc_val = encode_string_value(new_bytes, expire_ms);
          data_ks.insert(&raw_k, &enc_val)?;
          Ok(1)
        } else {
          Ok(0)
        }
      }
      None => Ok(-1),
    }
  }

  #[inline]
  pub fn cad<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<i32> {
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    match get_string_raw(self, key_bytes)? {
      Some((raw, _, offset)) => {
        if &raw[offset..] == val_bytes {
          self.data().rm(&raw_k)?;
          Ok(1)
        } else {
          Ok(0)
        }
      }
      None => Ok(-1),
    }
  }

  #[inline]
  pub fn digest<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<String>> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => Ok(Some(string_digest(&raw[offset..]))),
      None => Ok(None),
    }
  }

  #[inline]
  pub fn lcs<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(
    &self,
    key1: K1,
    key2: K2,
    opt_li: impl IntoIterator<Item = Lcs>,
  ) -> Result<StringLCSResult> {
    let args = Lcs::parse_options(opt_li);
    let raw1 = get_string_raw(self, key1.as_ref())?;
    let raw2 = get_string_raw(self, key2.as_ref())?;

    let empty = &[][..];
    let s1 = raw1.as_ref().map(|(r, _, o)| &r[*o..]).unwrap_or(empty);
    let s2 = raw2.as_ref().map(|(r, _, o)| &r[*o..]).unwrap_or(empty);

    compute_lcs_with(s1, s2, args)
  }
}
