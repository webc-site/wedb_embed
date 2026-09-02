pub mod chunk;
pub mod r#const;
pub mod gorilla;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod opt;
use std::{cmp::Ordering, collections::BinaryHeap, str};

pub use chunk::{ChunkHeader, MergeStats, TSChunk};
pub use r#const::*;
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
use rapidhash::{RapidHashMap, RapidHashSet};
/// Domain operation (aligned with Apache Kvrocks TSMQueryFilterParser).
/// 时序标签过滤器（对标 Apache Kvrocks TSMQueryFilterParser）
#[derive(Debug, Clone, Default)]
pub struct TimeSeriesLabelFilter {
  pub equals: RapidHashMap<String, RapidHashSet<String>>,
  pub not_equals: RapidHashMap<String, RapidHashSet<String>>,
  pub has_matchers: bool,
}

impl TimeSeriesLabelFilter {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn parse(filters: &[String]) -> Self {
    let mut filter = Self::new();
    for f in filters {
      filter.add_filter(f);
    }
    filter
  }

  pub fn add_filter(&mut self, expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
      return false;
    }

    let (op_pos, is_not_equal) = Self::find_operator(trimmed);
    if op_pos == usize::MAX {
      return false;
    }

    let label = trimmed[..op_pos].trim().to_string();
    let value_str = if is_not_equal {
      trimmed[op_pos + 2..].trim()
    } else {
      trimmed[op_pos + 1..].trim()
    };

    if is_not_equal {
      let mut vals = RapidHashSet::default();
      if value_str.is_empty() {
        // k!= 表示必须存在标签 k
        vals.insert(String::new());
      } else if value_str.starts_with('(') && value_str.ends_with(')') {
        for item in Self::split_value_list(&value_str[1..value_str.len() - 1]) {
          let unquoted = Self::unquote(item);
          if !unquoted.is_empty() {
            vals.insert(unquoted.to_string());
          }
        }
      } else {
        vals.insert(Self::unquote(value_str).to_string());
      }
      self.not_equals.entry(label).or_default().extend(vals);
      self.has_matchers = true;
      true
    } else {
      let mut vals = RapidHashSet::default();
      if value_str.is_empty() {
        // k= 表示标签 k 不能存在
        self.equals.entry(label).or_default();
      } else if value_str.starts_with('(') && value_str.ends_with(')') {
        for item in Self::split_value_list(&value_str[1..value_str.len() - 1]) {
          let unquoted = Self::unquote(item);
          if !unquoted.is_empty() {
            vals.insert(unquoted.to_string());
          }
        }
        self.equals.entry(label).or_default().extend(vals);
      } else {
        vals.insert(Self::unquote(value_str).to_string());
        self.equals.entry(label).or_default().extend(vals);
      }
      self.has_matchers = true;
      true
    }
  }

  fn find_operator(expr: &str) -> (usize, bool) {
    let mut quote = None;
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
      let b = bytes[i];
      if b == b'\'' || b == b'"' {
        if quote == Some(b) {
          quote = None;
        } else if quote.is_none() {
          quote = Some(b);
        }
      } else if quote.is_none() {
        if b == b'!' && i + 1 < len && bytes[i + 1] == b'=' {
          return (i, true);
        } else if b == b'=' {
          return (i, false);
        }
      }
      i += 1;
    }
    (usize::MAX, false)
  }

  fn split_value_list(list: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut quote = None;
    let mut depth = 0;
    let mut start = 0;
    let bytes = list.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
      if b == b'\'' || b == b'"' {
        if quote == Some(b) {
          quote = None;
        } else if quote.is_none() {
          quote = Some(b);
        }
      } else if quote.is_none() {
        if b == b'(' {
          depth += 1;
        } else if b == b')' && depth > 0 {
          depth -= 1;
        } else if b == b',' && depth == 0 {
          let val = list[start..i].trim();
          if !val.is_empty() {
            values.push(val);
          }
          start = i + 1;
        }
      }
    }
    if start < list.len() {
      let val = list[start..].trim();
      if !val.is_empty() {
        values.push(val);
      }
    }
    values
  }

  #[inline]
  fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2
      && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
      &s[1..s.len() - 1]
    } else {
      s
    }
  }

  pub fn matches(&self, meta_labels: &[(String, String)]) -> bool {
    if !self.has_matchers {
      return true;
    }

    for (k, allowed_vals) in &self.equals {
      let actual = meta_labels
        .iter()
        .find(|(lk, _)| lk == k)
        .map(|(_, lv)| lv.as_str());
      if allowed_vals.is_empty() {
        if actual.is_some() {
          return false;
        }
      } else {
        match actual {
          Some(actual_v) => {
            if !allowed_vals.contains(actual_v) {
              return false;
            }
          }
          None => return false,
        }
      }
    }

    for (k, forbidden_vals) in &self.not_equals {
      let actual = meta_labels
        .iter()
        .find(|(lk, _)| lk == k)
        .map(|(_, lv)| lv.as_str());
      if forbidden_vals.contains("") && actual.is_none() {
        return false;
      }
      if let Some(actual_v) = actual
        && forbidden_vals.contains(actual_v)
      {
        return false;
      }
    }

    true
  }
}

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

  let reduce = |values: &[f64]| -> f64 {
    if values.is_empty() {
      return 0.0;
    }
    let count = values.len() as f64;
    match reducer_type {
      GroupReducerType::None => 0.0,
      GroupReducerType::Count => count,
      GroupReducerType::First => values[0],
      GroupReducerType::Last => values[values.len() - 1],
      GroupReducerType::Sum => values.iter().sum(),
      GroupReducerType::Avg | GroupReducerType::Twa => values.iter().sum::<f64>() / count,
      GroupReducerType::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
      GroupReducerType::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
      GroupReducerType::Range => {
        let (min_v, max_v) = values
          .iter()
          .copied()
          .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), v| {
            (min.min(v), max.max(v))
          });
        max_v - min_v
      }
      GroupReducerType::VarP
      | GroupReducerType::StdP
      | GroupReducerType::VarS
      | GroupReducerType::StdS => {
        let (sum, sq_sum) = values
          .iter()
          .copied()
          .fold((0.0, 0.0), |(sum, sq_sum), v| (sum + v, sq_sum + v * v));
        let var_p = ((sq_sum - (sum * sum) / count) / count).max(0.0);
        match reducer_type {
          GroupReducerType::VarP => var_p,
          GroupReducerType::StdP => var_p.sqrt(),
          GroupReducerType::VarS => {
            if count <= 1.0 {
              0.0
            } else {
              var_p * count / (count - 1.0)
            }
          }
          GroupReducerType::StdS => {
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
  };

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
