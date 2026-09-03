use rapidhash::{HashMapExt, RapidHashMap as HashMap};

use crate::{
  api::hash::{
    CachedFieldState,
    hfe::{get_hfe_meta, load_field_state},
    meta::{
      HashFieldStateKind, HashItemKeyComposer, compose_hash_meta_key, is_immediate_expire,
    },
    opt::{HGetEx, HashGetEx, TTLAction},
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

    let mut meta = match get_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![None; fields.len()]),
    };

    let is_immediate =
      options.ttl_action == TTLAction::Set && is_immediate_expire(options.expire_at_ms, now_ms);

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
