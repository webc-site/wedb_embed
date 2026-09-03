use crate::{
  api::string::{
    MAX_STRING_SIZE,
    r#const::ERR_STRING_EXCEEDS_MAX_SIZE,
    key,
    meta::{
      decode_live_string_value, decode_string_value, is_string_expired, with_encoded_string_value,
    },
    opt::{StringMSet, StringSetType},
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{check_composite_meta_not_other_type_with_buf, cleanup_all_composite_data_with_buf},
  meta::current_now_ms,
  wedb::Db,
};

/// Multi-key string operations (MGET, MSET, MSETNX, etc.).
/// 多键字符串操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
