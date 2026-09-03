use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::hash::{
    r#const::{ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW},
    meta::{
      HashFieldStateKind, HashItemKeyComposer, HashMeta, compose_hash_meta_key,
      compose_hash_prefix_stack, decode_field_state, is_field_expired,
    },
    opt::HashLengthMode,
    parse_hash_float, parse_hash_integer,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  key_composer::KeyComposer,
  meta::current_now_ms,
  string::format_float_bytes,
  wedb::{Db, DbBatch},
};

// ── 辅助函数 ──

#[inline]
pub fn prepare_hash_meta_for_write<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
  batch: &mut DbBatch<E>,
) -> Result<(HashMeta, bool)>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  match get_meta_checked::<HashMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) => Ok((meta, true)),
    None => {
      let prefix = compose_hash_prefix_stack(&kc, k_bytes);
      clear_prefix_in_batch(db.data(), &prefix, batch)?;
      Ok((HashMeta::new_with_version(0, 0), false))
    }
  }
}

fn scan_and_repair_hash<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  meta: &mut HashMeta,
  now_ms: u64,
) -> Result<usize>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let prefix = compose_hash_prefix_stack(&kc, k_bytes);
  let mut repaired = *meta;
  repaired.base.size = 0;
  repaired.persist = 0;
  repaired.lower = 0;
  repaired.upper = 0;

  let mut batch = db.batch();
  let data_ks = db.data();

  for g in data_ks.prefix(&prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(prefix.as_slice()) {
      break;
    }
    if let Some(state) = decode_field_state(meta, v, now_ms) {
      match state.kind {
        HashFieldStateKind::ExpiredTTLPhysical => {
          batch.rm_data(k);
        }
        HashFieldStateKind::Persistent => {
          repaired.base.size += 1;
          repaired.persist += 1;
        }
        HashFieldStateKind::LiveTTL => {
          repaired.base.size += 1;
          if repaired.lower == 0 || state.expire < repaired.lower {
            repaired.lower = state.expire;
          }
          repaired.upper = repaired.upper.max(state.expire);
        }
        HashFieldStateKind::Missing => {}
      }
    }
  }

  if repaired.base.size == 0 {
    batch.rm_meta(meta_k);
  } else {
    repaired.clear_bounds_if_no_ttl_candidates();
    batch.insert_meta(meta_k, &repaired.encode());
  }

  batch.commit()?;
  *meta = repaired;
  Ok(repaired.base.size as usize)
}

// ── 纯 DbLike 泛型实现 ──

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn hget<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<Option<Vec<u8>>> {
    self.with_hget(key, field, |v| v.to_vec())
  }

  #[inline]
  pub fn with_hget<K: AsRef<[u8]>, F: AsRef<[u8]>, R>(
    &self,
    key: K,
    field: F,
    f: impl FnOnce(&[u8]) -> R,
  ) -> Result<Option<R>> {
    let key_bytes = key.as_ref();
    let field_bytes = field.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(None),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(None);
    }

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(field_bytes);

    if let Some(raw) = self.data().get(item_k)?
      && let Some((_, payload)) = meta.decode_live_subkey_value(&raw, now_ms)
    {
      Ok(Some(f(payload)))
    } else {
      Ok(None)
    }
  }

  #[inline]
  pub fn hset<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[(F, V)],
  ) -> Result<usize> {
    if fields.is_empty() {
      return Ok(0);
    }
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut batch = self.batch_with_capacity(fields.len() + 1);
    let (mut meta, metadata_existed) =
      prepare_hash_meta_for_write(self, key_bytes, &meta_k, now_ms, &mut batch)?;

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let data_ks = self.data();

    // 单字段极致快速路径 (Zero-Alloc Fast Path for Single Field)
    if fields.len() == 1 {
      let (f, v) = (&fields[0].0, &fields[0].1);
      let f_bytes = f.as_ref();
      let v_bytes = v.as_ref();
      let item_k = composer.key_for_field(f_bytes);

      let mut inserted_count = 0usize;
      if metadata_existed {
        let state_kind = if let Some(raw) = data_ks.get(item_k)? {
          decode_field_state(&meta, &raw, now_ms).map_or(HashFieldStateKind::Missing, |s| s.kind)
        } else {
          HashFieldStateKind::Missing
        };
        match state_kind {
          HashFieldStateKind::Missing => {
            meta.apply_missing_to_persistent();
            inserted_count = 1;
          }
          HashFieldStateKind::ExpiredTTLPhysical => {
            meta.apply_ttl_to_persistent();
            inserted_count = 1;
          }
          HashFieldStateKind::LiveTTL => {
            meta.apply_ttl_to_persistent();
          }
          HashFieldStateKind::Persistent => {}
        }
      } else {
        meta.apply_missing_to_persistent();
        inserted_count = 1;
      }

      meta.with_encoded_subkey_value(v_bytes, 0, |enc| batch.insert_data(item_k, enc));
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
      return Ok(inserted_count);
    }

    // 全新 Hash 表批量直写（免去 field 状态探测与缓存分配）
    if !metadata_existed {
      let mut seen = HashSet::with_capacity(fields.len());
      let mut inserted_count = 0usize;
      for (f, v) in fields {
        let f_bytes = f.as_ref();
        if seen.insert(f_bytes) {
          let item_k = composer.key_for_field(f_bytes);
          meta.with_encoded_subkey_value(v.as_ref(), 0, |enc| batch.insert_data(item_k, enc));
          meta.apply_missing_to_persistent();
          inserted_count += 1;
        }
      }
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
      return Ok(inserted_count);
    }

    // 既有元数据多字段通用路径（逆序去重保留最后写入值，就地解码状态，零额外堆分配）
    let mut seen = HashSet::with_capacity(fields.len());
    let mut unique_fields = Vec::with_capacity(fields.len());
    for (f, v) in fields.iter().rev() {
      let f_bytes = f.as_ref();
      if seen.insert(f_bytes) {
        unique_fields.push((f_bytes, v.as_ref()));
      }
    }
    unique_fields.reverse();

    let mut inserted_count = 0usize;

    for (f_bytes, v_bytes) in unique_fields {
      let item_k = composer.key_for_field(f_bytes);

      let state_kind = if let Some(raw) = data_ks.get(item_k)? {
        decode_field_state(&meta, &raw, now_ms).map_or(HashFieldStateKind::Missing, |s| s.kind)
      } else {
        HashFieldStateKind::Missing
      };

      match state_kind {
        HashFieldStateKind::Missing => {
          meta.apply_missing_to_persistent();
          inserted_count += 1;
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          meta.apply_ttl_to_persistent();
          inserted_count += 1;
        }
        HashFieldStateKind::LiveTTL => {
          meta.apply_ttl_to_persistent();
        }
        HashFieldStateKind::Persistent => {}
      }

      meta.with_encoded_subkey_value(v_bytes, 0, |enc| batch.insert_data(item_k, enc));
    }

    if meta.base.size == 0 {
      batch.rm_meta(&meta_k);
    } else {
      batch.insert_meta(&meta_k, &meta.encode());
    }
    batch.commit()?;

    Ok(inserted_count)
  }

  /// Sets multiple hash fields (HMSET, alias for HSET, aligned with Redis / Apache Kvrocks).
  /// 批量设置哈希字段（HMSET，HSET 的别名，对标 Redis / Apache Kvrocks）
  #[inline]
  pub fn hmset<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    fields: &[(F, V)],
  ) -> Result<usize> {
    self.hset(key, fields)
  }

  #[inline]
  pub fn hset_one<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    val: V,
  ) -> Result<usize> {
    self.hset(key, &[(field, val)])
  }

  #[inline]
  pub fn hsetnx<K: AsRef<[u8]>, F: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    field: F,
    val: V,
  ) -> Result<bool> {
    let key_bytes = key.as_ref();
    let field_bytes = field.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut batch = self.batch_with_capacity(2);
    let (mut meta, metadata_existed) =
      prepare_hash_meta_for_write(self, key_bytes, &meta_k, now_ms, &mut batch)?;

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(field_bytes);
    let data_ks = self.data();

    if metadata_existed {
      let state_kind = if let Some(raw) = data_ks.get(item_k)? {
        decode_field_state(&meta, &raw, now_ms).map_or(HashFieldStateKind::Missing, |s| s.kind)
      } else {
        HashFieldStateKind::Missing
      };

      match state_kind {
        HashFieldStateKind::Persistent | HashFieldStateKind::LiveTTL => {
          return Ok(false);
        }
        HashFieldStateKind::ExpiredTTLPhysical => {
          meta.apply_ttl_to_persistent();
        }
        HashFieldStateKind::Missing => {
          meta.apply_missing_to_persistent();
        }
      }
    } else {
      meta.apply_missing_to_persistent();
    }

    meta.with_encoded_subkey_value(val_bytes, 0, |enc| batch.insert_data(item_k, enc));
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
    Ok(true)
  }

  #[inline]
  pub fn hdel_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<usize> {
    self.hdel(key, &[field])
  }

  #[inline]
  pub fn hdel<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, fields: &[F]) -> Result<usize> {
    if fields.is_empty() {
      return Ok(0);
    }
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(0),
    };

    let mut deleted = 0usize;
    let mut physical_removed = 0usize;
    let mut batch = self.batch_with_capacity(fields.len() + 1);
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let data_ks = self.data();

    if fields.len() == 1 {
      let f_bytes = fields[0].as_ref();
      let item_k = composer.key_for_field(f_bytes);
      if let Some(raw) = data_ks.get(item_k)?
        && let Some((exp, _)) = meta.decode_subkey_value(&raw)
      {
        batch.rm_weak_data(item_k);
        physical_removed = 1;
        if !is_field_expired(exp, now_ms) {
          deleted = 1;
          if exp == 0 {
            meta.apply_persistent_to_deleted();
          } else {
            meta.apply_ttl_to_deleted();
          }
        } else {
          meta.apply_ttl_to_deleted();
        }
      }
    } else {
      let mut seen = HashSet::with_capacity(fields.len());
      for f in fields {
        let f_bytes = f.as_ref();
        if !seen.insert(f_bytes) {
          continue;
        }
        let item_k = composer.key_for_field(f_bytes);
        if let Some(raw) = data_ks.get(item_k)?
          && let Some((exp, _)) = meta.decode_subkey_value(&raw)
        {
          batch.rm_weak_data(item_k);
          physical_removed += 1;
          if !is_field_expired(exp, now_ms) {
            deleted += 1;
            if exp == 0 {
              meta.apply_persistent_to_deleted();
            } else {
              meta.apply_ttl_to_deleted();
            }
          } else {
            meta.apply_ttl_to_deleted();
          }
        }
      }
    }

    if physical_removed > 0 {
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        meta.clear_bounds_if_no_ttl_candidates();
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }
    Ok(deleted)
  }

  #[inline]
  pub fn hexists<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<bool> {
    Ok(self.with_hget(key, field, |_| ())?.is_some())
  }

  #[inline]
  pub fn hlen<K: AsRef<[u8]>>(&self, key: K) -> Result<usize> {
    self.hlen_with_mode(key, HashLengthMode::Accurate)
  }

  #[inline]
  pub fn hlen_with_mode<K: AsRef<[u8]>>(&self, key: K, mode: HashLengthMode) -> Result<usize> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let _meta_ks = self.meta();
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    if meta.is_legacy_subkey_encoding() || mode == HashLengthMode::Approximate {
      return Ok(meta.base.size as usize);
    }

    if meta.persist > meta.base.size {
      return scan_and_repair_hash(self, key_bytes, &meta_k, &mut meta, now_ms);
    }

    let ttl_candidates = meta.base.size.saturating_sub(meta.persist);
    if ttl_candidates == 0 {
      return Ok(meta.base.size as usize);
    }

    if meta.lower != 0 && now_ms < meta.lower {
      return Ok(meta.base.size as usize);
    }

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      let mut batch = self.batch();
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(0);
    }

    scan_and_repair_hash(self, key_bytes, &meta_k, &mut meta, now_ms)
  }

  #[inline]
  pub fn hmget<K: AsRef<[u8]>, F: AsRef<[u8]>>(
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

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(vec![None; fields.len()]),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(vec![None; fields.len()]);
    }

    let mut results = Vec::with_capacity(fields.len());
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let data_ks = self.data();

    for f in fields {
      let item_k = composer.key_for_field(f.as_ref());
      let val = if let Some(raw) = data_ks.get(item_k)?
        && let Some((_, payload)) = meta.decode_live_subkey_value(&raw, now_ms)
      {
        Some(payload.to_vec())
      } else {
        None
      };
      results.push(val);
    }
    Ok(results)
  }

  #[inline]
  pub fn hgetall<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut results = Vec::new();
    self.hiter(key, |f, v| {
      results.push((f.to_vec(), v.to_vec()));
      true
    })?;
    Ok(results)
  }

  #[inline]
  pub fn hkeys<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<Vec<u8>>> {
    let mut keys = Vec::new();
    self.hiter(key, |f, _| {
      keys.push(f.to_vec());
      true
    })?;
    Ok(keys)
  }

  #[inline]
  pub fn hvals<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<Vec<u8>>> {
    let mut vals = Vec::new();
    self.hiter(key, |_, v| {
      vals.push(v.to_vec());
      true
    })?;
    Ok(vals)
  }

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

  #[inline]
  pub fn hstrlen<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<usize> {
    Ok(self.with_hget(key, field, |v| v.len())?.unwrap_or(0))
  }

  #[inline]
  pub(crate) fn hiter_with_meta<F>(
    &self,
    kc: &KeyComposer,
    key_bytes: &[u8],
    meta: &HashMeta,
    now_ms: u64,
    mut f: F,
  ) -> Result<()>
  where
    F: FnMut(&[u8], &[u8]) -> bool,
  {
    let prefix = compose_hash_prefix_stack(kc, key_bytes);
    let prefix_len = prefix.len();

    for guard in self.data().prefix(&prefix) {
      let entry = guard?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some((_, payload)) = meta.decode_live_subkey_value(v, now_ms) {
        let field = &k[prefix_len..];
        if !f(field, payload) {
          break;
        }
      }
    }
    Ok(())
  }
}
