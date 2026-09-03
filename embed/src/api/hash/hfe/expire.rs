use rapidhash::{HashMapExt, RapidHashMap as HashMap};

use crate::{
  api::hash::{
    CachedFieldState,
    r#const::{
      HASH_EXPIRE_COND_FAILED, HASH_EXPIRE_DELETED, HASH_EXPIRE_SET_OK, HASH_FIELD_NOT_FOUND,
    },
    hfe::{
      apply_expire_in_batch, commit_hash_batch, get_live_hfe_meta, load_field_state,
      purge_expired_physical_field, remove_field_in_batch,
    },
    meta::{
      HashFieldStateKind, HashItemKeyComposer, compose_hash_meta_key, hexpire_condition_passes,
      is_immediate_expire,
    },
    opt::HExpire,
  },
  engine::Engine,
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

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
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    let target_expire_ms = if seconds <= 0 {
      0
    } else {
      now_ms.saturating_add((seconds as u64).saturating_mul(1000))
    };
    self.expire_field_one(key, field, target_expire_ms, condition, now_ms)
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
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    let target_expire_ms = if milliseconds <= 0 {
      0
    } else {
      now_ms.saturating_add(milliseconds as u64)
    };
    self.expire_field_one(key, field, target_expire_ms, condition, now_ms)
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
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    let target_expire_ms = unix_time_sec.saturating_mul(1000);
    self.expire_field_one(key, field, target_expire_ms, condition, now_ms)
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
    let now_ms = current_now_ms();
    let condition = opt_li.into_iter().next().unwrap_or_default();
    self.expire_field_one(key, field, unix_time_ms, condition, now_ms)
  }

  #[inline]
  pub(crate) fn expire_field_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    expire_at_ms: u64,
    condition: HExpire,
    now_ms: u64,
  ) -> Result<i64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);

    let mut meta = match get_live_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(HASH_FIELD_NOT_FOUND),
    };

    let is_immediate = is_immediate_expire(expire_at_ms, now_ms);
    let data_ks = self.data();
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let f_bytes = field.as_ref();
    let item_k = composer.key_for_field(f_bytes);
    let entry = load_field_state(data_ks, &meta, item_k, now_ms)?;

    match entry.kind {
      HashFieldStateKind::Missing => Ok(HASH_FIELD_NOT_FOUND),
      HashFieldStateKind::ExpiredTTLPhysical => {
        purge_expired_physical_field(&meta_k, &mut meta, item_k, self.batch_with_capacity(2))?;
        Ok(HASH_FIELD_NOT_FOUND)
      }
      HashFieldStateKind::Persistent | HashFieldStateKind::LiveTTL => {
        if !hexpire_condition_passes(condition, entry.kind, entry.expire, expire_at_ms) {
          return Ok(HASH_EXPIRE_COND_FAILED);
        }
        let mut batch = self.batch_with_capacity(2);
        if is_immediate {
          remove_field_in_batch(&mut meta, item_k, entry.kind, &mut batch);
          commit_hash_batch(&meta_k, &mut meta, batch)?;
          Ok(HASH_EXPIRE_DELETED)
        } else {
          apply_expire_in_batch(
            &mut meta,
            item_k,
            entry.kind,
            entry.raw.as_deref(),
            expire_at_ms,
            &mut batch,
          );
          commit_hash_batch(&meta_k, &mut meta, batch)?;
          Ok(HASH_EXPIRE_SET_OK)
        }
      }
    }
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
    if fields.len() == 1 {
      return Ok(vec![self.expire_field_one(
        key,
        &fields[0],
        expire_at_ms,
        condition,
        now_ms,
      )?]);
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);

    let mut meta = match get_live_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![HASH_FIELD_NOT_FOUND; fields.len()]),
    };

    let is_immediate = is_immediate_expire(expire_at_ms, now_ms);
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
        HashFieldStateKind::Persistent | HashFieldStateKind::LiveTTL => {
          if !hexpire_condition_passes(condition, entry.kind, entry.expire, expire_at_ms) {
            results.push(HASH_EXPIRE_COND_FAILED);
            continue;
          }

          if is_immediate {
            remove_field_in_batch(&mut meta, item_k, entry.kind, &mut batch);
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
            apply_expire_in_batch(
              &mut meta,
              item_k,
              entry.kind,
              entry.raw.as_deref(),
              expire_at_ms,
              &mut batch,
            );
            meta_changed = true;
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
      commit_hash_batch(&meta_k, &mut meta, batch)?;
    }

    Ok(results)
  }
}
