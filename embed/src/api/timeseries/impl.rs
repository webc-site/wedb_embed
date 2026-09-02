use std::ops::Bound;

use rapidhash::RapidHashMap;

use crate::{
  api::timeseries::{
    TimeSeriesLabelFilter, TsFilter,
    chunk::{ChunkHeader, TSChunk},
    r#const::*,
    gorilla::TSSample,
    group_samples_and_reduce, key,
    meta::{ChunkType, DuplicatePolicy, TimeSeriesMeta},
    opt::{
      AggregationType, Aggregator, BucketTimestampType, GroupReducerType, IntoTsRange,
      TSDownStreamMeta, TsCreate, TsInfoResult, TsMGet, TsMGetResult, TsMRange, TsMRangeResult,
      TsRange,
    },
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

/// TimeSeries data structure operations interface (TimeSeries).
/// 时序数据结构操作接口 (TimeSeries)
fn trigger_downstream_upsert<E: Engine>(db: &Db<E>, src_key: &[u8], ts: u64, val: f64) -> Result<()>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_ks = db.meta();
  let ds_prefix_k = key::downstream_prefix(&kc, src_key);

  for g in meta_ks.prefix(&ds_prefix_k) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if let Some(dst_key) = k.strip_prefix(ds_prefix_k.as_slice())
      && let Some(mut ds_meta) = TSDownStreamMeta::decode(v)
    {
      let bkt_left = ds_meta.aggregator.calculate_aligned_bucket_left(ts);
      let agg = &ds_meta.aggregator;

      if agg.agg_type.is_incremental() {
        let (inc_val, inc_policy) = match agg.agg_type {
          AggregationType::Sum => (val, DuplicatePolicy::Sum),
          AggregationType::Count => (1.0, DuplicatePolicy::Sum),
          AggregationType::Min => (val, DuplicatePolicy::Min),
          AggregationType::Max => (val, DuplicatePolicy::Max),
          _ => (val, DuplicatePolicy::Last),
        };
        let _ = db.ts_add(dst_key, bkt_left, inc_val, Some(inc_policy), None);
      } else {
        let bkt_right = ds_meta.aggregator.calculate_aligned_bucket_right(bkt_left);
        let end_bound = if bkt_right > 0 { bkt_right - 1 } else { 0 };
        let bucket_samples = db.ts_range_raw(src_key, bkt_left, end_bound)?;
        if !bucket_samples.is_empty() {
          let agg_val = agg.aggregate_samples(&bucket_samples);
          let _ = db.ts_add(
            dst_key,
            bkt_left,
            agg_val,
            Some(DuplicatePolicy::Last),
            None,
          );
        }
      }

      ds_meta.latest_bucket_idx = ds_meta.latest_bucket_idx.max(bkt_left);
      meta_ks.insert(k, &ds_meta.encode())?;
    }
  }
  Ok(())
}

fn cascade_downstream_del<E: Engine>(
  db: &Db<E>,
  src_key: &[u8],
  from_ts: u64,
  to_ts: u64,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_ks = db.meta();
  let ds_prefix_k = key::downstream_prefix(&kc, src_key);

  for g in meta_ks.prefix(&ds_prefix_k) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if let Some(dst_key) = k.strip_prefix(ds_prefix_k.as_slice())
      && let Some(ds_meta) = TSDownStreamMeta::decode(v)
    {
      let bkt_left = ds_meta.aggregator.calculate_aligned_bucket_left(from_ts);
      let bkt_right = ds_meta.aggregator.calculate_aligned_bucket_right(to_ts);
      let end_bound = if bkt_right > 0 { bkt_right - 1 } else { 0 };
      let _ = db.ts_del(dst_key, (bkt_left, end_bound));
    }
  }
  Ok(())
}

struct TsRangeQuery<'a> {
  start_ts: u64,
  end_ts: u64,
  count_limit: Option<usize>,
  filter_by_ts: &'a TsFilter,
  filter_by_value: Option<(f64, f64)>,
  agg_type: Option<AggregationType>,
  bucket_duration: u64,
  alignment: u64,
  is_return_empty: bool,
  bucket_timestamp_type: BucketTimestampType,
}

type GroupedSeriesEntry = (Vec<Vec<(u64, f64)>>, Vec<String>);

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn ts_create_one<K: AsRef<[u8]>>(&self, key: K) -> Result<()> {
    self.ts_create(key, [])
  }

  #[inline]
  pub fn ts_create<K: AsRef<[u8]>>(
    &self,
    key: K,
    opt_li: impl IntoIterator<Item = TsCreate>,
  ) -> Result<()> {
    let key_bytes = key.as_ref();
    let meta_k = key::meta(&self.kc(), key_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)?.is_some() {
      return Err(Error::invalid_data(ERR_TSDB_KEY_ALREADY_EXISTS));
    }

    let meta_ks = self.meta();
    let meta: TimeSeriesMeta = opt_li.into_iter().collect();
    meta_ks.insert(&meta_k, &meta.encode())?;
    Ok(())
  }

  #[inline]
  pub fn ts_alter<K: AsRef<[u8]>>(
    &self,
    key: K,
    retention_ms: Option<u64>,
    chunk_size: Option<u64>,
    duplicate_policy: Option<DuplicatePolicy>,
    labels: Option<Vec<(String, String)>>,
  ) -> Result<()> {
    let key_bytes = key.as_ref();
    let meta_k = key::meta(&self.kc(), key_bytes);
    let meta_ks = self.meta();
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(b) => b,
      None => return Err(Error::invalid_data(ERR_TSDB_KEY_NOT_EXISTS)),
    };

    if let Some(r) = retention_ms {
      meta.retention_time = r;
    }
    if let Some(c) = chunk_size {
      meta.chunk_size = if c == 0 {
        TimeSeriesMeta::DEFAULT_CHUNK_SIZE
      } else {
        c
      };
    }
    if let Some(p) = duplicate_policy {
      meta.duplicate_policy = p;
    }
    if let Some(l) = labels {
      meta.labels = l;
    }

    meta_ks.insert(&meta_k, &meta.encode())?;
    Ok(())
  }

  #[inline]
  pub fn ts_add<K: AsRef<[u8]>>(
    &self,
    key: K,
    timestamp: u64,
    value: f64,
    on_duplicate: Option<DuplicatePolicy>,
    create_opt: impl IntoIterator<Item = TsCreate>,
  ) -> Result<u64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = key::meta(&kc, key_bytes);
    let data_ks = self.data();
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => create_opt.into_iter().collect(),
    };

    if meta.retention_time > 0 && timestamp < meta.last_time.saturating_sub(meta.retention_time) {
      return Err(Error::invalid_data(ERR_TSDB_TIMESTAMP_OLDER_THAN_RETENTION));
    }

    let policy = on_duplicate.unwrap_or(meta.duplicate_policy);
    let sample = TSSample::new(timestamp, value);
    let prefix = key::prefix(&kc, key_bytes);

    let mut batch = self.batch();
    let mut final_value = value;

    if meta.total_samples == 0 {
      let encoded_chunk = TSChunk::encode_with_type(&[sample], meta.chunk_type);
      let item_k = key::chunk(&kc, key_bytes, timestamp);
      batch.insert_data(&item_k, &encoded_chunk);
      meta.total_samples = 1;
      meta.base.size = 1;
      meta.first_time = timestamp;
      meta.last_time = timestamp;
    } else {
      let target_item_k = key::chunk(&kc, key_bytes, timestamp);
      let found_entry = data_ks
        .range((
          Bound::Included(prefix.as_slice()),
          Bound::Included(target_item_k.as_slice()),
        ))
        .next_back()
        .transpose()?;
      let (old_k, old_data) = match found_entry {
        Some(entry) if entry.key().starts_with(&prefix) => {
          (entry.key().to_vec(), entry.value().to_vec())
        }
        _ => {
          let entry = data_ks
            .prefix(&prefix)
            .next()
            .ok_or_else(|| Error::invalid_data(ERR_TSDB_CORRUPTED_DATA_INDEX))??;
          (entry.key().to_vec(), entry.value().to_vec())
        }
      };

      let chunk_first_ts = if let Ok(b8) = old_k[prefix.len()..].try_into() {
        u64::from_be_bytes(b8)
      } else {
        0
      };

      // 极致优化快路径：未压缩 Chunk 单调递增快速原地追加（避免 O(K) 反序列化与重新编码，零中间堆分配）
      let fast_appended = if meta.chunk_type == ChunkType::Uncompressed
        && old_data.len() >= ChunkHeader::ENCODED_SIZE
        && let Some(header) = ChunkHeader::decode(&old_data)
        && !header.is_compressed
        && (header.count as usize) < (meta.chunk_size as usize).max(1)
      {
        let count = header.count as usize;
        let last_ts = if count > 0 && old_data.len() >= ChunkHeader::ENCODED_SIZE + count * 16 {
          let offset = ChunkHeader::ENCODED_SIZE + (count - 1) * 16;
          u64::from_be_bytes(old_data[offset..offset + 8].try_into().unwrap_or([0u8; 8]))
        } else {
          chunk_first_ts
        };

        if timestamp > last_ts {
          let mut new_data = old_data.clone();
          let new_count = (count + 1) as u32;
          new_data[4..8].copy_from_slice(&new_count.to_be_bytes());
          new_data.extend_from_slice(&timestamp.to_be_bytes());
          new_data.extend_from_slice(&value.to_be_bytes());
          batch.insert_data(&old_k, &new_data);
          meta.total_samples += 1;
          true
        } else {
          false
        }
      } else {
        false
      };

      if !fast_appended {
        let mut samples = TSChunk::decode_samples(&old_data)?;

        match samples.binary_search_by_key(&timestamp, |s| s.ts) {
          Ok(idx) => {
            match policy.merge_value(samples[idx].v, value) {
              Some(merged) => {
                final_value = merged;
                samples[idx].v = merged;
              }
              None => {
                return Err(Error::invalid_data(ERR_TSDB_DUPLICATE_BLOCK_MODE));
              }
            }
            let new_chunk = TSChunk::encode_with_type(&samples, meta.chunk_type);
            batch.insert_data(&old_k, &new_chunk);
          }
          Err(insert_idx) => {
            let is_latest_chunk = timestamp >= meta.last_time;
            let last_sample_ts = samples.last().map(|s| s.ts).unwrap_or(chunk_first_ts);

            if is_latest_chunk
              && samples.len() >= meta.chunk_size as usize
              && timestamp > last_sample_ts
            {
              let new_chunk = TSChunk::encode_with_type(&[sample], meta.chunk_type);
              let new_item_k = key::chunk(&kc, key_bytes, timestamp);
              batch.insert_data(&new_item_k, &new_chunk);
              meta.base.size += 1;
            } else {
              samples.insert(insert_idx, sample);
              let max_chunk_size = (meta.chunk_size as usize).max(1);
              if samples.len() <= max_chunk_size {
                let new_chunk = TSChunk::encode_with_type(&samples, meta.chunk_type);
                let chunk_start_ts = unsafe { samples.get_unchecked(0).ts };
                if chunk_start_ts != chunk_first_ts {
                  batch.rm_weak_data(&old_k);
                  let new_item_k = key::chunk(&kc, key_bytes, chunk_start_ts);
                  batch.insert_data(&new_item_k, &new_chunk);
                } else {
                  batch.insert_data(&old_k, &new_chunk);
                }
              } else {
                batch.rm_weak_data(&old_k);
                let mut added_chunks = 0u64;
                for chunk_slice in samples.chunks(max_chunk_size) {
                  let first_ts = chunk_slice[0].ts;
                  let new_chunk = TSChunk::encode_with_type(chunk_slice, meta.chunk_type);
                  let new_item_k = key::chunk(&kc, key_bytes, first_ts);
                  batch.insert_data(&new_item_k, &new_chunk);
                  added_chunks += 1;
                }
                if added_chunks > 1 {
                  meta.base.size += added_chunks - 1;
                }
              }
            }

            meta.total_samples += 1;
          }
        }
      }

      if meta.first_time == 0 || timestamp < meta.first_time {
        meta.first_time = timestamp;
      }
      if timestamp > meta.last_time {
        meta.last_time = timestamp;
      }
    }

    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;

    trigger_downstream_upsert(self, key_bytes, timestamp, final_value)?;
    Ok(timestamp)
  }

  #[inline]
  pub fn ts_madd_one<K: AsRef<[u8]>>(&self, key: K, timestamp: u64, value: f64) -> Result<u64> {
    self.ts_add(key, timestamp, value, None, None)
  }

  #[inline]
  pub fn ts_madd<K: AsRef<[u8]>>(&self, items: &[(K, u64, f64)]) -> Result<Vec<Result<u64>>> {
    let mut results = Vec::with_capacity(items.len());
    for (k, ts, v) in items {
      results.push(self.ts_add(k, *ts, *v, None, None));
    }
    Ok(results)
  }

  #[inline]
  pub fn ts_incrby<K: AsRef<[u8]>>(
    &self,
    key: K,
    value: f64,
    timestamp: Option<u64>,
    create_opt: impl IntoIterator<Item = TsCreate>,
  ) -> Result<u64> {
    let ts = timestamp.unwrap_or_else(current_now_ms);
    let latest = self.ts_get(key.as_ref())?;
    let new_val = match latest {
      Some((latest_ts, old_v)) => {
        if ts < latest_ts {
          return Err(Error::invalid_data(ERR_TSDB_TIMESTAMP_OLDER_THAN_MAX));
        }
        old_v + value
      }
      None => value,
    };
    self.ts_add(key, ts, new_val, Some(DuplicatePolicy::Last), create_opt)
  }

  #[inline]
  pub fn ts_decrby<K: AsRef<[u8]>>(
    &self,
    key: K,
    value: f64,
    timestamp: Option<u64>,
    create_opt: impl IntoIterator<Item = TsCreate>,
  ) -> Result<u64> {
    self.ts_incrby(key, -value, timestamp, create_opt)
  }

  #[inline]
  pub fn ts_get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<(u64, f64)>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = key::meta(&kc, key_bytes);
    let data_ks = self.data();
    let now_ms = current_now_ms();

    let meta = get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)?;

    if let Some(m) = meta
      && m.total_samples > 0
    {
      let prefix = key::prefix(&kc, key_bytes);
      if let Some(g) = data_ks.prefix(&prefix).next_back() {
        let entry = g?;
        let (k, v) = (entry.key(), entry.value());
        if k.starts_with(&prefix)
          && let Ok(Some(sample)) = TSChunk::get_latest_sample(v)
        {
          return Ok(Some(sample));
        }
      }
    }

    Ok(None)
  }

  #[inline]
  pub fn ts_range_one<K: AsRef<[u8]>>(
    &self,
    key: K,
    range: impl IntoTsRange,
  ) -> Result<Vec<(u64, f64)>> {
    self.ts_range(key, range, [])
  }

  #[inline]
  pub fn ts_range<K: AsRef<[u8]>>(
    &self,
    key: K,
    range: impl IntoTsRange,
    opt_li: impl IntoIterator<Item = TsRange>,
  ) -> Result<Vec<(u64, f64)>> {
    let (start_ts, end_ts) = range.into_ts_range();
    let mut count_limit = None;
    let mut filter_by_ts = TsFilter::default();
    let mut filter_by_value = None;
    let mut agg_type = None;
    let mut bucket_duration = 0;
    let mut alignment = 0;
    let mut is_return_empty = false;
    let mut bucket_timestamp_type = BucketTimestampType::Start;

    for opt in opt_li {
      match opt {
        TsRange::Count(c) => count_limit = Some(c),
        TsRange::FilterByTs(ts) => filter_by_ts = ts,
        TsRange::FilterByValue(min, max) => filter_by_value = Some((min, max)),
        TsRange::Aggregation(t, d) => {
          agg_type = Some(t);
          bucket_duration = d;
        }
        TsRange::Alignment(a) => alignment = a,
        TsRange::Latest => {}
        TsRange::Empty => is_return_empty = true,
        TsRange::BucketTimestamp(b) => bucket_timestamp_type = b,
      }
    }

    let query = TsRangeQuery {
      start_ts,
      end_ts,
      count_limit,
      filter_by_ts: &filter_by_ts,
      filter_by_value,
      agg_type,
      bucket_duration,
      alignment,
      is_return_empty,
      bucket_timestamp_type,
    };
    self.ts_range_internal(key.as_ref(), &query)
  }

  #[inline]
  fn ts_range_internal(
    &self,
    key_bytes: &[u8],
    query: &TsRangeQuery<'_>,
  ) -> Result<Vec<(u64, f64)>> {
    self.ts_range_internal_with_meta(key_bytes, None, query)
  }

  fn ts_range_internal_with_meta(
    &self,
    key_bytes: &[u8],
    meta: Option<&TimeSeriesMeta>,
    query: &TsRangeQuery<'_>,
  ) -> Result<Vec<(u64, f64)>> {
    let (eff_start_ts, eff_end_ts) =
      match query.filter_by_ts.clamp_range(query.start_ts, query.end_ts) {
        Some(range) => range,
        None => return Ok(Vec::new()),
      };

    let filter_opt = if query.filter_by_ts.is_empty() {
      None
    } else {
      Some(query.filter_by_ts)
    };
    let mut filtered = self.ts_range_raw_filtered_with_meta(
      key_bytes,
      meta,
      eff_start_ts,
      eff_end_ts,
      filter_opt,
    )?;
    query
      .filter_by_ts
      .filter_samples(&mut filtered, query.filter_by_value);

    if let Some(t) = query.agg_type {
      let agg = Aggregator::new(t, query.bucket_duration, query.alignment);
      filtered = agg.split_and_aggregate(
        &filtered,
        query.count_limit,
        query.is_return_empty,
        query.bucket_timestamp_type,
      );
    } else if let Some(limit) = query.count_limit {
      filtered.truncate(limit);
    }

    Ok(filtered)
  }

  #[inline]
  pub fn ts_revrange_one<K: AsRef<[u8]>>(
    &self,
    key: K,
    range: impl IntoTsRange,
  ) -> Result<Vec<(u64, f64)>> {
    self.ts_revrange(key, range, [])
  }

  #[inline]
  pub fn ts_revrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    range: impl IntoTsRange,
    opt_li: impl IntoIterator<Item = TsRange>,
  ) -> Result<Vec<(u64, f64)>> {
    let mut res = self.ts_range(key, range, opt_li)?;
    res.reverse();
    Ok(res)
  }

  #[inline]
  pub(crate) fn ts_range_raw<K: AsRef<[u8]>>(
    &self,
    key: K,
    from_ts: u64,
    to_ts: u64,
  ) -> Result<Vec<(u64, f64)>> {
    self.ts_range_raw_filtered(key, from_ts, to_ts, None)
  }

  #[inline]
  pub(crate) fn ts_range_raw_filtered<K: AsRef<[u8]>>(
    &self,
    key: K,
    from_ts: u64,
    to_ts: u64,
    filter_by_ts: Option<&TsFilter>,
  ) -> Result<Vec<(u64, f64)>> {
    self.ts_range_raw_filtered_with_meta(key.as_ref(), None, from_ts, to_ts, filter_by_ts)
  }

  pub(crate) fn ts_range_raw_filtered_with_meta(
    &self,
    key_bytes: &[u8],
    meta_arg: Option<&TimeSeriesMeta>,
    from_ts: u64,
    to_ts: u64,
    filter_by_ts: Option<&TsFilter>,
  ) -> Result<Vec<(u64, f64)>> {
    if from_ts > to_ts {
      return Ok(Vec::new());
    }

    let kc = self.kc();
    let prefix = key::prefix(&kc, key_bytes);
    let meta_k = key::meta(&kc, key_bytes);
    let data_ks = self.data();
    let now_ms = current_now_ms();

    let fetched_meta;
    let meta = match meta_arg {
      Some(m) => Some(m),
      None => {
        fetched_meta = get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)?;
        fetched_meta.as_ref()
      }
    };

    let retention_bound = if let Some(m) = meta
      && m.retention_time > 0
    {
      m.last_time.saturating_sub(m.retention_time)
    } else {
      0
    };

    let start_ts = from_ts.max(retention_bound);
    let end_ts = to_ts;
    let cap = meta
      .map(|m| (m.total_samples as usize).min(4096))
      .unwrap_or(0);
    let mut samples = Vec::with_capacity(cap);
    let mut decoded_buf = Vec::with_capacity(1024);

    let start_item_k = key::chunk(&kc, key_bytes, start_ts);
    let start_range_k = if let Some(g) = data_ks
      .range((
        Bound::Included(prefix.as_slice()),
        Bound::Included(start_item_k.as_slice()),
      ))
      .next_back()
    {
      let entry = g?;
      let (k, _) = (entry.key(), entry.value());
      if k.starts_with(&prefix) {
        k.to_vec()
      } else {
        prefix.clone()
      }
    } else {
      prefix.clone()
    };

    for g in data_ks.range((Bound::Included(start_range_k.as_slice()), Bound::Unbounded)) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&prefix) {
        break;
      }
      let sub = &k[prefix.len()..];
      if let Ok(b8) = sub.try_into() {
        let chunk_first_ts = u64::from_be_bytes(b8);
        if chunk_first_ts > end_ts {
          break;
        }
        if let Some(chunk_last_ts) = TSChunk::get_last_timestamp(v) {
          if chunk_last_ts < start_ts {
            continue;
          }
          if let Some(f) = filter_by_ts
            && !f.matches_range(chunk_first_ts, chunk_last_ts)
          {
            continue;
          }
        }
        decoded_buf.clear();
        if TSChunk::decode_samples_into(v, &mut decoded_buf).is_ok() && !decoded_buf.is_empty() {
          let first_sample_ts = decoded_buf[0].ts;
          let last_sample_ts = decoded_buf[decoded_buf.len() - 1].ts;
          if first_sample_ts >= start_ts && last_sample_ts <= end_ts {
            samples.extend(decoded_buf.iter().map(|s| (s.ts, s.v)));
          } else {
            let start_idx = decoded_buf.partition_point(|s| s.ts < start_ts);
            let end_idx = decoded_buf.partition_point(|s| s.ts <= end_ts);
            if start_idx < end_idx {
              samples.extend(decoded_buf[start_idx..end_idx].iter().map(|s| (s.ts, s.v)));
            }
          }
        }
      }
    }
    if !samples.windows(2).all(|w| w[0].0 <= w[1].0) {
      samples.sort_unstable_by_key(|s| s.0);
    }
    Ok(samples)
  }

  #[inline]
  pub fn ts_del<K: AsRef<[u8]>>(&self, key: K, range: impl IntoTsRange) -> Result<usize> {
    let (from_ts, to_ts) = range.into_ts_range();
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let prefix = key::prefix(&kc, key_bytes);
    let meta_k = key::meta(&kc, key_bytes);
    let meta_ks = self.meta();
    let data_ks = self.data();
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(b) => b,
      None => return Ok(0),
    };

    let ds_prefix_k = key::downstream_prefix(&kc, key_bytes);
    let retention_bound = if meta.retention_time > 0 && meta.retention_time < meta.last_time {
      meta.last_time - meta.retention_time
    } else {
      0
    };

    if retention_bound > 0 {
      for g in meta_ks.prefix(&ds_prefix_k) {
        let entry = g?;
        let (k, v) = (entry.key(), entry.value());
        if k.starts_with(&ds_prefix_k)
          && let Some(ds_meta) = TSDownStreamMeta::decode(v)
          && ds_meta.aggregator.calculate_aligned_bucket_left(from_ts) < retention_bound
        {
          return Err(Error::invalid_data(ERR_TSDB_CANNOT_DEL_WITH_RETENTION));
        }
      }
    }

    let mut total_deleted = 0;
    let mut deleted_chunks = 0u64;
    let mut batch = self.batch();
    let mut first_remaining_ts = None;
    let mut last_remaining_ts = None;

    for g in data_ks.prefix(&prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if k.starts_with(&prefix) {
        let sub = &k[prefix.len()..];
        if let Ok(b8) = sub.try_into() {
          let chunk_ts = u64::from_be_bytes(b8);
          let (new_chunk_data, deleted) =
            TSChunk::remove_samples_between(v, from_ts, to_ts, meta.chunk_type)?;
          if deleted > 0 {
            total_deleted += deleted;
            let count = TSChunk::get_count(&new_chunk_data);
            if count == 0 {
              batch.rm_weak_data(k);
              deleted_chunks += 1;
            } else {
              let new_first_ts = TSChunk::get_first_timestamp(&new_chunk_data).unwrap_or(chunk_ts);
              let new_last_ts =
                TSChunk::get_last_timestamp(&new_chunk_data).unwrap_or(new_first_ts);
              let new_key = key::chunk(&kc, key_bytes, new_first_ts);
              if new_key[..] != **k {
                batch.rm_weak_data(k);
              }
              batch.insert_data(&new_key, &new_chunk_data);
              if first_remaining_ts.is_none() {
                first_remaining_ts = Some(new_first_ts);
              }
              last_remaining_ts = Some(new_last_ts);
            }
          } else {
            let count = TSChunk::get_count(v);
            if count > 0 {
              let chunk_first = TSChunk::get_first_timestamp(v).unwrap_or(chunk_ts);
              let chunk_last = TSChunk::get_last_timestamp(v).unwrap_or(chunk_first);
              if first_remaining_ts.is_none() {
                first_remaining_ts = Some(chunk_first);
              }
              last_remaining_ts = Some(chunk_last);
            }
          }
        }
      }
    }

    if total_deleted == 0 {
      return Ok(0);
    }

    let mut updated_meta = meta;
    updated_meta.total_samples = updated_meta
      .total_samples
      .saturating_sub(total_deleted as u64);
    updated_meta.base.size = updated_meta.base.size.saturating_sub(deleted_chunks);

    if updated_meta.total_samples == 0 {
      updated_meta.base.size = 0;
      updated_meta.first_time = 0;
      updated_meta.last_time = 0;
    } else {
      if let Some(fts) = first_remaining_ts {
        updated_meta.first_time = fts;
      }
      if let Some(lts) = last_remaining_ts {
        updated_meta.last_time = lts;
      }
    }

    batch.insert_meta(&meta_k, &updated_meta.encode());
    batch.commit()?;

    cascade_downstream_del(self, key_bytes, from_ts, to_ts)?;
    Ok(total_deleted)
  }

  #[inline]
  pub fn ts_mget(&self, opt_li: impl IntoIterator<Item = TsMGet>) -> Result<Vec<TsMGetResult>> {
    let mut with_labels = false;
    let mut selected_labels = Vec::new();
    let mut filters = Vec::new();

    for opt in opt_li {
      match opt {
        TsMGet::WithLabels => with_labels = true,
        TsMGet::SelectedLabels(labels) => selected_labels = labels,
        TsMGet::Filters(f) => filters = f,
      }
    }

    let kc = self.kc();
    let meta_ks = self.meta();
    let data_ks = self.data();
    let now_ms = current_now_ms();
    let filter = TimeSeriesLabelFilter::parse(&filters);
    let prefix = key::meta_prefix(&kc);

    let mut results = Vec::new();
    for g in meta_ks.prefix(&prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if k.starts_with(&prefix)
        && TimeSeriesMeta::matches_labels_raw(v, &filter)
        && let Some(meta) = TimeSeriesMeta::decode(v)
        && !meta.is_expired(now_ms)
      {
        let name_bytes = &k[prefix.len()..];
        let name = String::from_utf8_lossy(name_bytes).into_owned();

        let latest_sample = if meta.total_samples > 0 {
          let data_prefix = key::prefix(&kc, name_bytes);
          if let Some(chunk_entry) = data_ks.prefix(&data_prefix).next_back() {
            let chunk = chunk_entry?;
            if chunk.key().starts_with(&data_prefix) {
              TSChunk::get_latest_sample(chunk.value()).ok().flatten()
            } else {
              None
            }
          } else {
            None
          }
        } else {
          None
        };

        let labels = if with_labels {
          meta.labels
        } else if !selected_labels.is_empty() {
          selected_labels
            .iter()
            .map(|sel_k| {
              let val = meta
                .labels
                .iter()
                .find(|(k, _)| k == sel_k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
              (sel_k.clone(), val.to_string())
            })
            .collect()
        } else {
          Vec::new()
        };

        results.push(TsMGetResult {
          name,
          labels,
          sample: latest_sample,
        });
      }
    }

    Ok(results)
  }

  #[inline]
  pub fn ts_mrange(
    &self,
    range: impl IntoTsRange,
    opt_li: impl IntoIterator<Item = TsMRange>,
  ) -> Result<Vec<TsMRangeResult>> {
    let (start_ts, end_ts) = range.into_ts_range();
    let mut with_labels = false;
    let mut selected_labels = Vec::new();
    let mut filters = Vec::new();
    let mut count_limit = None;
    let mut filter_by_ts = TsFilter::default();
    let mut filter_by_value = None;
    let mut agg_type = None;
    let mut bucket_duration = 0;
    let mut alignment = 0;
    let mut is_return_empty = false;
    let mut bucket_timestamp_type = BucketTimestampType::Start;
    let mut group_by_label = None;
    let mut reducer = GroupReducerType::Sum;

    for opt in opt_li {
      match opt {
        TsMRange::WithLabels => with_labels = true,
        TsMRange::SelectedLabels(labels) => selected_labels = labels,
        TsMRange::Filters(f) => filters = f,
        TsMRange::Count(c) => count_limit = Some(c),
        TsMRange::FilterByTs(ts) => filter_by_ts = ts,
        TsMRange::FilterByValue(min, max) => filter_by_value = Some((min, max)),
        TsMRange::Aggregation(t, d) => {
          agg_type = Some(t);
          bucket_duration = d;
        }
        TsMRange::Alignment(a) => alignment = a,
        TsMRange::Latest => {}
        TsMRange::Empty => is_return_empty = true,
        TsMRange::BucketTimestamp(b) => bucket_timestamp_type = b,
        TsMRange::GroupBy(label, red) => {
          group_by_label = Some(label);
          reducer = red;
        }
      }
    }

    let kc = self.kc();
    let meta_ks = self.meta();
    let now_ms = current_now_ms();
    let filter = TimeSeriesLabelFilter::parse(&filters);
    let prefix = key::meta_prefix(&kc);

    let query = TsRangeQuery {
      start_ts,
      end_ts,
      count_limit,
      filter_by_ts: &filter_by_ts,
      filter_by_value,
      agg_type,
      bucket_duration,
      alignment,
      is_return_empty,
      bucket_timestamp_type,
    };

    let mut series_results = Vec::new();

    for g in meta_ks.prefix(&prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if k.starts_with(&prefix)
        && TimeSeriesMeta::matches_labels_raw(v, &filter)
        && let Some(meta) = TimeSeriesMeta::decode(v)
        && !meta.is_expired(now_ms)
      {
        let name_bytes = &k[prefix.len()..];
        let name = String::from_utf8_lossy(name_bytes).into_owned();

        let labels = if with_labels {
          meta.labels.clone()
        } else if let Some(ref gl) = group_by_label {
          let mut res_labels = Vec::new();
          if let Some((_, v)) = meta.labels.iter().find(|(lk, _)| lk == gl) {
            res_labels.push((gl.clone(), v.clone()));
          }
          for sel_k in &selected_labels {
            if sel_k != gl {
              let val = meta
                .labels
                .iter()
                .find(|(lk, _)| lk == sel_k)
                .map(|(_, lv)| lv.as_str())
                .unwrap_or("");
              res_labels.push((sel_k.clone(), val.to_string()));
            }
          }
          res_labels
        } else if !selected_labels.is_empty() {
          selected_labels
            .iter()
            .map(|sel_k| {
              let val = meta
                .labels
                .iter()
                .find(|(k, _)| k == sel_k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
              (sel_k.clone(), val.to_string())
            })
            .collect()
        } else {
          Vec::new()
        };

        let samples = self.ts_range_internal_with_meta(name_bytes, Some(&meta), &query)?;
        series_results.push((name, labels, samples));
      }
    }

    if let Some(ref group_label) = group_by_label
      && reducer != GroupReducerType::None
    {
      let mut groups: RapidHashMap<String, GroupedSeriesEntry> = RapidHashMap::default();

      for (name, labels, samples) in series_results {
        let group_val = labels
          .iter()
          .find(|(k, _)| k == group_label)
          .map(|(_, v)| v.clone())
          .unwrap_or_default();

        let entry = groups.entry(group_val).or_default();
        entry.0.push(samples);
        entry.1.push(name);
      }

      let mut final_res = Vec::new();
      for (group_val, (all_samples, source_keys)) in groups {
        let reduced = group_samples_and_reduce(&all_samples, reducer);
        let mut labels = vec![(group_label.clone(), group_val.clone())];
        if with_labels {
          labels.push(("__reducer__".to_string(), reducer.as_str().to_string()));
          labels.push(("__source__".to_string(), source_keys.join(",")));
        }
        final_res.push(TsMRangeResult {
          name: format!("{group_label}={group_val}"),
          labels,
          samples: reduced,
          source_keys,
        });
      }
      final_res.sort_unstable_by(|a, b| a.name.cmp(&b.name));
      return Ok(final_res);
    }

    let final_res = series_results
      .into_iter()
      .map(|(name, labels, samples)| TsMRangeResult {
        source_keys: vec![name.clone()],
        name,
        labels,
        samples,
      })
      .collect();
    Ok(final_res)
  }

  #[inline]
  pub fn ts_mrevrange(
    &self,
    range: impl IntoTsRange,
    opt_li: impl IntoIterator<Item = TsMRange>,
  ) -> Result<Vec<TsMRangeResult>> {
    let mut res = self.ts_mrange(range, opt_li)?;
    for r in &mut res {
      r.samples.reverse();
    }
    Ok(res)
  }

  #[inline]
  pub fn ts_info<K: AsRef<[u8]>>(&self, key: K) -> Result<TsInfoResult> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = key::meta(&kc, key_bytes);
    let meta_ks = self.meta();

    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(b) => b,
      None => return Err(Error::invalid_data(ERR_TSDB_KEY_NOT_EXISTS)),
    };

    let ds_prefix_k = key::downstream_prefix(&kc, key_bytes);
    let mut downstream_rules = Vec::new();

    for g in meta_ks.prefix(&ds_prefix_k) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if let Some(dst_key) = k.strip_prefix(ds_prefix_k.as_slice())
        && let Some(ds_meta) = TSDownStreamMeta::decode(v)
      {
        downstream_rules.push((dst_key.to_vec(), ds_meta.aggregator));
      }
    }

    let memory_usage = 128 + meta.total_samples * 16;
    let (first_timestamp, last_timestamp) = if meta.total_samples > 0 {
      (meta.first_time, meta.last_time)
    } else {
      (0, 0)
    };

    Ok(TsInfoResult {
      total_samples: meta.total_samples,
      memory_usage,
      first_timestamp,
      last_timestamp,
      retention_time: meta.retention_time,
      chunk_count: meta.base.size as usize,
      chunk_size: meta.chunk_size,
      chunk_type: meta.chunk_type,
      duplicate_policy: meta.duplicate_policy,
      labels: meta.labels,
      source_key: meta.source_key,
      downstream_rules,
    })
  }

  #[inline]
  pub fn ts_queryindex(&self, filters: &[String]) -> Result<Vec<String>> {
    let filter = TimeSeriesLabelFilter::parse(filters);
    let prefix = key::meta_prefix(&self.kc());
    let meta_ks = self.meta();
    let now_ms = current_now_ms();

    let mut keys = Vec::new();
    for g in meta_ks.prefix(&prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if k.starts_with(&prefix)
        && TimeSeriesMeta::matches_labels_raw(v, &filter)
        && let Some(meta) = TimeSeriesMeta::decode(v)
        && !meta.is_expired(now_ms)
      {
        let name = String::from_utf8_lossy(&k[prefix.len()..]).into_owned();
        keys.push(name);
      }
    }
    keys.sort_unstable();
    Ok(keys)
  }

  #[inline]
  pub fn ts_createrule<SK: AsRef<[u8]>, DK: AsRef<[u8]>>(
    &self,
    src_key: SK,
    dst_key: DK,
    aggregator: AggregationType,
    bucket_duration: u64,
    alignment: Option<u64>,
  ) -> Result<()> {
    let src_bytes = src_key.as_ref();
    let dst_bytes = dst_key.as_ref();

    if src_bytes == dst_bytes {
      return Err(Error::invalid_data(ERR_TSDB_SRC_DST_SAME));
    }

    let kc = self.kc();
    let meta_ks = self.meta();
    let src_meta_k = key::meta(&kc, src_bytes);
    let dst_meta_k = key::meta(&kc, dst_bytes);

    let src_meta = match meta_ks.get(&src_meta_k)? {
      Some(b) => TimeSeriesMeta::decode(&b)
        .ok_or_else(|| Error::invalid_data(ERR_TSDB_CORRUPTED_SRC_META))?,
      None => return Err(Error::invalid_data(ERR_TSDB_NOT_TSDB_KEY)),
    };

    let mut dst_meta = match meta_ks.get(&dst_meta_k)? {
      Some(b) => TimeSeriesMeta::decode(&b)
        .ok_or_else(|| Error::invalid_data(ERR_TSDB_CORRUPTED_DST_META))?,
      None => return Err(Error::invalid_data(ERR_TSDB_NOT_TSDB_KEY)),
    };

    if !src_meta.source_key.is_empty() {
      return Err(Error::invalid_data(ERR_TSDB_SRC_ALREADY_HAS_RULE));
    }

    if !dst_meta.source_key.is_empty() && dst_meta.source_key.as_slice() != src_bytes {
      return Err(Error::invalid_data(ERR_TSDB_DST_ALREADY_HAS_SRC_RULE));
    }

    let dst_ds_prefix_k = key::downstream_prefix(&kc, dst_bytes);
    if meta_ks.prefix(&dst_ds_prefix_k).next().is_some() {
      return Err(Error::invalid_data(ERR_TSDB_DST_ALREADY_HAS_DST_RULE));
    }

    let ds_meta_k = key::downstream_meta(&kc, src_bytes, dst_bytes);
    let align = alignment.unwrap_or(0);
    let ds = TSDownStreamMeta::new(Aggregator::new(aggregator, bucket_duration, align));

    let mut batch = self.batch_with_capacity(2);
    batch.insert_meta(&ds_meta_k, &ds.encode());

    if dst_meta.source_key.as_slice() != src_bytes {
      dst_meta.source_key = src_bytes.to_vec();
      batch.insert_meta(&dst_meta_k, &dst_meta.encode());
    }

    batch.commit()?;
    Ok(())
  }

  #[inline]
  pub fn ts_deleterule<SK: AsRef<[u8]>, DK: AsRef<[u8]>>(
    &self,
    src_key: SK,
    dst_key: DK,
  ) -> Result<()> {
    let src_bytes = src_key.as_ref();
    let dst_bytes = dst_key.as_ref();

    let kc = self.kc();
    let meta_ks = self.meta();
    let src_meta_k = key::meta(&kc, src_bytes);
    let dst_meta_k = key::meta(&kc, dst_bytes);

    let dst_meta_bytes = meta_ks
      .get(&dst_meta_k)?
      .ok_or_else(|| Error::invalid_data(ERR_TSDB_NOT_TSDB_KEY))?;
    if !meta_ks.contains_key(&src_meta_k)? {
      return Err(Error::invalid_data(ERR_TSDB_NOT_TSDB_KEY));
    }

    let ds_meta_k = key::downstream_meta(&kc, src_bytes, dst_bytes);
    if !meta_ks.contains_key(&ds_meta_k)? {
      return Err(Error::invalid_data(ERR_TSDB_RULE_NOT_EXISTS));
    }

    let mut batch = self.batch_with_capacity(2);
    batch.rm_meta(&ds_meta_k);

    if let Some(mut dst_meta) = TimeSeriesMeta::decode(&dst_meta_bytes)
      && dst_meta.source_key.as_slice() == src_bytes
    {
      dst_meta.source_key.clear();
      batch.insert_meta(&dst_meta_k, &dst_meta.encode());
    }

    batch.commit()?;
    Ok(())
  }
}
