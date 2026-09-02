use std::collections::BTreeSet;

use bitcode::{Decode, Encode};

use crate::api::timeseries::meta::{ChunkType, DuplicatePolicy};

/// Command options enumeration.
/// TS.CREATE 选项枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TsCreate {
  RetentionTime(u64),
  ChunkSize(u64),
  ChunkType(ChunkType),
  DuplicatePolicy(DuplicatePolicy),
  SourceKey(String),
  Labels(Vec<(String, String)>),
}

/// Command options enumeration.
/// TS.RANGE 选项枚举
#[derive(Debug, Clone, PartialEq)]
pub enum TsRange {
  Count(usize),
  FilterByTs(BTreeSet<u64>),
  FilterByValue(f64, f64),
  Aggregation(AggregationType, u64),
  Alignment(u64),
  Latest,
  Empty,
  BucketTimestamp(BucketTimestampType),
}

/// Operation definition.
/// 多序列聚合组 Reducer 类型
/// Operation definition.
/// 桶时间戳对齐类型
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Encode,
  Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum BucketTimestampType {
  #[default]
  #[strum(serialize = "-", serialize = "START", serialize = "low")]
  Start = 0,
  #[strum(serialize = "+", serialize = "END", serialize = "high")]
  End = 1,
  #[strum(serialize = "~", serialize = "MID", serialize = "mid")]
  Mid = 2,
}

impl BucketTimestampType {
  #[inline(always)]
  pub const fn calculate_timestamp(&self, bucket_left: u64, bucket_duration: u64) -> u64 {
    match self {
      Self::Start => bucket_left,
      Self::End => bucket_left.saturating_add(bucket_duration),
      Self::Mid => bucket_left.saturating_add(bucket_duration / 2),
    }
  }
}

/// Operation definition.
/// 多序列聚合组 Reducer 类型
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Encode,
  Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum GroupReducerType {
  #[default]
  #[strum(serialize = "sum", serialize = "SUM")]
  Sum = 0,
  #[strum(serialize = "min", serialize = "MIN")]
  Min = 1,
  #[strum(serialize = "max", serialize = "MAX")]
  Max = 2,
  #[strum(serialize = "avg", serialize = "AVG")]
  Avg = 3,
  #[strum(serialize = "count", serialize = "COUNT")]
  Count = 4,
  #[strum(serialize = "range", serialize = "RANGE")]
  Range = 5,
  #[strum(serialize = "first", serialize = "FIRST")]
  First = 6,
  #[strum(serialize = "last", serialize = "LAST")]
  Last = 7,
  #[strum(serialize = "std.p", serialize = "STD.P", serialize = "std_p")]
  StdP = 8,
  #[strum(serialize = "std.s", serialize = "STD.S", serialize = "std_s")]
  StdS = 9,
  #[strum(serialize = "var.p", serialize = "VAR.P", serialize = "var_p")]
  VarP = 10,
  #[strum(serialize = "var.s", serialize = "VAR.S", serialize = "var_s")]
  VarS = 11,
  #[strum(serialize = "twa", serialize = "TWA")]
  Twa = 12,
  #[strum(serialize = "none", serialize = "NONE")]
  None = 13,
}

impl GroupReducerType {
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Sum => "sum",
      Self::Min => "min",
      Self::Max => "max",
      Self::Avg => "avg",
      Self::Count => "count",
      Self::Range => "range",
      Self::First => "first",
      Self::Last => "last",
      Self::StdP => "std.p",
      Self::StdS => "std.s",
      Self::VarP => "var.p",
      Self::VarS => "var.s",
      Self::Twa => "twa",
      Self::None => "none",
    }
  }
}

/// Operation definition.
/// 聚合函数类型
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Encode,
  Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum AggregationType {
  #[default]
  #[strum(serialize = "avg", serialize = "AVG")]
  Avg = 0,
  #[strum(serialize = "first", serialize = "FIRST")]
  First = 1,
  #[strum(serialize = "last", serialize = "LAST")]
  Last = 2,
  #[strum(serialize = "min", serialize = "MIN")]
  Min = 3,
  #[strum(serialize = "max", serialize = "MAX")]
  Max = 4,
  #[strum(serialize = "sum", serialize = "SUM")]
  Sum = 5,
  #[strum(serialize = "count", serialize = "COUNT")]
  Count = 6,
  #[strum(serialize = "std.p", serialize = "STD.P", serialize = "std_p")]
  StdP = 7,
  #[strum(serialize = "std.s", serialize = "STD.S", serialize = "std_s")]
  StdS = 8,
  #[strum(serialize = "var.p", serialize = "VAR.P", serialize = "var_p")]
  VarP = 9,
  #[strum(serialize = "var.s", serialize = "VAR.S", serialize = "var_s")]
  VarS = 10,
  #[strum(serialize = "range", serialize = "RANGE")]
  Range = 11,
  #[strum(serialize = "twa", serialize = "TWA")]
  Twa = 12,
}

impl AggregationType {
  #[inline]
  pub const fn is_incremental(&self) -> bool {
    matches!(self, Self::Sum | Self::Count | Self::Min | Self::Max)
  }
}

/// Returns or computes calculated value.
/// 聚合计算器
#[derive(Debug, Clone, PartialEq, Default, Encode, Decode)]
pub struct Aggregator {
  pub agg_type: AggregationType,
  pub bucket_duration: u64,
  pub alignment: u64,
}

impl Aggregator {
  #[inline]
  pub const fn new(agg_type: AggregationType, bucket_duration: u64, alignment: u64) -> Self {
    Self {
      agg_type,
      bucket_duration,
      alignment,
    }
  }

  #[inline]
  pub fn calculate_aligned_bucket_left(&self, ts: u64) -> u64 {
    if self.bucket_duration == 0 {
      return ts;
    }
    let align = self.alignment % self.bucket_duration;
    if ts < align {
      0
    } else {
      ((ts - align) / self.bucket_duration) * self.bucket_duration + align
    }
  }

  #[inline]
  pub fn calculate_aligned_bucket_right(&self, ts: u64) -> u64 {
    let left = self.calculate_aligned_bucket_left(ts);
    left.saturating_add(self.bucket_duration)
  }

  pub fn split_and_aggregate(
    &self,
    samples: &[(u64, f64)],
    count_limit: Option<usize>,
    is_return_empty: bool,
    bucket_timestamp_type: BucketTimestampType,
  ) -> Vec<(u64, f64)> {
    if samples.is_empty() {
      return Vec::new();
    }
    let mut results = Vec::new();
    let mut curr_bucket = self.calculate_aligned_bucket_left(samples[0].0);
    let mut bucket_samples = Vec::new();
    let mut last_val;

    for &(ts, v) in samples {
      let bkt = self.calculate_aligned_bucket_left(ts);
      if bkt == curr_bucket {
        bucket_samples.push((ts, v));
      } else {
        if is_return_empty {
          let agg_ts = bucket_timestamp_type.calculate_timestamp(curr_bucket, self.bucket_duration);
          let val = self.aggregate(&bucket_samples);
          last_val = val;
          results.push((agg_ts, val));
          if let Some(limit) = count_limit
            && results.len() >= limit
          {
            return results;
          }

          if self.bucket_duration > 0 {
            let mut next_bucket = curr_bucket.saturating_add(self.bucket_duration);
            while next_bucket < bkt {
              let empty_ts =
                bucket_timestamp_type.calculate_timestamp(next_bucket, self.bucket_duration);
              let empty_val = if self.agg_type == AggregationType::Last {
                last_val
              } else {
                0.0
              };
              results.push((empty_ts, empty_val));
              if let Some(limit) = count_limit
                && results.len() >= limit
              {
                return results;
              }
              next_bucket = next_bucket.saturating_add(self.bucket_duration);
            }
          }
        } else if !bucket_samples.is_empty() {
          let agg_ts = bucket_timestamp_type.calculate_timestamp(curr_bucket, self.bucket_duration);
          let val = self.aggregate(&bucket_samples);
          results.push((agg_ts, val));
          if let Some(limit) = count_limit
            && results.len() >= limit
          {
            return results;
          }
        }
        curr_bucket = bkt;
        bucket_samples.clear();
        bucket_samples.push((ts, v));
      }
    }

    if !bucket_samples.is_empty() || is_return_empty {
      let agg_ts = bucket_timestamp_type.calculate_timestamp(curr_bucket, self.bucket_duration);
      let val = self.aggregate(&bucket_samples);
      results.push((agg_ts, val));
    }

    if let Some(limit) = count_limit {
      results.truncate(limit);
    }
    results
  }

  #[inline]
  pub fn aggregate_samples(&self, samples: &[(u64, f64)]) -> f64 {
    self.aggregate(samples)
  }

  pub fn aggregate(&self, samples: &[(u64, f64)]) -> f64 {
    if samples.is_empty() {
      return 0.0;
    }
    let count = samples.len() as f64;
    match self.agg_type {
      AggregationType::Avg | AggregationType::Twa => {
        let sum: f64 = samples.iter().map(|s| s.1).sum();
        sum / count
      }
      AggregationType::First => samples[0].1,
      AggregationType::Last => samples[samples.len() - 1].1,
      AggregationType::Min => samples.iter().map(|s| s.1).fold(f64::INFINITY, f64::min),
      AggregationType::Max => samples
        .iter()
        .map(|s| s.1)
        .fold(f64::NEG_INFINITY, f64::max),
      AggregationType::Sum => samples.iter().map(|s| s.1).sum(),
      AggregationType::Count => count,
      AggregationType::Range => {
        let (min, max) = samples
          .iter()
          .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), s| {
            (min.min(s.1), max.max(s.1))
          });
        max - min
      }
      AggregationType::StdP
      | AggregationType::VarP
      | AggregationType::StdS
      | AggregationType::VarS => {
        let sum: f64 = samples.iter().map(|s| s.1).sum();
        let mean = sum / count;
        let var_p = samples.iter().map(|s| (s.1 - mean).powi(2)).sum::<f64>() / count;
        match self.agg_type {
          AggregationType::VarP => var_p,
          AggregationType::StdP => var_p.sqrt(),
          AggregationType::VarS => {
            if count <= 1.0 {
              0.0
            } else {
              var_p * count / (count - 1.0)
            }
          }
          AggregationType::StdS => {
            if count <= 1.0 {
              0.0
            } else {
              (var_p * count / (count - 1.0)).max(0.0).sqrt()
            }
          }
          _ => 0.0,
        }
      }
    }
  }
}

/// TS.MGET command options enumeration.
/// TS.MGET 选项枚举
#[derive(Debug, Clone)]
pub enum TsMGet {
  WithLabels,
  SelectedLabels(Vec<String>),
  Filters(Vec<String>),
}

/// Operation definition.
/// TS.MGET 结果
#[derive(Debug, Clone, PartialEq)]
pub struct TsMGetResult {
  pub name: String,
  pub labels: Vec<(String, String)>,
  pub sample: Option<(u64, f64)>,
}

/// Command options enumeration.
/// TS.MRANGE 选项枚举
#[derive(Debug, Clone)]
pub enum TsMRange {
  WithLabels,
  SelectedLabels(Vec<String>),
  Filters(Vec<String>),
  Count(usize),
  FilterByTs(BTreeSet<u64>),
  FilterByValue(f64, f64),
  Aggregation(AggregationType, u64),
  Alignment(u64),
  Latest,
  Empty,
  BucketTimestamp(BucketTimestampType),
  GroupBy(String, GroupReducerType),
}

/// Operation definition.
/// TS.MRANGE 结果
#[derive(Debug, Clone, PartialEq)]
pub struct TsMRangeResult {
  pub name: String,
  pub labels: Vec<(String, String)>,
  pub samples: Vec<(u64, f64)>,
  pub source_keys: Vec<String>,
}

/// Operation definition.
/// TS.INFO 结果信息
#[derive(Debug, Clone, PartialEq)]
pub struct TsInfoResult {
  pub total_samples: u64,
  pub memory_usage: u64,
  pub first_timestamp: u64,
  pub last_timestamp: u64,
  pub retention_time: u64,
  pub chunk_count: usize,
  pub chunk_size: u64,
  pub chunk_type: ChunkType,
  pub duplicate_policy: DuplicatePolicy,
  pub source_key: String,
  pub labels: Vec<(String, String)>,
  pub downstream_rules: Vec<(String, Aggregator)>,
}

/// Operation definition.
/// 降采样下游规则元数据
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct TSDownStreamMeta {
  pub aggregator: Aggregator,
  pub latest_bucket_idx: u64,
}

impl TSDownStreamMeta {
  #[inline]
  pub fn new(aggregator: Aggregator) -> Self {
    Self {
      aggregator,
      latest_bucket_idx: 0,
    }
  }

  #[inline]
  pub fn encode(&self) -> Vec<u8> {
    bitcode::encode(self)
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    bitcode::decode(bytes).ok()
  }
}

use std::ops::{Bound, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

pub trait IntoTsRange {
  fn into_ts_range(self) -> (u64, u64);
}

impl IntoTsRange for (u64, u64) {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    self
  }
}

impl IntoTsRange for &(u64, u64) {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    *self
  }
}

impl IntoTsRange for Range<u64> {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    (self.start, self.end.saturating_sub(1))
  }
}

impl IntoTsRange for RangeInclusive<u64> {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    (*self.start(), *self.end())
  }
}

impl IntoTsRange for RangeFrom<u64> {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    (self.start, u64::MAX)
  }
}

impl IntoTsRange for RangeTo<u64> {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    (0, self.end.saturating_sub(1))
  }
}

impl IntoTsRange for RangeToInclusive<u64> {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    (0, self.end)
  }
}

impl IntoTsRange for RangeFull {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    (0, u64::MAX)
  }
}

impl IntoTsRange for (Bound<u64>, Bound<u64>) {
  #[inline]
  fn into_ts_range(self) -> (u64, u64) {
    let start_ts = match self.0 {
      Bound::Included(v) => v,
      Bound::Excluded(v) => v.saturating_add(1),
      Bound::Unbounded => 0,
    };
    let end_ts = match self.1 {
      Bound::Included(v) => v,
      Bound::Excluded(v) => v.saturating_sub(1),
      Bound::Unbounded => u64::MAX,
    };
    (start_ts, end_ts)
  }
}
