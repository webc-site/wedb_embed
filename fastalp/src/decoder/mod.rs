mod delta;
mod standard;

use std::ptr::{copy_nonoverlapping, read_unaligned};

pub use delta::decode_delta;
pub use standard::decode_standard;

use crate::{
  constants::{EXC_COUNT_LEN, EXC_COUNT_LEN_U32},
  error::{Error, Result},
  float::AlpFloat,
  header::{ParsedHeader, read_header},
};

/// Generic floating-point decompression into `dst` buffer.
/// 通用解压浮点数组至 `dst` 缓冲区（自动分发 RAW、标准 FOR 与 Delta 差分块）
pub fn decompress_into<F: AlpFloat>(src: &[u8], dst: &mut Vec<F>) -> Result<()> {
  let ParsedHeader {
    type_byte,
    count,
    params,
    cursor,
    ..
  } = read_header(src)?;

  if count == 0 {
    return Ok(());
  }

  // RAW 原始数据解包路径：直接内存零拷贝恢复
  if type_byte == F::TYPE_RAW_BYTE {
    let raw_bytes_needed = count
      .checked_mul(size_of::<F>())
      .ok_or(Error::InvalidHeader)?;
    if src.len() < cursor + raw_bytes_needed {
      return Err(Error::UnexpectedEof {
        needed: cursor + raw_bytes_needed,
        available: src.len(),
      });
    }
    let old_len = dst.len();
    dst.reserve(count);
    // SAFETY: 上方已检验可用字节充足，直接将连续的原始浮点内存数据拷贝入 dst
    unsafe {
      copy_nonoverlapping(
        src.as_ptr().add(cursor),
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

  let (exp, fac, bit_width) = match params {
    Some(p) => p,
    None => return Err(Error::InvalidHeader),
  };

  if exp > F::MAX_EXPONENT || fac > F::MAX_FAC || fac > exp || bit_width > F::MAX_BIT_WIDTH {
    return Err(Error::UnsupportedParams {
      exp,
      fac,
      bit_width,
    });
  }

  let payload = &src[cursor..];
  if is_delta {
    decode_delta::<F>(payload, count, exp, fac, bit_width, is_dec, dst)
  } else {
    decode_standard::<F>(payload, count, exp, fac, bit_width, is_dec, dst)
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

/// Patches exceptions into decoded slice directly.
/// 将异常值字典打补丁至解码切片（统一处理普通 u16 与超大数组 u32 格式，严格校验内存边界）
#[inline]
pub(crate) fn patch_exceptions<F: AlpFloat>(
  src: &[u8],
  count: usize,
  start_idx: usize,
  dst: &mut [F],
) -> Result<()> {
  if src.is_empty() {
    return Ok(());
  }

  debug_assert!(
    dst.len() >= start_idx + count,
    "destination buffer too small for exceptions"
  );

  let mut cursor = 0;
  let is_large = count > u16::MAX as usize;
  let (exc_count, exc_count_len) = if is_large {
    if src.len() < cursor + EXC_COUNT_LEN_U32 {
      return Err(Error::UnexpectedEof {
        needed: cursor + EXC_COUNT_LEN_U32,
        available: src.len(),
      });
    }
    // SAFETY: 上方已校验 src.len() >= cursor + 4，read_unaligned 安全读取小端 u32
    let c =
      unsafe { u32::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u32>())) } as usize;
    (c, EXC_COUNT_LEN_U32)
  } else {
    if src.len() < cursor + EXC_COUNT_LEN {
      return Err(Error::UnexpectedEof {
        needed: cursor + EXC_COUNT_LEN,
        available: src.len(),
      });
    }
    // SAFETY: 上方已校验 src.len() >= cursor + 2，read_unaligned 安全读取小端 u16
    let c =
      unsafe { u16::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u16>())) } as usize;
    (c, EXC_COUNT_LEN)
  };
  cursor += exc_count_len;

  let entry_size = if is_large {
    F::EXC_ENTRY_SIZE_U32
  } else {
    F::EXC_ENTRY_SIZE
  };
  let exc_bytes_needed = exc_count
    .checked_mul(entry_size)
    .ok_or(Error::InvalidHeader)?;
  if src.len() < cursor + exc_bytes_needed {
    return Err(Error::UnexpectedEof {
      needed: cursor + exc_bytes_needed,
      available: src.len(),
    });
  }

  if is_large {
    for _ in 0..exc_count {
      let (pos, val) = F::read_exception_u32(&src[cursor..cursor + entry_size]);
      cursor += entry_size;
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      // SAFETY: 上方已校验 pos < count，且调用方保证 start_idx + count <= dst.len()
      unsafe {
        *dst.get_unchecked_mut(start_idx + pos) = val;
      }
    }
  } else {
    for _ in 0..exc_count {
      let (pos, val) = F::read_exception(&src[cursor..cursor + entry_size]);
      cursor += entry_size;
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      // SAFETY: 上方已校验 pos < count，且调用方保证 start_idx + count <= dst.len()
      unsafe {
        *dst.get_unchecked_mut(start_idx + pos) = val;
      }
    }
  }

  Ok(())
}
