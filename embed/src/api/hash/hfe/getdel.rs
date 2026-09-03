use rapidhash::{HashMapExt, RapidHashMap as HashMap};

use crate::{
  api::hash::{
    CachedFieldState,
    hfe::{get_hfe_meta, load_field_state},
    meta::{HashFieldStateKind, HashItemKeyComposer, compose_hash_meta_key},
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

    let mut meta = match get_hfe_meta(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![None; fields.len()]),
    };

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
        HashFieldStateKind::Persistent | HashFieldStateKind::LiveTTL => {
          let payload = entry
            .raw
            .as_ref()
            .and_then(|s| meta.decode_subkey_value(s))
            .map(|(_, p)| p.to_vec())
            .unwrap_or_default();
          results.push(Some(payload));
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
}
