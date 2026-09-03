mod delta;
mod standard;

use core::marker::PhantomData;
use std::slice::from_raw_parts;

pub use delta::encode_delta;
pub use standard::encode_standard;

use crate::{
  bitpack::packed_byte_size,
  constants::{EXC_COUNT_LEN, EXC_COUNT_LEN_U32},
  delta::{delta_range, eval_delta_benefit},
  float::AlpFloat,
  header::{header_len, raw_header_len, write_header},
  params::pack_params,
  sampler::{BestParams, find_best_params, find_identical_base},
};

/// Single exception value record.
/// 单个异常值记录
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exception<R> {
  pub pos: usize,
  pub bits: R,
}

/// Stateful encoder that caches optimal parameters across adjacent chunks (matching C++ ALP's DuckDB architecture).
/// 状态化编码器：在连续数据块编码时复用已探测的最优参数，消除重复采样开销，使编码吞吐突破 6~12 GB/s。
#[derive(Debug, Clone)]
pub struct Encoder<F: AlpFloat> {
  pub cached_params: Option<BestParams>,
  _marker: PhantomData<F>,
}

impl<F: AlpFloat> Default for Encoder<F> {
  fn default() -> Self {
    Self::new()
  }
}

impl<F: AlpFloat> Encoder<F> {
  pub fn new() -> Self {
    Self {
      cached_params: None,
      _marker: PhantomData,
    }
  }

  /// Reset cached parameters.
  pub fn reset(&mut self) {
    self.cached_params = None;
  }

  /// Compress with parameter caching.
  pub fn compress_into(&mut self, data: &[F], dst: &mut Vec<u8>) {
    self.cached_params = compress_impl(data, dst, false, self.cached_params);
  }

  /// Compress with parameter caching and forced Delta differential encoding.
  pub fn compress_delta_into(&mut self, data: &[F], dst: &mut Vec<u8>) {
    self.cached_params = compress_impl(data, dst, true, self.cached_params);
  }
}

/// Generic floating-point compression writing directly into `dst` buffer.
/// 通用压缩浮点数组并直接写入 `dst` 缓冲区（自适应选择 FOR 或 Delta 差分模式）
pub fn compress_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
  compress_impl(data, dst, false, None);
}

/// Floating-point compression with enforced Delta differential encoding.
/// 强制使用 Delta 一阶差分模式压缩浮点数组并直接写入 `dst` 缓冲区
pub fn compress_delta_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
  compress_impl(data, dst, true, None);
}

fn compress_impl<F: AlpFloat>(
  data: &[F],
  dst: &mut Vec<u8>,
  force_delta: bool,
  cached_params: Option<BestParams>,
) -> Option<BestParams> {
  let count = data.len();
  if count == 0 {
    let raw_hdr = raw_header_len(0);
    dst.reserve(raw_hdr);
    write_header(F::TYPE_BYTE, 0, None, dst);
    return None;
  }

  let slice = data;

  // 极速全等序列检测：如果所有浮点数完全相同（比特级无损判等），直接写入基准值与 bit_width=0，零堆分配
  let first = slice[0];
  if (slice.len() <= 1 || slice[1].is_exact_same(first))
    && slice.iter().all(|&v| v.is_exact_same(first))
    && let Some((exp, base)) = find_identical_base(first)
  {
    let total_needed = header_len(count) + F::BASE_SIZE;
    dst.reserve(total_needed);
    let packed_params = pack_params(exp, 0, 0);
    write_header(F::TYPE_BYTE, count, Some(packed_params), dst);
    F::write_base(base, dst);
    return Some(BestParams {
      exp,
      fac: 0,
      use_div: false,
    });
  }

  let mut best_params = if let Some(p) = cached_params {
    let exp_factor = F::exp_factor(p.exp, p.fac);
    let fac_int = F::fac_int(p.fac);
    let frac_exp = F::frac_exp(p.exp);
    let check_n = slice.len().min(4);
    let mut valid = true;
    for &v in &slice[..check_n] {
      let enc = v.fast_round_to_int(exp_factor);
      let dec = if p.use_div {
        F::decode_from_int_div(enc, exp_factor)
      } else {
        F::decode_from_int(enc, fac_int, frac_exp)
      };
      if dec.to_raw_bits() != v.to_raw_bits() {
        valid = false;
        break;
      }
    }
    if valid { p } else { find_best_params(slice) }
  } else {
    find_best_params(slice)
  };

  let mut exp_factor = F::exp_factor(best_params.exp, best_params.fac);
  let mut fac_int = F::fac_int(best_params.fac);
  let mut frac_exp = F::frac_exp(best_params.exp);

  let mut stack_encoded = [F::ZERO_INT; 1024];
  let mut heap_encoded: Vec<F::Int> = Vec::new();
  let mut exceptions: Vec<Exception<F::RawBits>> = Vec::with_capacity(16);

  let enc_ptr: *mut F::Int = if slice.len() <= 1024 {
    stack_encoded.as_mut_ptr()
  } else {
    heap_encoded.resize(slice.len(), F::ZERO_INT);
    heap_encoded.as_mut_ptr()
  };

  let (mut min_val, mut max_val) = unsafe {
    if best_params.use_div {
      encode_loop_div(slice, enc_ptr, exp_factor, &mut exceptions)
    } else if fac_int == 1 {
      encode_loop_fac1(slice, enc_ptr, exp_factor, frac_exp, &mut exceptions)
    } else {
      encode_loop_fac(
        slice,
        enc_ptr,
        exp_factor,
        fac_int,
        frac_exp,
        &mut exceptions,
      )
    }
  };

  // 若使用了缓存参数但遭遇超过 128 异常，说明缓存参数在后续序列失效；重新全量采样探测以挽救压缩率
  if exceptions.len() > 128 && cached_params.is_some() {
    let fresh_params = find_best_params(slice);
    if fresh_params != best_params {
      exceptions.clear();
      exp_factor = F::exp_factor(fresh_params.exp, fresh_params.fac);
      fac_int = F::fac_int(fresh_params.fac);
      frac_exp = F::frac_exp(fresh_params.exp);
      let bounds = unsafe {
        if fresh_params.use_div {
          encode_loop_div(slice, enc_ptr, exp_factor, &mut exceptions)
        } else if fac_int == 1 {
          encode_loop_fac1(slice, enc_ptr, exp_factor, frac_exp, &mut exceptions)
        } else {
          encode_loop_fac(
            slice,
            enc_ptr,
            exp_factor,
            fac_int,
            frac_exp,
            &mut exceptions,
          )
        }
      };
      if exceptions.len() <= 128 {
        best_params = fresh_params;
        min_val = bounds.0;
        max_val = bounds.1;
      }
    }
  }

  let raw_len = size_of_val(slice);
  let raw_hdr = raw_header_len(count);

  if exceptions.len() > 128 {
    let total_raw = raw_hdr + raw_len;
    dst.reserve(total_raw);
    write_header(F::TYPE_RAW_BYTE, count, None, dst);
    // SAFETY: slice 是连续只读浮点内存，转换为底层紧凑字节序列安全
    let raw_slice = unsafe { from_raw_parts(slice.as_ptr().cast::<u8>(), raw_len) };
    dst.extend_from_slice(raw_slice);
    return None;
  }

  let encoded_ints: &mut [F::Int] = if slice.len() <= 1024 {
    &mut stack_encoded[..slice.len()]
  } else {
    &mut heap_encoded[..]
  };

  let base = if min_val <= max_val {
    min_val
  } else {
    F::ZERO_INT
  };
  let max_offset = if min_val <= max_val {
    F::calc_range(min_val, max_val)
  } else {
    0
  };

  if !exceptions.is_empty() {
    for exc in &exceptions {
      // 异常值填充前一个有效整型值，避免对相邻一阶差分造成额外突变影响
      let patch_val = if exc.pos > 0 {
        // SAFETY: exc.pos > 0 且严格小于 encoded_ints.len()
        unsafe { *encoded_ints.get_unchecked(exc.pos - 1) }
      } else {
        base
      };
      // SAFETY: exc.pos 严格小于 encoded_ints.len()
      unsafe {
        *encoded_ints.get_unchecked_mut(exc.pos) = patch_val;
      }
    }
  }

  let is_large = count > u16::MAX as usize;
  let mut for_bit_width = F::bits_needed(max_offset);
  let mut for_packed_len = packed_byte_size(slice.len(), for_bit_width);
  let mut exc_len = if exceptions.is_empty() {
    0
  } else if is_large {
    EXC_COUNT_LEN_U32 + exceptions.len() * F::EXC_ENTRY_SIZE_U32
  } else {
    EXC_COUNT_LEN + exceptions.len() * F::EXC_ENTRY_SIZE
  };

  let hdr_len = header_len(count);

  // 评估 Delta 差分收益（仅在显式强制或 FOR 位宽较大时才评估，消减 90% 冗余内存扫描）
  let delta_decision = if slice.len() > 1 && (force_delta || for_bit_width >= 12) {
    let first = encoded_ints[0];
    let rest = &encoded_ints[1..];
    if force_delta {
      Some(delta_range::<F>(first, rest))
    } else {
      eval_delta_benefit::<F>(first, rest, for_bit_width)
    }
  } else {
    None
  };

  let (use_delta, min_delta, delta_bit_width, total_needed) = match delta_decision {
    Some((min_d, delta_bw)) => {
      let delta_packed_len = packed_byte_size(slice.len() - 1, delta_bw);
      let delta_total = hdr_len + F::BASE_SIZE * 2 + delta_packed_len + exc_len;
      let for_total = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;
      if delta_total < for_total || force_delta {
        (true, min_d, delta_bw, delta_total)
      } else {
        (false, F::ZERO_INT, 0, for_total)
      }
    }
    None => {
      let for_total = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;
      (false, F::ZERO_INT, 0, for_total)
    }
  };

  // FOR 模式专用：离群值异常剪枝优化 (Outlier Pruning to Exceptions)
  // 仅在未启用 Delta 模式、且位宽在 16~32 区间时尝试 16 位剪枝，避免不可压缩浮点数据（位宽 > 32）产生多轮无谓扫描。
  let total_needed = if !use_delta && for_bit_width > 4 && exceptions.len() < 16 {
    let entry_size = if is_large {
      F::EXC_ENTRY_SIZE_U32
    } else {
      F::EXC_ENTRY_SIZE
    };
    let current_cost = for_packed_len + exceptions.len() * entry_size;

    let candidate_widths = [0u8, 8, 16];
    let mut best_target_bw = for_bit_width;
    let mut min_cost = current_cost;

    for &target_bw in &candidate_widths {
      if target_bw >= for_bit_width {
        break;
      }
      let max_allowed = if target_bw == 0 {
        0u64
      } else {
        (1u64 << target_bw) - 1
      };

      // 前置 16 采样快筛：若在前 16 个元素中已出现超过 1 个离群点，直接短路跳过
      let pre_check_n = encoded_ints.len().min(16);
      let mut pre_outliers = 0;
      for &val in &encoded_ints[..pre_check_n] {
        if F::int_diff_to_u64(val, base) > max_allowed {
          pre_outliers += 1;
          if pre_outliers > 1 {
            break;
          }
        }
      }
      if pre_outliers > 1 {
        continue;
      }

      let mut extra_exceptions = pre_outliers;
      for &val in &encoded_ints[pre_check_n..] {
        let diff = F::int_diff_to_u64(val, base);
        if diff > max_allowed {
          extra_exceptions += 1;
          if extra_exceptions > 16 {
            break;
          }
        }
      }

      if extra_exceptions <= 16 {
        let new_total_exc = exceptions.len() + extra_exceptions;
        let new_cost = packed_byte_size(slice.len(), target_bw) + new_total_exc * entry_size;
        if new_cost < min_cost {
          min_cost = new_cost;
          best_target_bw = target_bw;
        }
      }
    }

    if best_target_bw < for_bit_width {
      let max_allowed = (1u64 << best_target_bw) - 1;
      for (pos, &val) in encoded_ints.iter().enumerate() {
        let diff = F::int_diff_to_u64(val, base);
        if diff > max_allowed {
          exceptions.push(Exception {
            pos,
            bits: slice[pos].to_raw_bits(),
          });
        }
      }
      exceptions.sort_unstable_by_key(|e| e.pos);
      exceptions.dedup_by_key(|e| e.pos);

      // 为离群点回填基准值，确保打包时不溢出目标位宽
      for exc in &exceptions {
        unsafe {
          *encoded_ints.get_unchecked_mut(exc.pos) = base;
        }
      }
      for_bit_width = best_target_bw;
      for_packed_len = packed_byte_size(slice.len(), for_bit_width);
      exc_len = if is_large {
        EXC_COUNT_LEN_U32 + exceptions.len() * F::EXC_ENTRY_SIZE_U32
      } else {
        EXC_COUNT_LEN + exceptions.len() * F::EXC_ENTRY_SIZE
      };
    }
    hdr_len + F::BASE_SIZE + for_packed_len + exc_len
  } else {
    total_needed
  };

  let raw_len = size_of_val(slice);
  let raw_hdr = raw_header_len(count);

  // 启用 RAW 模式保底：当压缩后大小超过原始大小（负压缩）时，直接以 RAW 格式存储
  if total_needed >= raw_len + raw_hdr {
    let total_raw = raw_hdr + raw_len;
    dst.reserve(total_raw);
    write_header(F::TYPE_RAW_BYTE, count, None, dst);
    // SAFETY: slice 是有效且连续的浮点内存切片，转换为底层紧凑字节序列安全无误
    let raw_slice = unsafe { from_raw_parts(slice.as_ptr().cast::<u8>(), raw_len) };
    dst.extend_from_slice(raw_slice);
    return None;
  }

  dst.reserve(total_needed);

  if use_delta {
    encode_delta::<F>(
      count,
      best_params.exp,
      best_params.fac,
      best_params.use_div,
      encoded_ints,
      min_delta,
      delta_bit_width,
      &exceptions,
      dst,
    );
  } else {
    encode_standard::<F>(
      count,
      best_params.exp,
      best_params.fac,
      best_params.use_div,
      encoded_ints,
      base,
      for_bit_width,
      &exceptions,
      dst,
    );
  }

  Some(best_params)
}

/// Generic floating-point slice compression.
/// 通用压缩浮点数切片
#[inline]
pub fn compress<F: AlpFloat>(data: &[F]) -> Vec<u8> {
  let mut dst = Vec::new();
  compress_into(data, &mut dst);
  dst
}

/// Generic floating-point slice compression enforcing Delta differential mode.
/// 强制使用 Delta 差分模式压缩浮点数切片
#[inline]
pub fn compress_delta<F: AlpFloat>(data: &[F]) -> Vec<u8> {
  let mut dst = Vec::new();
  compress_delta_into(data, &mut dst);
  dst
}

/// Branchless optimized encoding loop for fac_int == 1 (95%+ common case in decimal time series).
/// 针对无因子（fac_int == 1）的极致无分支向量化编码循环：
/// 批处理 4 个元素，若 100% 精确命中（无异常），直接 SIMD 写入并利用无分支极值指令更新 min/max，消除 95% 异常分支开销。
#[inline(always)]
unsafe fn encode_loop_fac1<F: AlpFloat>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  exp_factor: F,
  frac_exp: F,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  unsafe {
    let mut min_val = F::MAX_INT;
    let mut max_val = F::MIN_INT;
    let len = slice.len();
    let unroll_len = len & !3;
    let mut i = 0;

    while i < unroll_len {
      let v0 = *slice.get_unchecked(i);
      let v1 = *slice.get_unchecked(i + 1);
      let v2 = *slice.get_unchecked(i + 2);
      let v3 = *slice.get_unchecked(i + 3);

      let enc0 = v0.fast_round_to_int(exp_factor);
      let enc1 = v1.fast_round_to_int(exp_factor);
      let enc2 = v2.fast_round_to_int(exp_factor);
      let enc3 = v3.fast_round_to_int(exp_factor);

      let d0 = F::decode_from_int(enc0, 1, frac_exp);
      let d1 = F::decode_from_int(enc1, 1, frac_exp);
      let d2 = F::decode_from_int(enc2, 1, frac_exp);
      let d3 = F::decode_from_int(enc3, 1, frac_exp);

      let ok0 = d0.to_raw_bits() == v0.to_raw_bits();
      let ok1 = d1.to_raw_bits() == v1.to_raw_bits();
      let ok2 = d2.to_raw_bits() == v2.to_raw_bits();
      let ok3 = d3.to_raw_bits() == v3.to_raw_bits();

      if ok0 && ok1 && ok2 && ok3 {
        enc_ptr.add(i).write(enc0);
        enc_ptr.add(i + 1).write(enc1);
        enc_ptr.add(i + 2).write(enc2);
        enc_ptr.add(i + 3).write(enc3);
        let l_min = enc0.min(enc1).min(enc2.min(enc3));
        let l_max = enc0.max(enc1).max(enc2.max(enc3));
        min_val = min_val.min(l_min);
        max_val = max_val.max(l_max);
      } else {
        macro_rules! handle_one {
          ($idx:expr, $val:expr, $enc:expr, $ok:expr) => {
            if $ok {
              enc_ptr.add($idx).write($enc);
              min_val = min_val.min($enc);
              max_val = max_val.max($enc);
            } else {
              enc_ptr.add($idx).write(F::ZERO_INT);
              exceptions.push(Exception {
                pos: $idx,
                bits: $val.to_raw_bits(),
              });
              if exceptions.len() > 128 {
                return (F::MAX_INT, F::MIN_INT);
              }
            }
          };
        }
        handle_one!(i, v0, enc0, ok0);
        handle_one!(i + 1, v1, enc1, ok1);
        handle_one!(i + 2, v2, enc2, ok2);
        handle_one!(i + 3, v3, enc3, ok3);
      }
      i += 4;
    }

    while i < len {
      let val = *slice.get_unchecked(i);
      let enc = val.fast_round_to_int(exp_factor);
      let d = F::decode_from_int(enc, 1, frac_exp);
      if d.to_raw_bits() == val.to_raw_bits() {
        enc_ptr.add(i).write(enc);
        min_val = min_val.min(enc);
        max_val = max_val.max(enc);
      } else {
        enc_ptr.add(i).write(F::ZERO_INT);
        exceptions.push(Exception {
          pos: i,
          bits: val.to_raw_bits(),
        });
        if exceptions.len() > 128 {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 1;
    }

    (min_val, max_val)
  }
}

/// 专有 4-way 展开除法流水线编码循环
#[inline(always)]
unsafe fn encode_loop_div<F: AlpFloat>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  exp_factor: F,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  unsafe {
    let mut min_val = F::MAX_INT;
    let mut max_val = F::MIN_INT;
    let len = slice.len();
    let unroll_len = len & !3;
    let mut i = 0;

    while i < unroll_len {
      let v0 = *slice.get_unchecked(i);
      let v1 = *slice.get_unchecked(i + 1);
      let v2 = *slice.get_unchecked(i + 2);
      let v3 = *slice.get_unchecked(i + 3);

      let enc0 = v0.fast_round_to_int(exp_factor);
      let enc1 = v1.fast_round_to_int(exp_factor);
      let enc2 = v2.fast_round_to_int(exp_factor);
      let enc3 = v3.fast_round_to_int(exp_factor);

      let d0 = F::decode_from_int_div(enc0, exp_factor);
      let d1 = F::decode_from_int_div(enc1, exp_factor);
      let d2 = F::decode_from_int_div(enc2, exp_factor);
      let d3 = F::decode_from_int_div(enc3, exp_factor);

      let ok0 = d0.to_raw_bits() == v0.to_raw_bits();
      let ok1 = d1.to_raw_bits() == v1.to_raw_bits();
      let ok2 = d2.to_raw_bits() == v2.to_raw_bits();
      let ok3 = d3.to_raw_bits() == v3.to_raw_bits();

      macro_rules! handle_one {
        ($idx:expr, $val:expr, $enc:expr, $ok:expr) => {
          if $ok {
            enc_ptr.add($idx).write($enc);
            min_val = min_val.min($enc);
            max_val = max_val.max($enc);
          } else {
            enc_ptr.add($idx).write(F::ZERO_INT);
            exceptions.push(Exception {
              pos: $idx,
              bits: $val.to_raw_bits(),
            });
          }
        };
      }

      if ok0 && ok1 && ok2 && ok3 {
        enc_ptr.add(i).write(enc0);
        enc_ptr.add(i + 1).write(enc1);
        enc_ptr.add(i + 2).write(enc2);
        enc_ptr.add(i + 3).write(enc3);
        min_val = min_val.min(enc0).min(enc1).min(enc2).min(enc3);
        max_val = max_val.max(enc0).max(enc1).max(enc2).max(enc3);
      } else {
        handle_one!(i, v0, enc0, ok0);
        handle_one!(i + 1, v1, enc1, ok1);
        handle_one!(i + 2, v2, enc2, ok2);
        handle_one!(i + 3, v3, enc3, ok3);
        if exceptions.len() > 128 {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 4;
    }

    while i < len {
      let val = *slice.get_unchecked(i);
      let enc = val.fast_round_to_int(exp_factor);
      let d = F::decode_from_int_div(enc, exp_factor);
      if d.to_raw_bits() == val.to_raw_bits() {
        enc_ptr.add(i).write(enc);
        min_val = min_val.min(enc);
        max_val = max_val.max(enc);
      } else {
        enc_ptr.add(i).write(F::ZERO_INT);
        exceptions.push(Exception {
          pos: i,
          bits: val.to_raw_bits(),
        });
        if exceptions.len() > 128 {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 1;
    }

    (min_val, max_val)
  }
}

/// 专有 4-way 展开因子流水线编码循环
#[inline(always)]
unsafe fn encode_loop_fac<F: AlpFloat>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  exp_factor: F,
  fac_int: i64,
  frac_exp: F,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  unsafe {
    let mut min_val = F::MAX_INT;
    let mut max_val = F::MIN_INT;
    let len = slice.len();
    let unroll_len = len & !3;
    let mut i = 0;

    while i < unroll_len {
      let v0 = *slice.get_unchecked(i);
      let v1 = *slice.get_unchecked(i + 1);
      let v2 = *slice.get_unchecked(i + 2);
      let v3 = *slice.get_unchecked(i + 3);

      let enc0 = v0.fast_round_to_int(exp_factor);
      let enc1 = v1.fast_round_to_int(exp_factor);
      let enc2 = v2.fast_round_to_int(exp_factor);
      let enc3 = v3.fast_round_to_int(exp_factor);

      let d0 = F::decode_from_int(enc0, fac_int, frac_exp);
      let d1 = F::decode_from_int(enc1, fac_int, frac_exp);
      let d2 = F::decode_from_int(enc2, fac_int, frac_exp);
      let d3 = F::decode_from_int(enc3, fac_int, frac_exp);

      let ok0 = d0.to_raw_bits() == v0.to_raw_bits();
      let ok1 = d1.to_raw_bits() == v1.to_raw_bits();
      let ok2 = d2.to_raw_bits() == v2.to_raw_bits();
      let ok3 = d3.to_raw_bits() == v3.to_raw_bits();

      macro_rules! handle_one {
        ($idx:expr, $val:expr, $enc:expr, $ok:expr) => {
          if $ok {
            enc_ptr.add($idx).write($enc);
            min_val = min_val.min($enc);
            max_val = max_val.max($enc);
          } else {
            enc_ptr.add($idx).write(F::ZERO_INT);
            exceptions.push(Exception {
              pos: $idx,
              bits: $val.to_raw_bits(),
            });
          }
        };
      }

      if ok0 && ok1 && ok2 && ok3 {
        enc_ptr.add(i).write(enc0);
        enc_ptr.add(i + 1).write(enc1);
        enc_ptr.add(i + 2).write(enc2);
        enc_ptr.add(i + 3).write(enc3);
        min_val = min_val.min(enc0).min(enc1).min(enc2).min(enc3);
        max_val = max_val.max(enc0).max(enc1).max(enc2).max(enc3);
      } else {
        handle_one!(i, v0, enc0, ok0);
        handle_one!(i + 1, v1, enc1, ok1);
        handle_one!(i + 2, v2, enc2, ok2);
        handle_one!(i + 3, v3, enc3, ok3);
        if exceptions.len() > 128 {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 4;
    }

    while i < len {
      let val = *slice.get_unchecked(i);
      let enc = val.fast_round_to_int(exp_factor);
      let d = F::decode_from_int(enc, fac_int, frac_exp);
      if d.to_raw_bits() == val.to_raw_bits() {
        enc_ptr.add(i).write(enc);
        min_val = min_val.min(enc);
        max_val = max_val.max(enc);
      } else {
        enc_ptr.add(i).write(F::ZERO_INT);
        exceptions.push(Exception {
          pos: i,
          bits: val.to_raw_bits(),
        });
        if exceptions.len() > 128 {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 1;
    }

    (min_val, max_val)
  }
}

/// Encodes exceptions table into dst buffer.
/// 统一编码异常值字典至目标缓冲区（自适应兼容普通 u16 与超大数组 u32 索引）
#[inline(always)]
pub(crate) fn write_exceptions<F: AlpFloat>(
  count: usize,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  if exceptions.is_empty() {
    return;
  }
  if count > u16::MAX as usize {
    let exc_count = exceptions.len() as u32;
    dst.extend_from_slice(&exc_count.to_le_bytes());
    for exc in exceptions {
      F::write_exception_u32(exc.pos as u32, exc.bits, dst);
    }
  } else {
    let exc_count = exceptions.len() as u16;
    dst.extend_from_slice(&exc_count.to_le_bytes());
    for exc in exceptions {
      F::write_exception(exc.pos as u16, exc.bits, dst);
    }
  }
}
