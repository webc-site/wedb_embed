mod consumer;
mod decoder;
pub(crate) mod kernel;

pub use consumer::{AlpConsumer, AlpDeltaConsumer, ForConsumer, RawU64Consumer};
pub use decoder::{
  AlpDecoder, AlpDictDecoder, AlpDivDecoder, AlpFac1Decoder, AlpMulDecoder, AlpRdConstantDecoder,
};
pub(crate) use kernel::dispatch_bw;

use crate::{
  bitpack::packed_byte_size,
  error::{Error, Result},
  float::AlpFloat,
};

/// Core generic bit-unpacking engine operating on a monomorphized `AlpConsumer`.
///
/// # Safety
///
/// 1. `src` must contain at least `packed_byte_size(count, bit_width)` valid readable bytes.
/// 2. `dst_ptr` must point to valid continuous writable memory for at least `count` elements of type `T`.
/// 3. `bit_width` must be in `0..=64`.
#[inline]
pub unsafe fn bitunpack_core_consumer<T: Copy, C: AlpConsumer<T>>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  mut consumer: C,
  dst_ptr: *mut T,
) {
  if count == 0 {
    return;
  }
  debug_assert!(bit_width <= 64, "bit_width must be in 0..=64");
  if bit_width == 0 {
    unsafe {
      consumer.consume_zeros(count, dst_ptr);
    }
    return;
  }
  unsafe {
    dispatch_bw!(
      bit_width,
      src.as_ptr(),
      count,
      &mut consumer,
      dst_ptr,
      src.len()
    );
  }
}

/// Generic float bit-unpacking engine using `AlpDecoder` wrapped in `ForConsumer`.
///
/// # Safety
///
/// 1. `src` must contain at least `packed_byte_size(count, bit_width)` valid readable bytes.
/// 2. `dst_ptr` must point to valid continuous writable memory for at least `count` `F` elements.
#[inline]
pub unsafe fn bitunpack_core_generic<F: AlpFloat, D: AlpDecoder<F>>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  decoder: D,
  dst_ptr: *mut F,
) {
  unsafe {
    bitunpack_core_consumer(src, count, bit_width, ForConsumer::new(decoder), dst_ptr);
  }
}

/// Direct pointer bit-unpacking: unpacks `count` integers of `bit_width` from `src` directly to `dst_ptr` (zero-heap allocation, zero slice init).
/// 底层直接写入裸指针的解包逻辑（零堆分配、零未初始化切片构造）
///
/// # Safety
///
/// Caller must ensure `dst_ptr` has valid writable memory for at least `count` continuous `u64` elements.
#[inline]
pub(crate) unsafe fn bitunpack_u64_raw(
  src: &[u8],
  count: usize,
  bit_width: u8,
  dst_ptr: *mut u64,
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  if bit_width > 64 {
    return Err(Error::UnsupportedParams {
      exp: 0,
      fac: 0,
      bit_width,
    });
  }

  let required_bytes = packed_byte_size(count, bit_width);
  if src.len() < required_bytes {
    return Err(Error::UnexpectedEof {
      needed: required_bytes,
      available: src.len(),
    });
  }

  unsafe {
    bitunpack_core_consumer(src, count, bit_width, RawU64Consumer, dst_ptr);
  }

  Ok(())
}

/// Fast slice bit-unpacking: unpacks `count` integers of `bit_width` from `src` into `dst` slice (zero-heap allocation).
/// 高速切片位解包：从 `src` 解包出 `count` 个 `bit_width` 位的整数至 `dst` 切片（零堆分配）
pub fn bitunpack_u64_slice(src: &[u8], count: usize, bit_width: u8, dst: &mut [u64]) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  if dst.len() < count {
    return Err(Error::BufferTooSmall {
      needed: count,
      available: dst.len(),
    });
  }
  // SAFETY: dst has at least count elements
  unsafe { bitunpack_u64_raw(src, count, bit_width, dst.as_mut_ptr()) }
}

/// Fast bit-unpacking: unpacks `count` integers of `bit_width` from `src` into `dst` (zero double-init overhead).
/// 高速位解包：从 `src` 解包出 `count` 个 `bit_width` 位的整数至 `dst`（零双重初始化开销）
#[inline]
pub fn bitunpack_u64(src: &[u8], count: usize, bit_width: u8, dst: &mut Vec<u64>) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst has reserved count space, safely unpack into pointer without constructing uninitialized slice
  // SAFETY: dst 已预分配 count 空间，直接写入裸指针，消除未初始化切片构造的 UB 隐患
  unsafe {
    bitunpack_u64_raw(src, count, bit_width, dst.as_mut_ptr().add(old_len))?;
    dst.set_len(old_len + count);
  }
  Ok(())
}
