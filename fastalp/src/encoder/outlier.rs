use crate::{
  bitpack::packed_byte_size,
  encoder::exception::{Exception, exceptions_byte_size},
  float::AlpFloat,
};

/// Pruning threshold: only attempt when bit-width > 4 and exceptions < 16
/// 尝试剪枝门限：仅在位宽 > 4 且已有异常数 < 16 时尝试
const MIN_PRUNE_BIT_WIDTH: u8 = 4;
const MAX_PRUNE_EXCEPTIONS: usize = 16;
const CANDIDATE_WIDTHS: [u8; 9] = [48, 32, 28, 24, 20, 16, 12, 8, 0];

/// FOR mode only: outlier pruning to exceptions with descending bit-width search:
/// Exploits monotonicity - if a larger width violates exception budget, smaller widths will too.
/// FOR 模式专用：离群值异常剪枝优化 (Outlier Pruning to Exceptions)
/// 降序探索候选位宽：利用单调性数学性质，若较大位宽无法满足异常数限制，则更小位宽必然包含更多离群点，可直接短路终止搜索。
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

  // 1. 数学单调性极速短路：取小于当前位宽的最大候选位宽 c_max。
  // 若其离群点已超过容限，由单调性知更小候选位宽必然包含更多离群点，直接短路退出（通常在 20~40 元素内终止，耗时 < 10ns）
  let c_max = match CANDIDATE_WIDTHS
    .iter()
    .copied()
    .find(|&w| w < for_bit_width)
  {
    Some(w) => w,
    None => return for_bit_width,
  };
  let max_allowed_c = if c_max == 0 {
    0u64
  } else {
    (1u64 << c_max) - 1
  };
  let budget = MAX_PRUNE_EXCEPTIONS.saturating_sub(exceptions.len());
  let mut excess = 0usize;

  for &val in encoded_ints.iter() {
    let diff = F::int_diff_to_u64(val, base);
    if diff > max_allowed_c {
      excess += 1;
      if excess > budget {
        return for_bit_width;
      }
    }
  }

  let count = slice.len();
  let current_packed_len = packed_byte_size(count, for_bit_width);
  let current_cost = current_packed_len + exceptions_byte_size::<F>(exceptions.len(), is_large);

  // 仅当通过单调性筛选后（证明离群点确在容限内），才执行直方图与全局最优候选位宽评估
  let mut hist = [0u16; 65];
  for &val in encoded_ints.iter() {
    let diff = F::int_diff_to_u64(val, base);
    let bw = F::bits_needed(diff) as usize;
    hist[bw] += 1;
  }

  let mut exc_count = [0usize; 65];
  let mut running = 0usize;
  for w in (0..=64).rev() {
    exc_count[w] = running;
    running += hist[w] as usize;
  }

  let mut best_target_bw = for_bit_width;
  let mut min_cost = current_cost;

  for &target_bw in &CANDIDATE_WIDTHS {
    if target_bw >= for_bit_width {
      continue;
    }
    let extra_exceptions = exc_count[target_bw as usize];
    if extra_exceptions > MAX_PRUNE_EXCEPTIONS {
      continue;
    }

    let new_total_exc = exceptions.len() + extra_exceptions;
    let new_cost =
      packed_byte_size(count, target_bw) + exceptions_byte_size::<F>(new_total_exc, is_large);
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
      // SAFETY: exc.pos is strictly less than encoded_ints.len()
      unsafe {
        *encoded_ints.get_unchecked_mut(exc.pos) = base;
      }
    }
  }

  best_target_bw
}
