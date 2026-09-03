use crate::{
  constants::{EXC_COUNT_LEN, EXC_COUNT_LEN_U32},
  float::AlpFloat,
};

/// Default capacity for exceptions vector to avoid heap reallocation on typical outlier count.
/// 异常值向量默认预分配容量
pub const DEFAULT_EXCEPTIONS_CAP: usize = 16;

/// Single exception value record.
/// 单个异常值记录
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exception<R> {
  pub pos: usize,
  pub bits: R,
}

/// Calculates total byte length of encoded exceptions table.
/// 计算异常值表编码后占用的总字节数
#[inline(always)]
pub fn exceptions_byte_size<F: AlpFloat>(exc_count: usize, is_large: bool) -> usize {
  if exc_count == 0 {
    0
  } else if is_large {
    EXC_COUNT_LEN_U32 + exc_count * F::EXC_ENTRY_SIZE_U32
  } else {
    EXC_COUNT_LEN + exc_count * F::EXC_ENTRY_SIZE
  }
}

/// Encodes exceptions table into dst buffer.
/// 统一编码异常值字典至目标缓冲区（自适应兼容普通 u16 与超大数组 u32 索引）
#[inline(always)]
pub fn write_exceptions<F: AlpFloat>(
  count: usize,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  if exceptions.is_empty() {
    return;
  }
  let is_large = count > u16::MAX as usize;
  let needed = exceptions_byte_size::<F>(exceptions.len(), is_large);
  dst.reserve(needed);

  if is_large {
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
