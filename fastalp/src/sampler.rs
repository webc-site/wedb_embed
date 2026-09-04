use core::mem::size_of;

use crate::{
  constants::{EARLY_EXIT_BIT_WIDTH, SAMPLES_COUNT},
  float::AlpFloat,
};

/// Sample length for pure decimal multiplication pre-check
/// 纯十进制乘法前置快筛采样数量
const PRE_CHECK_LEN_MUL: usize = 6;
/// Sample length for factor pre-check
/// 混合因子前置快筛采样数量
const PRE_CHECK_LEN_FAC: usize = 4;
/// Max exceptions threshold for factor pre-check
/// 混合因子前置异常快速淘汰门限
const PRE_CHECK_MAX_EXC_FAC: usize = 2;
/// Sample length for decimal division early abort check
/// 十进制除法模式前置早停检查样本数
const DIV_EARLY_CHECK_LEN: usize = 6;
/// Exception threshold for decimal division early abort
/// 十进制除法模式前置早停异常数阈值
const DIV_EARLY_ABORT_EXC: usize = 3;
/// Penalty multiplier for non-zero factor
/// 混合因子额外开销倍数
const FAC_PENALTY_MULT: usize = 2;
/// Low cost threshold per value to skip factor search
/// 纯十进制极低位宽跳过因子穷举阈值（位/值）
const LOW_COST_THRESHOLD_PER_VAL: usize = 3;
/// High exponent threshold to trigger decimal division
/// 触发十进制除法的高精度指数阈值
const HIGH_EXP_DIV_THRESHOLD: u8 = 14;

/// Common high-frequency decimal exponent exploration order
/// Over 95% of sensor, financial, and metric time-series have 1-3 decimals or integers.
/// 常用高频十进制精度探索顺序（2, 1, 3, 0, 4..）
/// 工业传感器、金融量化、监控时序绝大多数为 1~3 位小数或整数，优先探索命中率超 95%
const EXP_PRIORITY: [u8; 19] = [
  2, 1, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
];

/// Optimal parameters discovered by sampling
/// 采样与最优系数选择结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BestParams {
  pub exp: u8,
  pub fac: u8,
  pub use_div: bool,
}

/// Checks if a float is an unencodable special value (NaN, Inf, -0.0, out of bounds).
/// 检查浮点数是否为不可编码的特殊值（NaN, Inf, -0.0, 超出范围）
#[inline(always)]
pub fn is_impossible<F: AlpFloat>(n: F) -> bool {
  n.is_impossible()
}

/// Fast single-value float encoding probe with pre-extracted exponent factors.
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

/// Tries to encode a single float into integer and verifies 100% bit-exact lossless roundtrip.
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

/// Fast exponent and base probe for identical/constant float sequences (zero-heap allocation, O(1) complexity).
/// 全等与常数浮点数序列的快速指数与基准值探测（零堆分配、O(1) 复杂度）
#[inline]
pub fn find_identical_base<F: AlpFloat>(val: F) -> Option<(u8, F::Int)> {
  const FAC_INT: i64 = 1;
  (0..=F::MAX_EXPONENT).find_map(|exp| {
    let frac_exp = F::frac_exp(exp);
    let exp_factor = F::exp_factor(exp, 0);
    F::try_encode_fast(val, exp_factor, FAC_INT, frac_exp).map(|base| (exp, base))
  })
}

/// Evaluates sample cost to find optimal exponent (exp), factor (fac), and division mode (use_div).
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
  for &val in samples
    .iter()
    .filter(|v| !v.is_impossible())
    .take(SAMPLES_COUNT)
  {
    valid_samples[sample_len] = val;
    sample_len += 1;
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
  let mut best_exceptions = sample_len;
  let mut best_params = BestParams {
    exp: 0,
    fac: 0,
    use_div: false,
  };

  let mut any_decimal = false;

  // Pass 1: probe pure decimal multiplication (fac == 0) prioritizing common exponents (2, 1, 3, 0, 4..)
  // 第一轮：优先探测纯十进制乘法组合 (fac == 0)，按高频精度优先探索 (2, 1, 3, 0, 4..)
  for &exp in &EXP_PRIORITY {
    if exp > F::MAX_EXPONENT {
      continue;
    }
    let frac_exp = F::frac_exp(exp);
    let exp_factor = F::exp_factor(exp, 0);
    const FAC_INT: i64 = 1;

    // Pre-check and extremum tracking in single iteration: eliminates scratch arrays and second-pass loop
    // 前置快筛与极值合并单次迭代：消除临时数组与二次循环开销
    let pre_n = active_samples.len().min(PRE_CHECK_LEN_MUL);
    let mut pre_exc = 0;
    let mut min_val = F::MAX_INT;
    let mut max_val = F::MIN_INT;

    for &val in &active_samples[..pre_n] {
      if let Some(enc) = F::try_encode_fast(val, exp_factor, FAC_INT, frac_exp) {
        min_val = min_val.min(enc);
        max_val = max_val.max(enc);
      } else {
        pre_exc += 1;
      }
    }
    let mul_feasible = pre_exc < pre_n;
    let mut exceptions = pre_exc;

    if mul_feasible {
      for &val in &active_samples[pre_n..] {
        if let Some(enc) = F::try_encode_fast(val, exp_factor, FAC_INT, frac_exp) {
          min_val = min_val.min(enc);
          max_val = max_val.max(enc);
        } else {
          exceptions += 1;
          if exceptions * F::EXCEPTION_PENALTY >= best_cost {
            break;
          }
        }
      }

      if exceptions != sample_len {
        any_decimal = true;
        if exceptions * F::EXCEPTION_PENALTY < best_cost {
          let max_offset = if min_val <= max_val {
            F::calc_range(min_val, max_val)
          } else {
            0
          };
          let bit_width = F::bits_needed(max_offset) as usize;
          let total_cost = bit_width * sample_len + exceptions * F::EXCEPTION_PENALTY;

          if total_cost < best_cost {
            best_cost = total_cost;
            best_exceptions = exceptions;
            best_params = BestParams {
              exp,
              fac: 0,
              use_div: false,
            };
            if total_cost == 0 || (exceptions == 0 && bit_width <= EARLY_EXIT_BIT_WIDTH) {
              return best_params;
            }
          }
        }
      }
    }

    // When fac == 0, evaluate Decimal Division Mode: triggered if mul infeasible, has exceptions, or high exponent
    // 当 fac == 0 时，评估十进制除法重构模式 (Decimal Division Mode)
    // 触发条件：乘法不可行、乘法存在异常、或高指数高精度场景 (exp >= HIGH_EXP_DIV_THRESHOLD)
    if exp > 0 && (!mul_feasible || exceptions > 0 || exp >= HIGH_EXP_DIV_THRESHOLD) {
      let mut div_exceptions = 0usize;
      let mut div_min = F::MAX_INT;
      let mut div_max = F::MIN_INT;

      for (idx, &val) in active_samples.iter().enumerate() {
        if let Some(enc) = F::try_encode_div(val, exp_factor) {
          div_min = div_min.min(enc);
          div_max = div_max.max(enc);
        } else {
          div_exceptions += 1;
          if div_exceptions >= DIV_EARLY_ABORT_EXC && idx < DIV_EARLY_CHECK_LEN {
            div_exceptions = sample_len;
            break;
          }
          if div_exceptions * F::EXCEPTION_PENALTY >= best_cost {
            break;
          }
        }
      }

      if div_exceptions != sample_len {
        any_decimal = true;
        if div_exceptions * F::EXCEPTION_PENALTY < best_cost {
          let max_offset = if div_min <= div_max {
            F::calc_range(div_min, div_max)
          } else {
            0
          };
          let bit_width = F::bits_needed(max_offset) as usize;
          let total_cost = bit_width * sample_len + div_exceptions * F::EXCEPTION_PENALTY;

          if total_cost < best_cost {
            best_cost = total_cost;
            best_exceptions = div_exceptions;
            best_params = BestParams {
              exp,
              fac: 0,
              use_div: true,
            };
            if total_cost == 0 || (div_exceptions == 0 && bit_width <= EARLY_EXIT_BIT_WIDTH) {
              return best_params;
            }
          }
        }
      }
    }
  }

  // If data is non-decimal or already low cost (<=3 bit/val), skip expensive factor search
  // 若数据非十进制，或已找到开销极小的十进制模型 (<=3 bit/val)，直接跳过高耗时的因子穷举
  if !any_decimal || best_cost <= sample_len * LOW_COST_THRESHOLD_PER_VAL {
    return best_params;
  }

  // If pure decimal achieved 0 exceptions, search downwards only (exp < best_exp).
  // 若纯十进制已达到 0 异常，只需向下搜索更小指数 (exp < best_exp)。
  // 指数 >= best_exp 的因子组合数值跨度与位宽恒大于等于纯十进制，不可能更优。
  let max_search_exp = if best_exceptions == 0 {
    best_params.exp.saturating_sub(1)
  } else {
    F::MAX_EXPONENT
  };

  if max_search_exp == 0 {
    return best_params;
  }

  // Pass 2: when pure decimal has higher cost, expand search to factors (fac > 0)
  // 第二轮：当纯十进制无法满足极低开销时，扩展搜索混合因子 (fac > 0)
  for exp in 1..=max_search_exp {
    let max_fac = exp.min(F::MAX_FAC);
    let frac_exp = F::frac_exp(exp);

    for fac in 1..=max_fac {
      let exp_factor = F::exp_factor(exp, fac);
      let fac_int = F::fac_int(fac);

      // Pre-check and extremum tracking in single iteration: eliminates scratch arrays and second-pass loop
      // 前置快筛与极值合并单次迭代：消除临时数组与二次循环
      let pre_n = active_samples.len().min(PRE_CHECK_LEN_FAC);
      let mut pre_exc = 0;
      let mut min_val = F::MAX_INT;
      let mut max_val = F::MIN_INT;

      for &val in &active_samples[..pre_n] {
        if let Some(enc) = F::try_encode_fast(val, exp_factor, fac_int, frac_exp) {
          min_val = min_val.min(enc);
          max_val = max_val.max(enc);
        } else {
          pre_exc += 1;
        }
      }
      let fac_penalty = sample_len * FAC_PENALTY_MULT;
      if pre_exc >= PRE_CHECK_MAX_EXC_FAC
        || pre_exc * F::EXCEPTION_PENALTY + fac_penalty >= best_cost
      {
        continue;
      }

      let mut exceptions = pre_exc;

      for &val in &active_samples[pre_n..] {
        if let Some(enc) = F::try_encode_fast(val, exp_factor, fac_int, frac_exp) {
          min_val = min_val.min(enc);
          max_val = max_val.max(enc);
        } else {
          exceptions += 1;
          if exceptions * F::EXCEPTION_PENALTY + fac_penalty >= best_cost {
            break;
          }
        }
      }

      if exceptions != sample_len && exceptions * F::EXCEPTION_PENALTY + fac_penalty < best_cost {
        let max_offset = if min_val <= max_val {
          F::calc_range(min_val, max_val)
        } else {
          0
        };
        let bit_width = F::bits_needed(max_offset) as usize;
        let total_cost = bit_width * sample_len + exceptions * F::EXCEPTION_PENALTY + fac_penalty;

        if total_cost < best_cost {
          best_cost = total_cost;
          best_params = BestParams {
            exp,
            fac,
            use_div: false,
          };
          if total_cost == 0 || (exceptions == 0 && bit_width <= EARLY_EXIT_BIT_WIDTH) {
            return best_params;
          }
        }
      }
    }
  }

  best_params
}
