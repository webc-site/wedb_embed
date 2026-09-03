use crate::{
  api::hash::{
    meta::{
      HashFieldStateKind, HashItemKeyComposer, HashMeta, compose_hash_meta_key,
      compose_hash_prefix_stack, decode_field_state,
    },
    opt::HashLengthMode,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  key_composer::KeyComposer,
  meta::current_now_ms,
  wedb::Db,
};

/// Scans and repairs metadata size and TTL boundaries when inconsistency is detected.
/// 扫描并修复哈希元数据大小与 TTL 边界
pub(crate) fn scan_and_repair_hash<E: Engine>(
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

/// Query, existence, and full scan hash operations (HEXISTS, HLEN, HMGET, HGETALL, HKEYS, HVALS, HSTRLEN).
/// 哈希查询、存在性探测与全量扫描接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
