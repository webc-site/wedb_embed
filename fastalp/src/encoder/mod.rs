mod delta;
mod standard;

use std::slice::from_raw_parts;

pub use delta::encode_delta;
pub use standard::encode_standard;

use crate::{
  bitpack::packed_byte_size,
  constants::{EXC_COUNT_LEN, HEADER_LEN, MIN_HEADER_LEN},
  delta::{delta_range, eval_delta_benefit},
  float::AlpFloat,
  params::pack_params,
  sampler::{BestParams, find_best_params, find_identical_base},
};

/// Single exception value record.
/// 单个异常值记录
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exception<R> {
  pub pos: u16,
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
  let count = data.len().min(u16::MAX as usize) as u16;
  if count == 0 {
    dst.reserve(MIN_HEADER_LEN);
    let count_bytes = 0u16.to_le_bytes();
    let header = [F::TYPE_BYTE, count_bytes[0], count_bytes[1]];
    dst.extend_from_slice(&header);
    return;
  }

  let slice = &data[..count as usize];

  // 极速全等序列检测：如果所有浮点数完全相同（比特级无损判等），直接写入基准值与 bit_width=0，零堆分配
  let first = slice[0];
  if slice.iter().all(|&v| v.is_exact_same(first))
    && let Some((exp, base)) = find_identical_base(first)
  {
    let total_needed = HEADER_LEN + F::BASE_SIZE;
    dst.reserve(total_needed);
    let count_bytes = count.to_le_bytes();
    let params_bytes = pack_params(exp, 0, 0).to_le_bytes();
    let header = [
      F::TYPE_BYTE,
      count_bytes[0],
      count_bytes[1],
      params_bytes[0],
      params_bytes[1],
    ];
    dst.extend_from_slice(&header);
    F::write_base(base, dst);
    return;
  }

  let BestParams { exp, fac, use_div } = find_best_params(slice);

  let exp_factor = F::exp_factor(exp, fac);
  let fac_int = F::fac_int(fac);
  let frac_exp = F::frac_exp(exp);

  let mut encoded_ints: Vec<F::Int> = Vec::with_capacity(slice.len());
  let mut exceptions = Vec::new();
  let mut min_val = F::MAX_INT;
  let mut max_val = F::MIN_INT;

  // SAFETY: encoded_ints 已分配 slice.len() 个插槽，通过指针直接写入，最后 set_len 安全更新长度
  unsafe {
    let enc_ptr: *mut F::Int = encoded_ints.as_mut_ptr();
    if use_div {
      for (i, &val) in slice.iter().enumerate() {
        if let Some(enc) = val.try_encode_div(exp_factor) {
          enc_ptr.add(i).write(enc);
          min_val = min_val.min(enc);
          max_val = max_val.max(enc);
        } else {
          enc_ptr.add(i).write(F::ZERO_INT);
          exceptions.push(Exception {
            pos: i as u16,
            bits: val.to_raw_bits(),
          });
        }
      }
    } else if fac_int == 1 {
      for (i, &val) in slice.iter().enumerate() {
        let enc = val.fast_round_to_int(exp_factor);
        let decoded = F::decode_from_int(enc, 1, frac_exp);
        if decoded.to_raw_bits() == val.to_raw_bits() {
          enc_ptr.add(i).write(enc);
          min_val = min_val.min(enc);
          max_val = max_val.max(enc);
        } else {
          enc_ptr.add(i).write(F::ZERO_INT);
          exceptions.push(Exception {
            pos: i as u16,
            bits: val.to_raw_bits(),
          });
        }
      }
    } else {
      for (i, &val) in slice.iter().enumerate() {
        match val.try_encode_fast(exp_factor, fac_int, frac_exp) {
          Some(enc) => {
            enc_ptr.add(i).write(enc);
            min_val = min_val.min(enc);
            max_val = max_val.max(enc);
          }
          None => {
            enc_ptr.add(i).write(F::ZERO_INT);
            exceptions.push(Exception {
              pos: i as u16,
              bits: val.to_raw_bits(),
            });
          }
        }
      }
    }
    encoded_ints.set_len(slice.len());
  }

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
        unsafe { *encoded_ints.get_unchecked(exc.pos as usize - 1) }
      } else {
        base
      };
      // SAFETY: exc.pos as usize 严格小于 encoded_ints.len()
      unsafe {
        *encoded_ints.get_unchecked_mut(exc.pos as usize) = patch_val;
      }
    }
  }

  let for_bit_width = F::bits_needed(max_offset);
  let for_packed_len = packed_byte_size(slice.len(), for_bit_width);
  let exc_len = if exceptions.is_empty() {
    0
  } else {
    EXC_COUNT_LEN + exceptions.len() * F::EXC_ENTRY_SIZE
  };

  // 评估 Delta 差分收益
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
      let delta_total = HEADER_LEN + F::BASE_SIZE * 2 + delta_packed_len + exc_len;
      let for_total = HEADER_LEN + F::BASE_SIZE + for_packed_len + exc_len;
      if delta_total < for_total || force_delta {
        (true, min_d, delta_bw, delta_total)
      } else {
        (false, F::ZERO_INT, 0, for_total)
      }
    }
    None => {
      let for_total = HEADER_LEN + F::BASE_SIZE + for_packed_len + exc_len;
      (false, F::ZERO_INT, 0, for_total)
    }
  };

  let raw_len = size_of_val(slice);

  // 启用 RAW 模式保底：当压缩后大小超过原始大小（负压缩）时，直接以 RAW 格式存储
  if total_needed >= raw_len + MIN_HEADER_LEN {
    let total_raw = MIN_HEADER_LEN + raw_len;
    dst.reserve(total_raw);
    let count_bytes = count.to_le_bytes();
    dst.extend_from_slice(&[F::TYPE_RAW_BYTE, count_bytes[0], count_bytes[1]]);
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
