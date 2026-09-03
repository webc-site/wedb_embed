use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::hash::{
    CachedFieldState,
    hfe::load_field_state,
    meta::{
      HashFieldStateKind, HashItemKeyComposer, compose_hash_meta_key, is_immediate_expire,
    },
    opt::{HSet, HashFieldSetCondition, HashSetEx, TTLAction},
    prepare_hash_meta_for_write,
    r#const::ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
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
    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);

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
}
