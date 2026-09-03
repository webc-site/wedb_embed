use std::ops::Bound;

use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::hash::{
    CachedFieldState, HashFieldPair, HashRandField, HashScanByFieldResult, HashScanResult,
    ceil_div_1000,
    r#const::{
      ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING, ERR_INCREMENT_NAN_OR_INFINITY,
      ERR_INCREMENT_OVERFLOW, HASH_EXPIRE_COND_FAILED, HASH_EXPIRE_DELETED, HASH_EXPIRE_SET_OK,
      HASH_FIELD_NOT_FOUND, HASH_FIELD_PERSISTENT,
    },
    meta::{
      HashFieldStateKind, HashItemKeyComposer, HashMeta, compose_hash_meta_key,
      compose_hash_prefix_stack, decode_field_state, hexpire_condition_passes, is_field_expired,
      is_immediate_expire,
    },
    opt::{
      HExpire, HGetEx, HSet, HashFieldSetCondition, HashGetEx, HashLengthMode, HashSetEx, RangeLex,
      TTLAction,
    },
    parse_hash_float, parse_hash_integer,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  key_composer::{KeyComposer, matches_glob_bytes},
  meta::current_now_ms,
  string::format_float_bytes,
  wedb::{Db, DbBatch},
};

// ── 辅助函数 ──

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

#[inline]
fn prefix_upper_bound(prefix: &[u8]) -> Bound<Vec<u8>> {
  let mut bound = prefix.to_vec();
  while let Some(last) = bound.pop() {
    if last < 0xFF {
      bound.push(last + 1);
      return Bound::Excluded(bound);
    }
  }
  Bound::Unbounded
}

#[inline]
fn hash_lex_range_bounds(prefix: &[u8], spec: &RangeLex) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
  let start = if spec.min_infinite {
    Bound::Included(prefix.to_vec())
  } else if spec.minex {
    let mut k = Vec::with_capacity(prefix.len() + spec.min.len() + 1);
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.min);
    k.push(0x00);
    Bound::Included(k)
  } else {
    let mut k = Vec::with_capacity(prefix.len() + spec.min.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.min);
    Bound::Included(k)
  };

  let end = if spec.max_infinite {
    prefix_upper_bound(prefix)
  } else if spec.maxex {
    let mut k = Vec::with_capacity(prefix.len() + spec.max.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.max);
    Bound::Excluded(k)
  } else {
    let mut k = Vec::with_capacity(prefix.len() + spec.max.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.max);
    Bound::Included(k)
  };

  (start, end)
}

// ── 纯 DbLike 泛型实现 ──

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn hget<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<Option<Vec<u8>>> {
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
      Ok(Some(payload.to_vec()))
    } else {
      Ok(None)
    }
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
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(false),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(false);
    }

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(field.as_ref());
    if let Some(raw) = self.data().get(item_k)? {
      Ok(meta.decode_live_subkey_value(&raw, now_ms).is_some())
    } else {
      Ok(false)
    }
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
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(Vec::new());
    }

    let mut results = Vec::with_capacity((meta.base.size as usize).min(4096));
    self.hiter_with_meta(&kc, key_bytes, &meta, now_ms, |f, v| {
      results.push((f.to_vec(), v.to_vec()));
      true
    })?;
    Ok(results)
  }

  #[inline]
  pub fn hkeys<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<Vec<u8>>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(Vec::new());
    }

    let mut keys = Vec::with_capacity((meta.base.size as usize).min(4096));
    self.hiter_with_meta(&kc, key_bytes, &meta, now_ms, |f, _| {
      keys.push(f.to_vec());
      true
    })?;
    Ok(keys)
  }

  #[inline]
  pub fn hvals<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<Vec<u8>>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(Vec::new());
    }

    let mut vals = Vec::with_capacity((meta.base.size as usize).min(4096));
    self.hiter_with_meta(&kc, key_bytes, &meta, now_ms, |_, v| {
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
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(0),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(0);
    }

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let item_k = composer.key_for_field(field.as_ref());
    let len = if let Some(raw) = self.data().get(item_k)?
      && let Some((_, payload)) = meta.decode_live_subkey_value(&raw, now_ms)
    {
      payload.len()
    } else {
      0
    };
    Ok(len)
  }

  #[inline]
  fn hiter_with_meta<F>(
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

  #[inline]
  pub fn hiter<K: AsRef<[u8]>, F>(&self, key: K, f: F) -> Result<()>
  where
    F: FnMut(&[u8], &[u8]) -> bool,
  {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(()),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(());
    }

    self.hiter_with_meta(&kc, key_bytes, &meta, now_ms, f)
  }

  #[inline]
  pub fn hrandfield<K: AsRef<[u8]>>(
    &self,
    key: K,
    count: i64,
    with_values: bool,
  ) -> Result<Vec<HashRandField>> {
    if count == 0 {
      return Ok(Vec::new());
    }

    if with_values {
      let mut all = self.hgetall(key)?;
      let total = all.len();
      if total == 0 {
        return Ok(Vec::new());
      }

      if count > 0 {
        let sample_cnt = (count as usize).min(total);
        for i in 0..sample_cnt {
          let j = fastrand::usize(i..total);
          all.swap(i, j);
        }
        all.truncate(sample_cnt);
        let out = all.into_iter().map(|(f, v)| (f, Some(v))).collect();
        Ok(out)
      } else {
        let total_sample = count.unsigned_abs() as usize;
        let mut out = Vec::with_capacity(total_sample);
        for _ in 0..total_sample {
          let idx = fastrand::usize(0..total);
          let (f, v) = &all[idx];
          out.push((f.clone(), Some(v.clone())));
        }
        Ok(out)
      }
    } else {
      let mut all_keys = self.hkeys(key)?;
      let total = all_keys.len();
      if total == 0 {
        return Ok(Vec::new());
      }

      if count > 0 {
        let sample_cnt = (count as usize).min(total);
        for i in 0..sample_cnt {
          let j = fastrand::usize(i..total);
          all_keys.swap(i, j);
        }
        all_keys.truncate(sample_cnt);
        let out = all_keys.into_iter().map(|f| (f, None)).collect();
        Ok(out)
      } else {
        let total_sample = count.unsigned_abs() as usize;
        let mut out = Vec::with_capacity(total_sample);
        for _ in 0..total_sample {
          let idx = fastrand::usize(0..total);
          out.push((all_keys[idx].clone(), None));
        }
        Ok(out)
      }
    }
  }

  /// Returns a single random field from the hash, or None if key does not exist or is empty (HRANDFIELD key).
  /// 随机返回哈希表中的一个字段（HRANDFIELD key，对标 Redis 6.2+ / Apache Kvrocks）
  #[inline]
  pub fn hrandfield_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let fields = self.hrandfield(key, 1, false)?;
    Ok(fields.into_iter().next().map(|(f, _)| f))
  }

  #[inline]
  pub fn hscan<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: usize,
    limit: usize,
    pattern: Option<&[u8]>,
  ) -> Result<HashScanResult> {
    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat = pattern.unwrap_or(b"*");

    let mut skipped = 0;
    let mut matched = Vec::with_capacity(limit);
    let mut has_more = false;

    self.hiter(key, |field, value| {
      if is_match_all || matches_glob_bytes(pat, field) {
        if skipped < cursor {
          skipped += 1;
        } else if matched.len() < limit {
          matched.push((field.to_vec(), value.to_vec()));
        } else {
          has_more = true;
          return false;
        }
      }
      true
    })?;

    let next_cursor = if has_more { cursor + matched.len() } else { 0 };
    Ok((next_cursor, matched))
  }

  #[inline]
  pub fn hscan_by_field<K: AsRef<[u8]>, C: AsRef<[u8]>>(
    &self,
    key: K,
    cursor_field: C,
    limit: usize,
    pattern: Option<&[u8]>,
  ) -> Result<HashScanByFieldResult> {
    if limit == 0 {
      return Ok((None, Vec::new()));
    }

    let key_bytes = key.as_ref();
    let cursor_bytes = cursor_field.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok((None, Vec::new())),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok((None, Vec::new()));
    }

    let prefix_buf = compose_hash_prefix_stack(&kc, key_bytes);
    let prefix = prefix_buf.as_slice();
    let prefix_len = prefix.len();
    let end_bound = prefix_upper_bound(prefix);
    let end_ref = match &end_bound {
      Bound::Excluded(b) => Bound::Excluded(b.as_slice()),
      _ => Bound::Unbounded,
    };

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let start_ref = if cursor_bytes.is_empty() {
      Bound::Included(prefix)
    } else {
      Bound::Excluded(composer.key_for_field(cursor_bytes))
    };

    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat = pattern.unwrap_or(b"*");

    let mut matched = Vec::with_capacity(limit);

    for g in self.data().range((start_ref, end_ref)) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(prefix) {
        break;
      }
      let field_bytes = &k[prefix_len..];

      if let Some((_, payload)) = meta.decode_live_subkey_value(entry.value(), now_ms)
        && (is_match_all || matches_glob_bytes(pat, field_bytes))
      {
        matched.push((field_bytes.to_vec(), payload.to_vec()));
        if matched.len() >= limit {
          break;
        }
      }
    }

    let next_cursor = if matched.len() == limit {
      matched.last().map(|(f, _)| f.clone())
    } else {
      None
    };

    Ok((next_cursor, matched))
  }

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
  fn query_field_expire_info<K: AsRef<[u8]>, F: AsRef<[u8]>, M: Fn(u64, u64) -> i64>(
    &self,
    key: K,
    fields: &[F],
    map_live_ttl: M,
  ) -> Result<Vec<i64>> {
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
    let res = self.httl(key, &[field])?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub fn hpttl_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    let res = self.hpttl(key, &[field])?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
  }

  #[inline]
  pub fn hpttl<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, fields: &[F]) -> Result<Vec<i64>> {
    self.query_field_expire_info(key, fields, |expire, now_ms| {
      expire.saturating_sub(now_ms) as i64
    })
  }

  #[inline]
  pub fn hexpiretime_one<K: AsRef<[u8]>, F: AsRef<[u8]>>(&self, key: K, field: F) -> Result<i64> {
    let res = self.hexpiretime(key, &[field])?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
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
    let res = self.hpexpiretime(key, &[field])?;
    Ok(res.into_iter().next().unwrap_or(HASH_FIELD_NOT_FOUND))
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

  #[inline]
  pub fn hrangebylex<K: AsRef<[u8]>>(&self, key: K, spec: RangeLex) -> Result<Vec<HashFieldPair>> {
    self.hrange_by_lex(key, spec)
  }

  #[inline]
  pub fn hrange_by_lex<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: RangeLex,
  ) -> Result<Vec<HashFieldPair>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if spec.count == Some(0) {
      return Ok(Vec::new());
    }

    let prefix = compose_hash_prefix_stack(&kc, key_bytes);
    let prefix_len = prefix.len();
    let data_ks = self.data();

    let (start_bound, end_bound) = hash_lex_range_bounds(prefix.as_slice(), &spec);
    let start_ref = match &start_bound {
      Bound::Included(b) => Bound::Included(b.as_slice()),
      Bound::Excluded(b) => Bound::Excluded(b.as_slice()),
      Bound::Unbounded => Bound::Unbounded,
    };
    let end_ref = match &end_bound {
      Bound::Included(b) => Bound::Included(b.as_slice()),
      Bound::Excluded(b) => Bound::Excluded(b.as_slice()),
      Bound::Unbounded => Bound::Unbounded,
    };

    let limit = spec.count.unwrap_or(usize::MAX);
    let mut matching = Vec::with_capacity(limit.min(128));
    let mut skipped = 0;

    let mut process_entry = |k: &[u8], v: &[u8]| -> bool {
      if !k.starts_with(prefix.as_slice()) {
        return false;
      }
      let field_bytes = &k[prefix_len..];
      if let Some((_, payload)) = meta.decode_live_subkey_value(v, now_ms) {
        if skipped < spec.offset {
          skipped += 1;
          return true;
        }
        matching.push((field_bytes.to_vec(), payload.to_vec()));
        if matching.len() >= limit {
          return false;
        }
      }
      true
    };

    if !spec.reversed {
      for g in data_ks.range((start_ref, end_ref)) {
        let entry = g?;
        if !process_entry(entry.key(), entry.value()) {
          break;
        }
      }
    } else {
      for g in data_ks.range((start_ref, end_ref)).rev() {
        let entry = g?;
        if !process_entry(entry.key(), entry.value()) {
          break;
        }
      }
    }

    Ok(matching)
  }
}
