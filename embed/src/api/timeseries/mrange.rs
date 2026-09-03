use rapidhash::RapidHashMap;

use crate::{
  api::timeseries::{
    BucketTimestampType, GroupReducerType, IntoTsRange, TSChunk, TimeSeriesLabelFilter,
    TimeSeriesMeta, TsFilter, TsMGet, TsMGetResult, TsMRange, TsMRangeResult,
    group_samples_and_reduce, key, range::TsRangeQuery,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

type GroupedSeriesEntry = (Vec<Vec<(u64, f64)>>, Vec<String>);

#[inline]
pub(crate) fn project_labels(
  labels: &[(String, String)],
  selected_labels: &[String],
  with_labels: bool,
  group_by_label: Option<&String>,
) -> Vec<(String, String)> {
  if with_labels {
    return labels.to_vec();
  }
  if let Some(gl) = group_by_label {
    let mut res_labels = Vec::new();
    if let Some((_, v)) = labels.iter().find(|(lk, _)| lk == gl) {
      res_labels.push((gl.clone(), v.clone()));
    }
    for sel_k in selected_labels {
      if sel_k != gl {
        let val = labels
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
        let val = labels
          .iter()
          .find(|(k, _)| k == sel_k)
          .map(|(_, v)| v.as_str())
          .unwrap_or("");
        (sel_k.clone(), val.to_string())
      })
      .collect()
  } else {
    Vec::new()
  }
}

#[inline]
pub(crate) fn iter_matching_series<E: Engine, F>(
  db: &Db<E>,
  filter: &TimeSeriesLabelFilter,
  now_ms: u64,
  mut f: F,
) -> Result<()>
where
  Error: From<E::Error>,
  F: FnMut(&[u8], TimeSeriesMeta) -> Result<()>,
{
  let kc = db.kc();
  let prefix = key::meta_prefix_stack(&kc);
  let meta_ks = db.meta();

  for g in meta_ks.prefix(&prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if k.starts_with(prefix.as_slice())
      && TimeSeriesMeta::matches_labels_raw(v, filter)
      && let Some(meta) = TimeSeriesMeta::decode(v)
      && !meta.is_expired(now_ms)
    {
      let name_bytes = &k[prefix.len()..];
      f(name_bytes, meta)?;
    }
  }
  Ok(())
}

/// Cross-series multi-key range query and filtering engine.
/// 跨序列多键范围检索与聚合引擎 (TS.MGET, TS.MRANGE, TS.MREVRANGE)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
    let data_ks = self.data();
    let now_ms = current_now_ms();
    let filter = TimeSeriesLabelFilter::parse(&filters);

    let mut results = Vec::new();
    iter_matching_series(self, &filter, now_ms, |name_bytes, meta| {
      let name = String::from_utf8_lossy(name_bytes).into_owned();

      let latest_sample = if meta.total_samples > 0 {
        let data_prefix = key::prefix_stack(&kc, name_bytes);
        if let Some(chunk_entry) = data_ks.prefix(&data_prefix).next_back() {
          let chunk = chunk_entry?;
          if chunk.key().starts_with(data_prefix.as_slice()) {
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

      let labels = project_labels(&meta.labels, &selected_labels, with_labels, None);

      results.push(TsMGetResult {
        name,
        labels,
        sample: latest_sample,
      });
      Ok(())
    })?;

    results.sort_unstable_by(|a, b| a.name.cmp(&b.name));
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

    let now_ms = current_now_ms();
    let filter = TimeSeriesLabelFilter::parse(&filters);

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

    iter_matching_series(self, &filter, now_ms, |name_bytes, meta| {
      let name = String::from_utf8_lossy(name_bytes).into_owned();

      let labels = project_labels(
        &meta.labels,
        &selected_labels,
        with_labels,
        group_by_label.as_ref(),
      );

      let samples = self.ts_range_internal_with_meta(name_bytes, Some(&meta), &query)?;
      series_results.push((name, labels, samples));
      Ok(())
    })?;

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
    let mut results = self.ts_mrange(range, opt_li)?;
    for r in &mut results {
      r.samples.reverse();
    }
    Ok(results)
  }
}
