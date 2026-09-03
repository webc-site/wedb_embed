use rapidhash::{HashMapExt, RapidHashMap as HashMap};

use crate::{
  api::hash::{
    CachedFieldState, ceil_div_1000,
    r#const::{HASH_EXPIRE_SET_OK, HASH_FIELD_NOT_FOUND, HASH_FIELD_PERSISTENT},
    hfe::{get_hfe_meta, load_field_state},
    meta::{
      HashFieldStateKind, HashItemKeyComposer, HashMeta, compose_hash_meta_key, decode_field_state,
    },
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

#[inline]
fn evaluate_field_ttl(
  meta: &HashMeta,
  raw_opt: Option<&[u8]>,
  now_ms: u64,
  map_live_ttl: impl Fn(u64, u64) -> i64,
) -> i64 {
  match raw_opt {
    None => HASH_FIELD_NOT_FOUND,
    Some(raw) => match decode_field_state(meta, raw, now_ms) {
      None => HASH_FIELD_NOT_FOUND,
      Some(s) => match s.kind {
        HashFieldStateKind::Missing | HashFieldStateKind::ExpiredTTLPhysical => {
          HASH_FIELD_NOT_FOUND
        }
        HashFieldStateKind::Persistent => HASH_FIELD_PERSISTENT,
        HashFieldStateKind::LiveTTL => map_live_ttl(s.expire, now_ms),
      },
    },
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(HASH_FIELD_NOT_FOUND),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(HASH_FIELD_NOT_FOUND);
    }

    let f_bytes = field.as_ref();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(f_bytes);
    let entry = load_field_state(self.data(), &meta, item_k, now_ms)?;

    match entry.kind {
      HashFieldStateKind::Missing => Ok(HASH_FIELD_NOT_FOUND),
      HashFieldStateKind::ExpiredTTLPhysical => {
        let mut batch = self.batch_with_capacity(2);
        batch.rm_data(item_k);
        meta.apply_ttl_to_deleted();
        if meta.base.size == 0 {
          batch.rm_meta(&meta_k);
        } else {
          batch.insert_meta(&meta_k, &meta.encode());
        }
        batch.commit()?;
        Ok(HASH_FIELD_NOT_FOUND)
      }
      HashFieldStateKind::Persistent => Ok(HASH_FIELD_PERSISTENT),
      HashFieldStateKind::LiveTTL => {
        let mut batch = self.batch_with_capacity(2);
        meta.apply_ttl_to_persistent();
        let payload = entry
          .raw
          .as_ref()
          .and_then(|s| meta.decode_subkey_value(s))
          .map(|(_, p)| p)
          .unwrap_or(b"");
        meta.with_encoded_subkey_value(payload, 0, |enc| batch.insert_data(item_k, enc));
        batch.insert_meta(&meta_k, &meta.encode());
        batch.commit()?;
        Ok(HASH_EXPIRE_SET_OK)
      }
    }
  }

  #[inline]
  pub fn hpersist<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, fields: &[F]) -> Result<Vec<i64>> {
    if fields.is_empty() {
      return Ok(Vec::new());
    }
    if fields.len() == 1 {
      return Ok(vec![self.hpersist_one(key, &fields[0])?]);
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]);
    }

    let mut results = Vec::with_capacity(fields.len());
    let mut batch = self.batch();
    let mut meta_changed = false;
    let mut state_cache: HashMap<&[u8], CachedFieldState> = HashMap::with_capacity(fields.len());
    let data_ks = self.data();
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
  pub(crate) fn query_field_expire_info_one<
    K: AsRef<[u8]>,
    F: AsRef<[u8]>,
    M: Fn(u64, u64) -> i64,
  >(
    &self,
    key: K,
    field: F,
    map_live_ttl: M,
  ) -> Result<i64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(HASH_FIELD_NOT_FOUND),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(HASH_FIELD_NOT_FOUND);
    }

    let f_bytes = field.as_ref();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(f_bytes);
    let raw = self.data().get(item_k)?;
    Ok(evaluate_field_ttl(
      &meta,
      raw.as_deref(),
      now_ms,
      map_live_ttl,
    ))
  }

  #[inline]
  pub(crate) fn query_field_expire_info<K: AsRef<[u8]>, F: AsRef<[u8]>, M: Fn(u64, u64) -> i64>(
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

    let meta = match get_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]);
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
      let raw = data_ks.get(item_k)?;
      let res = evaluate_field_ttl(&meta, raw.as_deref(), now_ms, &map_live_ttl);
      result_cache.insert(f_bytes, res);
      results.push(res);
    }

    Ok(results)
  }
}
