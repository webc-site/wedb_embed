//! High-performance differential (Delta) encoding utilities for consecutive time series float streams.
//! 针对连续时序浮点数流的高性能一阶差分（Delta）编码与前缀和还原模块

use crate::float::AlpFloat;

/// Calculates the min delta and required bit width for adjacent first-order differences.
/// 计算相邻一阶差分的极小值与所需比特位宽
#[inline(always)]
pub fn delta_range<F: AlpFloat>(first: F::Int, rest: &[F::Int]) -> (F::Int, u8) {
  let mut min_delta = F::MAX_INT;
  let mut max_delta = F::MIN_INT;
  let mut prev = first;

  for &curr in rest {
    let delta = F::int_sub(curr, prev);
    min_delta = min_delta.min(delta);
    max_delta = max_delta.max(delta);
    prev = curr;
  }

  let delta_bit_width = F::bits_needed(F::calc_range(min_delta, max_delta));
  (min_delta, delta_bit_width)
}

/// Evaluates whether delta encoding yields a smaller bit width than standard Frame-of-Reference (FOR).
/// 评估一阶差分编码相比直接 FOR 基准值对齐是否具有更窄的比特位宽优势
#[inline(always)]
pub fn eval_delta_benefit<F: AlpFloat>(
  first: F::Int,
  rest: &[F::Int],
  for_bit_width: u8,
) -> Option<(F::Int, u8)> {
  if rest.is_empty() {
    return None;
  }

  let (min_delta, delta_bit_width) = delta_range::<F>(first, rest);
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
