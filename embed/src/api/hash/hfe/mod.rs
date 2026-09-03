pub mod expire;
pub mod getdel;
pub mod getex;
pub mod setex;
pub mod ttl;

use crate::{
  api::{
    hash::{
      CachedFieldState,
      r#const::ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
      meta::{HashFieldStateKind, HashMeta, decode_field_state},
      opt::TTLAction,
    },
    key::get_meta_checked,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  wedb::{Db, DbBatch},
};

#[inline]
pub(crate) fn get_hfe_meta<E: Engine>(
  db: &Db<E>,
  key_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
) -> Result<Option<HashMeta>>
where
  Error: From<E::Error>,
{
  let meta = match get_meta_checked::<HashMeta, _>(db, key_bytes, meta_k, now_ms)? {
    Some(m) => m,
    None => return Ok(None),
  };
  if meta.is_legacy_subkey_encoding() {
    return Err(Error::invalid_data(
      ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
    ));
  }
  Ok(Some(meta))
}

#[inline]
pub(crate) fn get_live_hfe_meta<E: Engine>(
  db: &Db<E>,
  key_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
) -> Result<Option<HashMeta>>
where
  Error: From<E::Error>,
{
  match get_hfe_meta(db, key_bytes, meta_k, now_ms)? {
    Some(m) => {
      if m.upper != 0 && now_ms > m.upper && m.persist == 0 {
        Ok(None)
      } else {
        Ok(Some(m))
      }
    }
    None => Ok(None),
  }
}

#[inline]
pub(crate) fn commit_hash_meta_or_rm<E: Engine>(
  meta_k: &[u8],
  meta: &mut HashMeta,
  batch: &mut DbBatch<E>,
) {
  if meta.base.size == 0 {
    batch.rm_meta(meta_k);
  } else {
    meta.clear_bounds_if_no_ttl_candidates();
    batch.insert_meta(meta_k, &meta.encode());
  }
}

#[inline]
pub(crate) fn commit_hash_batch<E: Engine>(
  meta_k: &[u8],
  meta: &mut HashMeta,
  mut batch: DbBatch<E>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  commit_hash_meta_or_rm(meta_k, meta, &mut batch);
  batch.commit()?;
  Ok(())
}

#[inline]
pub(crate) fn purge_expired_physical_field<E: Engine>(
  meta_k: &[u8],
  meta: &mut HashMeta,
  item_k: &[u8],
  mut batch: DbBatch<E>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  batch.rm_data(item_k);
  meta.apply_ttl_to_deleted();
  commit_hash_batch(meta_k, meta, batch)
}

#[inline]
pub(crate) fn load_field_state<P: Partition>(
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
pub(crate) fn remove_field_in_batch<E: Engine>(
  meta: &mut HashMeta,
  item_k: &[u8],
  entry_kind: HashFieldStateKind,
  batch: &mut DbBatch<E>,
) {
  batch.rm_data(item_k);
  if entry_kind == HashFieldStateKind::Persistent {
    meta.apply_persistent_to_deleted();
  } else {
    meta.apply_ttl_to_deleted();
  }
}

#[inline]
pub(crate) fn extract_subkey_payload<'a>(meta: &HashMeta, raw: Option<&'a [u8]>) -> &'a [u8] {
  raw
    .and_then(|s| meta.decode_subkey_value(s))
    .map(|(_, p)| p)
    .unwrap_or(b"")
}

#[inline]
pub(crate) fn apply_persist_in_batch<E: Engine>(
  meta: &mut HashMeta,
  item_k: &[u8],
  raw: Option<&[u8]>,
  batch: &mut DbBatch<E>,
) {
  meta.apply_ttl_to_persistent();
  let payload = extract_subkey_payload(meta, raw);
  meta.with_encoded_subkey_value(payload, 0, |enc| batch.insert_data(item_k, enc));
}

#[inline]
pub(crate) fn apply_expire_in_batch<E: Engine>(
  meta: &mut HashMeta,
  item_k: &[u8],
  entry_kind: HashFieldStateKind,
  raw: Option<&[u8]>,
  expire_at_ms: u64,
  batch: &mut DbBatch<E>,
) {
  if entry_kind == HashFieldStateKind::Persistent {
    meta.apply_persistent_to_ttl(expire_at_ms);
  } else {
    meta.apply_ttl_to_ttl(expire_at_ms);
  }
  let payload = extract_subkey_payload(meta, raw);
  meta.with_encoded_subkey_value(payload, expire_at_ms, |enc| batch.insert_data(item_k, enc));
}

#[inline]
pub(crate) fn resolve_target_expire(
  ttl_action: TTLAction,
  options_expire: u64,
  entry_kind: HashFieldStateKind,
  entry_expire: u64,
) -> u64 {
  match ttl_action {
    TTLAction::Discard | TTLAction::Persist => 0,
    TTLAction::Keep => {
      if entry_kind == HashFieldStateKind::LiveTTL
        || entry_kind == HashFieldStateKind::ExpiredTTLPhysical
      {
        entry_expire
      } else {
        0
      }
    }
    TTLAction::Set => options_expire,
  }
}

#[inline]
pub(crate) fn apply_setex_field_in_batch<E: Engine>(
  meta: &mut HashMeta,
  item_k: &[u8],
  v_bytes: &[u8],
  entry_kind: HashFieldStateKind,
  target_expire: u64,
  batch: &mut DbBatch<E>,
) {
  match entry_kind {
    HashFieldStateKind::Missing | HashFieldStateKind::ExpiredTTLPhysical => {
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
  }
  meta.with_encoded_subkey_value(v_bytes, target_expire, |enc| batch.insert_data(item_k, enc));
}
