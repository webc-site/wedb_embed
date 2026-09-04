/// 根据 AlpParams 参数动态派发具体的解码器类型（零运行时虚函数开销，单次单态化展开）
macro_rules! dispatch_decoder {
  ($params:expr, $base:expr, $F:ty, $decoder:ident => $body:expr) => {{
    let (exp_factor, fac_int, frac_flt) = $params.factors::<$F>();
    if $params.use_div {
      let $decoder = $crate::bitpack::AlpDivDecoder {
        base: $base,
        exp_factor,
      };
      $body
    } else if fac_int == 1 {
      let $decoder = $crate::bitpack::AlpFac1Decoder {
        base: $base,
        frac_flt,
      };
      $body
    } else {
      let $decoder = $crate::bitpack::AlpMulDecoder {
        base: $base,
        fac_int,
        frac_flt,
      };
      $body
    }
  }};
}

mod delta;
mod standard;

use core::ptr::copy_nonoverlapping;
use std::ptr::read_unaligned;

pub use delta::{decode_delta, decode_delta_raw, decode_delta_slice};
pub use standard::{decode_standard, decode_standard_raw, decode_standard_slice};

use crate::{
  constants::{EXC_COUNT_LEN, EXC_COUNT_LEN_U32},
  error::{Error, Result},
  float::AlpFloat,
  header::{ParsedHeader, read_header},
};

/// Generic floating-point decompression directly into raw pointer memory.
/// 通用解压浮点数组至裸指针内存（零堆分配、零内存拷贝，避免未初始化切片构造）
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `dst_cap` 个连续可写 `F` 元素的有效内存。
pub unsafe fn decompress_into_raw<F: AlpFloat>(
  src: &[u8],
  dst_ptr: *mut F,
  dst_cap: usize,
) -> Result<usize> {
  let ParsedHeader {
    type_byte,
    count,
    params,
    cursor,
    ..
  } = read_header(src)?;

  if count == 0 {
    return Ok(0);
  }

  if dst_cap < count {
    return Err(Error::BufferTooSmall {
      needed: count,
      available: dst_cap,
    });
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
    // SAFETY: 上方已检验可用字节充足且 dst_cap >= count
    unsafe {
      copy_nonoverlapping(
        src.as_ptr().add(cursor),
        dst_ptr.cast::<u8>(),
        raw_bytes_needed,
      );
    }
    return Ok(count);
  }

  let is_delta = type_byte == F::TYPE_DELTA_BYTE || type_byte == F::TYPE_DEC_DELTA_BYTE;
  let is_standard = type_byte == F::TYPE_BYTE || type_byte == F::TYPE_DEC_BYTE;

  if !is_standard && !is_delta {
    return Err(Error::InvalidHeader);
  }

  let alp_params = match params {
    Some(p) => p,
    None => return Err(Error::InvalidHeader),
  };

  if !alp_params.validate::<F>() {
    return Err(Error::UnsupportedParams {
      exp: alp_params.exp,
      fac: alp_params.fac,
      bit_width: alp_params.bit_width,
    });
  }

  let payload = &src[cursor..];
  if is_delta {
    unsafe {
      decode_delta_raw::<F>(payload, count, alp_params, dst_ptr)?;
    }
  } else {
    unsafe {
      decode_standard_raw::<F>(payload, count, alp_params, dst_ptr)?;
    }
  }
  Ok(count)
}

/// Generic floating-point decompression into destination slice.
/// 通用解压浮点数组至目标切片（零堆分配、零内存拷贝）
#[inline(always)]
pub fn decompress_into_slice<F: AlpFloat>(src: &[u8], dst: &mut [F]) -> Result<usize> {
  unsafe { decompress_into_raw(src, dst.as_mut_ptr(), dst.len()) }
}

/// Generic floating-point decompression into `dst` buffer.
/// 通用解压浮点数组至 `dst` 缓冲区（自动分发 RAW、标准 FOR 与 Delta 差分块）
pub fn decompress_into<F: AlpFloat>(src: &[u8], dst: &mut Vec<F>) -> Result<()> {
  let ParsedHeader { count, .. } = read_header(src)?;
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst 已预留 count 个空间，decompress_into_raw 直接写入裸指针，
  // 严格初始化 count 个元素后安全更新 Vec 长度。绝不构造未初始化内存的切片引用。
  unsafe {
    let written = decompress_into_raw(src, dst.as_mut_ptr().add(old_len), count)?;
    dst.set_len(old_len + written);
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

/// Patches exceptions into decoded buffer directly using raw pointer.
/// 将异常值字典打补丁至解码缓冲区（统一处理普通 u16 与超大数组 u32 格式，严格校验内存边界）
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个有效 `F` 元素的内存空间。
#[inline]
pub(crate) unsafe fn patch_exceptions<F: AlpFloat>(
  src: &[u8],
  count: usize,
  dst_ptr: *mut F,
) -> Result<()> {
  if src.is_empty() {
    return Ok(());
  }

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

  let exc_slice = &src[cursor..cursor + exc_bytes_needed];
  if is_large {
    for chunk in exc_slice.chunks_exact(entry_size) {
      let (pos, val) = F::read_exception_u32(chunk);
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      // SAFETY: 上方已校验 pos < count，且调用方保证 count 范围内指针有效
      unsafe {
        *dst_ptr.add(pos) = val;
      }
    }
  } else {
    for chunk in exc_slice.chunks_exact(entry_size) {
      let (pos, val) = F::read_exception(chunk);
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      // SAFETY: 上方已校验 pos < count，且调用方保证 count 范围内指针有效
      unsafe {
        *dst_ptr.add(pos) = val;
      }
    }
  }

  Ok(())
}
