use std::ops::Bound;

use crate::{
  api::timeseries::{
    chunk::{ChunkHeader, TSChunk},
    r#const::*,
    gorilla::TSSample,
    key,
    meta::{ChunkType, DuplicatePolicy, TimeSeriesMeta},
    opt::{IntoTsRange, TSDownStreamMeta, TsCreate, TsInfoResult},
    rule::{cascade_downstream_del, trigger_downstream_upsert},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

/// TimeSeries data structure operations interface (TimeSeries).
/// 时序数据结构操作接口 (TimeSeries)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn ts_create_one<K: AsRef<[u8]>>(&self, key: K) -> Result<()> {
    let key_bytes = key.as_ref();
    let meta_k = key::meta(&self.kc(), key_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)?.is_some() {
      return Err(Error::invalid_data(ERR_TSDB_KEY_ALREADY_EXISTS));
    }

    let meta = TimeSeriesMeta::default();
    self.meta().insert(&meta_k, &meta.encode())?;
    Ok(())
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
    let prefix = key::prefix_stack(&kc, key_bytes);

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
      let prefix = key::prefix_stack(&kc, key_bytes);
      if let Some(g) = data_ks.prefix(&prefix).next_back() {
        let entry = g?;
        let (k, v) = (entry.key(), entry.value());
        if k.starts_with(prefix.as_slice())
          && let Ok(Some(sample)) = TSChunk::get_latest_sample(v)
        {
          return Ok(Some(sample));
        }
      }
    }

    Ok(None)
  }

  #[inline]
  pub fn ts_del<K: AsRef<[u8]>>(&self, key: K, range: impl IntoTsRange) -> Result<usize> {
    let (from_ts, to_ts) = range.into_ts_range();
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let prefix = key::prefix_stack(&kc, key_bytes);
    let meta_k = key::meta(&kc, key_bytes);
    let meta_ks = self.meta();
    let data_ks = self.data();
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<TimeSeriesMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(b) => b,
      None => return Ok(0),
    };

    let ds_prefix_k = key::downstream_prefix_stack(&kc, key_bytes);
    let retention_bound = if meta.retention_time > 0 && meta.retention_time < meta.last_time {
      meta.last_time - meta.retention_time
    } else {
      0
    };

    if retention_bound > 0 {
      for g in meta_ks.prefix(&ds_prefix_k) {
        let entry = g?;
        let (k, v) = (entry.key(), entry.value());
        if k.starts_with(ds_prefix_k.as_slice())
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
      if k.starts_with(prefix.as_slice()) {
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
              let (new_first_ts, new_last_ts) =
                TSChunk::get_boundary_timestamps(&new_chunk_data).unwrap_or((chunk_ts, chunk_ts));
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
              let (chunk_first, chunk_last) =
                TSChunk::get_boundary_timestamps(v).unwrap_or((chunk_ts, chunk_ts));
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

    let ds_prefix_k = key::downstream_prefix_stack(&kc, key_bytes);
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
}
