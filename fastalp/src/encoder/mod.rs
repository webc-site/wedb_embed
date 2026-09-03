mod delta;
mod standard;

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

/// Generic floating-point compression writing directly into `dst` buffer.
/// 通用压缩浮点数组并直接写入 `dst` 缓冲区（自适应选择 FOR 或 Delta 差分模式）
pub fn compress_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
  compress_impl(data, dst, false);
}

/// Floating-point compression with enforced Delta differential encoding.
/// 强制使用 Delta 一阶差分模式压缩浮点数组并直接写入 `dst` 缓冲区
pub fn compress_delta_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
  compress_impl(data, dst, true);
}

fn compress_impl<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>, force_delta: bool) {
  let count = data.len();
  if count == 0 {
    let raw_hdr = raw_header_len(0);
    dst.reserve(raw_hdr);
    write_header(F::TYPE_BYTE, 0, None, dst);
    return;
  }

  let slice = data;

  // 极速全等序列检测：如果所有浮点数完全相同（比特级无损判等），直接写入基准值与 bit_width=0，零堆分配
  let first = slice[0];
  if slice.iter().all(|&v| v.is_exact_same(first))
    && let Some((exp, base)) = find_identical_base(first)
  {
    let total_needed = header_len(count) + F::BASE_SIZE;
    dst.reserve(total_needed);
    let packed_params = pack_params(exp, 0, 0);
    write_header(F::TYPE_BYTE, count, Some(packed_params), dst);
    F::write_base(base, dst);
    return;
  }

  let BestParams { exp, fac, use_div } = find_best_params(slice);

  let exp_factor = F::exp_factor(exp, fac);
  let fac_int = F::fac_int(fac);
  let frac_exp = F::frac_exp(exp);

  let mut encoded_ints: Vec<F::Int> = Vec::with_capacity(slice.len());
  let mut exceptions = Vec::new();

  // SAFETY: encoded_ints 已分配 slice.len() 个插槽，encode_loop 通过指针直接写入，最后 set_len 安全更新长度
  let (min_val, max_val) = unsafe {
    let enc_ptr: *mut F::Int = encoded_ints.as_mut_ptr();
    let bounds = if use_div {
      encode_loop(
        slice,
        enc_ptr,
        |v| v.try_encode_div(exp_factor),
        &mut exceptions,
      )
    } else if fac_int == 1 {
      encode_loop(
        slice,
        enc_ptr,
        |v| {
          let enc = v.fast_round_to_int(exp_factor);
          let decoded = F::decode_from_int(enc, 1, frac_exp);
          if decoded.to_raw_bits() == v.to_raw_bits() {
            Some(enc)
          } else {
            None
          }
        },
        &mut exceptions,
      )
    } else {
      encode_loop(
        slice,
        enc_ptr,
        |v| v.try_encode_fast(exp_factor, fac_int, frac_exp),
        &mut exceptions,
      )
    };
    encoded_ints.set_len(slice.len());
    bounds
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

  // 评估 Delta 差分收益（基于未经污染的平滑序列）
  let delta_decision = if slice.len() > 1 {
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
  // 仅在未启用 Delta 模式时尝试剪枝，避免破坏一阶差分的时间平滑性。
  // 当绝大多数数据集中在极窄区间，仅少数离群点拉大整个数组位宽时，
  // 将离群点剥离至异常表可换取全量位打包大幅精简。
  let total_needed = if !use_delta && for_bit_width > 4 && exceptions.len() < 32 {
    let entry_size = if is_large {
      F::EXC_ENTRY_SIZE_U32
    } else {
      F::EXC_ENTRY_SIZE
    };
    let current_cost = for_packed_len + exceptions.len() * entry_size;

    let candidate_widths = [0u8, 1, 2, 4, 8, 16];
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

      let mut extra_exceptions = 0usize;
      for &val in &encoded_ints {
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
      let max_allowed = if best_target_bw == 0 {
        0u64
      } else {
        (1u64 << best_target_bw) - 1
      };
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
    return;
  }

  dst.reserve(total_needed);

  if use_delta {
    encode_delta::<F>(
      count,
      exp,
      fac,
      use_div,
      &mut encoded_ints,
      min_delta,
      delta_bit_width,
      &exceptions,
      dst,
    );
  } else {
    encode_standard::<F>(
      count,
      exp,
      fac,
      use_div,
      &encoded_ints,
      base,
      for_bit_width,
      &exceptions,
      dst,
    );
  }
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

/// Encodes slice using a given encoding function, tracking min/max values and pushing exceptions.
/// 单遍执行浮点数整型转换与极值追踪，统一异常收集（泛型闭包完全内联，零额外开销）
#[inline(always)]
unsafe fn encode_loop<F: AlpFloat, E: Fn(F) -> Option<F::Int>>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  encode_fn: E,
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

      let e0 = encode_fn(v0);
      let e1 = encode_fn(v1);
      let e2 = encode_fn(v2);
      let e3 = encode_fn(v3);

      macro_rules! handle_elem {
        ($idx:expr, $val:expr, $enc:expr) => {
          if let Some(enc) = $enc {
            enc_ptr.add($idx).write(enc);
            min_val = min_val.min(enc);
            max_val = max_val.max(enc);
          } else {
            enc_ptr.add($idx).write(F::ZERO_INT);
            exceptions.push(Exception {
              pos: $idx,
              bits: $val.to_raw_bits(),
            });
          }
        };
      }

      handle_elem!(i, v0, e0);
      handle_elem!(i + 1, v1, e1);
      handle_elem!(i + 2, v2, e2);
      handle_elem!(i + 3, v3, e3);

      i += 4;
    }

    while i < len {
      let val = *slice.get_unchecked(i);
      if let Some(enc) = encode_fn(val) {
        enc_ptr.add(i).write(enc);
        min_val = min_val.min(enc);
        max_val = max_val.max(enc);
      } else {
        enc_ptr.add(i).write(F::ZERO_INT);
        exceptions.push(Exception {
          pos: i,
          bits: val.to_raw_bits(),
        });
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
