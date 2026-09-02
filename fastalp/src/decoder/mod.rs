mod delta;
mod standard;

use std::ptr::copy_nonoverlapping;

pub use delta::decode_delta;
pub use standard::decode_standard;

use crate::{
  constants::{HDR_COUNT_START, HDR_PARAMS_START, HDR_TYPE_IDX, HEADER_LEN, MIN_HEADER_LEN},
  error::{Error, Result},
  float::AlpFloat,
  params::unpack_params,
};

/// Generic floating-point decompression into `dst` buffer.
/// 通用解压浮点数组至 `dst` 缓冲区（自动分发 RAW、标准 FOR 与 Delta 差分块）
pub fn decompress_into<F: AlpFloat>(src: &[u8], dst: &mut Vec<F>) -> Result<()> {
  if src.len() < MIN_HEADER_LEN {
    return Err(Error::UnexpectedEof {
      needed: MIN_HEADER_LEN,
      available: src.len(),
    });
  }

  let type_byte = src[HDR_TYPE_IDX];
  let count = u16::from_le_bytes([src[HDR_COUNT_START], src[HDR_COUNT_START + 1]]) as usize;
  if count == 0 {
    return Ok(());
  }

  // 极速 RAW 原始数据解包路径：直接内存零拷贝恢复
  if type_byte == F::TYPE_RAW_BYTE {
    let raw_bytes_needed = count * size_of::<F>();
    if src.len() < MIN_HEADER_LEN + raw_bytes_needed {
      return Err(Error::UnexpectedEof {
        needed: MIN_HEADER_LEN + raw_bytes_needed,
        available: src.len(),
      });
    }
    let old_len = dst.len();
    dst.reserve(count);
    // SAFETY: 上方已检验可用字节充足，直接将连续的原始浮点内存数据拷贝入 dst
    unsafe {
      copy_nonoverlapping(
        src.as_ptr().add(MIN_HEADER_LEN),
        dst.as_mut_ptr().add(old_len).cast::<u8>(),
        raw_bytes_needed,
      );
      dst.set_len(old_len + count);
    }
    return Ok(());
  }

  let is_delta = type_byte == F::TYPE_DELTA_BYTE || type_byte == F::TYPE_DEC_DELTA_BYTE;
  let is_dec = type_byte == F::TYPE_DEC_BYTE || type_byte == F::TYPE_DEC_DELTA_BYTE;
  let is_standard = type_byte == F::TYPE_BYTE || type_byte == F::TYPE_DEC_BYTE;

  if !is_standard && !is_delta {
    return Err(Error::InvalidHeader);
  }

  if src.len() < HEADER_LEN {
    return Err(Error::UnexpectedEof {
      needed: HEADER_LEN,
      available: src.len(),
    });
  }

  let params = u16::from_le_bytes([src[HDR_PARAMS_START], src[HDR_PARAMS_START + 1]]);
  let (exp, fac, bit_width) = unpack_params(params);

  if exp > F::MAX_EXPONENT || fac > F::MAX_FAC || fac > exp || bit_width > F::MAX_BIT_WIDTH {
    return Err(Error::UnsupportedParams {
      exp,
      fac,
      bit_width,
    });
  }

  if is_delta {
    decode_delta::<F>(src, count, exp, fac, bit_width, is_dec, dst)
  } else {
    decode_standard::<F>(src, count, exp, fac, bit_width, is_dec, dst)
  }
}

/// Generic floating-point slice decompression.
/// 通用解压浮点数切片
#[inline]
pub fn decompress<F: AlpFloat>(src: &[u8]) -> Result<Vec<F>> {
  let mut dst = Vec::new();
  decompress_into(src, &mut dst)?;
  Ok(dst)
}
