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
    },
    key::get_meta_checked,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  wedb::Db,
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
