//! High-performance differential (Delta) encoding utilities for consecutive time series float streams.
//! 针对连续时序浮点数流的高性能一阶差分（Delta）编码与前缀和还原模块

use crate::float::AlpFloat;

#[inline(always)]
fn scan_deltas<F: AlpFloat>(
  slice: &[F::Int],
  prev: &mut F::Int,
  min_delta: &mut F::Int,
  max_delta: &mut F::Int,
) {
  let (chunks, rem) = slice.as_chunks::<4>();
  for chunk in chunks {
    let d0 = F::int_sub(chunk[0], *prev);
    let d1 = F::int_sub(chunk[1], chunk[0]);
    let d2 = F::int_sub(chunk[2], chunk[1]);
    let d3 = F::int_sub(chunk[3], chunk[2]);
    *prev = chunk[3];

    let l_min = d0.min(d1).min(d2.min(d3));
    let l_max = d0.max(d1).max(d2.max(d3));
    *min_delta = (*min_delta).min(l_min);
    *max_delta = (*max_delta).max(l_max);
  }

  for &curr in rem {
    let delta = F::int_sub(curr, *prev);
    *min_delta = (*min_delta).min(delta);
    *max_delta = (*max_delta).max(delta);
    *prev = curr;
  }
}

/// Calculates the min delta and required bit width for adjacent first-order differences.
/// 计算相邻一阶差分的极小值与所需比特位宽（4路展开流水线计算）
#[inline(always)]
pub fn delta_range<F: AlpFloat>(first: F::Int, rest: &[F::Int]) -> (F::Int, u8) {
  let mut min_delta = F::MAX_INT;
  let mut max_delta = F::MIN_INT;
  let mut prev = first;
  scan_deltas::<F>(rest, &mut prev, &mut min_delta, &mut max_delta);
  let delta_bit_width = F::bits_needed(F::calc_range(min_delta, max_delta));
  (min_delta, delta_bit_width)
}

/// Evaluates whether delta encoding yields a smaller bit width than standard Frame-of-Reference (FOR).
/// 评估一阶差分编码相比直接 FOR 基准值对齐是否具有更窄的比特位宽优势（前置 16 采样快筛，无缝续扫零冗余内存访问）
#[inline(always)]
pub fn eval_delta_benefit<F: AlpFloat>(
  first: F::Int,
  rest: &[F::Int],
  for_bit_width: u8,
) -> Option<(F::Int, u8)> {
  if rest.is_empty() {
    return None;
  }

  // 数学性质快筛：子区间的极值跨度恒 <= 全量区间极值跨度。
  // 若前 16 个元素的差分位宽已 >= for_bit_width，则全集差分位宽绝不可能小于 for_bit_width，立即短路早停。
  let pre_n = rest.len().min(16);
  let mut min_delta = F::MAX_INT;
  let mut max_delta = F::MIN_INT;
  let mut prev = first;
  scan_deltas::<F>(&rest[..pre_n], &mut prev, &mut min_delta, &mut max_delta);

  let pre_bw = F::bits_needed(F::calc_range(min_delta, max_delta));
  if pre_bw >= for_bit_width {
    return None;
  }

  if pre_n < rest.len() {
    scan_deltas::<F>(&rest[pre_n..], &mut prev, &mut min_delta, &mut max_delta);
  }

  let delta_bit_width = F::bits_needed(F::calc_range(min_delta, max_delta));
  if delta_bit_width < for_bit_width {
    Some((min_delta, delta_bit_width))
  } else {
    None
  }
}

/// Computes adjacent first-order deltas in place backwards: data[i] = data[i] - data[i-1].
/// 逆向就地计算相邻一阶差分，彻底消除多余堆分配
#[inline(always)]
pub fn in_place_deltas<F: AlpFloat>(data: &mut [F::Int]) {
  for i in (1..data.len()).rev() {
    data[i] = F::int_sub(data[i], data[i - 1]);
  }
}

/// Rapidly reconstructs float values from a linear arithmetic progression (bit_width == 0).
/// 当差分位宽为 0（等差数列/恒定斜率）时极速无分支还原浮点数组
#[inline(always)]
pub fn reconstruct_ramp_into_floats<F: AlpFloat>(
  first: F::Int,
  constant_delta: F::Int,
  count: usize,
  fac_int: i64,
  frac_flt: F,
  dst: &mut [F],
) {
  let mut curr = first;
  for slot in dst.iter_mut().take(count) {
    *slot = F::decode_from_int(curr, fac_int, frac_flt);
    curr = F::int_add(curr, constant_delta);
  }
}
