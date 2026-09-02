use std::ptr::copy_nonoverlapping;

use crate::{
  bitpack::{bitunpack_into, packed_byte_size},
  constants::{
    EXC_COUNT_LEN, HDR_COUNT_START, HDR_PARAMS_START, HDR_TYPE_IDX, HEADER_LEN, MIN_HEADER_LEN,
  },
  error::{Error, Result},
  float::AlpFloat,
  params::unpack_params,
};

/// Generic floating-point decompression into `dst` buffer.
/// 通用解压浮点数组至 `dst` 缓冲区
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

  if type_byte != F::TYPE_BYTE {
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

  let mut cursor = HEADER_LEN;

  if src.len() < cursor + F::BASE_SIZE {
    return Err(Error::UnexpectedEof {
      needed: cursor + F::BASE_SIZE,
      available: src.len(),
    });
  }
  let base = F::read_base(&src[cursor..cursor + F::BASE_SIZE]);
  cursor += F::BASE_SIZE;

  let packed_len = packed_byte_size(count, bit_width);
  if src.len() < cursor + packed_len {
    return Err(Error::UnexpectedEof {
      needed: cursor + packed_len,
      available: src.len(),
    });
  }

  let start_idx = dst.len();
  let fac_int = F::fac_int(fac);
  let frac_flt = F::frac_exp(exp);

  bitunpack_into(
    &src[cursor..cursor + packed_len],
    count,
    bit_width,
    base,
    fac_int,
    frac_flt,
    dst,
  )?;
  cursor += packed_len;

  if cursor == src.len() {
    return Ok(());
  }

  if src.len() < cursor + EXC_COUNT_LEN {
    return Err(Error::UnexpectedEof {
      needed: cursor + EXC_COUNT_LEN,
      available: src.len(),
    });
  }

  let exc_count = u16::from_le_bytes([src[cursor], src[cursor + 1]]) as usize;
  cursor += EXC_COUNT_LEN;

  let exc_bytes_needed = exc_count * F::EXC_ENTRY_SIZE;
  if src.len() < cursor + exc_bytes_needed {
    return Err(Error::UnexpectedEof {
      needed: cursor + exc_bytes_needed,
      available: src.len(),
    });
  }

  for _ in 0..exc_count {
    let (pos, val) = F::read_exception(&src[cursor..cursor + F::EXC_ENTRY_SIZE]);
    cursor += F::EXC_ENTRY_SIZE;

    if pos >= count {
      return Err(Error::CorruptedData { index: pos, count });
    }
    // SAFETY: bitunpack_into 已经向 dst 写入了 count 个元素，因此 dst.len() 此时等于 start_idx + count。上方已校验 pos < count，因此 start_idx + pos 严格小于 dst.len()。
    unsafe {
      *dst.get_unchecked_mut(start_idx + pos) = val;
    }
  }

  Ok(())
}

/// Generic floating-point slice decompression.
/// 通用解压浮点数切片
#[inline]
pub fn decompress<F: AlpFloat>(src: &[u8]) -> Result<Vec<F>> {
  let mut dst = Vec::new();
  decompress_into(src, &mut dst)?;
  Ok(dst)
}
