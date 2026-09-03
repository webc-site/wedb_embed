use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::hash::meta::{
    HashFieldStateKind, HashItemKeyComposer, HashMeta, compose_hash_meta_key,
    compose_hash_prefix_stack, decode_field_state, is_field_expired,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::current_now_ms,
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
        }
        if exp == 0 {
          meta.apply_persistent_to_deleted();
        } else {
          meta.apply_ttl_to_deleted();
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
}
