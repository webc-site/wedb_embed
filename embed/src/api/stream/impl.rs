use std::ops::Bound;

use crate::{
  api::stream::{
    StreamEntry,
    r#const::*,
    decode_stream_entry_fields, encode_stream_entry_pairs, key,
    meta::{StreamId, StreamMeta},
    opt::{StreamAdd, StreamLen, StreamTrimStrategy},
    parse_stream_id_from_subkey,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{cleanup_composite_data_raw, get_meta_checked},
  key_composer::KeyTag,
  meta::{current_now_ms, generate_version},
  wedb::{Db, DbBatch},
};

/// Retrieves live non-expired StreamMeta (internal helper method).
/// 获取当前未过期的有效 StreamMeta（内部辅助方法）
#[inline]
pub(crate) fn get_stream_meta<E: Engine>(
  db: &Db<E>,
  key: &[u8],
  now_ms: u64,
) -> Result<Option<StreamMeta>>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_k = key::meta(&kc, key);
  get_meta_checked::<StreamMeta, _>(db, key, &meta_k, now_ms)
}

/// Reads a stream entry by StreamId (internal helper method).
/// 按 StreamId 获取单个流条目的字段（内部辅助方法）
#[inline]
pub(crate) fn get_stream_entry<E: Engine>(
  db: &Db<E>,
  key: &[u8],
  id: StreamId,
) -> Result<Option<StreamEntry>>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let item_k = key::item(&kc, key, id.ms, id.seq);
  if let Some(v) = db.data().get(&item_k)? {
    let fields = decode_stream_entry_fields(&v).unwrap_or_default();
    Ok(Some((id, fields)))
  } else {
    Ok(None)
  }
}

/// Cleans up obsolete data keys, consumer groups, and PEL entries of expired stream.
/// 清理已过期流的残留数据键与消费者组、PEL（内部辅助方法）
#[inline]
pub(crate) fn clean_stream_residue<E: Engine>(
  db: &Db<E>,
  key: &[u8],
  batch: &mut DbBatch<E>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let mut buf = Vec::with_capacity(32 + key.len());
  cleanup_composite_data_raw(
    db.data(),
    db.meta(),
    &db.kc(),
    KeyTag::StreamMeta as u8,
    key,
    batch,
    &mut buf,
  )
}

/// Retrieves last generated StreamId from metadata.
/// 获取最后生成的 StreamId
pub fn stream_last_id<E: Engine, K: AsRef<[u8]>>(db: &Db<E>, key: K) -> Result<StreamId>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => Ok(meta.last_generated_id),
    None => Ok(StreamId::min()),
  }
}

/// Appends an entry to a stream (XADD).
/// 向流追加新条目
pub fn stream_add<E: Engine, K: AsRef<[u8]>, FK: AsRef<[u8]>, FV: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  options: impl Into<StreamAdd>,
  field_vals: &[(FK, FV)],
) -> Result<StreamId>
where
  Error: From<E::Error>,
{
  let options = options.into();
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);

  let now_ms = current_now_ms();
  let opt_m = get_stream_meta(db, key_bytes, now_ms)?;
  let mut need_clean_residue = false;

  let mut meta = match opt_m {
    Some(decoded) => decoded,
    None => {
      if options.nomkstream {
        return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
      }
      need_clean_residue = true;
      StreamMeta::new(0, generate_version())
    }
  };

  let next_entry_id = options
    .next_id_strategy
    .generate_id(meta.last_generated_id, now_ms)?;

  let mut batch = db.batch();

  if need_clean_residue {
    clean_stream_residue(db, key_bytes, &mut batch)?;
  }

  let mut should_add = true;

  if options.trim_options.strategy != StreamTrimStrategy::None {
    let mut trim_opts = options.trim_options;
    if trim_opts.strategy == StreamTrimStrategy::MaxLen {
      trim_opts.max_len = if trim_opts.max_len > 0 {
        trim_opts.max_len - 1
      } else {
        0
      };
    }
    super::trim::trim_stream_internal(db, &mut meta, key_bytes, trim_opts, &mut batch)?;

    if trim_opts.strategy == StreamTrimStrategy::MinId && next_entry_id < trim_opts.min_id {
      should_add = false;
    }
    if trim_opts.strategy == StreamTrimStrategy::MaxLen && options.trim_options.max_len == 0 {
      should_add = false;
    }
  }

  if should_add {
    let encoded_payload = encode_stream_entry_pairs(field_vals);
    let item_k = key::item(&kc, key_bytes, next_entry_id.ms, next_entry_id.seq);
    batch.insert_data(&item_k, &encoded_payload);

    meta.last_generated_id = next_entry_id;
    meta.last_entry_id = next_entry_id;
    meta.base.size += 1;

    if meta.base.size == 1 {
      meta.first_entry_id = next_entry_id;
      meta.recorded_first_entry_id = next_entry_id;
    }
  } else {
    meta.last_generated_id = next_entry_id;
    meta.max_deleted_entry_id = next_entry_id;
  }

  meta.entries_added += 1;
  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;

  Ok(next_entry_id)
}

/// Returns the number of entries in a stream (XLEN).
/// 获取流中条目总数
pub fn stream_len<E: Engine, K: AsRef<[u8]>>(db: &Db<E>, key: K) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => Ok(meta.base.size),
    None => Ok(0),
  }
}

/// Retrieves stream length with options aligned with Apache Kvrocks Stream::Len.
/// XLEN key with options（对标 Apache Kvrocks Stream::Len，通过 range seek 快速定位）
pub fn stream_len_with_options<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  options: StreamLen,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => return Ok(0),
  };

  if !options.with_entry_id {
    return Ok(meta.base.size);
  }

  if options.entry_id > meta.last_entry_id {
    return Ok(if options.to_first { meta.base.size } else { 0 });
  }
  if options.entry_id < meta.first_entry_id {
    return Ok(if options.to_first { 0 } else { meta.base.size });
  }
  if (!options.to_first && options.entry_id == meta.first_entry_id)
    || (options.to_first && options.entry_id == meta.last_entry_id)
  {
    return Ok(meta.base.size.saturating_sub(1));
  }

  let kc = db.kc();
  let prefix = key::prefix_stack(&kc, key_bytes);
  let target_k = key::item(&kc, key_bytes, options.entry_id.ms, options.entry_id.seq);
  let data_ks = db.data();
  let mut count = 0u64;

  if options.to_first {
    for g in data_ks.range((
      Bound::Included(prefix.as_slice()),
      Bound::Included(target_k.as_slice()),
    )) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
        if sid >= options.entry_id {
          break;
        }
        count += 1;
      }
    }
  } else {
    for g in data_ks.range((Bound::Included(target_k.as_slice()), Bound::Unbounded)) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..])
        && sid > options.entry_id
      {
        count += 1;
      }
    }
  }

  Ok(count)
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn xlast_id<K: AsRef<[u8]>>(&self, key: K) -> Result<StreamId> {
    stream_last_id(self, key)
  }

  #[inline]
  pub fn xadd<K: AsRef<[u8]>, FK: AsRef<[u8]>, FV: AsRef<[u8]>>(
    &self,
    key: K,
    options: impl Into<StreamAdd>,
    field_vals: &[(FK, FV)],
  ) -> Result<StreamId> {
    stream_add(self, key, options, field_vals)
  }

  #[inline]
  pub fn xlen<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    stream_len(self, key)
  }
}
