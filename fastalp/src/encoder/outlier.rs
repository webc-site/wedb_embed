use crate::{
  bitpack::packed_byte_size,
  encoder::exception::{Exception, exceptions_byte_size},
  float::AlpFloat,
};

/// 尝试剪枝门限：仅在位宽 > 4 且已有异常数 < 16 时尝试
const MIN_PRUNE_BIT_WIDTH: u8 = 4;
const MAX_PRUNE_EXCEPTIONS: usize = 16;
const PRE_CHECK_LEN: usize = 16;
const PRE_CHECK_MAX_OUTLIERS: usize = 4;
const CANDIDATE_WIDTHS: [u8; 9] = [48, 32, 28, 24, 20, 16, 12, 8, 0];

/// FOR 模式专用：离群值异常剪枝优化 (Outlier Pruning to Exceptions)
/// 降序探索候选位宽：利用单调性数学性质，若较大位宽无法满足异常数限制，
/// 则更小位宽必然包含更多离群点，可直接短路终止搜索。
pub(crate) fn try_prune_outliers<F: AlpFloat>(
  slice: &[F],
  encoded_ints: &mut [F::Int],
  base: F::Int,
  for_bit_width: u8,
  exceptions: &mut Vec<Exception<F::RawBits>>,
  is_large: bool,
) -> u8 {
  if for_bit_width <= MIN_PRUNE_BIT_WIDTH || exceptions.len() >= MAX_PRUNE_EXCEPTIONS {
    return for_bit_width;
  }

  let current_packed_len = packed_byte_size(slice.len(), for_bit_width);
  let current_cost = current_packed_len + exceptions_byte_size::<F>(exceptions.len(), is_large);

  let mut best_target_bw = for_bit_width;
  let mut min_cost = current_cost;

  for &target_bw in &CANDIDATE_WIDTHS {
    if target_bw >= for_bit_width {
      continue;
    }
    let max_allowed = if target_bw == 0 {
      0u64
    } else {
      (1u64 << target_bw) - 1
    };

    // 前置 16 采样快筛：若在前 16 个元素中已出现超过 4 个离群点，
    // 由单调性可知任何更小位宽的离群点数均 >= 当前位宽，直接短路跳出候选循环
    let pre_check_n = encoded_ints.len().min(PRE_CHECK_LEN);
    let mut pre_outliers = 0;
    for &val in &encoded_ints[..pre_check_n] {
      if F::int_diff_to_u64(val, base) > max_allowed {
        pre_outliers += 1;
        if pre_outliers > PRE_CHECK_MAX_OUTLIERS {
          break;
        }
      }
    }
    if pre_outliers > PRE_CHECK_MAX_OUTLIERS {
      break;
    }

    let mut extra_exceptions = pre_outliers;
    for &val in &encoded_ints[pre_check_n..] {
      let diff = F::int_diff_to_u64(val, base);
      if diff > max_allowed {
        extra_exceptions += 1;
        if extra_exceptions > MAX_PRUNE_EXCEPTIONS {
          break;
        }
      }
    }

    if extra_exceptions > MAX_PRUNE_EXCEPTIONS {
      break;
    }

    let new_total_exc = exceptions.len() + extra_exceptions;
    let new_cost = packed_byte_size(slice.len(), target_bw)
      + exceptions_byte_size::<F>(new_total_exc, is_large);
    if new_cost < min_cost {
      min_cost = new_cost;
      best_target_bw = target_bw;
    }
  }

  if best_target_bw < for_bit_width {
    let max_allowed = if best_target_bw == 0 {
      0u64
    } else {
      (1u64 << best_target_bw) - 1
    };
    for (pos, (&v, &val)) in slice.iter().zip(encoded_ints.iter()).enumerate() {
      let diff = F::int_diff_to_u64(val, base);
      if diff > max_allowed {
        exceptions.push(Exception {
          pos,
          bits: v.to_raw_bits(),
        });
      }
    }
    exceptions.sort_unstable_by_key(|e| e.pos);
    exceptions.dedup_by_key(|e| e.pos);

    // 为离群点回填基准值，确保打包时不溢出目标位宽
    for exc in &*exceptions {
      // SAFETY: exc.pos 严格小于 encoded_ints.len()
      unsafe {
        *encoded_ints.get_unchecked_mut(exc.pos) = base;
      }
    }
  }

  best_target_bw
}
