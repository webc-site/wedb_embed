pub mod chunk;
pub mod r#const;
pub mod filter;
pub mod gorilla;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod opt;
pub mod reducer;
use std::{cmp::Ordering, collections::BinaryHeap};

pub use chunk::{ChunkHeader, MergeStats, TSChunk};
pub use r#const::*;
pub use filter::{LabelMatcher, TimeSeriesLabelFilter, TsFilter};
pub use gorilla::TSSample;
pub use key::{
  chunk as compose_ts_chunk, chunk as compose_ts_item,
  downstream_meta as compose_ts_downstream_meta, downstream_prefix as compose_ts_downstream_prefix,
  meta as compose_ts_meta_key, meta_prefix as compose_ts_meta_prefix, prefix as compose_ts_prefix,
};
pub use meta::{ChunkType, DuplicatePolicy, TimeSeriesMeta, TimeSeriesMetaArgs};
pub use opt::{
  AggregationType, Aggregator, BucketTimestampType, GroupReducerType, IntoTsRange,
  TSDownStreamMeta, TsCreate, TsInfoResult, TsMGet, TsMGetResult, TsMRange, TsMRangeResult,
  TsRange,
};
pub use reducer::Reducer;

/// Domain operation (aligned with Apache Kvrocks GroupSamplesAndReduce).
/// 多序列聚合归约（对标 Apache Kvrocks GroupSamplesAndReduce）
pub fn group_samples_and_reduce(
  all_samples: &[Vec<(u64, f64)>],
  reducer_type: GroupReducerType,
) -> Vec<(u64, f64)> {
  if reducer_type == GroupReducerType::None || all_samples.is_empty() {
    return Vec::new();
  }

  #[derive(Eq, PartialEq)]
  struct HeapItem {
    ts: u64,
    vec_idx: usize,
    sample_idx: usize,
  }

  impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
      other.ts.cmp(&self.ts) // 最小堆
    }
  }

  impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
    }
  }

  let mut heap = BinaryHeap::new();
  for (i, vec) in all_samples.iter().enumerate() {
    if !vec.is_empty() {
      heap.push(HeapItem {
        ts: vec[0].0,
        vec_idx: i,
        sample_idx: 0,
      });
    }
  }

  let mut result = Vec::new();
  let mut current_ts = None;
  let mut current_values = Vec::new();

  let reduce = |values: &[f64]| -> f64 { Reducer::reduce_f64(values, reducer_type) };

  while let Some(top) = heap.pop() {
    let val = all_samples[top.vec_idx][top.sample_idx].1;
    match current_ts {
      Some(ts) if ts == top.ts => {
        current_values.push(val);
      }
      Some(ts) => {
        result.push((ts, reduce(&current_values)));
        current_values.clear();
        current_values.push(val);
        current_ts = Some(top.ts);
      }
      None => {
        current_ts = Some(top.ts);
        current_values.push(val);
      }
    }

    let next_idx = top.sample_idx + 1;
    if next_idx < all_samples[top.vec_idx].len() {
      heap.push(HeapItem {
        ts: all_samples[top.vec_idx][next_idx].0,
        vec_idx: top.vec_idx,
        sample_idx: next_idx,
      });
    }
  }

  if let Some(ts) = current_ts {
    result.push((ts, reduce(&current_values)));
  }

  result
}
