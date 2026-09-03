use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::{
    hash::{
      CachedFieldState,
      hfe::load_field_state,
      key,
      meta::{
        HashFieldState, HashFieldStateKind, HashItemKeyComposer, compose_hash_meta_key,
        decode_field_state, is_immediate_expire,
      },
      opt::{HSet, HashFieldSetCondition, HashSetEx, TTLAction},
      prepare_hash_meta_for_write,
      r#const::ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING,
    },
  },
  engine::{Engine, Partition},
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

    if field_values.len() == 1 {
      let (f, v) = (&field_values[0].0, &field_values[0].1);
      let f_bytes = f.as_ref();
      let v_bytes = v.as_ref();
      let item_k = key::field(&kc, key_bytes, f_bytes);
      let data_ks = self.data();
      let raw_opt = if metadata_existed {
        data_ks.get(item_k.as_slice())?
      } else {
        None
      };

      let state = match raw_opt.as_ref() {
        None => HashFieldState {
          kind: HashFieldStateKind::Missing,
          expire: 0,
          value: b"",
        },
        Some(raw) => decode_field_state(&meta, raw, now_ms).unwrap_or(HashFieldState {
          kind: HashFieldStateKind::Missing,
          expire: 0,
          value: b"",
        }),
      };

      let condition_met = match options.condition {
        HashFieldSetCondition::None => true,
        HashFieldSetCondition::Fnx => {
          state.kind == HashFieldStateKind::Missing
            || state.kind == HashFieldStateKind::ExpiredTTLPhysical
        }
        HashFieldSetCondition::Fxx => {
          state.kind == HashFieldStateKind::Persistent || state.kind == HashFieldStateKind::LiveTTL
        }
      };

      if !condition_met {
        return Ok(false);
      }

      let is_immediate =
        options.ttl_action == TTLAction::Set && is_immediate_expire(options.expire_at_ms, now_ms);

      if is_immediate {
        match state.kind {
          HashFieldStateKind::Missing => {}
          HashFieldStateKind::ExpiredTTLPhysical => {
            batch.rm_data(item_k.as_slice());
            meta.apply_ttl_to_deleted();
          }
          HashFieldStateKind::Persistent => {
            batch.rm_data(item_k.as_slice());
            meta.apply_persistent_to_deleted();
          }
          HashFieldStateKind::LiveTTL => {
            batch.rm_data(item_k.as_slice());
            meta.apply_ttl_to_deleted();
          }
        }
      } else {
        let target_expire = match options.ttl_action {
          TTLAction::Discard | TTLAction::Persist => 0,
          TTLAction::Keep => {
            if state.kind == HashFieldStateKind::LiveTTL
              || state.kind == HashFieldStateKind::ExpiredTTLPhysical
            {
              state.expire
            } else {
              0
            }
          }
          TTLAction::Set => options.expire_at_ms,
        };

        match state.kind {
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
          HashFieldStateKind::ExpiredTTLPhysical => {
            meta.apply_ttl_to_deleted();
            if target_expire == 0 {
              meta.apply_missing_to_persistent();
            } else {
              meta.apply_missing_to_ttl(target_expire);
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
        meta.with_encoded_subkey_value(v_bytes, target_expire, |enc| {
          batch.insert_data(item_k.as_slice(), enc)
        });
      }

      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
      return Ok(true);
    }

    let mut state_cache: HashMap<&[u8], CachedFieldState> =
      HashMap::with_capacity(field_values.len());
    let data_ks = self.data();
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
}
