use std::slice::from_raw_parts;

use crate::{
  bitpack::{bitpack_encoded, packed_byte_size},
  constants::{EXC_COUNT_LEN, HEADER_LEN, MIN_HEADER_LEN},
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
/// 通用压缩浮点数组并直接写入 `dst` 缓冲区
pub fn compress_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
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

  let BestParams { exp, fac } = find_best_params(slice);

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
    for (i, &val) in slice.iter().enumerate() {
      match F::try_encode_fast(val, exp_factor, fac_int, frac_exp) {
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
      // SAFETY: exc.pos 是在上方遍历 slice (0..slice.len()) 时记录的索引，encoded_ints 的长度与 slice.len() 完全一致，因此 exc.pos as usize 严格小于 encoded_ints.len()，索引安全有效。
      unsafe {
        *encoded_ints.get_unchecked_mut(exc.pos as usize) = base;
      }
    }
  }

  let bit_width = F::bits_needed(max_offset);
  let packed_len = packed_byte_size(slice.len(), bit_width);
  let exc_len = if exceptions.is_empty() {
    0
  } else {
    EXC_COUNT_LEN + exceptions.len() * F::EXC_ENTRY_SIZE
  };
  let total_needed = HEADER_LEN + F::BASE_SIZE + packed_len + exc_len;
  let raw_len = size_of_val(slice);

  // 启用 RAW 模式保底：当 ALP 编码后大小超过原始大小（负压缩）时，直接以 RAW 格式存储
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

  // 1. Header (5B): 1B 类型 + 2B 数量 + 2B 参数 (exp, fac, bit_width)
  let count_bytes = count.to_le_bytes();
  let params_bytes = pack_params(exp, fac, bit_width).to_le_bytes();
  let header = [
    F::TYPE_BYTE,
    count_bytes[0],
    count_bytes[1],
    params_bytes[0],
    params_bytes[1],
  ];
  dst.extend_from_slice(&header);

  // 2. Base
  F::write_base(base, dst);

  // 3. Bitpacked data
  bitpack_encoded::<F>(&encoded_ints, base, bit_width, dst);

  // 4. Exceptions (仅在存在异常值时写入)
  if !exceptions.is_empty() {
    let exc_count = exceptions.len() as u16;
    dst.extend_from_slice(&exc_count.to_le_bytes());
    for exc in exceptions {
      F::write_exception(exc.pos, exc.bits, dst);
    }
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
