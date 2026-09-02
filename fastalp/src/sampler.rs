use core::mem::size_of;

use crate::{
  constants::{EARLY_EXIT_BIT_WIDTH, SAMPLES_COUNT},
  float::AlpFloat,
};

/// Sampling and optimal factor selection result.
/// 采样与最优系数选择结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BestParams {
  pub exp: u8,
  pub fac: u8,
  pub use_div: bool,
}

/// Checks whether float is special non-encodable value (NaN, Inf, -0.0, out of range).
/// 检查浮点数是否为不可编码的特殊值（NaN, Inf, -0.0, 超出范围）
#[inline(always)]
pub fn is_impossible<F: AlpFloat>(n: F) -> bool {
  n.is_impossible()
}

/// High performance single float encoding probe with pre-extracted power factors.
/// 高性能单值浮点数编码探测（已预提取幂表因子）
#[inline(always)]
pub fn try_encode_fast<F: AlpFloat>(
  val: F,
  exp_factor: F,
  fac_int: i64,
  frac_exp: F,
) -> Option<F::Int> {
  val.try_encode_fast(exp_factor, fac_int, frac_exp)
}

/// Attempts to encode float as integer and verifies 100% lossless reconstruction.
/// 尝试将单个浮点数编码为整型，并验证反解是否 100% 精确无损
#[inline(always)]
pub fn try_encode_value<F: AlpFloat>(val: F, exp: u8, fac: u8) -> Option<F::Int> {
  if exp > F::MAX_EXPONENT || fac > exp || fac > F::MAX_FAC {
    return None;
  }
  let exp_factor = F::exp_factor(exp, fac);
  let fac_int = F::fac_int(fac);
  let frac_exp = F::frac_exp(exp);
  val.try_encode_fast(exp_factor, fac_int, frac_exp)
}

/// Fast single-pass search for identical/constant values.
/// 全等/常数浮点数序列的极速指数与基准值探测（零堆分配、O(1) 搜索）
#[inline]
pub fn find_identical_base<F: AlpFloat>(val: F) -> Option<(u8, F::Int)> {
  for exp in 0..=F::MAX_EXPONENT {
    let frac_exp = F::frac_exp(exp);
    let exp_factor = F::exp_factor(exp, 0);
    let fac_int = F::fac_int(0);
    if let Some(base) = F::try_encode_fast(val, exp_factor, fac_int, frac_exp) {
      return Some((exp, base));
    }
  }
  None
}

/// Discovers best exponent, factor, and division mode by evaluating cost over sampled subset.
/// 通过对采样样本进行代价评估，找出最优的指数 (exp)、因子 (fac) 与除法重构模式 (use_div)
pub fn find_best_params<F: AlpFloat>(samples: &[F]) -> BestParams {
  if samples.is_empty() {
    return BestParams {
      exp: 0,
      fac: 0,
      use_div: false,
    };
  }

  let mut valid_samples: [F; SAMPLES_COUNT] = [F::ZERO; SAMPLES_COUNT];
  let mut sample_len = 0;
  for &val in samples {
    if !val.is_impossible() {
      valid_samples[sample_len] = val;
      sample_len += 1;
      if sample_len == SAMPLES_COUNT {
        break;
      }
    }
  }

  let active_samples = &valid_samples[..sample_len];
  if sample_len == 0 {
    return BestParams {
      exp: 0,
      fac: 0,
      use_div: false,
    };
  }

  let mut best_cost = size_of::<F>() * 8 * sample_len;
  let mut best_params = BestParams {
    exp: 0,
    fac: 0,
    use_div: false,
  };

  for exp in 0..=F::MAX_EXPONENT {
    let max_fac = exp.min(F::MAX_FAC);
    let frac_exp = F::frac_exp(exp);

    for fac in 0..=max_fac {
      let exp_factor = F::exp_factor(exp, fac);
      let fac_int = F::fac_int(fac);

      let mut exceptions = 0usize;
      let mut min_val = F::MAX_INT;
      let mut max_val = F::MIN_INT;

      for &val in active_samples {
        if let Some(enc) = F::try_encode_fast(val, exp_factor, fac_int, frac_exp) {
          min_val = min_val.min(enc);
          max_val = max_val.max(enc);
        } else {
          exceptions += 1;
          if exceptions * F::EXCEPTION_PENALTY >= best_cost {
            break;
          }
        }
      }

      let zero_exceptions_fac0 = fac == 0 && exceptions == 0;

      if exceptions != sample_len && exceptions * F::EXCEPTION_PENALTY < best_cost {
        let max_offset = if min_val <= max_val {
          F::calc_range(min_val, max_val)
        } else {
          0
        };
        let bit_width = F::bits_needed(max_offset) as usize;
        let fac_penalty = if fac > 0 { sample_len * 2 } else { 0 };
        let total_cost = bit_width * sample_len + exceptions * F::EXCEPTION_PENALTY + fac_penalty;

        if total_cost < best_cost {
          best_cost = total_cost;
          best_params = BestParams {
            exp,
            fac,
            use_div: false,
          };
          if total_cost == 0 {
            return best_params;
          }
          if exceptions == 0 && bit_width <= EARLY_EXIT_BIT_WIDTH {
            return best_params;
          }
        }
      }

      // 当 fac == 0 且标准乘法存在异常时，才评估十进制除法重构模式 (Decimal Division Mode)
      // 若乘法无异常，乘法解码更快，无需且不应尝试除法
      if fac == 0 && exp > 0 && exceptions > 0 {
        let mut div_exceptions = 0usize;
        let mut div_min = F::MAX_INT;
        let mut div_max = F::MIN_INT;

        for &val in active_samples {
          if let Some(enc) = F::try_encode_div(val, exp_factor) {
            div_min = div_min.min(enc);
            div_max = div_max.max(enc);
          } else {
            div_exceptions += 1;
            if div_exceptions * F::EXCEPTION_PENALTY >= best_cost {
              break;
            }
          }
        }

        if div_exceptions != sample_len && div_exceptions * F::EXCEPTION_PENALTY < best_cost {
          let max_offset = if div_min <= div_max {
            F::calc_range(div_min, div_max)
          } else {
            0
          };
          let bit_width = F::bits_needed(max_offset) as usize;
          let total_cost = bit_width * sample_len + div_exceptions * F::EXCEPTION_PENALTY;

          if total_cost < best_cost {
            best_cost = total_cost;
            best_params = BestParams {
              exp,
              fac: 0,
              use_div: true,
            };
            if total_cost == 0 {
              return best_params;
            }
            if div_exceptions == 0 && bit_width <= EARLY_EXIT_BIT_WIDTH {
              return best_params;
            }
          }
        }
      }

      if zero_exceptions_fac0 {
        break;
      }
    }
  }

  best_params
}
