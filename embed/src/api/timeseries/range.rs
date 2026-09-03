use std::ops::Bound;

use crate::{
  api::timeseries::{
    chunk::TSChunk,
    filter::TsFilter,
    key,
    meta::TimeSeriesMeta,
    opt::{AggregationType, Aggregator, BucketTimestampType, IntoTsRange, TsRange},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

#[derive(Debug)]
pub(crate) struct TsRangeQuery<'a> {
  pub(crate) start_ts: u64,
  pub(crate) end_ts: u64,
  pub(crate) count_limit: Option<usize>,
  pub(crate) filter_by_ts: &'a TsFilter,
  pub(crate) filter_by_value: Option<(f64, f64)>,
  pub(crate) agg_type: Option<AggregationType>,
  pub(crate) bucket_duration: u64,
  pub(crate) alignment: u64,
  pub(crate) is_return_empty: bool,
  pub(crate) bucket_timestamp_type: BucketTimestampType,
}

/// Single-series range query operations (TS.RANGE, TS.REVRANGE).
/// 单时间序列范围查询操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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

  pub(crate) fn ts_range_internal_with_meta(
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
    let prefix = key::prefix_stack(&kc, key_bytes);
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
      let k = entry.key();
      if k.starts_with(prefix.as_slice()) {
        k.to_vec()
      } else {
        prefix.to_vec()
      }
    } else {
      prefix.to_vec()
    };

    for g in data_ks.range((Bound::Included(start_range_k.as_slice()), Bound::Unbounded)) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix.as_slice()) {
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
}
