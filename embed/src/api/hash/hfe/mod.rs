pub mod expire;
pub mod getdel;
pub mod getex;
pub mod setex;
pub mod ttl;

use crate::{
  api::hash::{
    CachedFieldState,
    meta::{HashFieldStateKind, HashMeta, decode_field_state},
  },
  engine::Partition,
  error::{Error, Result},
};

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
