use std::{cmp::Ordering, collections::BinaryHeap, mem::take};

use super::{r#const::*, key, meta::TDigestMeta, opt::TDigestInfo};
use crate::{
  engine::{Engine, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::{current_now_ms, generate_version},
  wedb::Db,
};

/// Safe floating-point comparison with epsilon tolerance (aligned with Kvrocks DoubleCompare).
/// 浮点数安全比较（容忍微小精度误差，对标 Apache Kvrocks DoubleCompare）
#[inline]
pub fn double_compare(a: f64, b: f64, rel_eps: f64, abs_eps: f64) -> Ordering {
  if a.is_nan() || b.is_nan() {
    return a.total_cmp(&b);
  }
  let diff = a - b;
  let adiff = diff.abs();
  if adiff <= abs_eps {
    return Ordering::Equal;
  }
  let maxab = a.abs().max(b.abs());
  if adiff <= maxab * rel_eps {
    return Ordering::Equal;
  }
  if diff < 0.0 {
    Ordering::Less
  } else {
    Ordering::Greater
  }
}

/// Double-precision equality check (aligned with Kvrocks DoubleEqual).
/// 浮点数相等判断（对标 Apache Kvrocks DoubleEqual）
#[inline]
pub fn double_equal(a: f64, b: f64) -> bool {
  double_compare(a, b, REL_EPS, ABS_EPS).is_eq()
}

/// Linear interpolation function lerp (aligned with Kvrocks Lerp).
/// 线性插值（对标 Apache Kvrocks Lerp）
#[inline]
pub const fn lerp(a: f64, b: f64, t: f64) -> f64 {
  a + t * (b - a)
}

/// Individual centroid node (aligned with Apache Kvrocks Centroid).
/// 单个质心节点 (Centroid，对标 Apache Kvrocks Centroid)
#[derive(Debug, Clone, Copy, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct Centroid {
  pub mean: f64,
  pub weight: f64,
}

impl Centroid {
  #[inline]
  pub const fn new(mean: f64, weight: f64) -> Self {
    Self { mean, weight }
  }

  /// In-place weighted merge with another centroid (aligned with Kvrocks Centroid::Merge).
  /// 与另一个质心就地加权合并（对标 Apache Kvrocks Centroid::Merge）
  #[inline]
  pub fn merge(&mut self, other: &Centroid) {
    self.weight += other.weight;
    self.mean += (other.mean - self.mean) * other.weight / self.weight;
  }

  #[inline]
  pub fn add(&mut self, mean: f64, weight: f64) {
    self.merge(&Centroid::new(mean, weight));
  }
}

/// Centroid collection with delta compression factor (aligned with Kvrocks CentroidsWithDelta).
/// 带压缩因子的质心集合（对标 Apache Kvrocks CentroidsWithDelta）
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct CentroidsWithDelta {
  pub centroids: Vec<Centroid>,
  pub delta: u32,
  pub min: f64,
  pub max: f64,
  pub total_weight: f64,
}

/// K1 scale function (aligned with Apache Kvrocks / Arrow ScalerK1).
/// K_1 比例缩放函数 (ScalerK1，对标 Apache Kvrocks / Apache Arrow ScalerK1)
#[derive(Debug, Clone, Copy)]
pub struct ScalerK1 {
  pub delta_norm: f64,
  pub inv_delta_norm: f64,
}

impl ScalerK1 {
  #[inline]
  pub fn new(delta: u32) -> Self {
    let norm = delta as f64 * INV_TWO_PI;
    Self {
      delta_norm: norm,
      inv_delta_norm: if norm != 0.0 { 1.0 / norm } else { 0.0 },
    }
  }

  #[inline]
  pub fn k(&self, q: f64) -> f64 {
    let q_clamped = q.clamp(0.0, 1.0);
    self.delta_norm * (2.0 * q_clamped - 1.0).asin()
  }

  #[inline]
  pub fn q(&self, k: f64) -> f64 {
    ((k * self.inv_delta_norm).sin() + 1.0) * 0.5
  }
}

/// T-Digest centroid stream merger (aligned with Apache Kvrocks TDigestMerger).
/// T-Digest 质心流合并器（对标 Apache Kvrocks TDigestMerger）
pub struct TDigestMerger {
  scaler: ScalerK1,
  total_weight: f64,
  weight_so_far: f64,
  weight_limit: f64,
}

impl TDigestMerger {
  #[inline]
  pub fn new(delta: u32) -> Self {
    Self {
      scaler: ScalerK1::new(delta),
      total_weight: 0.0,
      weight_so_far: 0.0,
      weight_limit: -1.0,
    }
  }

  #[inline]
  pub fn reset(&mut self, total_weight: f64) {
    self.total_weight = total_weight;
    self.weight_so_far = 0.0;
    self.weight_limit = -1.0;
  }

  /// Adds a single centroid to the merger.
  /// 向合并器添加单个质心
  #[inline]
  pub fn add(&mut self, output: &mut Vec<Centroid>, centroid: Centroid) {
    let weight = self.weight_so_far + centroid.weight;
    if weight <= self.weight_limit {
      if let Some(last) = output.last_mut() {
        last.merge(&centroid);
      } else {
        output.push(centroid);
      }
    } else {
      let quantile = if self.total_weight > 0.0 {
        self.weight_so_far / self.total_weight
      } else {
        0.0
      };
      let next_k = self.scaler.k(quantile) + 1.0;
      let next_weight_limit = self.total_weight * self.scaler.q(next_k);
      if next_weight_limit <= self.weight_limit {
        self.weight_limit = self.total_weight;
      } else {
        self.weight_limit = next_weight_limit;
      }
      output.push(centroid);
    }
    self.weight_so_far = weight;
  }

  /// Validates K-size constraint of centroid sequence (aligned with Kvrocks TDigestMerger::Validate).
  /// 校验质心序列的 K-Size 约束（对标 Apache Kvrocks TDigestMerger::Validate）
  pub fn validate(&self, tdigest: &[Centroid], total_weight: f64) -> Result<()> {
    let mut q_prev = 0.0;
    let mut k_prev = self.scaler.k(0.0);
    for i in tdigest {
      let q = q_prev + i.weight / total_weight;
      let k = self.scaler.k(q);
      if i.weight != 1.0 && (k - k_prev) > 1.001 {
        let diff = k - k_prev;
        return Err(Error::invalid_data(format!("oversized centroid: {diff}")));
      }
      k_prev = k;
      q_prev = q;
    }
    Ok(())
  }
}

/// Centroid stream cursor helper structure.
/// 质心流游标辅助结构
struct CentroidCursor<'a> {
  slice: &'a [Centroid],
  pos: usize,
}

impl CentroidCursor<'_> {
  #[inline]
  fn peek(&self) -> Option<&Centroid> {
    self.slice.get(self.pos)
  }

  #[inline]
  fn advance(&mut self) -> Option<Centroid> {
    if self.pos < self.slice.len() {
      let c = self.slice[self.pos];
      self.pos += 1;
      Some(c)
    } else {
      None
    }
  }
}

/// Priority queue min-heap node for K-way centroid stream merging.
/// 优先队列堆节点（用于多路质心流 K 路归并，小顶堆）
struct CentroidHeapItem {
  mean: f64,
  list_idx: usize,
}

impl PartialEq for CentroidHeapItem {
  #[inline]
  fn eq(&self, other: &Self) -> bool {
    double_equal(self.mean, other.mean)
  }
}

impl Eq for CentroidHeapItem {}

impl PartialOrd for CentroidHeapItem {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for CentroidHeapItem {
  #[inline]
  fn cmp(&self, other: &Self) -> Ordering {
    // 小顶堆：反向比较 mean，以较小 mean 优先
    double_compare(other.mean, self.mean, REL_EPS, ABS_EPS)
      .then_with(|| other.list_idx.cmp(&self.list_idx))
  }
}

/// Merges multiple centroid collections (aligned with Kvrocks TDigestMerge).
/// 合并多个质心集合（对标 Apache Kvrocks TDigestMerge）
pub fn tdigest_merge_centroids_list(
  centroids_list: &[CentroidsWithDelta],
  delta: u32,
) -> CentroidsWithDelta {
  tdigest_merge_buffer_and_centroids(&[], centroids_list, delta)
}

/// Merges raw buffer and centroid collection (aligned with Kvrocks TDigestMerge).
/// 合并原始缓冲区与质心集合（对标 Apache Kvrocks TDigestMerge(buffer, centroids)）
pub fn tdigest_merge_buffer_and_centroids(
  buffer: &[f64],
  centroids_list: &[CentroidsWithDelta],
  delta: u32,
) -> CentroidsWithDelta {
  let effective_delta = delta.max(10);
  let mut total_w = 0.0;
  let mut min_val = f64::MAX;
  let mut max_val = -f64::MAX;

  // 单次遍历过滤 NaN、累加权重并提取全局极值
  let mut sorted_buf = Vec::with_capacity(buffer.len());
  for &v in buffer {
    if !v.is_nan() {
      sorted_buf.push(v);
      total_w += 1.0;
      if v < min_val {
        min_val = v;
      }
      if v > max_val {
        max_val = v;
      }
    }
  }
  sorted_buf.sort_unstable_by(|a, b| a.total_cmp(b));

  let mut single_non_empty = None;
  let mut active_lists = Vec::with_capacity(centroids_list.len());
  for list in centroids_list {
    if list.centroids.is_empty() {
      continue;
    }
    total_w += list.total_weight;
    if list.min < min_val {
      min_val = list.min;
    }
    if list.max > max_val {
      max_val = list.max;
    }
    single_non_empty = Some(list);
    active_lists.push(CentroidCursor {
      slice: &list.centroids,
      pos: 0,
    });
  }

  if sorted_buf.is_empty() && active_lists.is_empty() {
    return CentroidsWithDelta {
      centroids: Vec::new(),
      delta: effective_delta,
      min: f64::MAX,
      max: -f64::MAX,
      total_weight: 0.0,
    };
  }

  // 单个已排序列且无 buffer 且 delta 匹配时直接快速返回
  if sorted_buf.is_empty()
    && active_lists.len() == 1
    && let Some(list) = single_non_empty
    && list.delta == effective_delta
  {
    return CentroidsWithDelta {
      centroids: list.centroids.clone(),
      delta: effective_delta,
      min: min_val,
      max: max_val,
      total_weight: total_w,
    };
  }

  let mut merger = TDigestMerger::new(effective_delta);
  merger.reset(total_w);
  let mut output = Vec::with_capacity(effective_delta as usize);

  let mut b_idx = 0;
  let b_len = sorted_buf.len();

  // 针对常见情况（0、1 或 2 个质心列表 + buffer）进行高度内联的零分配双指针流式合并
  if active_lists.is_empty() {
    for &v in &sorted_buf {
      merger.add(&mut output, Centroid::new(v, 1.0));
    }
  } else if active_lists.len() == 1 {
    let cursor = &mut active_lists[0];
    while b_idx < b_len && cursor.pos < cursor.slice.len() {
      let c = cursor.slice[cursor.pos];
      let bv = sorted_buf[b_idx];
      if double_compare(c.mean, bv, REL_EPS, ABS_EPS).is_lt() {
        merger.add(&mut output, c);
        cursor.pos += 1;
      } else {
        merger.add(&mut output, Centroid::new(bv, 1.0));
        b_idx += 1;
      }
    }
    while let Some(c) = cursor.advance() {
      merger.add(&mut output, c);
    }
    while b_idx < b_len {
      merger.add(&mut output, Centroid::new(sorted_buf[b_idx], 1.0));
      b_idx += 1;
    }
  } else if active_lists.len() == 2 && sorted_buf.is_empty() {
    // 双质心流快速合并（TDIGEST.MERGE 两路合并常见场景）
    let (c0_slice, c1_slice) = (active_lists[0].slice, active_lists[1].slice);
    let mut p0 = 0;
    let mut p1 = 0;
    let l0 = c0_slice.len();
    let l1 = c1_slice.len();

    while p0 < l0 && p1 < l1 {
      let v0 = c0_slice[p0];
      let v1 = c1_slice[p1];
      if double_compare(v0.mean, v1.mean, REL_EPS, ABS_EPS).is_lt() {
        merger.add(&mut output, v0);
        p0 += 1;
      } else {
        merger.add(&mut output, v1);
        p1 += 1;
      }
    }
    while p0 < l0 {
      merger.add(&mut output, c0_slice[p0]);
      p0 += 1;
    }
    while p1 < l1 {
      merger.add(&mut output, c1_slice[p1]);
      p1 += 1;
    }
  } else {
    // 多路质心流合并 (K-way PriorityQueue Merge, O(N log K))
    let mut heap = BinaryHeap::with_capacity(active_lists.len());
    for (i, cur) in active_lists.iter().enumerate() {
      if let Some(c) = cur.peek() {
        heap.push(CentroidHeapItem {
          mean: c.mean,
          list_idx: i,
        });
      }
    }

    while !heap.is_empty() || b_idx < b_len {
      let next_buf_val = if b_idx < b_len {
        Some(sorted_buf[b_idx])
      } else {
        None
      };

      match (heap.peek(), next_buf_val) {
        (None, None) => break,
        (Some(&CentroidHeapItem { list_idx, .. }), None) => {
          heap.pop();
          if let Some(c) = active_lists[list_idx].advance() {
            merger.add(&mut output, c);
          }
          if let Some(next_c) = active_lists[list_idx].peek() {
            heap.push(CentroidHeapItem {
              mean: next_c.mean,
              list_idx,
            });
          }
        }
        (None, Some(bv)) => {
          merger.add(&mut output, Centroid::new(bv, 1.0));
          b_idx += 1;
        }
        (Some(&CentroidHeapItem { mean, list_idx }), Some(bv)) => {
          if double_compare(mean, bv, REL_EPS, ABS_EPS).is_lt() {
            heap.pop();
            if let Some(c) = active_lists[list_idx].advance() {
              merger.add(&mut output, c);
            }
            if let Some(next_c) = active_lists[list_idx].peek() {
              heap.push(CentroidHeapItem {
                mean: next_c.mean,
                list_idx,
              });
            }
          } else {
            merger.add(&mut output, Centroid::new(bv, 1.0));
            b_idx += 1;
          }
        }
      }
    }
  }

  CentroidsWithDelta {
    centroids: output,
    delta: effective_delta,
    min: min_val,
    max: max_val,
    total_weight: total_w,
  }
}

/// Quantile estimation algorithm aligned with Apache Kvrocks TDigestQuantile.
/// 分位数估算算法（对标 Apache Kvrocks TDigestQuantile）
pub fn tdigest_quantile_calc(
  centroids: &[Centroid],
  min: f64,
  max: f64,
  total_weight: f64,
  q: f64,
) -> f64 {
  if q.is_nan() || !(0.0..=1.0).contains(&q) || centroids.is_empty() || total_weight <= 0.0 {
    return f64::NAN;
  }

  let index = q * total_weight;
  if index <= 1.0 {
    return min;
  } else if index >= total_weight - 1.0 {
    return max;
  }

  let mut weight_sum = 0.0;
  let mut ci = 0;
  for (i, c) in centroids.iter().enumerate() {
    weight_sum += c.weight;
    ci = i;
    if index <= weight_sum {
      break;
    }
  }

  let centroid = centroids[ci];
  let mut diff = index + centroid.weight * 0.5 - weight_sum;

  if centroid.weight == 1.0 && diff.abs() < 0.5 {
    return centroid.mean;
  }

  let mut ci_left = ci;
  let mut ci_right = ci;
  if diff > 0.0 {
    if ci_right == centroids.len() - 1 {
      let c = centroids[ci_right];
      return lerp(c.mean, max, diff / (c.weight * 0.5));
    }
    ci_right += 1;
  } else {
    if ci_left == 0 {
      let c = centroids[0];
      return lerp(min, c.mean, index / (c.weight * 0.5));
    }
    ci_left -= 1;
    diff += centroids[ci_left].weight * 0.5 + centroids[ci_right].weight * 0.5;
  }

  let lc = centroids[ci_left];
  let rc = centroids[ci_right];
  diff /= lc.weight * 0.5 + rc.weight * 0.5;
  lerp(lc.mean, rc.mean, diff)
}

/// Cumulative distribution function CDF calculation aligned with Apache Kvrocks TDigestCDF.
/// 累积分布函数 CDF 计算（对标 Apache Kvrocks TDigestCDF 与 RedisBloom）
pub fn tdigest_cdf_calc(
  centroids: &[Centroid],
  centroids_min: f64,
  centroids_max: f64,
  total_weight: f64,
  inputs: &[f64],
) -> Vec<f64> {
  if centroids.is_empty() || total_weight <= 0.0 {
    return vec![f64::NAN; inputs.len()];
  }

  let mut result = vec![f64::NAN; inputs.len()];
  if inputs.is_empty() {
    return result;
  }

  // 分离有效数值与 NaN，记录原始索引
  let mut indexed: Vec<(f64, usize)> = Vec::with_capacity(inputs.len());
  for (i, &v) in inputs.iter().enumerate() {
    if !v.is_nan() {
      indexed.push((v, i));
    }
  }

  if indexed.is_empty() {
    return result;
  }

  indexed.sort_unstable_by(|(a, _), (b, _)| a.total_cmp(b));

  let n_inputs = indexed.len();
  let mut i = 0;

  if centroids.len() == 1 {
    let width = centroids_max - centroids_min;
    while i < n_inputs {
      let val = indexed[i].0;
      let start = i;
      while i < n_inputs && double_equal(indexed[i].0, val) {
        i += 1;
      }
      let weight = if val < centroids_min {
        0.0
      } else if val > centroids_max {
        total_weight
      } else if val - centroids_min <= width {
        total_weight * 0.5
      } else {
        (val - centroids_min) / width * total_weight
      };
      let prob = (weight / total_weight).clamp(0.0, 1.0);
      for item in &indexed[start..i] {
        result[item.1] = prob;
      }
    }
    return result;
  }

  // 1. 小于 centroids_min
  while i < n_inputs && indexed[i].0 < centroids_min {
    let val = indexed[i].0;
    let start = i;
    while i < n_inputs && double_equal(indexed[i].0, val) {
      i += 1;
    }
    for item in &indexed[start..i] {
      result[item.1] = 0.0;
    }
  }

  // 2. 在 [centroids_min, centroids[0].mean) 区间
  let first_mean = centroids[0].mean;
  let first_weight = centroids[0].weight;
  let first_width = first_mean - centroids_min;

  while i < n_inputs && indexed[i].0 < first_mean {
    let val = indexed[i].0;
    let start = i;
    while i < n_inputs && double_equal(indexed[i].0, val) {
      i += 1;
    }
    let weight = if first_width > 0.0 {
      if double_equal(val, centroids_min) {
        HALF_SINGLETON_BOUNDARY_WEIGHT
      } else {
        lerp(
          HALF_SINGLETON_BOUNDARY_WEIGHT,
          first_weight * 0.5,
          (val - centroids_min) / first_width,
        )
      }
    } else {
      0.0
    };
    let prob = (weight / total_weight).clamp(0.0, 1.0);
    for item in &indexed[start..i] {
      result[item.1] = prob;
    }
  }

  // 3. 在中间质心区间 [centroids[0].mean, centroids[last].mean]
  let mut c_idx = 0;
  let mut weight_so_far = 0.0;
  let n_centroids = centroids.len();

  while c_idx < n_centroids - 1 && i < n_inputs {
    let val = indexed[i].0;
    let current_c = centroids[c_idx];
    let next_c = centroids[c_idx + 1];

    if double_equal(val, current_c.mean) {
      let start = i;
      while i < n_inputs && double_equal(indexed[i].0, val) {
        i += 1;
      }
      let mut dw = 0.0;
      let mut same_idx = c_idx;
      while same_idx < n_centroids && double_equal(centroids[same_idx].mean, current_c.mean) {
        dw += centroids[same_idx].weight;
        same_idx += 1;
      }
      let weight = weight_so_far + dw * 0.5;
      let prob = (weight / total_weight).clamp(0.0, 1.0);
      for item in &indexed[start..i] {
        result[item.1] = prob;
      }
      continue;
    }

    if current_c.mean < val && val < next_c.mean {
      let start = i;
      while i < n_inputs && double_equal(indexed[i].0, val) {
        i += 1;
      }
      let mean_diff = next_c.mean - current_c.mean;
      let weight = if mean_diff > 0.0 {
        let mut left_exclude = 0.0;
        let mut right_exclude = 0.0;
        if current_c.weight == SINGLETON_BOUNDARY_WEIGHT {
          if next_c.weight == SINGLETON_BOUNDARY_WEIGHT {
            weight_so_far + SINGLETON_BOUNDARY_WEIGHT
          } else {
            left_exclude = HALF_SINGLETON_BOUNDARY_WEIGHT;
            let dw = (current_c.weight + next_c.weight) * 0.5;
            let dw_no_singleton = dw - left_exclude - right_exclude;
            let base = weight_so_far + current_c.weight * 0.5 + left_exclude;
            lerp(
              base,
              base + dw_no_singleton,
              (val - current_c.mean) / mean_diff,
            )
          }
        } else {
          if next_c.weight == SINGLETON_BOUNDARY_WEIGHT {
            right_exclude = HALF_SINGLETON_BOUNDARY_WEIGHT;
          }
          let dw = (current_c.weight + next_c.weight) * 0.5;
          let dw_no_singleton = dw - left_exclude - right_exclude;
          let base = weight_so_far + current_c.weight * 0.5 + left_exclude;
          lerp(
            base,
            base + dw_no_singleton,
            (val - current_c.mean) / mean_diff,
          )
        }
      } else {
        weight_so_far + current_c.weight * 0.5
      };
      let prob = (weight / total_weight).clamp(0.0, 1.0);
      for item in &indexed[start..i] {
        result[item.1] = prob;
      }
      continue;
    }

    if val >= next_c.mean {
      weight_so_far += current_c.weight;
      c_idx += 1;
    }
  }

  // 4. 在 (centroids[last].mean, centroids_max] 区间
  let last_c = centroids[n_centroids - 1];
  let last_width = centroids_max - last_c.mean;

  while i < n_inputs && indexed[i].0 <= centroids_max {
    let val = indexed[i].0;
    let start = i;
    while i < n_inputs && double_equal(indexed[i].0, val) {
      i += 1;
    }
    let weight = if double_equal(val, last_c.mean) {
      total_weight - last_c.weight * 0.5
    } else if val > last_c.mean {
      if last_width > 0.0 {
        if double_equal(val, centroids_max) {
          total_weight - HALF_SINGLETON_BOUNDARY_WEIGHT
        } else {
          lerp(
            total_weight - last_c.weight * 0.5,
            total_weight - HALF_SINGLETON_BOUNDARY_WEIGHT,
            (val - last_c.mean) / last_width,
          )
        }
      } else {
        total_weight
      }
    } else {
      total_weight
    };
    let prob = (weight / total_weight).clamp(0.0, 1.0);
    for item in &indexed[start..i] {
      result[item.1] = prob;
    }
  }

  // 5. 大于 centroids_max
  while i < n_inputs {
    let val = indexed[i].0;
    let start = i;
    while i < n_inputs && double_equal(indexed[i].0, val) {
      i += 1;
    }
    for item in &indexed[start..i] {
      result[item.1] = 1.0;
    }
  }

  result
}

/// Rank estimation algorithm aligned with Apache Kvrocks TDigestRank.
/// 排名估算算法（对标 Apache Kvrocks TDigestRank）
pub fn tdigest_rank_calc(
  centroids: &[Centroid],
  min: f64,
  max: f64,
  total_weight: f64,
  inputs: &[f64],
  reverse: bool,
) -> Vec<i64> {
  if centroids.is_empty() || total_weight <= 0.0 {
    return vec![-2; inputs.len()];
  }

  let mut result = vec![-2; inputs.len()];
  let mut indexed: Vec<(f64, usize)> = Vec::with_capacity(inputs.len());
  for (i, &v) in inputs.iter().enumerate() {
    if !v.is_nan() {
      indexed.push((v, i));
    }
  }

  if indexed.is_empty() {
    return result;
  }

  if reverse {
    indexed.sort_unstable_by(|(a, _), (b, _)| b.total_cmp(a));

    let mut it_idx = 0;
    let n_inputs = indexed.len();

    while it_idx < n_inputs && indexed[it_idx].0 > max {
      let val = indexed[it_idx].0;
      let start = it_idx;
      while it_idx < n_inputs && double_equal(indexed[it_idx].0, val) {
        it_idx += 1;
      }
      for item in &indexed[start..it_idx] {
        result[item.1] = -1;
      }
    }

    let mut cumulative_weight = 0.0;
    let mut c_idx = centroids.len();

    while c_idx > 0 && it_idx < n_inputs {
      c_idx -= 1;
      let centroid = centroids[c_idx];
      let input_val = indexed[it_idx].0;

      if double_equal(centroid.mean, input_val) {
        let current_mean = centroid.mean;
        let mut current_mean_cum_w = cumulative_weight + centroid.weight * 0.5;
        cumulative_weight += centroid.weight;

        while c_idx > 0 && double_equal(centroids[c_idx - 1].mean, current_mean) {
          c_idx -= 1;
          current_mean_cum_w += centroids[c_idx].weight * 0.5;
          cumulative_weight += centroids[c_idx].weight;
        }

        let start = it_idx;
        while it_idx < n_inputs && double_equal(indexed[it_idx].0, input_val) {
          it_idx += 1;
        }
        for item in &indexed[start..it_idx] {
          result[item.1] = current_mean_cum_w as i64;
        }
      } else if double_compare(centroid.mean, input_val, REL_EPS, ABS_EPS).is_gt() {
        cumulative_weight += centroid.weight;
      } else {
        let start = it_idx;
        while it_idx < n_inputs && double_equal(indexed[it_idx].0, input_val) {
          it_idx += 1;
        }
        for item in &indexed[start..it_idx] {
          result[item.1] = cumulative_weight as i64;
        }
        c_idx += 1; // 重新处理当前质心
      }
    }

    while it_idx < n_inputs {
      let val = indexed[it_idx].0;
      let start = it_idx;
      while it_idx < n_inputs && double_equal(indexed[it_idx].0, val) {
        it_idx += 1;
      }
      for item in &indexed[start..it_idx] {
        result[item.1] = total_weight as i64;
      }
    }
  } else {
    indexed.sort_unstable_by(|(a, _), (b, _)| a.total_cmp(b));

    let mut it_idx = 0;
    let n_inputs = indexed.len();

    while it_idx < n_inputs && indexed[it_idx].0 < min {
      let val = indexed[it_idx].0;
      let start = it_idx;
      while it_idx < n_inputs && double_equal(indexed[it_idx].0, val) {
        it_idx += 1;
      }
      for item in &indexed[start..it_idx] {
        result[item.1] = -1;
      }
    }

    let mut cumulative_weight = 0.0;
    let mut c_idx = 0;
    let n_centroids = centroids.len();

    while c_idx < n_centroids && it_idx < n_inputs {
      let centroid = centroids[c_idx];
      let input_val = indexed[it_idx].0;

      if double_equal(centroid.mean, input_val) {
        let current_mean = centroid.mean;
        let mut current_mean_cum_w = cumulative_weight + centroid.weight * 0.5;
        cumulative_weight += centroid.weight;

        while c_idx + 1 < n_centroids && double_equal(centroids[c_idx + 1].mean, current_mean) {
          c_idx += 1;
          current_mean_cum_w += centroids[c_idx].weight * 0.5;
          cumulative_weight += centroids[c_idx].weight;
        }

        let start = it_idx;
        while it_idx < n_inputs && double_equal(indexed[it_idx].0, input_val) {
          it_idx += 1;
        }
        for item in &indexed[start..it_idx] {
          result[item.1] = current_mean_cum_w as i64;
        }
        c_idx += 1;
      } else if double_compare(centroid.mean, input_val, REL_EPS, ABS_EPS).is_lt() {
        cumulative_weight += centroid.weight;
        c_idx += 1;
      } else {
        let start = it_idx;
        while it_idx < n_inputs && double_equal(indexed[it_idx].0, input_val) {
          it_idx += 1;
        }
        for item in &indexed[start..it_idx] {
          result[item.1] = cumulative_weight as i64;
        }
      }
    }

    while it_idx < n_inputs {
      let val = indexed[it_idx].0;
      let start = it_idx;
      while it_idx < n_inputs && double_equal(indexed[it_idx].0, val) {
        it_idx += 1;
      }
      for item in &indexed[start..it_idx] {
        result[item.1] = total_weight as i64;
      }
    }
  }

  result
}

/// Query value by rank aligned with Apache Kvrocks TDigestByRank.
/// 按排名查询值（对标 Apache Kvrocks TDigestByRank）
pub fn tdigest_by_rank_calc(
  centroids: &[Centroid],
  total_weight: f64,
  inputs: &[i64],
  reverse: bool,
) -> Vec<f64> {
  if centroids.is_empty() || total_weight <= 0.0 {
    return vec![f64::NAN; inputs.len()];
  }

  let mut result = vec![f64::NAN; inputs.len()];
  let mut indexed: Vec<(i64, usize)> = inputs.iter().enumerate().map(|(i, &r)| (r, i)).collect();

  indexed.sort_unstable_by_key(|&(r, _)| r);

  let mut it_idx = 0;
  let n_inputs = indexed.len();
  let mut cumulative_weight = 0.0;
  let inf_val = if reverse {
    -f64::INFINITY
  } else {
    f64::INFINITY
  };

  if reverse {
    for c in centroids.iter().rev() {
      cumulative_weight += c.weight;
      let target_w = cumulative_weight as i64;
      while it_idx < n_inputs && indexed[it_idx].0 < target_w {
        result[indexed[it_idx].1] = c.mean;
        it_idx += 1;
      }
    }
  } else {
    for c in centroids {
      cumulative_weight += c.weight;
      let target_w = cumulative_weight as i64;
      while it_idx < n_inputs && indexed[it_idx].0 < target_w {
        result[indexed[it_idx].1] = c.mean;
        it_idx += 1;
      }
    }
  }

  let total_w_int = total_weight as i64;
  while it_idx < n_inputs {
    if indexed[it_idx].0 >= total_w_int {
      result[indexed[it_idx].1] = inf_val;
    }
    it_idx += 1;
  }

  result
}

/// Trimmed mean calculation aligned with Apache Kvrocks TDigestTrimmedMean.
/// 截断均值计算（对标 Apache Kvrocks TDigestTrimmedMean）
pub fn tdigest_trimmed_mean_calc(
  centroids: &[Centroid],
  total_weight: f64,
  low_cut: f64,
  high_cut: f64,
) -> f64 {
  if centroids.is_empty()
    || low_cut.is_nan()
    || high_cut.is_nan()
    || !(0.0..=1.0).contains(&low_cut)
    || !(0.0..=1.0).contains(&high_cut)
    || low_cut >= high_cut
    || total_weight <= 0.0
  {
    return f64::NAN;
  }

  let leftmost_weight = (total_weight * low_cut).floor();
  let rightmost_weight = (total_weight * high_cut).ceil();

  let mut count_done = 0.0;
  let mut trimmed_sum = 0.0;
  let mut trimmed_count = 0.0;

  for c in centroids {
    let n_weight = c.weight;
    let mut count_add = n_weight;

    count_add -= (leftmost_weight - count_done).max(0.0).min(count_add);
    count_add = (rightmost_weight - count_done).max(0.0).min(count_add);

    count_done += n_weight;

    trimmed_sum += c.mean * count_add;
    trimmed_count += count_add;

    if count_done >= rightmost_weight {
      break;
    }
  }

  if trimmed_count == 0.0 {
    f64::NAN
  } else {
    trimmed_sum / trimmed_count
  }
}

/// T-Digest core algorithm structure.
/// T-Digest 核心算法结构
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct TDigestState {
  pub compression: f64,
  pub capacity: usize,
  pub centroids: Vec<Centroid>,
  pub unmerged_buffer: Vec<f64>,
  pub total_weight: f64,
  pub merged_weight: f64,
  pub total_observations: u64,
  pub merge_times: u64,
  pub min: f64,
  pub max: f64,
}

impl TDigestState {
  #[inline]
  pub fn new(compression: f64) -> Self {
    let comp = if compression <= 0.0 {
      DEFAULT_COMPRESSION as f64
    } else {
      compression
    };
    let cap = calculate_capacity(comp as u32);
    Self {
      compression: comp,
      capacity: cap,
      centroids: Vec::with_capacity(cap),
      unmerged_buffer: Vec::with_capacity(cap),
      total_weight: 0.0,
      merged_weight: 0.0,
      total_observations: 0,
      merge_times: 0,
      min: f64::MAX,
      max: -f64::MAX,
    }
  }

  #[inline]
  pub fn is_empty(&self) -> bool {
    self.total_observations == 0 && self.total_weight <= 0.0
  }

  #[inline]
  pub fn reset(&mut self) {
    self.centroids.clear();
    self.unmerged_buffer.clear();
    self.total_weight = 0.0;
    self.merged_weight = 0.0;
    self.total_observations = 0;
    self.merge_times = 0;
    self.min = f64::MAX;
    self.max = -f64::MAX;
  }

  #[inline]
  pub fn add(&mut self, val: f64, weight: f64) {
    if val.is_nan() || weight <= 0.0 {
      return;
    }
    if weight == 1.0 {
      self.unmerged_buffer.push(val);
    } else {
      self.ensure_merged();
      self.centroids.push(Centroid::new(val, weight));
      self.merged_weight += weight;
      self.compress();
    }
    self.total_weight += weight;
    self.total_observations += 1;
    if val < self.min {
      self.min = val;
    }
    if val > self.max {
      self.max = val;
    }
    if self.unmerged_buffer.len() >= self.capacity {
      self.compress();
    }
  }

  #[inline]
  pub fn add_batch(&mut self, values: &[f64]) {
    self
      .unmerged_buffer
      .reserve(values.len().min(self.capacity));
    for &val in values {
      if !val.is_nan() {
        self.unmerged_buffer.push(val);
        self.total_weight += 1.0;
        self.total_observations += 1;
        if val < self.min {
          self.min = val;
        }
        if val > self.max {
          self.max = val;
        }
        if self.unmerged_buffer.len() >= self.capacity {
          self.compress();
        }
      }
    }
  }

  #[inline]
  pub fn ensure_merged(&mut self) {
    if !self.unmerged_buffer.is_empty() {
      self.compress();
    }
  }

  pub fn compress(&mut self) {
    if self.unmerged_buffer.is_empty() {
      return;
    }
    let delta = self.compression as u32;
    let effective_delta = delta.max(10);
    let mut merger = TDigestMerger::new(effective_delta);
    merger.reset(self.total_weight);

    // 过滤 NaN 并原地排序
    self.unmerged_buffer.retain(|v| !v.is_nan());
    self.unmerged_buffer.sort_unstable_by(|a, b| a.total_cmp(b));

    let mut output = Vec::with_capacity(effective_delta as usize);

    let mut bi = 0;
    let mut ci = 0;
    let b_len = self.unmerged_buffer.len();
    let c_len = self.centroids.len();

    while bi < b_len && ci < c_len {
      if double_compare(
        self.centroids[ci].mean,
        self.unmerged_buffer[bi],
        REL_EPS,
        ABS_EPS,
      )
      .is_lt()
      {
        merger.add(&mut output, self.centroids[ci]);
        ci += 1;
      } else {
        merger.add(&mut output, Centroid::new(self.unmerged_buffer[bi], 1.0));
        bi += 1;
      }
    }
    while ci < c_len {
      merger.add(&mut output, self.centroids[ci]);
      ci += 1;
    }
    while bi < b_len {
      merger.add(&mut output, Centroid::new(self.unmerged_buffer[bi], 1.0));
      bi += 1;
    }

    self.unmerged_buffer.clear();
    self.centroids = output;
    self.merged_weight = self.total_weight;
    self.merge_times += 1;
  }

  pub fn merge_from(&mut self, other: &mut TDigestState) {
    other.ensure_merged();
    if other.is_empty() {
      return;
    }
    self.ensure_merged();

    let delta = (self.compression as u32).max(other.compression as u32);
    let list = [
      CentroidsWithDelta {
        centroids: take(&mut self.centroids),
        delta: self.compression as u32,
        min: self.min,
        max: self.max,
        total_weight: self.merged_weight,
      },
      CentroidsWithDelta {
        centroids: take(&mut other.centroids),
        delta: other.compression as u32,
        min: other.min,
        max: other.max,
        total_weight: other.merged_weight,
      },
    ];

    let merged = tdigest_merge_centroids_list(&list, delta);
    self.compression = delta as f64;
    self.capacity = calculate_capacity(delta);
    self.centroids = merged.centroids;
    self.merged_weight = merged.total_weight;
    self.total_weight = merged.total_weight;
    self.total_observations += other.total_observations;
    self.merge_times += 1;
    self.min = merged.min;
    self.max = merged.max;
  }

  pub fn merge_with_options(
    &mut self,
    sources: &mut [TDigestState],
    override_dest: bool,
    compression: Option<u32>,
    dest_existed: bool,
  ) {
    self.ensure_merged();
    for s in sources.iter_mut() {
      s.ensure_merged();
    }

    let mut max_source_comp = DEFAULT_COMPRESSION;
    for s in sources.iter() {
      max_source_comp = max_source_comp.max(s.compression as u32);
    }

    let final_comp = if let Some(c) = compression {
      c
    } else if override_dest || !dest_existed {
      max_source_comp
    } else {
      self.compression as u32
    };

    let mut list = Vec::with_capacity(sources.len() + 1);
    let mut total_obs = 0u64;

    if !override_dest && dest_existed && !self.is_empty() {
      total_obs += self.total_observations;
      list.push(CentroidsWithDelta {
        centroids: take(&mut self.centroids),
        delta: self.compression as u32,
        min: self.min,
        max: self.max,
        total_weight: self.merged_weight,
      });
    }

    for s in sources.iter_mut() {
      if !s.is_empty() {
        total_obs += s.total_observations;
        list.push(CentroidsWithDelta {
          centroids: take(&mut s.centroids),
          delta: s.compression as u32,
          min: s.min,
          max: s.max,
          total_weight: s.merged_weight,
        });
      }
    }

    if list.is_empty() {
      self.reset();
      self.compression = final_comp as f64;
      self.capacity = calculate_capacity(final_comp);
      return;
    }

    let merged = tdigest_merge_centroids_list(&list, final_comp);
    self.compression = final_comp as f64;
    self.capacity = calculate_capacity(final_comp);
    self.centroids = merged.centroids;
    self.merged_weight = merged.total_weight;
    self.total_weight = merged.total_weight;
    self.total_observations = total_obs;
    self.merge_times += 1;
    self.min = merged.min;
    self.max = merged.max;
  }

  #[inline]
  pub fn min(&self) -> f64 {
    if self.is_empty() { f64::NAN } else { self.min }
  }

  #[inline]
  pub fn max(&self) -> f64 {
    if self.is_empty() { f64::NAN } else { self.max }
  }

  #[inline]
  pub fn quantile(&mut self, q: f64) -> f64 {
    self.ensure_merged();
    tdigest_quantile_calc(&self.centroids, self.min, self.max, self.total_weight, q)
  }

  #[inline]
  pub fn cdf(&mut self, val: f64) -> f64 {
    self.ensure_merged();
    let res = tdigest_cdf_calc(
      &self.centroids,
      self.min,
      self.max,
      self.total_weight,
      &[val],
    );
    res.first().copied().unwrap_or(f64::NAN)
  }

  #[inline]
  pub fn rank(&mut self, val: f64) -> i64 {
    self.ensure_merged();
    let res = tdigest_rank_calc(
      &self.centroids,
      self.min,
      self.max,
      self.total_weight,
      &[val],
      false,
    );
    res.first().copied().unwrap_or(-2)
  }

  #[inline]
  pub fn revrank(&mut self, val: f64) -> i64 {
    self.ensure_merged();
    let res = tdigest_rank_calc(
      &self.centroids,
      self.min,
      self.max,
      self.total_weight,
      &[val],
      true,
    );
    res.first().copied().unwrap_or(-2)
  }

  #[inline]
  pub fn byrank(&mut self, r: u64) -> f64 {
    self.ensure_merged();
    let res = tdigest_by_rank_calc(&self.centroids, self.total_weight, &[r as i64], false);
    res.first().copied().unwrap_or(f64::NAN)
  }

  #[inline]
  pub fn byrevrank(&mut self, r: u64) -> f64 {
    self.ensure_merged();
    let res = tdigest_by_rank_calc(&self.centroids, self.total_weight, &[r as i64], true);
    res.first().copied().unwrap_or(f64::NAN)
  }

  #[inline]
  pub fn trimmed_mean(&mut self, low_cut: f64, high_cut: f64) -> f64 {
    self.ensure_merged();
    tdigest_trimmed_mean_calc(&self.centroids, self.total_weight, low_cut, high_cut)
  }

  #[inline]
  pub fn info(&self) -> TDigestInfo {
    let unmerged_w = (self.total_weight - self.merged_weight).max(0.0);
    TDigestInfo {
      compression: self.compression as u32,
      capacity: self.capacity,
      merged_nodes: self.centroids.len(),
      unmerged_nodes: self.unmerged_buffer.len(),
      merged_weight: self.merged_weight,
      unmerged_weight: unmerged_w,
      total_weight: self.total_weight,
      observations: self.total_observations,
      total_compressions: self.merge_times,
      minimum: if self.is_empty() {
        None
      } else {
        Some(self.min)
      },
      maximum: if self.is_empty() {
        None
      } else {
        Some(self.max)
      },
    }
  }
}

/// TDigest merger utility.
/// TDigest 合并器工具
pub struct TDigestMergerTool;

impl TDigestMergerTool {
  pub fn merge(dest: &mut TDigestState, sources: &mut [TDigestState]) {
    for src in sources {
      dest.merge_from(src);
    }
  }
}

#[inline]
pub fn get_tdigest<E: Engine>(db: &Db<E>, key_bytes: &[u8]) -> Result<TDigestState>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let now_ms = current_now_ms();
  if get_meta_checked::<TDigestMeta, _>(db, key_bytes, &meta_k, now_ms)?.is_none() {
    return Err(Error::invalid_data("ERR key does not exist"));
  }
  let data_k = key::prefix(&kc, key_bytes);
  match db.data().get(&data_k)? {
    Some(bytes) => bitcode::decode::<TDigestState>(&bytes)
      .map_err(|e| Error::invalid_data(format!("ERR tdigest deserialize: {e}"))),
    None => Err(Error::invalid_data("ERR key does not exist")),
  }
}

#[inline]
pub fn save_tdigest<E: Engine>(db: &Db<E>, key_bytes: &[u8], td: &TDigestState) -> Result<()>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let data_k = key::prefix(&kc, key_bytes);
  let serialized = bitcode::encode(td);

  let meta = TDigestMeta::new(td.compression as u32, 0, generate_version());
  let mut batch = db.batch();
  batch.insert_meta(&meta_k, &meta.encode());
  batch.insert_data(&data_k, &serialized);
  batch.commit()?;
  Ok(())
}
