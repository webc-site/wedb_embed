use core::{
  mem::{MaybeUninit, size_of_val},
  slice::from_raw_parts_mut,
};
use std::slice::from_raw_parts;

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
  },
  float::AlpFloat,
  header::{header_len, raw_header_len, write_header},
  params::AlpParams,
  sampler::{BestParams, find_best_params, find_identical_base},
};

#[inline(always)]
fn check_roundtrip<F: AlpFloat>(slice: &[F], exp_factor: F, decode: impl Fn(F::Int) -> F) -> bool {
  slice.iter().all(|&v| {
    let enc = v.fast_round_to_int(exp_factor);
    decode(enc).is_exact_same(v)
  })
}

/// 检查并快筛缓存参数是否继续适用于新样本
#[inline]
pub(crate) fn validate_cached_params<F: AlpFloat>(params: BestParams, sample: &[F]) -> bool {
  let exp_factor = F::exp_factor(params.exp, params.fac);
  let fac_int = F::fac_int(params.fac);
  let frac_exp = F::frac_exp(params.exp);
  let check_n = sample.len().min(4);
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

/// 写入未压缩的原始浮点切片至目标缓冲区作为保底回退
#[inline]
fn write_raw_fallback<F: AlpFloat>(slice: &[F], count: usize, dst: &mut Vec<u8>) {
  let raw_len = size_of_val(slice);
  let raw_hdr = raw_header_len(count);
  dst.reserve(raw_hdr + raw_len);
  write_header(F::TYPE_RAW_BYTE, count, None, dst);
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

/// 核心压缩引擎：执行参数快筛、编码展开、离群值剪枝、FOR/Delta 调度与 RAW 回退保底
pub(crate) fn compress_into_engine<F: AlpFloat>(
  slice: &[F],
  dst: &mut Vec<u8>,
  force_delta: bool,
  cached_params: Option<BestParams>,
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

  // 全等序列检测：如果所有浮点数完全相同（比特级无损判等），直接写入基准值与 bit_width=0，零堆分配
  let first = slice[0];
  let is_all_identical = slice[1..].iter().all(|&v| v.is_exact_same(first));
  if is_all_identical && let Some((exp, base)) = find_identical_base(first) {
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

  // 1. 参数选择：优先使用验证通过的缓存参数，否则全量采样
  let mut best_params = match cached_params {
    Some(p) if validate_cached_params(p, slice) => p,
    _ => find_best_params(slice),
  };

  // 准备内部工作缓冲区（采用 MaybeUninit 避免 8KB 栈内存重复清零开销）
  let mut stack_encoded = MaybeUninit::<[F::Int; 1024]>::uninit();
  exceptions.clear();
  encoded_buf.clear();

  let use_stack = count <= 1024 && encoded_buf.capacity() < count;
  let enc_ptr: *mut F::Int = if use_stack {
    stack_encoded.as_mut_ptr().cast::<F::Int>()
  } else {
    if encoded_buf.capacity() < count {
      encoded_buf.reserve(count);
    }
    encoded_buf.as_mut_ptr()
  };

  // 2. 主编码内核执行
  let (mut min_val, mut max_val) = unsafe { encode_pass(slice, enc_ptr, best_params, exceptions) };

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

  // 4. 不可压缩回退 RAW 模式（异常 > MAX_EXCEPTIONS）
  if exceptions.len() > MAX_EXCEPTIONS {
    write_raw_fallback(slice, count, dst);
    return None;
  }

  let encoded_ints: &mut [F::Int] = if use_stack {
    // SAFETY: encode_pass 已在 0..count 范围内完整写入有效整数
    unsafe { from_raw_parts_mut(stack_encoded.as_mut_ptr().cast::<F::Int>(), count) }
  } else {
    // SAFETY: encode_slice 已在 0..count 范围内完整写入有效整数
    unsafe { encoded_buf.set_len(count) };
    &mut encoded_buf[..count]
  };

  let (base, max_offset) = if min_val <= max_val {
    (min_val, F::calc_range(min_val, max_val))
  } else {
    (F::ZERO_INT, 0)
  };

  // 5. 异常值回填：填充前一个有效整型值，消除相邻一阶差分的跳变
  if !exceptions.is_empty() {
    for exc in exceptions.iter() {
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
  let mut for_packed_len = packed_byte_size(count, for_bit_width);
  let mut exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
  let hdr_len = header_len(count);
  let for_total = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;

  // 6. 评估 Delta 差分收益
  let delta_decision = if count > 1 && (force_delta || for_bit_width >= 4) {
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

  // 7. FOR 模式离群值剪枝
  if !use_delta {
    let new_bw = try_prune_outliers::<F>(
      slice,
      encoded_ints,
      base,
      for_bit_width,
      exceptions,
      is_large,
    );
    if new_bw < for_bit_width {
      for_bit_width = new_bw;
      for_packed_len = packed_byte_size(count, for_bit_width);
      exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
    }
    total_needed = hdr_len + F::BASE_SIZE + for_packed_len + exc_len;
  }

  // 8. 负压缩保底回退 RAW 模式
  let raw_len = size_of_val(slice);
  let raw_hdr = raw_header_len(count);
  if total_needed >= raw_len + raw_hdr {
    write_raw_fallback(slice, count, dst);
    return None;
  }

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
