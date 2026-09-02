use super::opt::{AggregationType, GroupReducerType};

/// Unified high-performance reducer engine for single-series aggregations
/// and multi-series group reductions (aligned with Apache Kvrocks Reducer).
///
/// 统一高性能规约计算引擎（对标 Apache Kvrocks Reducer）：
/// 1. 单次遍历 (Single Pass)：均值与方差采用 Welford 增量算法，时间复杂度严格为 $O(N)$。
/// 2. 避免灾难性抵消：杜绝 $\sum x^2 - (\sum x)^2 / n$ 朴素二次公式在大数小方差场景下的精度崩溃。
/// 3. 零堆内存分配：全流程使用迭代器流式计算，空间复杂度为 $O(1)$。
pub struct Reducer;

impl Reducer {
  /// 规约 `f64` 样本值序列（用于多序列 GroupReducer）
  pub fn reduce_f64(values: &[f64], reducer: GroupReducerType) -> f64 {
    if values.is_empty() || reducer == GroupReducerType::None {
      return 0.0;
    }
    match reducer {
      GroupReducerType::Count => values.len() as f64,
      GroupReducerType::First => values[0],
      GroupReducerType::Last => values[values.len() - 1],
      GroupReducerType::Sum => values.iter().copied().sum(),
      GroupReducerType::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
      GroupReducerType::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
      GroupReducerType::Avg | GroupReducerType::Twa => {
        let sum: f64 = values.iter().copied().sum();
        sum / (values.len() as f64)
      }
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
        let (var_p, var_s) = Self::compute_welford_var(values.iter().copied(), values.len());
        match reducer {
          GroupReducerType::VarP => var_p,
          GroupReducerType::StdP => var_p.sqrt(),
          GroupReducerType::VarS => var_s,
          GroupReducerType::StdS => var_s.sqrt(),
          _ => 0.0,
        }
      }
      GroupReducerType::None => 0.0,
    }
  }

  /// Reduces a slice of `(u64, f64)` timeseries samples for single series bucket aggregation.
  /// 规约 `(u64, f64)` 时序采样切片（用于单序列桶聚合）
  pub fn reduce_samples(samples: &[(u64, f64)], agg_type: AggregationType) -> f64 {
    if samples.is_empty() {
      return 0.0;
    }
    match agg_type {
      AggregationType::Count => samples.len() as f64,
      AggregationType::First => samples[0].1,
      AggregationType::Last => samples[samples.len() - 1].1,
      AggregationType::Sum => samples.iter().map(|s| s.1).sum(),
      AggregationType::Min => samples.iter().map(|s| s.1).fold(f64::INFINITY, f64::min),
      AggregationType::Max => samples
        .iter()
        .map(|s| s.1)
        .fold(f64::NEG_INFINITY, f64::max),
      AggregationType::Avg | AggregationType::Twa => {
        let sum: f64 = samples.iter().map(|s| s.1).sum();
        sum / (samples.len() as f64)
      }
      AggregationType::Range => {
        let (min_v, max_v) = samples
          .iter()
          .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), s| {
            (min.min(s.1), max.max(s.1))
          });
        max_v - min_v
      }
      AggregationType::VarP
      | AggregationType::StdP
      | AggregationType::VarS
      | AggregationType::StdS => {
        let (var_p, var_s) = Self::compute_welford_var(samples.iter().map(|s| s.1), samples.len());
        match agg_type {
          AggregationType::VarP => var_p,
          AggregationType::StdP => var_p.sqrt(),
          AggregationType::VarS => var_s,
          AggregationType::StdS => var_s.sqrt(),
          _ => 0.0,
        }
      }
    }
  }

  /// 基于 Welford 单遍流式增量算法计算总体方差 (VarP) 与样本方差 (VarS)
  #[inline]
  fn compute_welford_var(iter: impl Iterator<Item = f64>, count: usize) -> (f64, f64) {
    if count == 0 {
      return (0.0, 0.0);
    }
    let n = count as f64;
    let mut mean = 0.0;
    let mut m2 = 0.0;

    for (i, val) in iter.enumerate() {
      let k = (i + 1) as f64;
      let delta = val - mean;
      mean += delta / k;
      let delta2 = val - mean;
      m2 += delta * delta2;
    }

    let var_p = (m2 / n).max(0.0);
    let var_s = if count <= 1 {
      0.0
    } else {
      (m2 / (n - 1.0)).max(0.0)
    };
    (var_p, var_s)
  }
}
