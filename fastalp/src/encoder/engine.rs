use core::{
  mem::{MaybeUninit, size_of_val},
  slice::{from_raw_parts, from_raw_parts_mut},
};

use crate::{
  bitpack::packed_byte_size,
  constants::MAX_EXCEPTIONS,
  delta::{delta_range, eval_delta_benefit},
  encoder::{
    delta::encode_delta,
    exception::{Exception, exceptions_byte_size},
    kernel::encode_slice,
    outlier::try_prune_outliers,
    standard::encode_standard,
    state::CachedTargetBw,
  },
  float::AlpFloat,
  header::{header_len, raw_header_len, write_header},
  params::AlpParams,
  sampler::{BestParams, find_best_params, find_identical_base},
};

/// Bit-width threshold to trigger high bit-width outlier pre-pruning
/// 针对高位宽数据触发离群值前置剪枝的位宽阈值
const HIGH_BW_PRUNE_THRESHOLD: u8 = 16;
/// Minimum bit-width threshold to evaluate Delta benefit
/// 评估 Delta 差分收益的最小位宽门限（低于此门限时直接走 FOR 编码）
const DELTA_EVAL_MIN_BW: u8 = 4;
/// Minimum bit-width floor for low bit-width outlier pruning in FOR mode
/// FOR 模式低位宽离群值剪枝位宽下限
const LOW_BW_PRUNE_MIN: u8 = 4;
/// Stack work buffer capacity in elements
/// 栈上预分配工作缓冲区大小（元素个数，避免小数组堆分配）
const STACK_BUFFER_CAPACITY: usize = 1024;
/// Sample count for quick validation of cached parameters
/// 缓存参数快速校验抽样数量
const CACHE_VALIDATE_SAMPLE_N: usize = 4;

#[inline(always)]
fn check_roundtrip<F: AlpFloat>(slice: &[F], exp_factor: F, decode: impl Fn(F::Int) -> F) -> bool {
  slice.iter().all(|&v| {
    let enc = v.fast_round_to_int(exp_factor);
    decode(enc).is_exact_same(v)
  })
}

/// Quickly validates whether cached parameters still apply to the new sample.
/// 检查并快筛缓存参数是否继续适用于新样本
#[inline]
pub(crate) fn validate_cached_params<F: AlpFloat>(params: BestParams, sample: &[F]) -> bool {
  let exp_factor = F::exp_factor(params.exp, params.fac);
  let fac_int = F::fac_int(params.fac);
  let frac_exp = F::frac_exp(params.exp);
  let check_n = sample.len().min(CACHE_VALIDATE_SAMPLE_N);
  let check_slice = &sample[..check_n];

  if params.use_div {
    check_roundtrip(check_slice, exp_factor, |enc| {
      F::decode_from_int_div(enc, exp_factor)
    })
  } else if fac_int == 1 {
    check_roundtrip(check_slice, exp_factor, |enc| {
      F::decode_from_int_fac1(enc, frac_exp)
    })
  } else {
    check_roundtrip(check_slice, exp_factor, |enc| {
      F::decode_from_int(enc, fac_int, frac_exp)
    })
  }
}

/// Writes uncompressed raw float slice into destination buffer as fallback.
/// 写入未压缩的原始浮点切片至目标缓冲区作为保底回退
#[inline]
fn write_raw_fallback<F: AlpFloat>(slice: &[F], count: usize, dst: &mut Vec<u8>) {
  let raw_len = size_of_val(slice);
  let raw_hdr = raw_header_len(count);
  dst.reserve(raw_hdr + raw_len);
  write_header(F::TYPE_RAW_BYTE, count, None, dst);
  // SAFETY: slice is valid continuous float memory, safely cast to raw byte sequence
  // SAFETY: slice 是有效且连续的浮点内存切片，转换为底层紧凑字节序列安全无误
  let raw_slice = unsafe { from_raw_parts(slice.as_ptr().cast::<u8>(), raw_len) };
  dst.extend_from_slice(raw_slice);
}

#[inline(always)]
unsafe fn encode_pass<F: AlpFloat>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  params: BestParams,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  let exp_factor = F::exp_factor(params.exp, params.fac);
  let fac_int = F::fac_int(params.fac);
  let frac_exp = F::frac_exp(params.exp);
  unsafe {
    encode_slice(
      slice,
      enc_ptr,
      exp_factor,
      fac_int,
      frac_exp,
      params.use_div,
      exceptions,
    )
  }
}

/// Applies outlier pruning for a specific target bit-width (DRY helper).
/// 根据指定目标位宽应用离群值剪枝并更新异常字典
#[inline(always)]
fn apply_target_bw<F: AlpFloat>(
  slice: &[F],
  encoded_ints: &mut [F::Int],
  base: F::Int,
  target_bw: u8,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) {
  let max_allowed = if target_bw == 0 {
    0u64
  } else {
    (1u64 << target_bw) - 1
  };
  let had_prev = !exceptions.is_empty();
  for (pos, (&v, val_mut)) in slice.iter().zip(encoded_ints.iter_mut()).enumerate() {
    let diff = F::int_diff_to_u64(*val_mut, base);
    if diff > max_allowed {
      exceptions.push(Exception {
        pos,
        bits: v.to_raw_bits(),
      });
      *val_mut = base;
    }
  }
  if had_prev && exceptions.len() > 1 {
    exceptions.sort_unstable_by_key(|e| e.pos);
    exceptions.dedup_by_key(|e| e.pos);
  }
}

/// Core compression engine: parameter probing, unrolled encoding, outlier pruning, FOR/Delta scheduling, and RAW fallback.
/// 核心压缩引擎：执行参数快筛、编码展开、离群值剪枝、FOR/Delta 调度与 RAW 回退保底
#[allow(clippy::too_many_arguments)]
pub(crate) fn compress_into_engine<F: AlpFloat>(
  slice: &[F],
  dst: &mut Vec<u8>,
  force_delta: bool,
  cached_params: Option<BestParams>,
  cached_target_bw: &mut CachedTargetBw,
  cached_use_delta: &mut Option<bool>,
  encoded_buf: &mut Vec<F::Int>,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> Option<BestParams> {
  let count = slice.len();
  if count == 0 {
    let raw_hdr = raw_header_len(0);
    dst.reserve(raw_hdr);
    write_header(F::TYPE_BYTE, 0, None, dst);
    return None;
  }

  // Identical sequence detection: check first two elements then full slice (O(1) fast exit)
  // 全等序列检测：先探测第 1 个与第 0 个元素是否相同（若不同在单指令周期内快速短路）
  let first = slice[0];
  if (count == 1 || slice[1].is_exact_same(first))
    && let Some((exp, base)) = find_identical_base(first)
    && slice[1..].iter().all(|&v| v.is_exact_same(first))
  {
    let total_needed = header_len(count) + F::BASE_SIZE;
    dst.reserve(total_needed);
    let params = AlpParams::new(exp, 0, 0, false);
    write_header(F::TYPE_BYTE, count, Some(params.pack()), dst);
    F::write_base(base, dst);
    return Some(BestParams {
      exp,
      fac: 0,
      use_div: false,
    });
  }

  // 1. Parameter selection: prefer validated cached params, else run full sampling
  // 1. 参数选择：优先使用验证通过的缓存参数，否则全量采样
  let mut best_params = match cached_params {
    Some(p) if validate_cached_params(p, slice) => p,
    _ => find_best_params(slice),
  };

  // Prepare work buffer using MaybeUninit to eliminate 8KB stack zeroing overhead
  // 准备内部工作缓冲区（采用 MaybeUninit 避免 8KB 栈内存重复清零开销）
  let mut stack_encoded = MaybeUninit::<[F::Int; STACK_BUFFER_CAPACITY]>::uninit();
  exceptions.clear();
  encoded_buf.clear();

  let use_stack = count <= STACK_BUFFER_CAPACITY && encoded_buf.capacity() < count;
  let enc_ptr: *mut F::Int = if use_stack {
    stack_encoded.as_mut_ptr().cast::<F::Int>()
  } else {
    if encoded_buf.capacity() < count {
      encoded_buf.reserve(count);
    }
    encoded_buf.as_mut_ptr()
  };

  // 2. Execute main encoding kernel
  // 2. 主编码内核执行
  let (mut min_val, mut max_val) = unsafe { encode_pass(slice, enc_ptr, best_params, exceptions) };

  // 3. Cache salvage: resample if cached params produced too many exceptions
  // 3. 缓存失效挽救机制：若使用了缓存参数但异常 > MAX_EXCEPTIONS，重新采样尝试挽救
  if exceptions.len() > MAX_EXCEPTIONS && cached_params.is_some() {
    let fresh_params = find_best_params(slice);
    if fresh_params != best_params {
      exceptions.clear();
      let (fresh_min, fresh_max) = unsafe { encode_pass(slice, enc_ptr, fresh_params, exceptions) };
      if exceptions.len() <= MAX_EXCEPTIONS {
        best_params = fresh_params;
        min_val = fresh_min;
        max_val = fresh_max;
      }
    }
  }

  // 4. Incompressible fallback to RAW mode when exceptions exceed threshold
  // 4. 不可压缩回退 RAW 模式（异常 > MAX_EXCEPTIONS）
  if exceptions.len() > MAX_EXCEPTIONS {
    write_raw_fallback(slice, count, dst);
    return None;
  }

  let encoded_ints: &mut [F::Int] = if use_stack {
    // SAFETY: encode_pass has fully initialized valid integers in 0..count
    // SAFETY: encode_pass 已在 0..count 范围内完整写入有效整数
    unsafe { from_raw_parts_mut(stack_encoded.as_mut_ptr().cast::<F::Int>(), count) }
  } else {
    // SAFETY: encode_slice has fully initialized valid integers in 0..count
    // SAFETY: encode_slice 已在 0..count 范围内完整写入有效整数
    unsafe { encoded_buf.set_len(count) };
    &mut encoded_buf[..count]
  };

  let (base, max_offset) = if min_val <= max_val {
    (min_val, F::calc_range(min_val, max_val))
  } else {
    (F::ZERO_INT, 0)
  };

  let is_large = count > u16::MAX as usize;
  let mut for_bit_width = F::bits_needed(max_offset);
  let mut did_pre_prune = false;

  // 5. Outlier pre-pruning: narrow bit-width and eliminate isolated spikes
  // 5. 离群值预剪枝：若位宽较高先尝试剪枝收窄位宽并消除尖峰
  if for_bit_width >= HIGH_BW_PRUNE_THRESHOLD && exceptions.len() < MAX_EXCEPTIONS {
    did_pre_prune = true;
    match *cached_target_bw {
      CachedTargetBw::Pruned(target_bw) if target_bw < for_bit_width => {
        apply_target_bw(slice, encoded_ints, base, target_bw, exceptions);
        for_bit_width = target_bw;
      }
      CachedTargetBw::Disabled | CachedTargetBw::Pruned(_) => {}
      CachedTargetBw::Uninit => {
        let pruned_bw = try_prune_outliers::<F>(
          slice,
          encoded_ints,
          base,
          for_bit_width,
          exceptions,
          is_large,
        );
        if pruned_bw < for_bit_width {
          *cached_target_bw = CachedTargetBw::Pruned(pruned_bw);
          for_bit_width = pruned_bw;
        } else {
          *cached_target_bw = CachedTargetBw::Disabled;
        }
      }
    }
  }

  // Exception backfill: patch with predecessor value to eliminate delta cliffs
  // 异常值回填：填充前一个有效整型值，消除相邻一阶差分的跳变
  if !exceptions.is_empty() {
    for exc in exceptions.iter() {
      let patch_val = if exc.pos > 0 {
        // SAFETY: exc.pos > 0 and strictly less than encoded_ints.len()
        // SAFETY: exc.pos > 0 且严格小于 encoded_ints.len()
        unsafe { *encoded_ints.get_unchecked(exc.pos - 1) }
      } else {
        base
      };
      // SAFETY: exc.pos is strictly less than encoded_ints.len()
      // SAFETY: exc.pos 严格小于 encoded_ints.len()
      unsafe {
        *encoded_ints.get_unchecked_mut(exc.pos) = patch_val;
      }
    }
  }

  let mut for_packed_len = packed_byte_size(count, for_bit_width);
  let mut exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
  let hdr_len = header_len(count);
  let for_total = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;

  // 6. Evaluate Delta benefit (threshold relaxed to >= DELTA_EVAL_MIN_BW for smooth sequences)
  // 6. 评估 Delta 差分收益 (门限放宽至 >= DELTA_EVAL_MIN_BW，平滑数据和线性斜坡可压缩至 0~3 位)
  let delta_decision = if count > 1 {
    if force_delta {
      let first = encoded_ints[0];
      let rest = &encoded_ints[1..];
      Some(delta_range::<F>(first, rest))
    } else {
      match *cached_use_delta {
        Some(false) => None,
        Some(true) => {
          let first = encoded_ints[0];
          let rest = &encoded_ints[1..];
          Some(delta_range::<F>(first, rest))
        }
        None => {
          if for_bit_width >= DELTA_EVAL_MIN_BW {
            let first = encoded_ints[0];
            let rest = &encoded_ints[1..];
            eval_delta_benefit::<F>(first, rest, for_bit_width)
          } else {
            None
          }
        }
      }
    }
  } else {
    None
  };

  let (use_delta, min_delta, delta_bit_width, mut total_needed) = match delta_decision {
    Some((min_d, delta_bw)) => {
      let delta_packed_len = packed_byte_size(count - 1, delta_bw);
      let delta_total = hdr_len + F::BASE_SIZE * 2 + delta_packed_len + exc_len;
      if delta_total < for_total || force_delta {
        (true, min_d, delta_bw, delta_total)
      } else {
        (false, F::ZERO_INT, 0, for_total)
      }
    }
    None => (false, F::ZERO_INT, 0, for_total),
  };

  if !force_delta && cached_use_delta.is_none() {
    *cached_use_delta = Some(use_delta);
  }

  // 7. Low bit-width outlier pruning for FOR mode (when Delta was not selected)
  // 7. FOR 模式低位宽离群值剪枝 (针对未进前置剪枝且未进 Delta 的情况，如 8/12 位剪枝)
  if !did_pre_prune
    && !use_delta
    && for_bit_width > LOW_BW_PRUNE_MIN
    && for_bit_width < HIGH_BW_PRUNE_THRESHOLD
  {
    match *cached_target_bw {
      CachedTargetBw::Pruned(target_bw) if target_bw < for_bit_width => {
        apply_target_bw(slice, encoded_ints, base, target_bw, exceptions);
        for_bit_width = target_bw;
        for_packed_len = packed_byte_size(count, for_bit_width);
        exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
        total_needed = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;
      }
      CachedTargetBw::Disabled | CachedTargetBw::Pruned(_) => {}
      CachedTargetBw::Uninit => {
        let new_bw = try_prune_outliers::<F>(
          slice,
          encoded_ints,
          base,
          for_bit_width,
          exceptions,
          is_large,
        );
        if new_bw < for_bit_width {
          *cached_target_bw = CachedTargetBw::Pruned(new_bw);
          for_bit_width = new_bw;
          for_packed_len = packed_byte_size(count, for_bit_width);
          exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
        } else {
          *cached_target_bw = CachedTargetBw::Disabled;
        }
        total_needed = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;
      }
    }
  }

  // 8. Fallback to RAW mode if compressed size exceeds raw data
  // 8. 负压缩保底回退 RAW 模式
  let raw_len = size_of_val(slice);
  let raw_hdr = raw_header_len(count);
  if total_needed >= raw_len + raw_hdr {
    write_raw_fallback(slice, count, dst);
    return None;
  }

  // 9. Write final encoded bitstream
  // 9. 最终输出写入
  dst.reserve(total_needed);
  if use_delta {
    let params = AlpParams::from_best_params(best_params, delta_bit_width);
    encode_delta::<F>(params, encoded_ints, min_delta, exceptions, dst);
  } else {
    let params = AlpParams::from_best_params(best_params, for_bit_width);
    encode_standard::<F>(params, encoded_ints, base, exceptions, dst);
  }

  Some(best_params)
}

#[doc(hidden)]
pub fn profile_compress_breakdown<F: AlpFloat>(slice: &[F]) {
  use std::time::Instant;
  let count = slice.len();
  let best_params = find_best_params(slice);
  let mut encoded_buf = vec![F::Int::default(); count];
  let enc_ptr = encoded_buf.as_mut_ptr();
  let mut exceptions = Vec::new();

  let iters = 10000;

  // 1. encode_pass
  let start = Instant::now();
  let mut min_val = F::MAX_INT;
  let mut max_val = F::MIN_INT;
  for _ in 0..iters {
    exceptions.clear();
    let (mn, mx) = unsafe { encode_pass(slice, enc_ptr, best_params, &mut exceptions) };
    min_val = mn;
    max_val = mx;
  }
  let t_enc = start.elapsed().as_nanos() as f64 / iters as f64;

  let (base, max_offset) = if min_val <= max_val {
    (min_val, F::calc_range(min_val, max_val))
  } else {
    (F::ZERO_INT, 0)
  };
  let is_large = count > u16::MAX as usize;
  let for_bit_width = F::bits_needed(max_offset);

  // 2. try_prune_outliers
  let start = Instant::now();
  for _ in 0..iters {
    let mut exc_copy = exceptions.clone();
    let _ = try_prune_outliers::<F>(
      slice,
      &mut encoded_buf,
      base,
      for_bit_width,
      &mut exc_copy,
      is_large,
    );
  }
  let t_prune = start.elapsed().as_nanos() as f64 / iters as f64;

  // 3. eval_delta_benefit
  let start = Instant::now();
  let first = encoded_buf[0];
  let rest = &encoded_buf[1..];
  for _ in 0..iters {
    let _ = eval_delta_benefit::<F>(first, rest, for_bit_width);
  }
  let t_delta = start.elapsed().as_nanos() as f64 / iters as f64;

  // 4. encode_standard (bitpack)
  let mut dst = Vec::with_capacity(count * 8 + 64);
  let params = AlpParams::from_best_params(best_params, for_bit_width);
  let start = Instant::now();
  for _ in 0..iters {
    dst.clear();
    encode_standard::<F>(params, &encoded_buf, base, &exceptions, &mut dst);
  }
  let t_pack = start.elapsed().as_nanos() as f64 / iters as f64;

  println!(
    "  Breakdown: enc={:5.1} ns | prune={:5.1} ns | delta={:5.1} ns | pack={:5.1} ns (bw={}, exc={})",
    t_enc,
    t_prune,
    t_delta,
    t_pack,
    for_bit_width,
    exceptions.len()
  );
}
