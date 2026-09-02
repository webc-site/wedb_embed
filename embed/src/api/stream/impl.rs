use std::{ops::Bound, str};

use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::stream::{
    StreamEntry,
    r#const::*,
    decode_stream_entry_fields, encode_stream_entry_pairs, key,
    meta::{
      StreamAutoClaimResult, StreamClaimResult, StreamConsumerGroupMeta, StreamConsumerMeta,
      StreamGetPendingEntryResult, StreamId, StreamInfo, StreamMeta, StreamNack, StreamPelEntry,
      StreamReadResult,
    },
    opt::{
      StreamAdd, StreamAutoClaim, StreamClaim, StreamLen, StreamPending, StreamRange, StreamTrim,
      StreamTrimStrategy,
    },
    parse_stream_id_from_subkey,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::{current_now_ms, generate_version},
  wedb::{Db, DbBatch},
};

/// Calculates consumer group lag (aligned with Kvrocks CheckLagValid).
/// 对标 Kvrocks CheckLagValid 计算消费者组 Lag
pub fn check_lag_valid(stream_meta: &StreamMeta, group_meta: &mut StreamConsumerGroupMeta) {
  let mut valid = false;
  if stream_meta.entries_added == 0 {
    group_meta.lag = 0;
    valid = true;
  } else if group_meta.entries_read != -1
    && !stream_range_has_tombstones(stream_meta, group_meta.last_delivered_id)
  {
    group_meta.lag = stream_meta
      .entries_added
      .saturating_sub(group_meta.entries_read as u64);
    valid = true;
  } else {
    let entries_read =
      stream_estimate_distance_from_first_ever_entry(stream_meta, group_meta.last_delivered_id);
    if entries_read != -1 {
      group_meta.lag = stream_meta
        .entries_added
        .saturating_sub(entries_read as u64);
      valid = true;
    }
  }
  if !valid {
    group_meta.lag = u64::MAX;
  }
}

/// Checks whether stream range contains tombstone entries aligned with Kvrocks StreamRangeHasTombstones.
/// 对标 Kvrocks StreamRangeHasTombstones
fn stream_range_has_tombstones(meta: &StreamMeta, start_id: StreamId) -> bool {
  let end_id = StreamId::max();
  if meta.base.size == 0 || meta.max_deleted_entry_id.is_min() {
    return false;
  }
  if meta.first_entry_id > meta.max_deleted_entry_id {
    return false;
  }
  start_id <= meta.max_deleted_entry_id && meta.max_deleted_entry_id <= end_id
}

/// Estimates distance from first ever entry aligned with Kvrocks StreamEstimateDistanceFromFirstEverEntry.
/// 对标 Kvrocks StreamEstimateDistanceFromFirstEverEntry
fn stream_estimate_distance_from_first_ever_entry(meta: &StreamMeta, id: StreamId) -> i64 {
  if meta.entries_added == 0 {
    return 0;
  }
  if meta.base.size == 0 && id < meta.last_entry_id {
    return meta.entries_added as i64;
  }
  if id == meta.last_entry_id {
    return meta.entries_added as i64;
  } else if id > meta.last_entry_id {
    return -1;
  }
  if meta.max_deleted_entry_id.is_min() || meta.max_deleted_entry_id < meta.first_entry_id {
    if id < meta.first_entry_id {
      return meta.entries_added.saturating_sub(meta.base.size) as i64;
    } else if id == meta.first_entry_id {
      return (meta.entries_added.saturating_sub(meta.base.size) + 1) as i64;
    }
  }
  -1
}

/// Retrieves live non-expired StreamMeta (internal helper method).
/// 获取当前未过期的有效 StreamMeta（内部辅助方法）
#[inline]
fn get_stream_meta<E: Engine>(db: &Db<E>, key: &[u8], now_ms: u64) -> Result<Option<StreamMeta>>
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
fn clean_stream_residue<E: Engine>(db: &Db<E>, key: &[u8], batch: &mut DbBatch<E>) -> Result<()>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let data_ks = db.data();
  let s_prefix = key::prefix(&kc, key);
  for g in data_ks.prefix(&s_prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if k.starts_with(&s_prefix) {
      batch.rm_data(k);
    }
  }
  let g_prefix = key::group_prefix(&kc, key);
  for g in data_ks.prefix(&g_prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if k.starts_with(&g_prefix) {
      batch.rm_data(k);
    }
  }
  let c_prefix = key::consumer_prefix_all(&kc, key);
  for g in data_ks.prefix(&c_prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if k.starts_with(&c_prefix) {
      batch.rm_data(k);
    }
  }
  let p_prefix = key::pel_prefix_all(&kc, key);
  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if k.starts_with(&p_prefix) {
      batch.rm_data(k);
    }
  }
  Ok(())
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
  let prefix = key::prefix(&kc, key);
  let data_ks = db.data();
  let mut delete_cnt = 0u64;
  let mut last_deleted_id = StreamId::min();
  let mut new_first_id = StreamId::min();
  let mut found_next_first = false;

  let mut iter = data_ks.prefix(&prefix);
  while let Some(g) = iter.next() {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if !k.starts_with(&prefix) {
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
          let (nk, _) = (next_entry.key(), next_entry.value());
          if !nk.starts_with(&prefix) {
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
  let prefix = key::prefix(&kc, key_bytes);
  let data_ks = db.data();
  let mut count = 0u64;
  for g in data_ks.prefix(&prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if !k.starts_with(&prefix) {
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

  let prefix = key::prefix(&kc, key_bytes);
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
      if !k.starts_with(&prefix) {
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
      if !k.starts_with(&prefix) {
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
  let meta_k = key::meta(&db.kc(), key_bytes);
  let _meta_ks = db.meta();

  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(m) => m,
    None => return Ok(0),
  };

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
  let prefix = key::prefix(&kc, key_bytes);
  let data_ks = db.data();
  let mut delete_cnt = 0u64;
  let mut last_deleted_id = StreamId::min();
  let mut new_first_id = StreamId::min();
  let mut found_next_first = false;

  let mut batch = db.batch();
  let mut iter = data_ks.prefix(&prefix);
  while let Some(g) = iter.next() {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if !k.starts_with(&prefix) {
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
          let (nk, _) = (next_entry.key(), next_entry.value());
          if !nk.starts_with(&prefix) {
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

  if delete_cnt == 0 {
    return Ok(0);
  }

  meta.base.size = meta.base.size.saturating_sub(delete_cnt);
  if meta.base.size == 0 {
    meta.first_entry_id.clear();
    meta.last_entry_id.clear();
    meta.recorded_first_entry_id.clear();
  } else if found_next_first {
    meta.first_entry_id = new_first_id;
    meta.recorded_first_entry_id = new_first_id;
  }
  if !last_deleted_id.is_min() {
    meta.max_deleted_entry_id = meta.max_deleted_entry_id.max(last_deleted_id);
  }

  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;

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
        let prefix = key::prefix(&kc, key_bytes);
        if need_new_first {
          for g in data_ks.prefix(&prefix) {
            let entry = g?;
            let (k, _) = (entry.key(), entry.value());
            if !k.starts_with(&prefix) {
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
            let (k, _) = (entry.key(), entry.value());
            if !k.starts_with(&prefix) {
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

/// Creates a stream consumer group (XGROUP CREATE) aligned with Apache Kvrocks Stream::CreateGroup.
/// XGROUP CREATE key groupname id|$ [MKSTREAM] [ENTRIESREAD entries_read]（对标 Apache Kvrocks Stream::CreateGroup）
pub fn stream_group_create<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  last_id: &str,
  mkstream: bool,
  entries_read: Option<i64>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let _meta_ks = db.meta();
  let data_ks = db.data();

  let now_ms = current_now_ms();
  let opt_m = get_stream_meta(db, key_bytes, now_ms)?;
  let mut need_clean_residue = false;

  let mut meta = match opt_m {
    Some(decoded) => decoded,
    None => {
      if !mkstream {
        return Err(Error::invalid_data(ERR_XGROUP_KEY_REQUIRE_EXIST));
      }
      need_clean_residue = true;
      StreamMeta::new(0, generate_version())
    }
  };

  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  if data_ks.contains_key(&group_k)? {
    return Err(Error::redis(ERR_GROUP_BUSY));
  }

  let last_delivered_id = if last_id == "$" {
    meta.last_entry_id
  } else {
    StreamId::parse(last_id)?
  };

  let group_meta = StreamConsumerGroupMeta {
    consumer_number: 0,
    pending_number: 0,
    last_delivered_id,
    entries_read: entries_read.unwrap_or(-1),
    lag: 0,
  };

  let mut batch = db.batch();

  if need_clean_residue {
    clean_stream_residue(db, key_bytes, &mut batch)?;
  }

  batch.insert_data(&group_k, &group_meta.encode());
  meta.group_number += 1;
  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;

  Ok(())
}

/// Destroys a stream consumer group (XGROUP DESTROY) aligned with Apache Kvrocks Stream::DestroyGroup.
/// XGROUP DESTROY key groupname（对标 Apache Kvrocks Stream::DestroyGroup）
pub fn stream_group_destroy<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
) -> Result<bool>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let _meta_ks = db.meta();
  let data_ks = db.data();

  let now_ms = current_now_ms();
  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
    }
  };

  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  if !data_ks.contains_key(&group_k)? {
    return Ok(false);
  }

  let mut batch = db.batch();
  batch.rm_data(&group_k);

  let c_prefix = key::consumer_prefix(&kc, key_bytes, group_name.as_bytes());
  for g in data_ks.prefix(&c_prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if k.starts_with(&c_prefix) {
      batch.rm_data(k);
    }
  }

  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, _) = (entry.key(), entry.value());
    if k.starts_with(&p_prefix) {
      batch.rm_data(k);
    }
  }

  meta.group_number = meta.group_number.saturating_sub(1);
  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;

  Ok(true)
}

/// Creates a consumer in consumer group (XGROUP CREATECONSUMER) aligned with Kvrocks.
/// XGROUP CREATECONSUMER key groupname consumername（对标 Apache Kvrocks Stream::CreateConsumer）
pub fn stream_group_create_consumer<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
) -> Result<i32>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_KEY_GROUP_MUST_EXIST));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  if data_ks.contains_key(&consumer_k)? {
    return Ok(0);
  }

  let consumer_meta = StreamConsumerMeta {
    pending_number: 0,
    last_attempted_interaction_ms: now_ms,
    last_successful_interaction_ms: now_ms,
  };

  group_meta.consumer_number += 1;
  let mut batch = db.batch();
  batch.insert_data(&consumer_k, &consumer_meta.encode());
  batch.insert_data(&group_k, &group_meta.encode());
  batch.commit()?;

  Ok(1)
}

/// Deletes a consumer from consumer group (XGROUP DELCONSUMER) aligned with Kvrocks.
/// XGROUP DELCONSUMER key groupname consumername（对标 Apache Kvrocks Stream::DeleteConsumer）
pub fn stream_group_del_consumer<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_GROUP_MUST_EXIST));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let consumer_bytes = match data_ks.get(&consumer_k)? {
    Some(b) => b,
    None => return Ok(0),
  };
  let consumer_meta = StreamConsumerMeta::decode(&consumer_bytes).unwrap_or_default();
  let deleted_pel = consumer_meta.pending_number;

  let mut batch = db.batch();
  batch.rm_data(&consumer_k);

  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if k.starts_with(&p_prefix)
      && let Some(pel) = StreamPelEntry::decode(v)
      && pel.consumer_name == consumer_name
    {
      batch.rm_data(k);
    }
  }

  group_meta.consumer_number = group_meta.consumer_number.saturating_sub(1);
  group_meta.pending_number = group_meta.pending_number.saturating_sub(deleted_pel);
  batch.insert_data(&group_k, &group_meta.encode());
  batch.commit()?;

  Ok(deleted_pel)
}

/// Sets consumer group last delivered ID (XGROUP SETID) aligned with Kvrocks.
/// XGROUP SETID key groupname id|$ [ENTRIESREAD entries_read]（对标 Apache Kvrocks Stream::GroupSetId）
pub fn stream_group_set_id<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  last_id: &str,
  entries_read: Option<i64>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
    }
  };

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_GROUP_MUST_EXIST));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let parsed_id = if last_id == "$" {
    meta.last_entry_id
  } else {
    StreamId::parse(last_id)?
  };

  group_meta.last_delivered_id = parsed_id;
  if let Some(er) = entries_read {
    group_meta.entries_read = er;
  }

  data_ks.insert(&group_k, &group_meta.encode())?;
  Ok(())
}

/// Acknowledges pending stream messages (XACK) aligned with Apache Kvrocks Stream::DeletePelEntries.
/// XACK key groupname id [id ...]（对标 Apache Kvrocks Stream::DeletePelEntries）
pub fn stream_ack<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  entry_ids: &[StreamId],
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());

  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => return Ok(0),
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let mut acknowledged = 0u64;
  let mut consumer_acks: HashMap<String, u64> = HashMap::new();
  let mut seen_ids: HashSet<StreamId> = HashSet::default();
  let mut batch = db.batch();

  for &id in entry_ids {
    if !seen_ids.insert(id) {
      continue;
    }
    let pel_k = key::pel_item(&kc, key_bytes, group_name.as_bytes(), id.ms, id.seq);
    if let Some(pel_bytes) = data_ks.get(&pel_k)? {
      if let Some(pel_entry) = StreamPelEntry::decode(&pel_bytes) {
        *consumer_acks.entry(pel_entry.consumer_name).or_insert(0) += 1;
      }
      acknowledged += 1;
      batch.rm_data(&pel_k);
    }
  }

  if acknowledged > 0 {
    group_meta.pending_number = group_meta.pending_number.saturating_sub(acknowledged);
    batch.insert_data(&group_k, &group_meta.encode());

    for (consumer_name, ack_cnt) in consumer_acks {
      let consumer_k = key::consumer_meta(
        &kc,
        key_bytes,
        group_name.as_bytes(),
        consumer_name.as_bytes(),
      );
      if let Some(c_bytes) = data_ks.get(&consumer_k)?
        && let Some(mut c_meta) = StreamConsumerMeta::decode(&c_bytes)
      {
        c_meta.pending_number = c_meta.pending_number.saturating_sub(ack_cnt);
        batch.insert_data(&consumer_k, &c_meta.encode());
      }
    }
    batch.commit()?;
  }

  Ok(acknowledged)
}

/// Changes the ownership of pending stream messages (XCLAIM).
/// 转移待处理流消息的所有权
pub fn stream_claim<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
  min_idle_time_ms: u64,
  entry_ids: &[StreamId],
  options: StreamClaim,
) -> Result<StreamClaimResult>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::not_found(ERR_GROUP_NOT_FOUND));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let mut consumer_meta = match data_ks.get(&consumer_k)? {
    Some(b) => StreamConsumerMeta::decode(&b).unwrap_or_default(),
    None => {
      group_meta.consumer_number += 1;
      StreamConsumerMeta::default()
    }
  };

  consumer_meta.last_attempted_interaction_ms = now_ms;
  consumer_meta.last_successful_interaction_ms = now_ms;

  let mut result = StreamClaimResult::default();
  let mut seen_ids: HashSet<StreamId> = HashSet::default();
  let mut batch = db.batch();

  for &id in entry_ids {
    if !seen_ids.insert(id) {
      continue;
    }
    let item_k = key::item(&kc, key_bytes, id.ms, id.seq);
    let raw_item_val = data_ks.get(&item_k)?;
    if raw_item_val.is_none() {
      continue;
    }

    let pel_k = key::pel_item(&kc, key_bytes, group_name.as_bytes(), id.ms, id.seq);
    let raw_pel_val = data_ks.get(&pel_k)?;

    let is_forced = raw_pel_val.is_none() && options.force;
    let mut pel_entry = if let Some(b) = raw_pel_val {
      StreamPelEntry::decode(&b).unwrap_or(StreamPelEntry {
        last_delivery_time_ms: 0,
        last_delivery_count: 0,
        consumer_name: String::new(),
      })
    } else if is_forced {
      StreamPelEntry {
        last_delivery_time_ms: 0,
        last_delivery_count: 0,
        consumer_name: String::new(),
      }
    } else {
      continue;
    };

    if now_ms.saturating_sub(pel_entry.last_delivery_time_ms) < min_idle_time_ms {
      continue;
    }

    if is_forced {
      group_meta.pending_number += 1;
    }

    if options.just_id {
      result.ids.push(id);
    } else if let Some(val_bytes) = raw_item_val {
      let fields = decode_stream_entry_fields(&val_bytes).unwrap_or_default();
      result.entries.push((id, fields));
    }

    if !pel_entry.consumer_name.is_empty() && pel_entry.consumer_name != consumer_name {
      let orig_k = key::consumer_meta(
        &kc,
        key_bytes,
        group_name.as_bytes(),
        pel_entry.consumer_name.as_bytes(),
      );
      if let Some(orig_b) = data_ks.get(&orig_k)?
        && let Some(mut orig_c_meta) = StreamConsumerMeta::decode(&orig_b)
      {
        orig_c_meta.pending_number = orig_c_meta.pending_number.saturating_sub(1);
        batch.insert_data(&orig_k, &orig_c_meta.encode());
      }
    }

    if pel_entry.consumer_name != consumer_name {
      consumer_meta.pending_number += 1;
      pel_entry.consumer_name = consumer_name.to_string();
    }

    if options.with_time {
      pel_entry.last_delivery_time_ms = options.last_delivery_time_ms;
    } else {
      pel_entry.last_delivery_time_ms = now_ms.saturating_sub(options.idle_time_ms);
    }
    if pel_entry.last_delivery_time_ms > now_ms {
      pel_entry.last_delivery_time_ms = now_ms;
    }

    if options.with_retry_count {
      pel_entry.last_delivery_count = options.last_delivery_count;
    } else if !options.just_id {
      pel_entry.last_delivery_count += 1;
    }

    batch.insert_data(&pel_k, &pel_entry.encode());
  }

  if let Some(last_deliv) = options.last_delivered_id
    && last_deliv > group_meta.last_delivered_id
  {
    group_meta.last_delivered_id = last_deliv;
  }

  batch.insert_data(&consumer_k, &consumer_meta.encode());
  batch.insert_data(&group_k, &group_meta.encode());
  batch.commit()?;

  Ok(result)
}

/// Automatically claims pending stream messages (XAUTOCLAIM) aligned with Apache Kvrocks Stream::AutoClaim.
/// XAUTOCLAIM key group consumer min-idle-time start [COUNT count] [JUSTID]（对标 Apache Kvrocks Stream::AutoClaim）
pub fn stream_autoclaim<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
  options: StreamAutoClaim,
) -> Result<StreamAutoClaimResult>
where
  Error: From<E::Error>,
{
  if options.exclude_start && options.start_id.is_max() {
    return Err(Error::invalid_data(ERR_INVALID_START_ID_INTERVAL));
  }

  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::not_found(ERR_GROUP_NOT_FOUND));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let mut consumer_meta = match data_ks.get(&consumer_k)? {
    Some(b) => StreamConsumerMeta::decode(&b).unwrap_or_default(),
    None => {
      group_meta.consumer_number += 1;
      StreamConsumerMeta::default()
    }
  };

  consumer_meta.last_attempted_interaction_ms = now_ms;
  consumer_meta.last_successful_interaction_ms = now_ms;

  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  let mut count = options.count;
  let mut attempts = options.attempts_factors * count;

  let mut deleted_entries = Vec::new();
  let mut claimed_entries = Vec::new();
  let mut next_claim_id = StreamId::min();
  let mut has_next = false;
  let mut batch = db.batch();

  let mut claimed_from_others: HashMap<String, u64> = HashMap::new();
  let mut deleted_from_consumers: HashMap<String, u64> = HashMap::new();

  let mut iter = data_ks.prefix(&p_prefix);
  while let Some(g) = iter.next() {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&p_prefix) {
      break;
    }
    let sid = match parse_stream_id_from_subkey(&k[p_prefix.len()..]) {
      Some(s) => s,
      None => continue,
    };
    if sid < options.start_id || (options.exclude_start && sid == options.start_id) {
      continue;
    }

    if count == 0 || attempts == 0 {
      next_claim_id = sid;
      has_next = true;
      break;
    }

    attempts -= 1;

    if let Some(mut pel_entry) = StreamPelEntry::decode(v) {
      if now_ms.saturating_sub(pel_entry.last_delivery_time_ms) < options.min_idle_time_ms {
        continue;
      }

      let item_k = key::item(&kc, key_bytes, sid.ms, sid.seq);
      let raw_item_val = data_ks.get(&item_k)?;

      if raw_item_val.is_none() {
        deleted_entries.push(sid);
        batch.rm_weak_data(k);
        *deleted_from_consumers
          .entry(pel_entry.consumer_name.clone())
          .or_insert(0) += 1;
        count -= 1;
      } else {
        let fields = if options.just_id {
          Vec::new()
        } else if let Some(ref val_slice) = raw_item_val {
          decode_stream_entry_fields(val_slice).unwrap_or_default()
        } else {
          Vec::new()
        };

        claimed_entries.push((sid, fields));
        count -= 1;

        if pel_entry.consumer_name != consumer_name {
          *claimed_from_others
            .entry(pel_entry.consumer_name.clone())
            .or_insert(0) += 1;
          pel_entry.consumer_name = consumer_name.to_string();
          pel_entry.last_delivery_time_ms = now_ms;
          if !options.just_id {
            pel_entry.last_delivery_count += 1;
          }
          batch.insert_data(k, &pel_entry.encode());
        }
      }

      if count == 0 || attempts == 0 {
        for next_g in iter.by_ref() {
          let next_entry = next_g?;
          let (nk, _) = (next_entry.key(), next_entry.value());
          if !nk.starts_with(&p_prefix) {
            break;
          }
          if let Some(nsid) = parse_stream_id_from_subkey(&nk[p_prefix.len()..]) {
            next_claim_id = nsid;
            has_next = true;
            break;
          }
        }
        break;
      }
    }
  }

  if !has_next {
    next_claim_id = StreamId::min();
  }

  let total_claimed: u64 = claimed_from_others.values().sum();
  let total_deleted = deleted_entries.len() as u64;

  if total_claimed > 0 || total_deleted > 0 {
    consumer_meta.pending_number += total_claimed;
    if let Some(dec) = deleted_from_consumers.get(consumer_name) {
      consumer_meta.pending_number = consumer_meta.pending_number.saturating_sub(*dec);
    }
    batch.insert_data(&consumer_k, &consumer_meta.encode());

    let mut other_decrements: HashMap<String, u64> = claimed_from_others;
    for (c, cnt) in &deleted_from_consumers {
      if c != consumer_name {
        *other_decrements.entry(c.clone()).or_insert(0) += *cnt;
      }
    }

    for (other_consumer, dec_cnt) in other_decrements {
      let other_k = key::consumer_meta(
        &kc,
        key_bytes,
        group_name.as_bytes(),
        other_consumer.as_bytes(),
      );
      if let Some(other_b) = data_ks.get(&other_k)?
        && let Some(mut other_meta) = StreamConsumerMeta::decode(&other_b)
      {
        other_meta.pending_number = other_meta.pending_number.saturating_sub(dec_cnt);
        batch.insert_data(&other_k, &other_meta.encode());
      }
    }

    if total_deleted > 0 {
      group_meta.pending_number = group_meta.pending_number.saturating_sub(total_deleted);
      batch.insert_data(&group_k, &group_meta.encode());
    }
  }

  batch.commit()?;

  Ok(StreamAutoClaimResult {
    next_claim_id,
    entries: claimed_entries,
    deleted_ids: deleted_entries,
  })
}

/// Retrieves pending entries summary (XPENDING summary) aligned with Kvrocks.
/// XPENDING summary（对标 Apache Kvrocks Stream::GetPendingEntries summary）
pub fn stream_pending_summary<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
) -> Result<StreamGetPendingEntryResult>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  let data_ks = db.data();

  let mut first_id = StreamId::max();
  let mut last_id = StreamId::min();
  let mut total_pending = 0u64;
  let mut consumer_counts: HashMap<String, u64> = HashMap::new();
  let mut consumer_order: Vec<String> = Vec::new();

  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&p_prefix) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[p_prefix.len()..])
      && let Some(pel) = StreamPelEntry::decode(v)
    {
      total_pending += 1;
      if sid < first_id {
        first_id = sid;
      }
      if sid > last_id {
        last_id = sid;
      }
      if !consumer_counts.contains_key(&pel.consumer_name) {
        consumer_order.push(pel.consumer_name.clone());
      }
      *consumer_counts.entry(pel.consumer_name).or_insert(0) += 1;
    }
  }

  if total_pending == 0 {
    return Ok(StreamGetPendingEntryResult {
      pending_number: 0,
      first_entry_id: StreamId::min(),
      last_entry_id: StreamId::min(),
      consumer_infos: Vec::new(),
    });
  }

  let consumer_infos = consumer_order
    .into_iter()
    .map(|name| {
      let cnt = consumer_counts.get(&name).copied().unwrap_or(0);
      (name, cnt)
    })
    .collect();
  Ok(StreamGetPendingEntryResult {
    pending_number: total_pending,
    first_entry_id: first_id,
    last_entry_id: last_id,
    consumer_infos,
  })
}

/// Retrieves pending entries within range (XPENDING range) aligned with Kvrocks.
/// XPENDING range（对标 Apache Kvrocks Stream::GetPendingEntries range）
pub fn stream_pending_range<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  options: StreamPending,
) -> Result<Vec<StreamNack>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  let data_ks = db.data();

  let max_count = options.count.unwrap_or(usize::MAX);
  let mut results = Vec::new();
  let now_ms = current_now_ms();

  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&p_prefix) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[p_prefix.len()..]) {
      if options.exclude_end {
        if sid >= options.end_id {
          break;
        }
      } else if sid > options.end_id {
        break;
      }

      let within_start = if options.exclude_start {
        sid > options.start_id
      } else {
        sid >= options.start_id
      };
      let within_end = if options.exclude_end {
        sid < options.end_id
      } else {
        sid <= options.end_id
      };

      if within_start
        && within_end
        && let Some(pel) = StreamPelEntry::decode(v)
      {
        if options.with_time && now_ms.saturating_sub(pel.last_delivery_time_ms) < options.idle_time
        {
          continue;
        }
        if let Some(ref target_c) = options.consumer
          && &pel.consumer_name != target_c
        {
          continue;
        }
        results.push(StreamNack {
          id: sid,
          pel_entry: pel,
        });
        if results.len() >= max_count {
          break;
        }
      }
    }
  }

  Ok(results)
}

/// Reads entries from a stream starting from a given ID (XREAD).
/// 从流中读取指定 ID 之后的新条目
pub fn stream_read<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  start_id: StreamId,
  count: Option<usize>,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let options = StreamRange {
    start: start_id,
    end: StreamId::max(),
    count,
    reverse: false,
    exclude_start: true,
    exclude_end: false,
  };
  stream_range_with_options(db, key, options)
}

/// Reads entries across multiple streams (multi-key XREAD).
/// XREAD 多流联合查询
pub fn stream_read_streams<E: Engine>(
  db: &Db<E>,
  streams: &[(&str, StreamId)],
  count: Option<usize>,
) -> Result<Vec<StreamReadResult>>
where
  Error: From<E::Error>,
{
  let mut results = Vec::with_capacity(streams.len());
  for &(stream_name, start_id) in streams {
    let entries = stream_read(db, stream_name, start_id, count)?;
    if !entries.is_empty() {
      results.push(StreamReadResult {
        name: stream_name.to_string(),
        entries,
      });
    }
  }
  Ok(results)
}

/// Reads entries via consumer group (XREADGROUP) aligned with Apache Kvrocks Stream::RangeWithPending.
/// XREADGROUP GROUP group consumer [COUNT count] [NOACK] STREAMS key ID（对标 Apache Kvrocks Stream::RangeWithPending）
pub fn stream_readgroup<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
  start_id_str: &str,
  count: Option<usize>,
  noack: bool,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::not_found(ERR_GROUP_NOT_FOUND));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let mut consumer_meta = match data_ks.get(&consumer_k)? {
    Some(b) => StreamConsumerMeta::decode(&b).unwrap_or_default(),
    None => {
      group_meta.consumer_number += 1;
      StreamConsumerMeta::default()
    }
  };

  consumer_meta.last_attempted_interaction_ms = now_ms;
  consumer_meta.last_successful_interaction_ms = now_ms;

  let max_count = count.unwrap_or(usize::MAX);
  let mut batch = db.batch();

  if start_id_str == ">" {
    let start_id = group_meta.last_delivered_id;
    let options = StreamRange {
      start: start_id,
      end: StreamId::max(),
      count: Some(max_count),
      reverse: false,
      exclude_start: true,
      exclude_end: false,
    };
    let entries = stream_range_with_options(db, key.as_ref(), options)?;
    let mut max_id = StreamId::min();

    for (sid, _) in &entries {
      if *sid > max_id {
        max_id = *sid;
      }
      if !noack {
        let pel_k = key::pel_item(&kc, key_bytes, group_name.as_bytes(), sid.ms, sid.seq);
        let pel_entry = StreamPelEntry {
          last_delivery_time_ms: now_ms,
          last_delivery_count: 1,
          consumer_name: consumer_name.to_string(),
        };
        batch.insert_data(&pel_k, &pel_entry.encode());
        group_meta.entries_read += 1;
        group_meta.pending_number += 1;
        consumer_meta.pending_number += 1;
      }
    }

    if max_id > group_meta.last_delivered_id {
      group_meta.last_delivered_id = max_id;
    }

    batch.insert_data(&group_k, &group_meta.encode());
    batch.insert_data(&consumer_k, &consumer_meta.encode());
    batch.commit()?;

    Ok(entries)
  } else {
    let start_id = StreamId::parse(start_id_str)?;
    let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
    let mut entries = Vec::new();

    for g in data_ks.prefix(&p_prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&p_prefix) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[p_prefix.len()..])
        && sid >= start_id
        && let Some(mut pel) = StreamPelEntry::decode(v)
        && pel.consumer_name == consumer_name
      {
        let item_k = key::item(&kc, key_bytes, sid.ms, sid.seq);
        if let Some(item_v) = data_ks.get(&item_k)? {
          let fields = decode_stream_entry_fields(&item_v).unwrap_or_default();
          entries.push((sid, fields));
          pel.last_delivery_count += 1;
          pel.last_delivery_time_ms = now_ms;
          batch.insert_data(k, &pel.encode());

          if entries.len() >= max_count {
            break;
          }
        }
      }
    }

    batch.insert_data(&group_k, &group_meta.encode());
    batch.insert_data(&consumer_k, &consumer_meta.encode());
    batch.commit()?;

    Ok(entries)
  }
}

/// Reads entries across multiple streams via consumer group (multi-key XREADGROUP).
/// XREADGROUP 多流联合消费
pub fn stream_readgroup_streams<E: Engine>(
  db: &Db<E>,
  group_name: &str,
  consumer_name: &str,
  streams: &[(&str, &str)],
  count: Option<usize>,
  noack: bool,
) -> Result<Vec<StreamReadResult>>
where
  Error: From<E::Error>,
{
  let mut results = Vec::with_capacity(streams.len());
  for &(stream_name, start_id_str) in streams {
    let entries = stream_readgroup(
      db,
      stream_name,
      group_name,
      consumer_name,
      start_id_str,
      count,
      noack,
    )?;
    if !entries.is_empty() {
      results.push(StreamReadResult {
        name: stream_name.to_string(),
        entries,
      });
    }
  }
  Ok(results)
}

/// Returns stream information (XINFO STREAM) aligned with Apache Kvrocks Stream::GetStreamInfo.
/// XINFO STREAM key [FULL [COUNT count]]（对标 Apache Kvrocks Stream::GetStreamInfo）
pub fn stream_info_stream<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  full: bool,
  count: Option<usize>,
) -> Result<StreamInfo>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
    }
  };

  let kc = db.kc();
  let data_ks = db.data();
  let mut first_entry = None;
  let mut last_entry = None;

  if meta.base.size > 0 && !meta.first_entry_id.is_min() {
    let item_k = key::item(
      &kc,
      key_bytes,
      meta.first_entry_id.ms,
      meta.first_entry_id.seq,
    );
    if let Some(v) = data_ks.get(&item_k)? {
      let fields = decode_stream_entry_fields(&v).unwrap_or_default();
      first_entry = Some((meta.first_entry_id, fields));
    }
  }

  if meta.base.size > 0 && !meta.last_entry_id.is_min() {
    let item_k = key::item(
      &kc,
      key_bytes,
      meta.last_entry_id.ms,
      meta.last_entry_id.seq,
    );
    if let Some(v) = data_ks.get(&item_k)? {
      let fields = decode_stream_entry_fields(&v).unwrap_or_default();
      last_entry = Some((meta.last_entry_id, fields));
    }
  }

  let entries = if full {
    let count_limit = match count {
      Some(0) | None => None,
      Some(c) => Some(c),
    };
    stream_range(
      db,
      key.as_ref(),
      StreamId::min(),
      StreamId::max(),
      count_limit,
    )?
  } else {
    Vec::new()
  };

  Ok(StreamInfo {
    size: meta.base.size,
    entries_added: meta.entries_added,
    last_generated_id: meta.last_generated_id,
    max_deleted_entry_id: meta.max_deleted_entry_id,
    recorded_first_entry_id: meta.recorded_first_entry_id,
    first_entry,
    last_entry,
    groups: meta.group_number,
    entries,
  })
}

/// Returns consumer groups information (XINFO GROUPS) aligned with Apache Kvrocks Stream::GetGroupInfo.
/// XINFO GROUPS key（对标 Apache Kvrocks Stream::GetGroupInfo）
pub fn stream_info_groups<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
) -> Result<Vec<(String, StreamConsumerGroupMeta)>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
    }
  };

  let kc = db.kc();
  let data_ks = db.data();
  let g_prefix = key::group_prefix(&kc, key_bytes);
  let mut groups = Vec::new();

  for g in data_ks.prefix(&g_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&g_prefix) {
      break;
    }
    let group_name = str::from_utf8(&k[g_prefix.len()..])
      .unwrap_or("")
      .to_string();
    if let Some(mut g_meta) = StreamConsumerGroupMeta::decode(v) {
      check_lag_valid(&meta, &mut g_meta);
      groups.push((group_name, g_meta));
    }
  }

  Ok(groups)
}

/// Returns consumer information (XINFO CONSUMERS) aligned with Apache Kvrocks Stream::GetConsumerInfo.
/// XINFO CONSUMERS key groupname（对标 Apache Kvrocks Stream::GetConsumerInfo）
pub fn stream_info_consumers<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
) -> Result<Vec<(String, StreamConsumerMeta)>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let c_prefix = key::consumer_prefix(&kc, key_bytes, group_name.as_bytes());
  let mut consumers = Vec::new();
  for g in data_ks.prefix(&c_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&c_prefix) {
      break;
    }
    let consumer_name = str::from_utf8(&k[c_prefix.len()..])
      .unwrap_or("")
      .to_string();
    if let Some(c_meta) = StreamConsumerMeta::decode(v) {
      consumers.push((consumer_name, c_meta));
    }
  }

  Ok(consumers)
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
  pub fn xgroup_create<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    last_id: &str,
    mkstream: bool,
    entries_read: Option<i64>,
  ) -> Result<()> {
    stream_group_create(self, key, group_name, last_id, mkstream, entries_read)
  }

  #[inline]
  pub fn xgroup_destroy<K: AsRef<[u8]>>(&self, key: K, group_name: &str) -> Result<bool> {
    stream_group_destroy(self, key, group_name)
  }

  #[inline]
  pub fn xgroup_create_consumer<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
  ) -> Result<i32> {
    stream_group_create_consumer(self, key, group_name, consumer_name)
  }

  #[inline]
  pub fn xgroup_del_consumer<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
  ) -> Result<u64> {
    stream_group_del_consumer(self, key, group_name, consumer_name)
  }

  #[inline]
  pub fn xgroup_set_id<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    last_id: &str,
    entries_read: Option<i64>,
  ) -> Result<()> {
    stream_group_set_id(self, key, group_name, last_id, entries_read)
  }

  #[inline]
  pub fn xack<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    entry_ids: &[StreamId],
  ) -> Result<u64> {
    stream_ack(self, key, group_name, entry_ids)
  }

  #[inline]
  pub fn xclaim<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
    min_idle_time_ms: u64,
    entry_ids: &[StreamId],
    options: StreamClaim,
  ) -> Result<StreamClaimResult> {
    stream_claim(
      self,
      key,
      group_name,
      consumer_name,
      min_idle_time_ms,
      entry_ids,
      options,
    )
  }

  #[inline]
  pub fn xautoclaim<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
    options: StreamAutoClaim,
  ) -> Result<StreamAutoClaimResult> {
    stream_autoclaim(self, key, group_name, consumer_name, options)
  }

  #[inline]
  pub fn xpending_summary<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
  ) -> Result<StreamGetPendingEntryResult> {
    stream_pending_summary(self, key, group_name)
  }

  #[inline]
  pub fn xpending_range<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    options: StreamPending,
  ) -> Result<Vec<StreamNack>> {
    stream_pending_range(self, key, group_name, options)
  }

  #[inline]
  pub fn xread<K: AsRef<[u8]>>(
    &self,
    key: K,
    start_id: StreamId,
    count: Option<usize>,
  ) -> Result<Vec<StreamEntry>> {
    stream_read(self, key, start_id, count)
  }

  #[inline]
  pub fn xread_streams(
    &self,
    streams: &[(&str, StreamId)],
    count: Option<usize>,
  ) -> Result<Vec<StreamReadResult>> {
    stream_read_streams(self, streams, count)
  }

  #[inline]
  pub fn xreadgroup<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
    start_id_str: &str,
    count: Option<usize>,
    noack: bool,
  ) -> Result<Vec<StreamEntry>> {
    stream_readgroup(
      self,
      key,
      group_name,
      consumer_name,
      start_id_str,
      count,
      noack,
    )
  }

  #[inline]
  pub fn xreadgroup_streams(
    &self,
    group_name: &str,
    consumer_name: &str,
    streams: &[(&str, &str)],
    count: Option<usize>,
    noack: bool,
  ) -> Result<Vec<StreamReadResult>> {
    stream_readgroup_streams(self, group_name, consumer_name, streams, count, noack)
  }

  #[inline]
  pub fn xinfo_stream<K: AsRef<[u8]>>(
    &self,
    key: K,
    full: bool,
    count: Option<usize>,
  ) -> Result<StreamInfo> {
    stream_info_stream(self, key, full, count)
  }

  #[inline]
  pub fn xinfo_groups<K: AsRef<[u8]>>(
    &self,
    key: K,
  ) -> Result<Vec<(String, StreamConsumerGroupMeta)>> {
    stream_info_groups(self, key)
  }

  #[inline]
  pub fn xinfo_consumers<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
  ) -> Result<Vec<(String, StreamConsumerMeta)>> {
    stream_info_consumers(self, key, group_name)
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
