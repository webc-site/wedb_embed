use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::{
    hash::{
      CachedFieldState, ceil_div_1000,
      r#const::{
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING, HASH_EXPIRE_COND_FAILED, HASH_EXPIRE_DELETED,
        HASH_EXPIRE_SET_OK, HASH_FIELD_NOT_FOUND, HASH_FIELD_PERSISTENT,
      },
      key,
      meta::{
        HashFieldState, HashFieldStateKind, HashItemKeyComposer, HashMeta, compose_hash_meta_key,
        decode_field_state, hexpire_condition_passes, is_immediate_expire,
      },
      opt::{HExpire, HGetEx, HSet, HashFieldSetCondition, HashGetEx, HashSetEx, TTLAction},
      prepare_hash_meta_for_write,
    },
    key::get_meta_checked,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

#[inline]
fn load_field_state<P: Partition>(
  data_ks: &P,
  meta: &HashMeta,
  item_k: &[u8],
  now_ms: u64,
) -> Result<CachedFieldState>
where
  Error: From<P::Error>,
{
  match data_ks.get(item_k)? {
    Some(raw) => {
      if let Some(state) = decode_field_state(meta, &raw, now_ms) {
        Ok(CachedFieldState {
          kind: state.kind,
          expire: state.expire,
          raw: Some(Box::from(&*raw)),
        })
      } else {
        Ok(CachedFieldState {
          kind: HashFieldStateKind::Missing,
          expire: 0,
          raw: None,
        })
      }
    }
    None => Ok(CachedFieldState {
      kind: HashFieldStateKind::Missing,
      expire: 0,
      raw: None,
    }),
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn hexpire<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    seconds: i64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<Vec<i64>> {
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    let target_expire_ms = if seconds <= 0 {
      0
    } else {
      now_ms.saturating_add((seconds as u64).saturating_mul(1000))
    };
    self.expire_fields(key, fields, target_expire_ms, condition, now_ms)
  }

  #[inline]
  pub fn hexpire_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    seconds: i64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<i64> {
    let res = self.hexpire(key, &[field], seconds, opt_li)?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub fn hpexpire<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    milliseconds: i64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<Vec<i64>> {
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    let target_expire_ms = if milliseconds <= 0 {
      0
    } else {
      now_ms.saturating_add(milliseconds as u64)
    };
    self.expire_fields(key, fields, target_expire_ms, condition, now_ms)
  }

  #[inline]
  pub fn hpexpire_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    milliseconds: i64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<i64> {
    let res = self.hpexpire(key, &[field], milliseconds, opt_li)?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub fn hexpireat<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    unix_time_sec: u64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<Vec<i64>> {
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    let target_expire_ms = unix_time_sec.saturating_mul(1000);
    self.expire_fields(key, fields, target_expire_ms, condition, now_ms)
  }

  #[inline]
  pub fn hexpireat_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    unix_time_sec: u64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<i64> {
    let res = self.hexpireat(key, &[field], unix_time_sec, opt_li)?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub fn hpexpireat<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    unix_time_ms: u64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<Vec<i64>> {
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    self.expire_fields(key, fields, unix_time_ms, condition, now_ms)
  }

  #[inline]
  pub fn hpexpireat_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    unix_time_ms: u64,
    opt_li: impl IntoIterator<Item = HExpire>,
  ) -> Result<i64> {
    let res = self.hpexpireat(key, &[field], unix_time_ms, opt_li)?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub(crate) fn expire_fields<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    expire_at_ms: u64,
    condition: HExpire,
    now_ms: u64,
  ) -> Result<Vec<i64>> {
    if fields.is_empty() {
      return Ok(Vec::new());
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);

    let mut meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]),
    };

    if meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    let is_immediate = is_immediate_expire(expire_at_ms, now_ms);

    if fields.len() == 1 {
      let f_bytes = fields[0].as_ref();
      let item_k = key::field(&kc, key_bytes, f_bytes);
      let data_ks = self.data();
      let raw_opt = data_ks.get(item_k.as_slice())?;
      let (state_kind, state_expire, payload) = match raw_opt.as_ref() {
        None => (HashFieldStateKind::Missing, 0, &[][..]),
        Some(raw) => match decode_field_state(&meta, raw, now_ms) {
          None => (HashFieldStateKind::Missing, 0, &[][..]),
          Some(s) => {
            let p = meta.decode_subkey_value(raw).map(|(_, p)| p).unwrap_or(b"");
            (s.kind, s.expire, p)
          }
        },
      };

      match state_kind {
        HashFieldStateKind::Missing => return Ok(vec![HASH_FIELD_NOT_FOUND]),
        HashFieldStateKind::ExpiredTTLPhysical => {
          let mut batch = self.batch();
          batch.rm_data(item_k.as_slice());
          meta.apply_ttl_to_deleted();
          if meta.base.size == 0 {
            batch.rm_meta(&meta_k);
          } else {
            batch.insert_meta(&meta_k, &meta.encode());
          }
          batch.commit()?;
          return Ok(vec![HASH_FIELD_NOT_FOUND]);
        }
        HashFieldStateKind::Persistent | HashFieldStateKind::LiveTTL => {
          if !hexpire_condition_passes(condition, state_kind, state_expire, expire_at_ms) {
            return Ok(vec![HASH_EXPIRE_COND_FAILED]);
          }
          let mut batch = self.batch();
          if is_immediate {
            batch.rm_data(item_k.as_slice());
            if state_kind == HashFieldStateKind::Persistent {
              meta.apply_persistent_to_deleted();
            } else {
              meta.apply_ttl_to_deleted();
            }
            if meta.base.size == 0 {
              batch.rm_meta(&meta_k);
            } else {
              batch.insert_meta(&meta_k, &meta.encode());
            }
            batch.commit()?;
            return Ok(vec![HASH_EXPIRE_DELETED]);
          } else {
            if state_kind == HashFieldStateKind::Persistent {
              meta.apply_persistent_to_ttl(expire_at_ms);
            } else {
              meta.apply_ttl_to_ttl(expire_at_ms);
            }
            meta.with_encoded_subkey_value(payload, expire_at_ms, |enc| {
              batch.insert_data(item_k.as_slice(), enc)
            });
            batch.insert_meta(&meta_k, &meta.encode());
            batch.commit()?;
            return Ok(vec![HASH_EXPIRE_SET_OK]);
          }
        }
      }
    }

    let mut results = Vec::with_capacity(fields.len());
    let mut batch = self.batch();
    let mut meta_changed = false;
    let mut state_cache: HashMap<&[u8], CachedFieldState> = HashMap::with_capacity(fields.len());
    let data_ks = self.data();
    let _meta_ks = self.meta();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);

    for f in fields {
      let f_bytes = f.as_ref();
      let item_k = composer.key_for_field(f_bytes);

      let entry = if let Some(cached) = state_cache.get(f_bytes) {
        cached.clone()
      } else {
        let state_entry = load_field_state(data_ks, &meta, item_k, now_ms)?;
        state_cache.insert(f_bytes, state_entry.clone());
        state_entry
      };

      match entry.kind {
        HashFieldStateKind::Missing => {
          results.push(HASH_FIELD_NOT_FOUND);
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          batch.rm_data(item_k);
          meta.apply_ttl_to_deleted();
          meta_changed = true;
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Missing,
              expire: 0,
              raw: None,
            },
          );
          results.push(HASH_FIELD_NOT_FOUND);
        }
        HashFieldStateKind::Persistent | HashFieldStateKind::LiveTTL => {
          if !hexpire_condition_passes(condition, entry.kind, entry.expire, expire_at_ms) {
            results.push(HASH_EXPIRE_COND_FAILED);
            continue;
          }

          if is_immediate {
            batch.rm_data(item_k);
            if entry.kind == HashFieldStateKind::Persistent {
              meta.apply_persistent_to_deleted();
            } else {
              meta.apply_ttl_to_deleted();
            }
            meta_changed = true;
            state_cache.insert(
              f_bytes,
              CachedFieldState {
                kind: HashFieldStateKind::Missing,
                expire: 0,
                raw: None,
              },
            );
            results.push(HASH_EXPIRE_DELETED);
          } else {
            if entry.kind == HashFieldStateKind::Persistent {
              meta.apply_persistent_to_ttl(expire_at_ms);
            } else {
              meta.apply_ttl_to_ttl(expire_at_ms);
            }
            meta_changed = true;
            let payload = entry
              .raw
              .as_ref()
              .and_then(|s| meta.decode_subkey_value(s))
              .map(|(_, p)| p)
              .unwrap_or(b"");
            meta.with_encoded_subkey_value(payload, expire_at_ms, |enc| {
              batch.insert_data(item_k, enc)
            });
            state_cache.insert(
              f_bytes,
              CachedFieldState {
                kind: HashFieldStateKind::LiveTTL,
                expire: expire_at_ms,
                raw: entry.raw,
              },
            );
            results.push(HASH_EXPIRE_SET_OK);
          }
        }
      }
    }

    if meta_changed {
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(results)
  }

  #[inline]
  fn query_field_expire_info_one<K: AsRef<[u8]>, F: AsRef<[u8]>, M: Fn(u64, u64) -> i64>(
    &self,
    key: K,
    field: F,
    map_live_ttl: M,
  ) -> Result<i64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(HASH_FIELD_NOT_FOUND),
    };

    if meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    let f_bytes = field.as_ref();
    let item_k = key::field(&kc, key_bytes, f_bytes);
    let data_ks = self.data();
    match data_ks.get(item_k.as_slice())? {
      None => Ok(HASH_FIELD_NOT_FOUND),
      Some(raw) => match decode_field_state(&meta, &raw, now_ms) {
        None => Ok(HASH_FIELD_NOT_FOUND),
        Some(s) => match s.kind {
          HashFieldStateKind::Missing | HashFieldStateKind::ExpiredTTLPhysical => {
            Ok(HASH_FIELD_NOT_FOUND)
          }
          HashFieldStateKind::Persistent => Ok(HASH_FIELD_PERSISTENT),
          HashFieldStateKind::LiveTTL => Ok(map_live_ttl(s.expire, now_ms)),
        },
      },
    }
  }

  #[inline]
  fn query_field_expire_info<K: AsRef<[u8]>, F: AsRef<[u8]>, M: Fn(u64, u64) -> i64>(
    &self,
    key: K,
    fields: &[F],
    map_live_ttl: M,
  ) -> Result<Vec<i64>> {
    if fields.len() == 1 {
      return Ok(vec![self.query_field_expire_info_one(
        key,
        &fields[0],
        map_live_ttl,
      )?]);
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]),
    };

    if meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    let mut result_cache: HashMap<&[u8], i64> = HashMap::with_capacity(fields.len());
    let mut results = Vec::with_capacity(fields.len());
    let data_ks = self.data();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);

    for f in fields {
      let f_bytes = f.as_ref();
      if let Some(&cached) = result_cache.get(f_bytes) {
        results.push(cached);
        continue;
      }

      let item_k = composer.key_for_field(f_bytes);
      let res = match data_ks.get(item_k)? {
        None => HASH_FIELD_NOT_FOUND,
        Some(raw) => match decode_field_state(&meta, &raw, now_ms) {
          None => HASH_FIELD_NOT_FOUND,
          Some(s) => match s.kind {
            HashFieldStateKind::Missing | HashFieldStateKind::ExpiredTTLPhysical => {
              HASH_FIELD_NOT_FOUND
            }
            HashFieldStateKind::Persistent => HASH_FIELD_PERSISTENT,
            HashFieldStateKind::LiveTTL => map_live_ttl(s.expire, now_ms),
          },
        },
      };
      result_cache.insert(f_bytes, res);
      results.push(res);
    }

    Ok(results)
  }

  #[inline]
  pub fn httl<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, fields: &[F]) -> Result<Vec<i64>> {
    self.query_field_expire_info(key, fields, |expire, now_ms| {
      let remain_ms = expire.saturating_sub(now_ms);
      ceil_div_1000(remain_ms) as i64
    })
  }

  #[inline]
  pub fn httl_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    self.query_field_expire_info_one(key, field, |expire, now_ms| {
      let remain_ms = expire.saturating_sub(now_ms);
      ceil_div_1000(remain_ms) as i64
    })
  }

  #[inline]
  pub fn hpttl_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    self.query_field_expire_info_one(key, field, |expire, now_ms| {
      expire.saturating_sub(now_ms) as i64
    })
  }

  #[inline]
  pub fn hpttl<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, fields: &[F]) -> Result<Vec<i64>> {
    self.query_field_expire_info(key, fields, |expire, now_ms| {
      expire.saturating_sub(now_ms) as i64
    })
  }

  #[inline]
  pub fn hexpiretime_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    self.query_field_expire_info_one(key, field, |expire, _| (expire / 1000) as i64)
  }

  #[inline]
  pub fn hexpiretime<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
  ) -> Result<Vec<i64>> {
    self.query_field_expire_info(key, fields, |expire, _| (expire / 1000) as i64)
  }

  #[inline]
  pub fn hpexpiretime_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    self.query_field_expire_info_one(key, field, |expire, _| expire as i64)
  }

  #[inline]
  pub fn hpexpiretime<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
  ) -> Result<Vec<i64>> {
    self.query_field_expire_info(key, fields, |expire, _| expire as i64)
  }

  #[inline]
  pub fn hpersist_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    let res = self.hpersist(key, &[field])?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub fn hpersist<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, fields: &[F]) -> Result<Vec<i64>> {
    if fields.is_empty() {
      return Ok(Vec::new());
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]),
    };

    if meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    if fields.len() == 1 {
      let f_bytes = fields[0].as_ref();
      let item_k = key::field(&kc, key_bytes, f_bytes);
      let data_ks = self.data();
      let raw_opt = data_ks.get(item_k.as_slice())?;
      let (state_kind, payload) = match raw_opt.as_ref() {
        None => (HashFieldStateKind::Missing, &[][..]),
        Some(raw) => match decode_field_state(&meta, raw, now_ms) {
          None => (HashFieldStateKind::Missing, &[][..]),
          Some(s) => {
            let p = meta.decode_subkey_value(raw).map(|(_, p)| p).unwrap_or(b"");
            (s.kind, p)
          }
        },
      };

      match state_kind {
        HashFieldStateKind::Missing => return Ok(vec![HASH_FIELD_NOT_FOUND]),
        HashFieldStateKind::Persistent => return Ok(vec![HASH_FIELD_PERSISTENT]),
        HashFieldStateKind::ExpiredTTLPhysical => {
          let mut batch = self.batch();
          batch.rm_data(item_k.as_slice());
          meta.apply_ttl_to_deleted();
          if meta.base.size == 0 {
            batch.rm_meta(&meta_k);
          } else {
            batch.insert_meta(&meta_k, &meta.encode());
          }
          batch.commit()?;
          return Ok(vec![HASH_FIELD_NOT_FOUND]);
        }
        HashFieldStateKind::LiveTTL => {
          let mut batch = self.batch();
          meta.apply_ttl_to_persistent();
          meta
            .with_encoded_subkey_value(payload, 0, |enc| batch.insert_data(item_k.as_slice(), enc));
          batch.insert_meta(&meta_k, &meta.encode());
          batch.commit()?;
          return Ok(vec![HASH_EXPIRE_SET_OK]);
        }
      }
    }

    let mut results = Vec::with_capacity(fields.len());
    let mut batch = self.batch();
    let mut meta_changed = false;
    let mut state_cache: HashMap<&[u8], CachedFieldState> = HashMap::with_capacity(fields.len());
    let data_ks = self.data();
    let _meta_ks = self.meta();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);

    for f in fields {
      let f_bytes = f.as_ref();
      let item_k = composer.key_for_field(f_bytes);

      let entry = if let Some(cached) = state_cache.get(f_bytes) {
        cached.clone()
      } else {
        let state_entry = load_field_state(data_ks, &meta, item_k, now_ms)?;
        state_cache.insert(f_bytes, state_entry.clone());
        state_entry
      };

      match entry.kind {
        HashFieldStateKind::Missing => {
          results.push(HASH_FIELD_NOT_FOUND);
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          batch.rm_data(item_k);
          meta.apply_ttl_to_deleted();
          meta_changed = true;
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Missing,
              expire: 0,
              raw: None,
            },
          );
          results.push(HASH_FIELD_NOT_FOUND);
        }
        HashFieldStateKind::Persistent => {
          results.push(HASH_FIELD_PERSISTENT);
        }
        HashFieldStateKind::LiveTTL => {
          meta.apply_ttl_to_persistent();
          meta_changed = true;
          let payload = entry
            .raw
            .as_ref()
            .and_then(|s| meta.decode_subkey_value(s))
            .map(|(_, p)| p)
            .unwrap_or(b"");
          meta.with_encoded_subkey_value(payload, 0, |enc| batch.insert_data(item_k, enc));
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Persistent,
              expire: 0,
              raw: entry.raw,
            },
          );
          results.push(HASH_EXPIRE_SET_OK);
        }
      }
    }

    if meta_changed {
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(results)
  }

  #[inline]
  pub fn hgetdel_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
  ) -> Result<Option<Vec<u8>>> {
    let res = self.hgetdel(key, &[field])?;
    Ok(res.into_iter().next().flatten())
  }

  #[inline]
  pub fn hgetdel<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
  ) -> Result<Vec<Option<Vec<u8>>>> {
    if fields.is_empty() {
      return Ok(Vec::new());
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![None; fields.len()]),
    };

    if meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    if fields.len() == 1 {
      let f_bytes = fields[0].as_ref();
      let item_k = key::field(&kc, key_bytes, f_bytes);
      let data_ks = self.data();
      let raw_opt = data_ks.get(item_k.as_slice())?;
      let (state_kind, payload) = match raw_opt.as_ref() {
        None => (HashFieldStateKind::Missing, None),
        Some(raw) => match decode_field_state(&meta, raw, now_ms) {
          None => (HashFieldStateKind::Missing, None),
          Some(s) => {
            let p = meta.decode_subkey_value(raw).map(|(_, p)| p.to_vec());
            (s.kind, p)
          }
        },
      };

      match state_kind {
        HashFieldStateKind::Missing => return Ok(vec![None]),
        HashFieldStateKind::ExpiredTTLPhysical => {
          let mut batch = self.batch();
          batch.rm_data(item_k.as_slice());
          meta.apply_ttl_to_deleted();
          if meta.base.size == 0 {
            batch.rm_meta(&meta_k);
          } else {
            batch.insert_meta(&meta_k, &meta.encode());
          }
          batch.commit()?;
          return Ok(vec![None]);
        }
        HashFieldStateKind::Persistent => {
          let mut batch = self.batch();
          batch.rm_data(item_k.as_slice());
          meta.apply_persistent_to_deleted();
          if meta.base.size == 0 {
            batch.rm_meta(&meta_k);
          } else {
            batch.insert_meta(&meta_k, &meta.encode());
          }
          batch.commit()?;
          return Ok(vec![payload]);
        }
        HashFieldStateKind::LiveTTL => {
          let mut batch = self.batch();
          batch.rm_data(item_k.as_slice());
          meta.apply_ttl_to_deleted();
          if meta.base.size == 0 {
            batch.rm_meta(&meta_k);
          } else {
            batch.insert_meta(&meta_k, &meta.encode());
          }
          batch.commit()?;
          return Ok(vec![payload]);
        }
      }
    }

    let mut results = Vec::with_capacity(fields.len());
    let mut batch = self.batch();
    let mut meta_changed = false;
    let mut state_cache: HashMap<&[u8], CachedFieldState> = HashMap::with_capacity(fields.len());
    let data_ks = self.data();
    let _meta_ks = self.meta();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);

    for f in fields {
      let f_bytes = f.as_ref();
      let item_k = composer.key_for_field(f_bytes);

      let entry = if let Some(cached) = state_cache.get(f_bytes) {
        cached.clone()
      } else {
        let state_entry = load_field_state(data_ks, &meta, item_k, now_ms)?;
        state_cache.insert(f_bytes, state_entry.clone());
        state_entry
      };

      match entry.kind {
        HashFieldStateKind::Missing => {
          results.push(None);
        }
        HashFieldStateKind::Persistent => {
          let payload = entry
            .raw
            .as_ref()
            .and_then(|s| meta.decode_subkey_value(s))
            .map(|(_, p)| p.to_vec())
            .unwrap_or_default();
          results.push(Some(payload));
          batch.rm_data(item_k);
          meta.apply_persistent_to_deleted();
          meta_changed = true;
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Missing,
              expire: 0,
              raw: None,
            },
          );
        }
        HashFieldStateKind::LiveTTL => {
          let payload = entry
            .raw
            .as_ref()
            .and_then(|s| meta.decode_subkey_value(s))
            .map(|(_, p)| p.to_vec())
            .unwrap_or_default();
          results.push(Some(payload));
          batch.rm_data(item_k);
          meta.apply_ttl_to_deleted();
          meta_changed = true;
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Missing,
              expire: 0,
              raw: None,
            },
          );
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          batch.rm_data(item_k);
          meta.apply_ttl_to_deleted();
          meta_changed = true;
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Missing,
              expire: 0,
              raw: None,
            },
          );
          results.push(None);
        }
      }
    }

    if meta_changed {
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(results)
  }

  #[inline]
  pub fn hsetex_one<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    val: V,
    opt_li: impl IntoIterator<Item = HSet>,
  ) -> Result<bool> {
    self.hsetex(key, &[(field, val)], opt_li)
  }

  #[inline]
  pub fn hsetex<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    field_values: &[(F, V)],
    opt_li: impl IntoIterator<Item = HSet>,
  ) -> Result<bool> {
    let now_ms = current_now_ms();
    let opts = HashSetEx::from_options(opt_li, now_ms);
    self.set_fields_with_expire(key, field_values, opts)
  }

  #[inline]
  pub(crate) fn set_fields_with_expire<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    field_values: &[(F, V)],
    options: HashSetEx,
  ) -> Result<bool> {
    if field_values.is_empty() {
      return Ok(false);
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut batch = self.batch();
    let (mut meta, metadata_existed) =
      prepare_hash_meta_for_write(self, key_bytes, &meta_k, now_ms, &mut batch)?;

    if metadata_existed && meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    if field_values.len() == 1 {
      let (f, v) = (&field_values[0].0, &field_values[0].1);
      let f_bytes = f.as_ref();
      let v_bytes = v.as_ref();
      let item_k = key::field(&kc, key_bytes, f_bytes);
      let data_ks = self.data();
      let raw_opt = if metadata_existed {
        data_ks.get(item_k.as_slice())?
      } else {
        None
      };

      let state = match raw_opt.as_ref() {
        None => HashFieldState {
          kind: HashFieldStateKind::Missing,
          expire: 0,
          value: b"",
        },
        Some(raw) => decode_field_state(&meta, raw, now_ms).unwrap_or(HashFieldState {
          kind: HashFieldStateKind::Missing,
          expire: 0,
          value: b"",
        }),
      };

      let condition_met = match options.condition {
        HashFieldSetCondition::None => true,
        HashFieldSetCondition::Fnx => {
          state.kind == HashFieldStateKind::Missing
            || state.kind == HashFieldStateKind::ExpiredTTLPhysical
        }
        HashFieldSetCondition::Fxx => {
          state.kind == HashFieldStateKind::Persistent || state.kind == HashFieldStateKind::LiveTTL
        }
      };

      if !condition_met {
        return Ok(false);
      }

      let is_immediate =
        options.ttl_action == TTLAction::Set && is_immediate_expire(options.expire_at_ms, now_ms);

      if is_immediate {
        match state.kind {
          HashFieldStateKind::Missing => {}
          HashFieldStateKind::ExpiredTTLPhysical => {
            batch.rm_data(item_k.as_slice());
            meta.apply_ttl_to_deleted();
          }
          HashFieldStateKind::Persistent => {
            batch.rm_data(item_k.as_slice());
            meta.apply_persistent_to_deleted();
          }
          HashFieldStateKind::LiveTTL => {
            batch.rm_data(item_k.as_slice());
            meta.apply_ttl_to_deleted();
          }
        }
      } else {
        let target_expire = match options.ttl_action {
          TTLAction::Discard | TTLAction::Persist => 0,
          TTLAction::Keep => {
            if state.kind == HashFieldStateKind::LiveTTL
              || state.kind == HashFieldStateKind::ExpiredTTLPhysical
            {
              state.expire
            } else {
              0
            }
          }
          TTLAction::Set => options.expire_at_ms,
        };

        match state.kind {
          HashFieldStateKind::Missing => {
            if target_expire == 0 {
              meta.apply_missing_to_persistent();
            } else {
              meta.apply_missing_to_ttl(target_expire);
            }
          }
          HashFieldStateKind::Persistent => {
            if target_expire != 0 {
              meta.apply_persistent_to_ttl(target_expire);
            }
          }
          HashFieldStateKind::ExpiredTTLPhysical => {
            meta.apply_ttl_to_deleted();
            if target_expire == 0 {
              meta.apply_missing_to_persistent();
            } else {
              meta.apply_missing_to_ttl(target_expire);
            }
          }
          HashFieldStateKind::LiveTTL => {
            if target_expire == 0 {
              meta.apply_ttl_to_persistent();
            } else {
              meta.apply_ttl_to_ttl(target_expire);
            }
          }
        }
        meta.with_encoded_subkey_value(v_bytes, target_expire, |enc| {
          batch.insert_data(item_k.as_slice(), enc)
        });
      }

      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
      return Ok(true);
    }

    let mut state_cache: HashMap<&[u8], CachedFieldState> =
      HashMap::with_capacity(field_values.len());
    let data_ks = self.data();
    let _meta_ks = self.meta();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let meta_changed = false;

    // 1. 先行校验 Fnx / Fxx 前置条件
    if options.condition != HashFieldSetCondition::None {
      for (f, _) in field_values {
        let f_bytes = f.as_ref();
        let item_k = composer.key_for_field(f_bytes);
        let state_entry = if metadata_existed {
          load_field_state(data_ks, &meta, item_k, now_ms)?
        } else {
          CachedFieldState {
            kind: HashFieldStateKind::Missing,
            expire: 0,
            raw: None,
          }
        };
        state_cache.insert(f_bytes, state_entry.clone());

        let condition_met = match options.condition {
          HashFieldSetCondition::None => true,
          HashFieldSetCondition::Fnx => {
            state_entry.kind == HashFieldStateKind::Missing
              || state_entry.kind == HashFieldStateKind::ExpiredTTLPhysical
          }
          HashFieldSetCondition::Fxx => {
            state_entry.kind == HashFieldStateKind::Persistent
              || state_entry.kind == HashFieldStateKind::LiveTTL
          }
        };

        if !condition_met {
          // 清理物理过期的脏数据
          if meta_changed {
            if meta.base.size == 0 {
              batch.rm_meta(&meta_k);
            } else {
              batch.insert_meta(&meta_k, &meta.encode());
            }
            batch.commit()?;
          }
          return Ok(false);
        }
      }
    }

    // 2. 去重并逆序保留最新值
    let mut seen = HashSet::with_capacity(field_values.len());
    let mut unique_field_values = Vec::with_capacity(field_values.len());
    for (f, v) in field_values.iter().rev() {
      let f_bytes = f.as_ref();
      if seen.insert(f_bytes) {
        unique_field_values.push((f_bytes, v.as_ref()));
      }
    }
    unique_field_values.reverse();

    let is_immediate =
      options.ttl_action == TTLAction::Set && is_immediate_expire(options.expire_at_ms, now_ms);

    for (f_bytes, v_bytes) in unique_field_values {
      let item_k = composer.key_for_field(f_bytes);

      let entry = if let Some(cached) = state_cache.get(f_bytes) {
        cached.clone()
      } else if metadata_existed {
        let state_entry = load_field_state(data_ks, &meta, item_k, now_ms)?;
        state_cache.insert(f_bytes, state_entry.clone());
        state_entry
      } else {
        CachedFieldState {
          kind: HashFieldStateKind::Missing,
          expire: 0,
          raw: None,
        }
      };

      if is_immediate {
        match entry.kind {
          HashFieldStateKind::Missing => continue,
          HashFieldStateKind::Persistent => {
            meta.apply_persistent_to_deleted();
          }
          HashFieldStateKind::LiveTTL | HashFieldStateKind::ExpiredTTLPhysical => {
            meta.apply_ttl_to_deleted();
          }
        }
        batch.rm_data(item_k);
        state_cache.insert(
          f_bytes,
          CachedFieldState {
            kind: HashFieldStateKind::Missing,
            expire: 0,
            raw: None,
          },
        );
        continue;
      }

      let target_expire = match options.ttl_action {
        TTLAction::Discard | TTLAction::Persist => 0,
        TTLAction::Keep => {
          if entry.kind == HashFieldStateKind::LiveTTL
            || entry.kind == HashFieldStateKind::ExpiredTTLPhysical
          {
            entry.expire
          } else {
            0
          }
        }
        TTLAction::Set => options.expire_at_ms,
      };

      match entry.kind {
        HashFieldStateKind::Missing => {
          if target_expire == 0 {
            meta.apply_missing_to_persistent();
          } else {
            meta.apply_missing_to_ttl(target_expire);
          }
        }
        HashFieldStateKind::Persistent => {
          if target_expire != 0 {
            meta.apply_persistent_to_ttl(target_expire);
          }
        }
        HashFieldStateKind::LiveTTL => {
          if target_expire == 0 {
            meta.apply_ttl_to_persistent();
          } else {
            meta.apply_ttl_to_ttl(target_expire);
          }
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          if target_expire == 0 {
            meta.apply_missing_to_persistent();
          } else {
            meta.apply_missing_to_ttl(target_expire);
          }
        }
      }

      meta.with_encoded_subkey_value(v_bytes, target_expire, |enc| batch.insert_data(item_k, enc));
      state_cache.insert(
        f_bytes,
        CachedFieldState {
          kind: if target_expire == 0 {
            HashFieldStateKind::Persistent
          } else {
            HashFieldStateKind::LiveTTL
          },
          expire: target_expire,
          raw: None,
        },
      );
    }

    if meta.base.size == 0 {
      if metadata_existed {
        batch.rm_meta(&meta_k);
        batch.commit()?;
      }
    } else {
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
    }

    Ok(true)
  }

  #[inline]
  pub fn hgetex<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    opt_li: impl IntoIterator<Item = HGetEx>,
  ) -> Result<Option<Vec<u8>>> {
    let now_ms = current_now_ms();
    let opts = HashGetEx::from_options(opt_li, now_ms);
    let res = self.get_fields_with_expire(key, &[field], opts)?;
    Ok(res.into_iter().next().flatten())
  }

  #[inline]
  pub fn hmget_ex<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    opt_li: impl IntoIterator<Item = HGetEx>,
  ) -> Result<Vec<Option<Vec<u8>>>> {
    let now_ms = current_now_ms();
    let opts = HashGetEx::from_options(opt_li, now_ms);
    self.get_fields_with_expire(key, fields, opts)
  }

  #[inline]
  pub(crate) fn get_fields_with_expire<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[F],
    options: HashGetEx,
  ) -> Result<Vec<Option<Vec<u8>>>> {
    if fields.is_empty() {
      return Ok(Vec::new());
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![None; fields.len()]),
    };

    if meta.is_legacy_subkey_encoding() {
      return Err(Error::invalid_data(
        ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      ));
    }

    let is_immediate =
      options.ttl_action == TTLAction::Set && is_immediate_expire(options.expire_at_ms, now_ms);

    if fields.len() == 1 {
      let f_bytes = fields[0].as_ref();
      let item_k = key::field(&kc, key_bytes, f_bytes);
      let data_ks = self.data();
      let raw_opt = data_ks.get(item_k.as_slice())?;
      let (state_kind, _state_expire, payload) = match raw_opt.as_ref() {
        None => (HashFieldStateKind::Missing, 0, None),
        Some(raw) => match decode_field_state(&meta, raw, now_ms) {
          None => (HashFieldStateKind::Missing, 0, None),
          Some(s) => {
            let p = meta.decode_subkey_value(raw).map(|(_, p)| p.to_vec());
            (s.kind, s.expire, p)
          }
        },
      };

      match state_kind {
        HashFieldStateKind::Missing => return Ok(vec![None]),
        HashFieldStateKind::ExpiredTTLPhysical => {
          let mut batch = self.batch();
          batch.rm_data(item_k.as_slice());
          meta.apply_ttl_to_deleted();
          if meta.base.size == 0 {
            batch.rm_meta(&meta_k);
          } else {
            batch.insert_meta(&meta_k, &meta.encode());
          }
          batch.commit()?;
          return Ok(vec![None]);
        }
        HashFieldStateKind::Persistent => {
          if is_immediate {
            let mut batch = self.batch();
            batch.rm_data(item_k.as_slice());
            meta.apply_persistent_to_deleted();
            if meta.base.size == 0 {
              batch.rm_meta(&meta_k);
            } else {
              batch.insert_meta(&meta_k, &meta.encode());
            }
            batch.commit()?;
          } else if options.ttl_action == TTLAction::Set {
            let mut batch = self.batch();
            meta.apply_persistent_to_ttl(options.expire_at_ms);
            let p = payload.as_deref().unwrap_or(b"");
            meta.with_encoded_subkey_value(p, options.expire_at_ms, |enc| {
              batch.insert_data(item_k.as_slice(), enc)
            });
            batch.insert_meta(&meta_k, &meta.encode());
            batch.commit()?;
          }
          return Ok(vec![payload]);
        }
        HashFieldStateKind::LiveTTL => {
          if is_immediate {
            let mut batch = self.batch();
            batch.rm_data(item_k.as_slice());
            meta.apply_ttl_to_deleted();
            if meta.base.size == 0 {
              batch.rm_meta(&meta_k);
            } else {
              batch.insert_meta(&meta_k, &meta.encode());
            }
            batch.commit()?;
          } else {
            match options.ttl_action {
              TTLAction::Persist => {
                let mut batch = self.batch();
                meta.apply_ttl_to_persistent();
                let p = payload.as_deref().unwrap_or(b"");
                meta
                  .with_encoded_subkey_value(p, 0, |enc| batch.insert_data(item_k.as_slice(), enc));
                batch.insert_meta(&meta_k, &meta.encode());
                batch.commit()?;
              }
              TTLAction::Set => {
                let mut batch = self.batch();
                meta.apply_ttl_to_ttl(options.expire_at_ms);
                let p = payload.as_deref().unwrap_or(b"");
                meta.with_encoded_subkey_value(p, options.expire_at_ms, |enc| {
                  batch.insert_data(item_k.as_slice(), enc)
                });
                batch.insert_meta(&meta_k, &meta.encode());
                batch.commit()?;
              }
              TTLAction::Discard | TTLAction::Keep => {}
            }
          }
          return Ok(vec![payload]);
        }
      }
    }

    let mut results = Vec::with_capacity(fields.len());
    let mut batch = self.batch();
    let mut meta_changed = false;
    let mut state_cache: HashMap<&[u8], CachedFieldState> = HashMap::with_capacity(fields.len());
    let data_ks = self.data();
    let _meta_ks = self.meta();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);

    for f in fields {
      let f_bytes = f.as_ref();
      let item_k = composer.key_for_field(f_bytes);

      let entry = if let Some(cached) = state_cache.get(f_bytes) {
        cached.clone()
      } else {
        let state_entry = load_field_state(data_ks, &meta, item_k, now_ms)?;
        state_cache.insert(f_bytes, state_entry.clone());
        state_entry
      };

      match entry.kind {
        HashFieldStateKind::Missing => {
          results.push(None);
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          batch.rm_data(item_k);
          meta.apply_ttl_to_deleted();
          meta_changed = true;
          state_cache.insert(
            f_bytes,
            CachedFieldState {
              kind: HashFieldStateKind::Missing,
              expire: 0,
              raw: None,
            },
          );
          results.push(None);
        }
        HashFieldStateKind::Persistent => {
          let payload = entry
            .raw
            .as_ref()
            .and_then(|s| meta.decode_subkey_value(s))
            .map(|(_, p)| p)
            .unwrap_or(b"");
          results.push(Some(payload.to_vec()));

          if is_immediate {
            batch.rm_data(item_k);
            meta.apply_persistent_to_deleted();
            meta_changed = true;
            state_cache.insert(
              f_bytes,
              CachedFieldState {
                kind: HashFieldStateKind::Missing,
                expire: 0,
                raw: None,
              },
            );
          } else if options.ttl_action == TTLAction::Set && options.expire_at_ms != 0 {
            meta.apply_persistent_to_ttl(options.expire_at_ms);
            meta_changed = true;
            meta.with_encoded_subkey_value(payload, options.expire_at_ms, |enc| {
              batch.insert_data(item_k, enc)
            });
            state_cache.insert(
              f_bytes,
              CachedFieldState {
                kind: HashFieldStateKind::LiveTTL,
                expire: options.expire_at_ms,
                raw: entry.raw,
              },
            );
          }
        }
        HashFieldStateKind::LiveTTL => {
          let payload = entry
            .raw
            .as_ref()
            .and_then(|s| meta.decode_subkey_value(s))
            .map(|(_, p)| p)
            .unwrap_or(b"");
          results.push(Some(payload.to_vec()));

          if is_immediate {
            batch.rm_data(item_k);
            meta.apply_ttl_to_deleted();
            meta_changed = true;
            state_cache.insert(
              f_bytes,
              CachedFieldState {
                kind: HashFieldStateKind::Missing,
                expire: 0,
                raw: None,
              },
            );
          } else {
            match options.ttl_action {
              TTLAction::Persist => {
                meta.apply_ttl_to_persistent();
                meta_changed = true;
                meta.with_encoded_subkey_value(payload, 0, |enc| batch.insert_data(item_k, enc));
                state_cache.insert(
                  f_bytes,
                  CachedFieldState {
                    kind: HashFieldStateKind::Persistent,
                    expire: 0,
                    raw: entry.raw,
                  },
                );
              }
              TTLAction::Set => {
                meta.apply_ttl_to_ttl(options.expire_at_ms);
                meta_changed = true;
                meta.with_encoded_subkey_value(payload, options.expire_at_ms, |enc| {
                  batch.insert_data(item_k, enc)
                });
                state_cache.insert(
                  f_bytes,
                  CachedFieldState {
                    kind: HashFieldStateKind::LiveTTL,
                    expire: options.expire_at_ms,
                    raw: entry.raw,
                  },
                );
              }
              TTLAction::Keep | TTLAction::Discard => {}
            }
          }
        }
      }
    }

    if meta_changed {
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(results)
  }
}
