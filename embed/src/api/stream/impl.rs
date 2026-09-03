use std::ops::Bound;

use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::stream::{
    StreamEntry,
    r#const::*,
    decode_stream_entry_fields, encode_stream_entry_pairs, key,
    meta::{StreamId, StreamMeta},
    opt::{StreamAdd, StreamLen, StreamRange, StreamTrim, StreamTrimStrategy},
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

/// Internal stream trimming logic aligned with Apache Kvrocks Stream::trim.
/// 内部流裁剪逻辑（对标 Apache Kvrocks Stream::trim）
fn trim_stream_internal<E: Engine>(
  db: &Db<E>,
  meta: &mut StreamMeta,
  key: &[u8],
  options: StreamTrim,
  batch: &mut DbBatch<E>,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  if meta.base.size == 0 {
    return Ok(0);
  }
  if options.strategy == StreamTrimStrategy::MaxLen && meta.base.size <= options.max_len {
    return Ok(0);
  }
  if options.strategy == StreamTrimStrategy::MinId && meta.first_entry_id >= options.min_id {
    return Ok(0);
  }

  let kc = db.kc();
  let prefix = key::prefix_stack(&kc, key);
  let data_ks = db.data();
  let mut delete_cnt = 0u64;
  let mut last_deleted_id = StreamId::min();
  let mut new_first_id = StreamId::min();
  let mut found_next_first = false;

  let mut iter = data_ks.prefix(&prefix);
  while let Some(g) = iter.next() {
    let entry = g?;
    let k = entry.key();
    if !k.starts_with(prefix.as_slice()) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
      if options.strategy == StreamTrimStrategy::MaxLen
        && (meta.base.size.saturating_sub(delete_cnt)) <= options.max_len
      {
        new_first_id = sid;
        found_next_first = true;
        break;
      }
      if options.strategy == StreamTrimStrategy::MinId && sid >= options.min_id {
        new_first_id = sid;
        found_next_first = true;
        break;
      }

      delete_cnt += 1;
      last_deleted_id = sid;
      batch.rm_weak_data(k);

      if let Some(lim) = options.limit
        && delete_cnt as usize >= lim
      {
        for next_g in iter.by_ref() {
          let next_entry = next_g?;
          let nk = next_entry.key();
          if !nk.starts_with(prefix.as_slice()) {
            break;
          }
          if let Some(nsid) = parse_stream_id_from_subkey(&nk[prefix.len()..]) {
            new_first_id = nsid;
            found_next_first = true;
            break;
          }
        }
        break;
      }
    }
  }

  if delete_cnt > 0 {
    meta.base.size = meta.base.size.saturating_sub(delete_cnt);
    if last_deleted_id > meta.max_deleted_entry_id {
      meta.max_deleted_entry_id = last_deleted_id;
    }
    if meta.base.size == 0 || !found_next_first {
      meta.base.size = 0;
      meta.first_entry_id.clear();
      meta.last_entry_id.clear();
      meta.recorded_first_entry_id.clear();
    } else if found_next_first {
      meta.first_entry_id = new_first_id;
      meta.recorded_first_entry_id = new_first_id;
    }
  }

  Ok(delete_cnt)
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
  let _meta_ks = db.meta();
  let _data_ks = db.data();

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
    trim_stream_internal(db, &mut meta, key_bytes, trim_opts, &mut batch)?;

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
/// XLEN key with options（对标 Apache Kvrocks Stream::Len）
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
  let data_ks = db.data();
  let mut count = 0u64;
  for g in data_ks.prefix(&prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if !k.starts_with(prefix.as_slice()) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
      if options.to_first {
        if sid >= options.entry_id {
          break;
        }
        count += 1;
      } else if sid > options.entry_id {
        count += 1;
      }
    }
  }

  Ok(count)
}

/// Returns stream entries within a range (XRANGE).
/// 获取指定 ID 范围内的流条目
pub fn stream_range<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  start: StreamId,
  end: StreamId,
  count: Option<usize>,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let options = StreamRange {
    start,
    end,
    count,
    reverse: false,
    exclude_start: false,
    exclude_end: false,
  };
  stream_range_with_options(db, key, options)
}

/// Returns stream entries in reverse order within a range (XREVRANGE).
/// 逆序获取指定 ID 范围内的流条目
pub fn stream_revrange<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  end: StreamId,
  start: StreamId,
  count: Option<usize>,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let options = StreamRange {
    start: end,
    end: start,
    count,
    reverse: true,
    exclude_start: false,
    exclude_end: false,
  };
  stream_range_with_options(db, key, options)
}

/// Scans stream entries within range aligned with Apache Kvrocks Stream::Range.
/// XRANGE / XREVRANGE 配置扫描（对标 Apache Kvrocks Stream::Range）
pub fn stream_range_with_options<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  options: StreamRange,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  if options.exclude_start && options.start.is_max() {
    return Err(Error::invalid_data(ERR_INVALID_START_ID_INTERVAL));
  }
  if options.exclude_end && options.end.is_min() {
    return Err(Error::invalid_data(ERR_INVALID_END_ID_INTERVAL));
  }
  if let Some(0) = options.count {
    return Ok(Vec::new());
  }

  if (!options.reverse && options.end < options.start)
    || (options.reverse && options.start < options.end)
  {
    return Ok(Vec::new());
  }

  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Ok(Vec::new());
  }

  let kc = db.kc();
  let data_ks = db.data();

  // 单点查询快速路径（O(1) 直接点查）
  if options.start == options.end {
    if options.exclude_start || options.exclude_end {
      return Ok(Vec::new());
    }
    let item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    if let Some(v) = data_ks.get(&item_k)? {
      let fields = decode_stream_entry_fields(&v).unwrap_or_default();
      return Ok(vec![(options.start, fields)]);
    }
    return Ok(Vec::new());
  }

  let prefix = key::prefix_stack(&kc, key_bytes);
  let max_count = options.count.unwrap_or(usize::MAX);

  if !options.reverse {
    let start_item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    let end_item_k = key::item(&kc, key_bytes, options.end.ms, options.end.seq);
    let mut results = Vec::with_capacity(max_count.min(1024));

    for g in data_ks.range((
      Bound::Included(start_item_k.as_slice()),
      Bound::Included(end_item_k.as_slice()),
    )) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
        if options.exclude_start && sid <= options.start {
          continue;
        }
        if options.exclude_end && sid >= options.end {
          break;
        }
        if sid > options.end {
          break;
        }

        let fields = decode_stream_entry_fields(v).unwrap_or_default();
        results.push((sid, fields));
        if results.len() >= max_count {
          break;
        }
      }
    }
    Ok(results)
  } else {
    let low_item_k = key::item(&kc, key_bytes, options.end.ms, options.end.seq);
    let high_item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    let mut results = Vec::with_capacity(max_count.min(1024));

    for g in data_ks
      .range((
        Bound::Included(low_item_k.as_slice()),
        Bound::Included(high_item_k.as_slice()),
      ))
      .rev()
    {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
        if options.exclude_start && sid >= options.start {
          continue;
        }
        if sid > options.start {
          continue;
        }
        if options.exclude_end && sid <= options.end {
          break;
        }
        if sid < options.end {
          break;
        }

        let fields = decode_stream_entry_fields(v).unwrap_or_default();
        results.push((sid, fields));
        if results.len() >= max_count {
          break;
        }
      }
    }
    Ok(results)
  }
}

/// Trims stream entries (XTRIM) aligned with Apache Kvrocks Stream::Trim.
/// XTRIM key [MAXLEN|MINID ...]（对标 Apache Kvrocks Stream::Trim）
pub fn stream_trim<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  options: StreamTrim,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(m) => m,
    None => return Ok(0),
  };

  let mut batch = db.batch();
  let delete_cnt = trim_stream_internal(db, &mut meta, key_bytes, options, &mut batch)?;
  if delete_cnt > 0 {
    let meta_k = key::meta(&db.kc(), key_bytes);
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
  }

  Ok(delete_cnt)
}

/// Deletes stream entries by ID (XDEL) aligned with Apache Kvrocks Stream::DeleteEntries.
/// XDEL key id [id ...]（对标 Apache Kvrocks Stream::DeleteEntries）
pub fn stream_del<E: Engine, K: AsRef<[u8]>>(db: &Db<E>, key: K, ids: &[StreamId]) -> Result<u64>
where
  Error: From<E::Error>,
{
  if ids.is_empty() {
    return Ok(0);
  }

  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  let meta_k = key::meta(&db.kc(), key_bytes);
  let _meta_ks = db.meta();

  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(m) => m,
    None => return Ok(0),
  };

  if meta.base.size == 0 {
    return Ok(0);
  }

  let kc = db.kc();
  let data_ks = db.data();
  let mut batch = db.batch();
  let mut deleted_cnt = 0u64;
  let mut deleted_ids = HashSet::with_capacity(ids.len());

  for &id in ids {
    if !deleted_ids.insert(id) {
      continue;
    }
    let item_k = key::item(&kc, key_bytes, id.ms, id.seq);
    if data_ks.contains_key(&item_k)? {
      batch.rm_weak_data(&item_k);
      deleted_cnt += 1;
      meta.max_deleted_entry_id = meta.max_deleted_entry_id.max(id);
    }
  }

  if deleted_cnt > 0 {
    meta.base.size = meta.base.size.saturating_sub(deleted_cnt);
    if meta.base.size == 0 {
      meta.first_entry_id.clear();
      meta.last_entry_id.clear();
      meta.recorded_first_entry_id.clear();
    } else {
      let need_new_first = deleted_ids.contains(&meta.first_entry_id);
      let need_new_last = deleted_ids.contains(&meta.last_entry_id);

      if need_new_first || need_new_last {
        let prefix = key::prefix_stack(&kc, key_bytes);
        if need_new_first {
          for g in data_ks.prefix(&prefix) {
            let entry = g?;
            let k = entry.key();
            if !k.starts_with(prefix.as_slice()) {
              break;
            }
            if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..])
              && !deleted_ids.contains(&sid)
            {
              meta.first_entry_id = sid;
              meta.recorded_first_entry_id = sid;
              break;
            }
          }
        }
        if need_new_last {
          for g in data_ks.prefix(&prefix).rev() {
            let entry = g?;
            let k = entry.key();
            if !k.starts_with(prefix.as_slice()) {
              break;
            }
            if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..])
              && !deleted_ids.contains(&sid)
            {
              meta.last_entry_id = sid;
              break;
            }
          }
        }
      }
    }

    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
  }

  Ok(deleted_cnt)
}

/// Sets stream last-id and statistics (XSETID) aligned with Apache Kvrocks Stream::SetId.
/// XSETID key last-id [ENTRIESADDED entries_added] [MAXDELETEDID max_deleted_id]（对标 Apache Kvrocks Stream::SetId）
pub fn stream_setid<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  last_generated_id: StreamId,
  entries_added: Option<u64>,
  max_deleted_id: Option<StreamId>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  if let Some(max_del) = max_deleted_id
    && last_generated_id < max_del
  {
    return Err(Error::invalid_data(ERR_SET_ID_MAX_DEL_GREATER));
  }

  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let meta_ks = db.meta();

  let now_ms = current_now_ms();
  let opt_m = get_stream_meta(db, key_bytes, now_ms)?;
  let is_empty = opt_m.is_none();

  if is_empty {
    if entries_added.is_none() || entries_added == Some(0) {
      return Err(Error::invalid_data(ERR_EMPTY_STREAM_ENTRIES_ADDED));
    }
    if max_deleted_id.is_none() || max_deleted_id == Some(StreamId::min()) {
      return Err(Error::invalid_data(ERR_EMPTY_STREAM_MAX_DELETED));
    }
  }

  let mut meta = match opt_m {
    Some(decoded) => decoded,
    None => StreamMeta::new(0, generate_version()),
  };

  if meta.base.size > 0 && last_generated_id < meta.last_generated_id {
    return Err(Error::invalid_data(ERR_SET_ID_SMALLER_THAN_TOP));
  }

  if meta.base.size > 0
    && let Some(ea) = entries_added
    && ea < meta.base.size
  {
    return Err(Error::invalid_data(ERR_SET_ID_ENTRIES_ADDED_SMALLER));
  }

  meta.last_generated_id = last_generated_id;
  if let Some(ea) = entries_added {
    meta.entries_added = ea;
  }
  if let Some(max_del) = max_deleted_id
    && !max_del.is_min()
  {
    meta.max_deleted_entry_id = max_del;
  }

  meta_ks.insert(&meta_k, &meta.encode())?;
  Ok(())
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

  #[inline]
  pub fn xrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    options: impl Into<StreamRange>,
  ) -> Result<Vec<StreamEntry>> {
    let mut opts = options.into();
    opts.reverse = false;
    stream_range_with_options(self, key, opts)
  }

  #[inline]
  pub fn xrevrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    options: impl Into<StreamRange>,
  ) -> Result<Vec<StreamEntry>> {
    let mut opts = options.into();
    opts.reverse = true;
    stream_range_with_options(self, key, opts)
  }

  #[inline]
  pub fn xtrim<K: AsRef<[u8]>>(&self, key: K, options: StreamTrim) -> Result<u64> {
    stream_trim(self, key, options)
  }

  #[inline]
  pub fn xdel<K: AsRef<[u8]>>(&self, key: K, ids: &[StreamId]) -> Result<u64> {
    stream_del(self, key, ids)
  }

  #[inline]
  pub fn xsetid<K: AsRef<[u8]>>(
    &self,
    key: K,
    last_generated_id: StreamId,
    entries_added: Option<u64>,
    max_deleted_id: Option<StreamId>,
  ) -> Result<()> {
    stream_setid(self, key, last_generated_id, entries_added, max_deleted_id)
  }
}
