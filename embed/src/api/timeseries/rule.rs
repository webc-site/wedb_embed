use crate::{
  api::timeseries::{
    r#const::*,
    filter::TimeSeriesLabelFilter,
    key,
    meta::{DuplicatePolicy, TimeSeriesMeta},
    opt::{AggregationType, Aggregator, TSDownStreamMeta},
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

/// Downstream compaction rules and label indexing (TS.CREATERULE, TS.DELETERULE, TS.QUERYINDEX).
/// 时间序列下游聚合规则与标签索引操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn ts_queryindex(&self, filters: &[String]) -> Result<Vec<String>> {
    let filter = TimeSeriesLabelFilter::parse(filters);
    let now_ms = current_now_ms();

    let mut keys = Vec::new();
    super::mrange::iter_matching_series(self, &filter, now_ms, |name_bytes, _| {
      keys.push(String::from_utf8_lossy(name_bytes).into_owned());
      Ok(())
    })?;
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

    let dst_ds_prefix_k = key::downstream_prefix_stack(&kc, dst_bytes);
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

pub(crate) fn trigger_downstream_upsert<E: Engine>(
  db: &Db<E>,
  src_key: &[u8],
  ts: u64,
  val: f64,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_ks = db.meta();
  let ds_prefix_k = key::downstream_prefix_stack(&kc, src_key);

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

pub(crate) fn cascade_downstream_del<E: Engine>(
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
  let ds_prefix_k = key::downstream_prefix_stack(&kc, src_key);

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
