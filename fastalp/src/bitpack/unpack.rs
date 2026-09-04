use core::{ptr::copy_nonoverlapping, slice::from_raw_parts_mut};

use crate::{
  bitpack::packed_byte_size,
  constants::{BYTES_U64, LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT},
  error::{Error, Result},
  float::AlpFloat,
  params::bit_mask,
};

const MASK_1BIT: u8 = 0x01;
const MASK_2BIT: u8 = 0x03;
const MASK_4BIT: u8 = 0x0f;

const BITS_1: u8 = 1;
const BITS_2: u8 = 2;
const BITS_4: u8 = 4;
const BITS_8: u8 = 8;
const BITS_16: u8 = 16;
const BITS_32: u8 = 32;
const BITS_64: u8 = 64;

const CHUNK_8: usize = 8;
const CHUNK_4: usize = 4;
const CHUNK_2: usize = 2;

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
  if bit_width == 0 {
    dst[..count].fill(0);
    return Ok(());
  }

  let required_bytes = packed_byte_size(count, bit_width);
  if src.len() < required_bytes {
    return Err(Error::UnexpectedEof {
      needed: required_bytes,
      available: src.len(),
    });
  }

  // SAFETY:
  // 1. Verified src.len() >= required_bytes above, ensuring pointer is within bounds.
  //    上方已校验 src.len() >= required_bytes，保证读指针与 read_unaligned 严格在 src 有效内存边界内；
  // 2. dst.len() >= count, ensuring sufficient destination space without out-of-bounds risk.
  //    dst.len() >= count，写入 0..count 空间完全充足且无越界风险。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr();

    if bit_width == BITS_1 {
      let full_bytes = count / CHUNK_8;
      for &b in &src[..full_bytes] {
        *dst_ptr.add(0) = (b & MASK_1BIT) as u64;
        *dst_ptr.add(1) = ((b >> 1) & MASK_1BIT) as u64;
        *dst_ptr.add(2) = ((b >> 2) & MASK_1BIT) as u64;
        *dst_ptr.add(3) = ((b >> 3) & MASK_1BIT) as u64;
        *dst_ptr.add(4) = ((b >> 4) & MASK_1BIT) as u64;
        *dst_ptr.add(5) = ((b >> 5) & MASK_1BIT) as u64;
        *dst_ptr.add(6) = ((b >> 6) & MASK_1BIT) as u64;
        *dst_ptr.add(7) = ((b >> 7) & MASK_1BIT) as u64;
        dst_ptr = dst_ptr.add(CHUNK_8);
      }
      let rem = count % CHUNK_8;
      if rem > 0 {
        let b = *src.get_unchecked(full_bytes);
        for shift in 0..rem {
          *dst_ptr = ((b >> shift) & MASK_1BIT) as u64;
          dst_ptr = dst_ptr.add(1);
        }
      }
      return Ok(());
    } else if bit_width == BITS_2 {
      let full_bytes = count / CHUNK_4;
      for &b in &src[..full_bytes] {
        *dst_ptr.add(0) = (b & MASK_2BIT) as u64;
        *dst_ptr.add(1) = ((b >> 2) & MASK_2BIT) as u64;
        *dst_ptr.add(2) = ((b >> 4) & MASK_2BIT) as u64;
        *dst_ptr.add(3) = ((b >> 6) & MASK_2BIT) as u64;
        dst_ptr = dst_ptr.add(CHUNK_4);
      }
      let rem = count % CHUNK_4;
      if rem > 0 {
        let b = *src.get_unchecked(full_bytes);
        for i in 0..rem {
          *dst_ptr = ((b >> (i * 2)) & MASK_2BIT) as u64;
          dst_ptr = dst_ptr.add(1);
        }
      }
      return Ok(());
    } else if bit_width == BITS_4 {
      let full_bytes = count / CHUNK_2;
      let (byte_chunks, byte_rem) = src[..full_bytes].as_chunks::<CHUNK_2>();
      for chunk in byte_chunks {
        let b0 = chunk[0];
        let b1 = chunk[1];
        *dst_ptr.add(0) = (b0 & MASK_4BIT) as u64;
        *dst_ptr.add(1) = (b0 >> 4) as u64;
        *dst_ptr.add(2) = (b1 & MASK_4BIT) as u64;
        *dst_ptr.add(3) = (b1 >> 4) as u64;
        dst_ptr = dst_ptr.add(CHUNK_4);
      }
      for &b in byte_rem {
        *dst_ptr.add(0) = (b & MASK_4BIT) as u64;
        *dst_ptr.add(1) = (b >> 4) as u64;
        dst_ptr = dst_ptr.add(CHUNK_2);
      }
      if !count.is_multiple_of(CHUNK_2) {
        let b = *src.get_unchecked(full_bytes);
        *dst_ptr = (b & MASK_4BIT) as u64;
      }
      return Ok(());
    } else if bit_width == BITS_8 {
      let dst_slice = from_raw_parts_mut(dst_ptr, count);
      for (&b, d) in src[..count].iter().zip(dst_slice.iter_mut()) {
        *d = b as u64;
      }
      return Ok(());
    } else if bit_width == BITS_16 {
      let src_ptr = src.as_ptr().cast::<u16>();
      for i in 0..count {
        *dst_ptr.add(i) = u16::from_le(src_ptr.add(i).read_unaligned()) as u64;
      }
      return Ok(());
    } else if bit_width == BITS_32 {
      let src_ptr = src.as_ptr().cast::<u32>();
      for i in 0..count {
        *dst_ptr.add(i) = u32::from_le(src_ptr.add(i).read_unaligned()) as u64;
      }
      return Ok(());
    } else if bit_width == BITS_64 {
      if cfg!(target_endian = "little") {
        copy_nonoverlapping(src.as_ptr(), dst_ptr.cast::<u8>(), count * BYTES_U64);
      } else {
        let src_ptr = src.as_ptr().cast::<u64>();
        for i in 0..count {
          *dst_ptr.add(i) = u64::from_le(src_ptr.add(i).read_unaligned());
        }
      }
      return Ok(());
    }

    match bit_width {
      3 => unpack_u64_le16::<3>(src.as_ptr(), count, dst_ptr, src.len()),
      5 => unpack_u64_le16::<5>(src.as_ptr(), count, dst_ptr, src.len()),
      6 => unpack_u64_le16::<6>(src.as_ptr(), count, dst_ptr, src.len()),
      7 => unpack_u64_le16::<7>(src.as_ptr(), count, dst_ptr, src.len()),
      9 => unpack_u64_le16::<9>(src.as_ptr(), count, dst_ptr, src.len()),
      10 => unpack_u64_le16::<10>(src.as_ptr(), count, dst_ptr, src.len()),
      11 => unpack_u64_le16::<11>(src.as_ptr(), count, dst_ptr, src.len()),
      12 => unpack_u64_le16::<12>(src.as_ptr(), count, dst_ptr, src.len()),
      13 => unpack_u64_le16::<13>(src.as_ptr(), count, dst_ptr, src.len()),
      14 => unpack_u64_le16::<14>(src.as_ptr(), count, dst_ptr, src.len()),
      15 => unpack_u64_le16::<15>(src.as_ptr(), count, dst_ptr, src.len()),
      17 => unpack_u64_17_to_32::<17>(src.as_ptr(), count, dst_ptr, src.len()),
      18 => unpack_u64_17_to_32::<18>(src.as_ptr(), count, dst_ptr, src.len()),
      19 => unpack_u64_17_to_32::<19>(src.as_ptr(), count, dst_ptr, src.len()),
      20 => unpack_u64_17_to_32::<20>(src.as_ptr(), count, dst_ptr, src.len()),
      21 => unpack_u64_17_to_32::<21>(src.as_ptr(), count, dst_ptr, src.len()),
      22 => unpack_u64_17_to_32::<22>(src.as_ptr(), count, dst_ptr, src.len()),
      23 => unpack_u64_17_to_32::<23>(src.as_ptr(), count, dst_ptr, src.len()),
      24 => unpack_u64_17_to_32::<24>(src.as_ptr(), count, dst_ptr, src.len()),
      25 => unpack_u64_17_to_32::<25>(src.as_ptr(), count, dst_ptr, src.len()),
      26 => unpack_u64_17_to_32::<26>(src.as_ptr(), count, dst_ptr, src.len()),
      27 => unpack_u64_17_to_32::<27>(src.as_ptr(), count, dst_ptr, src.len()),
      28 => unpack_u64_17_to_32::<28>(src.as_ptr(), count, dst_ptr, src.len()),
      29 => unpack_u64_17_to_32::<29>(src.as_ptr(), count, dst_ptr, src.len()),
      30 => unpack_u64_17_to_32::<30>(src.as_ptr(), count, dst_ptr, src.len()),
      31 => unpack_u64_17_to_32::<31>(src.as_ptr(), count, dst_ptr, src.len()),
      33 => unpack_u64_33_to_56::<33>(src.as_ptr(), count, dst_ptr, src.len()),
      42 => unpack_u64_33_to_56::<42>(src.as_ptr(), count, dst_ptr, src.len()),
      bw => unpack_u64_generic(src.as_ptr(), count, dst_ptr, src.len(), bw),
    }
  }

  Ok(())
}

#[inline(always)]
unsafe fn unpack_u64_le16<const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  dst_ptr: *mut u64,
  src_len: usize,
) {
  let mask: u64 = (1u64 << BW) - 1;
  let safe_limit_16 = src_len.saturating_sub(16);
  let max_safe_groups = safe_limit_16 / BW;
  let fast_end_8 = (max_safe_groups * 8).min(count & !7);
  let mut byte_offset = 0;
  let mut i = 0;

  // SAFETY: Caller guarantees src_ptr has at least src_len bytes readable and dst_ptr has count writable elements
  // SAFETY: 调用方保证从 src_ptr 可读取至少 src_len 字节，且 dst_ptr 具备容纳 count 个元素的可写空间
  unsafe {
    while i + 16 <= fast_end_8 {
      let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
      let chunk1 = u128::from_le(
        src_ptr
          .add(byte_offset + BW)
          .cast::<u128>()
          .read_unaligned(),
      );
      let w0 = chunk0 as u64;
      let w1 = (chunk0 >> (BW * 4)) as u64;
      *dst_ptr.add(i) = w0 & mask;
      *dst_ptr.add(i + 1) = (w0 >> BW) & mask;
      *dst_ptr.add(i + 2) = (w0 >> (BW * 2)) & mask;
      *dst_ptr.add(i + 3) = (w0 >> (BW * 3)) & mask;
      *dst_ptr.add(i + 4) = w1 & mask;
      *dst_ptr.add(i + 5) = (w1 >> BW) & mask;
      *dst_ptr.add(i + 6) = (w1 >> (BW * 2)) & mask;
      *dst_ptr.add(i + 7) = (w1 >> (BW * 3)) & mask;

      let w2 = chunk1 as u64;
      let w3 = (chunk1 >> (BW * 4)) as u64;
      *dst_ptr.add(i + 8) = w2 & mask;
      *dst_ptr.add(i + 9) = (w2 >> BW) & mask;
      *dst_ptr.add(i + 10) = (w2 >> (BW * 2)) & mask;
      *dst_ptr.add(i + 11) = (w2 >> (BW * 3)) & mask;
      *dst_ptr.add(i + 12) = w3 & mask;
      *dst_ptr.add(i + 13) = (w3 >> BW) & mask;
      *dst_ptr.add(i + 14) = (w3 >> (BW * 2)) & mask;
      *dst_ptr.add(i + 15) = (w3 >> (BW * 3)) & mask;

      byte_offset += BW * 2;
      i += 16;
    }

    while i + 8 <= fast_end_8 {
      let chunk = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
      let w0 = chunk as u64;
      let w1 = (chunk >> (BW * 4)) as u64;
      *dst_ptr.add(i) = w0 & mask;
      *dst_ptr.add(i + 1) = (w0 >> BW) & mask;
      *dst_ptr.add(i + 2) = (w0 >> (BW * 2)) & mask;
      *dst_ptr.add(i + 3) = (w0 >> (BW * 3)) & mask;
      *dst_ptr.add(i + 4) = w1 & mask;
      *dst_ptr.add(i + 5) = (w1 >> BW) & mask;
      *dst_ptr.add(i + 6) = (w1 >> (BW * 2)) & mask;
      *dst_ptr.add(i + 7) = (w1 >> (BW * 3)) & mask;
      byte_offset += BW;
      i += 8;
    }

    let safe_limit_bytes = src_len.saturating_sub(BYTES_U64);
    while i < count {
      let bit_pos = i * BW;
      let byte_offset = bit_pos >> 3;
      let word = if byte_offset <= safe_limit_bytes {
        u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
      } else {
        let mut buf = [0u8; 8];
        let available = src_len.saturating_sub(byte_offset).min(8);
        if available > 0 {
          copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
        }
        u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
      };
      *dst_ptr.add(i) = (word >> (bit_pos & 7)) & mask;
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_u64_17_to_32<const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  dst_ptr: *mut u64,
  src_len: usize,
) {
  let mask: u64 = (1u64 << BW) - 1;
  let mid_byte: usize = (4 * BW) / 8;
  let mid_shift: usize = (4 * BW) & 7;
  let safe_limit_32 = src_len.saturating_sub(mid_byte + 16);
  let max_safe_groups = safe_limit_32 / BW;
  let fast_end_8 = (max_safe_groups * 8).min(count & !7);
  let mut byte_offset = 0;
  let mut i = 0;

  // SAFETY: Caller guarantees src_ptr has at least src_len bytes readable and dst_ptr has count writable elements
  // SAFETY: 调用方保证从 src_ptr 可读取至少 src_len 字节，且 dst_ptr 具备容纳 count 个元素的可写空间
  unsafe {
    while i + 8 <= fast_end_8 {
      let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
      let chunk1 = u128::from_le(
        src_ptr
          .add(byte_offset + mid_byte)
          .cast::<u128>()
          .read_unaligned(),
      );
      *dst_ptr.add(i) = (chunk0 as u64) & mask;
      *dst_ptr.add(i + 1) = ((chunk0 >> BW) as u64) & mask;
      *dst_ptr.add(i + 2) = ((chunk0 >> (BW * 2)) as u64) & mask;
      *dst_ptr.add(i + 3) = ((chunk0 >> (BW * 3)) as u64) & mask;
      *dst_ptr.add(i + 4) = ((chunk1 >> mid_shift) as u64) & mask;
      *dst_ptr.add(i + 5) = ((chunk1 >> (mid_shift + BW)) as u64) & mask;
      *dst_ptr.add(i + 6) = ((chunk1 >> (mid_shift + BW * 2)) as u64) & mask;
      *dst_ptr.add(i + 7) = ((chunk1 >> (mid_shift + BW * 3)) as u64) & mask;
      byte_offset += BW;
      i += 8;
    }

    let safe_limit_bytes = src_len.saturating_sub(BYTES_U64);
    while i < count {
      let bit_pos = i * BW;
      let byte_offset = bit_pos >> 3;
      let word = if byte_offset <= safe_limit_bytes {
        u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
      } else {
        let mut buf = [0u8; 8];
        let available = src_len.saturating_sub(byte_offset).min(8);
        if available > 0 {
          copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
        }
        u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
      };
      *dst_ptr.add(i) = (word >> (bit_pos & 7)) & mask;
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_u64_33_to_56<const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  dst_ptr: *mut u64,
  src_len: usize,
) {
  let mask: u64 = if BW == 64 { u64::MAX } else { (1u64 << BW) - 1 };
  let safe_limit_16 = src_len.saturating_sub(16);
  let max_safe_i = (safe_limit_16 * 8) / BW;
  let fast_end_4 = max_safe_i.saturating_sub(3).min(count);
  let fast_limit = max_safe_i.min(count);
  let mut i = 0;

  // SAFETY: Caller guarantees src_ptr has at least src_len bytes readable and dst_ptr has count writable elements
  // SAFETY: 调用方保证从 src_ptr 可读取至少 src_len 字节，且 dst_ptr 具备容纳 count 个元素的可写空间
  unsafe {
    while i + 4 <= fast_end_4 {
      let p0 = i * BW;
      let p1 = p0 + BW;
      let p2 = p1 + BW;
      let p3 = p2 + BW;
      let w0 =
        (u128::from_le(src_ptr.add(p0 >> 3).cast::<u128>().read_unaligned()) >> (p0 & 7)) as u64;
      let w1 =
        (u128::from_le(src_ptr.add(p1 >> 3).cast::<u128>().read_unaligned()) >> (p1 & 7)) as u64;
      let w2 =
        (u128::from_le(src_ptr.add(p2 >> 3).cast::<u128>().read_unaligned()) >> (p2 & 7)) as u64;
      let w3 =
        (u128::from_le(src_ptr.add(p3 >> 3).cast::<u128>().read_unaligned()) >> (p3 & 7)) as u64;
      *dst_ptr.add(i) = w0 & mask;
      *dst_ptr.add(i + 1) = w1 & mask;
      *dst_ptr.add(i + 2) = w2 & mask;
      *dst_ptr.add(i + 3) = w3 & mask;
      i += 4;
    }

    while i < fast_limit {
      let p = i * BW;
      let w =
        (u128::from_le(src_ptr.add(p >> 3).cast::<u128>().read_unaligned()) >> (p & 7)) as u64;
      *dst_ptr.add(i) = w & mask;
      i += 1;
    }

    let safe_limit_bytes = src_len.saturating_sub(BYTES_U64);
    while i < count {
      let bit_pos = i * BW;
      let byte_offset = bit_pos >> 3;
      let word = if byte_offset <= safe_limit_bytes {
        u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
      } else {
        let mut buf = [0u8; 8];
        let available = src_len.saturating_sub(byte_offset).min(8);
        if available > 0 {
          copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
        }
        u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
      };
      *dst_ptr.add(i) = (word >> (bit_pos & 7)) & mask;
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_u64_generic(
  src_ptr: *const u8,
  count: usize,
  dst_ptr: *mut u64,
  src_len: usize,
  bit_width: u8,
) {
  let mask = bit_mask(bit_width);
  let bw = bit_width as usize;
  let safe_limit_16 = src_len.saturating_sub(16);
  let max_safe_i = (safe_limit_16 * 8) / bw;
  let fast_end_4 = max_safe_i.saturating_sub(3).min(count);
  let fast_limit = max_safe_i.min(count);
  let mut i = 0;

  // SAFETY: Caller guarantees src_ptr has at least src_len bytes readable and dst_ptr has count writable elements
  // SAFETY: 调用方保证从 src_ptr 可读取至少 src_len 字节，且 dst_ptr 具备容纳 count 个元素的可写空间
  unsafe {
    while i + 4 <= fast_end_4 {
      let p0 = i * bw;
      let p1 = p0 + bw;
      let p2 = p1 + bw;
      let p3 = p2 + bw;
      let w0 =
        (u128::from_le(src_ptr.add(p0 >> 3).cast::<u128>().read_unaligned()) >> (p0 & 7)) as u64;
      let w1 =
        (u128::from_le(src_ptr.add(p1 >> 3).cast::<u128>().read_unaligned()) >> (p1 & 7)) as u64;
      let w2 =
        (u128::from_le(src_ptr.add(p2 >> 3).cast::<u128>().read_unaligned()) >> (p2 & 7)) as u64;
      let w3 =
        (u128::from_le(src_ptr.add(p3 >> 3).cast::<u128>().read_unaligned()) >> (p3 & 7)) as u64;
      *dst_ptr.add(i) = w0 & mask;
      *dst_ptr.add(i + 1) = w1 & mask;
      *dst_ptr.add(i + 2) = w2 & mask;
      *dst_ptr.add(i + 3) = w3 & mask;
      i += 4;
    }

    while i < fast_limit {
      let p = i * bw;
      let w =
        (u128::from_le(src_ptr.add(p >> 3).cast::<u128>().read_unaligned()) >> (p & 7)) as u64;
      *dst_ptr.add(i) = w & mask;
      i += 1;
    }

    let safe_limit_bytes = src_len.saturating_sub(16);
    while i < count {
      let bit_pos = i * bw;
      let byte_offset = bit_pos >> 3;
      let word = if byte_offset <= safe_limit_bytes {
        (u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned()) >> (bit_pos & 7))
          as u64
      } else {
        let mut buf = [0u8; 16];
        let available = src_len.saturating_sub(byte_offset).min(16);
        if available > 0 {
          copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
        }
        (u128::from_le(buf.as_ptr().cast::<u128>().read_unaligned()) >> (bit_pos & 7)) as u64
      };
      *dst_ptr.add(i) = word & mask;
      i += 1;
    }
  }
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
  // SAFETY: dst has reserved count space, safely unpack into slice without double-init overhead
  // SAFETY: dst 已预分配 count 空间，通过切片零堆分配安全解包，消除 resize 双重写零开销
  let slice = unsafe { from_raw_parts_mut(dst.as_mut_ptr().add(old_len), count) };
  bitunpack_u64_slice(src, count, bit_width, slice)?;
  unsafe {
    dst.set_len(old_len + count);
  }
  Ok(())
}

/// Generic trait for ALP floating-point reconstruction decoders.
/// ALP 浮点重构解码器通用抽象 Trait
pub trait AlpDecoder<F: AlpFloat>: Copy {
  /// Reconstructs float from unsigned integer offset
  /// 根据无符号整型偏移量还原浮点数
  fn decode_offset(&self, off: u64) -> F;
  /// Reconstructs float from encoded integer value
  /// 根据已编码整型原值还原浮点数
  fn decode_int(&self, val: F::Int) -> F;
  /// Builds 1-bit decoding lookup table
  /// 构建 1-bit 解码查找表
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT];
  /// Builds 2-bit decoding lookup table
  /// 构建 2-bit 解码查找表
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT];
  /// Builds 4-bit decoding lookup table
  /// 构建 4-bit 解码查找表
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT];
}

/// High-efficiency decoder for factor == 1 (pure multiplication).
/// 针对纯乘法且因子为 1 (fac_int == 1) 的高效解码器
#[derive(Copy, Clone)]
pub struct AlpFac1Decoder<F: AlpFloat> {
  pub base: F::Int,
  pub frac_flt: F,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpFac1Decoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::decode_from_offset_fac1(off, self.base, self.frac_flt)
  }

  #[inline(always)]
  fn decode_int(&self, val: F::Int) -> F {
    F::decode_from_int_fac1(val, self.frac_flt)
  }

  #[inline(always)]
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT] {
    F::build_lut::<LUT_SIZE_1BIT>(self.base, 1, self.frac_flt)
  }

  #[inline(always)]
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT] {
    F::build_lut::<LUT_SIZE_2BIT>(self.base, 1, self.frac_flt)
  }

  #[inline(always)]
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT] {
    F::build_lut::<LUT_SIZE_4BIT>(self.base, 1, self.frac_flt)
  }
}

/// General multiplier decoder for fac_int != 1.
/// 针对带因子乘法 (fac_int != 1) 的通用乘法解码器
#[derive(Copy, Clone)]
pub struct AlpMulDecoder<F: AlpFloat> {
  pub base: F::Int,
  pub fac_int: i64,
  pub frac_flt: F,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpMulDecoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::decode_from_offset(off, self.base, self.fac_int, self.frac_flt)
  }

  #[inline(always)]
  fn decode_int(&self, val: F::Int) -> F {
    F::decode_from_int(val, self.fac_int, self.frac_flt)
  }

  #[inline(always)]
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT] {
    F::build_lut::<LUT_SIZE_1BIT>(self.base, self.fac_int, self.frac_flt)
  }

  #[inline(always)]
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT] {
    F::build_lut::<LUT_SIZE_2BIT>(self.base, self.fac_int, self.frac_flt)
  }

  #[inline(always)]
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT] {
    F::build_lut::<LUT_SIZE_4BIT>(self.base, self.fac_int, self.frac_flt)
  }
}

/// Decimal division decoder for division mode (use_div == true).
/// 针对除法模式 (use_div == true) 的十进制除法解码器
#[derive(Copy, Clone)]
pub struct AlpDivDecoder<F: AlpFloat> {
  pub base: F::Int,
  pub exp_factor: F,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpDivDecoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::decode_from_offset_div(off, self.base, self.exp_factor)
  }

  #[inline(always)]
  fn decode_int(&self, val: F::Int) -> F {
    F::decode_from_int_div(val, self.exp_factor)
  }

  #[inline(always)]
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT] {
    F::build_lut_div::<LUT_SIZE_1BIT>(self.base, self.exp_factor)
  }

  #[inline(always)]
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT] {
    F::build_lut_div::<LUT_SIZE_2BIT>(self.base, self.exp_factor)
  }

  #[inline(always)]
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT] {
    F::build_lut_div::<LUT_SIZE_4BIT>(self.base, self.exp_factor)
  }
}

/// Core generic bit-unpacking and float reconstruction kernel: writes directly to target raw pointer (zero heap allocation, zero abstraction overhead).
/// 核心通用位解包与浮点重构内核：直接写入目标裸指针（零堆分配、零抽象开销）
///
/// # Safety
/// 1. `src` must contain at least `packed_byte_size(count, bit_width)` valid bytes.
///    `src` 必须至少包含 `packed_byte_size(count, bit_width)` 字节有效数据；
/// 2. `dst_ptr` must point to valid memory for at least `count` continuous writable `F` elements.
///    `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
pub unsafe fn bitunpack_core_generic<F: AlpFloat, D: AlpDecoder<F>>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  decoder: D,
  mut dst_ptr: *mut F,
) {
  // SAFETY: Caller guarantees src has at least packed_byte_size bytes, and dst_ptr has capacity for count elements
  // SAFETY: 调用方保证 src 具备至少 packed_byte_size 字节，且 dst_ptr 具备容纳 count 个元素的可写空间
  unsafe {
    if bit_width == BITS_1 {
      let lut = decoder.build_lut_1();
      let full_bytes = count / CHUNK_8;
      for &b in &src[..full_bytes] {
        *dst_ptr.add(0) = *lut.get_unchecked((b & MASK_1BIT) as usize);
        *dst_ptr.add(1) = *lut.get_unchecked(((b >> 1) & MASK_1BIT) as usize);
        *dst_ptr.add(2) = *lut.get_unchecked(((b >> 2) & MASK_1BIT) as usize);
        *dst_ptr.add(3) = *lut.get_unchecked(((b >> 3) & MASK_1BIT) as usize);
        *dst_ptr.add(4) = *lut.get_unchecked(((b >> 4) & MASK_1BIT) as usize);
        *dst_ptr.add(5) = *lut.get_unchecked(((b >> 5) & MASK_1BIT) as usize);
        *dst_ptr.add(6) = *lut.get_unchecked(((b >> 6) & MASK_1BIT) as usize);
        *dst_ptr.add(7) = *lut.get_unchecked(((b >> 7) & MASK_1BIT) as usize);
        dst_ptr = dst_ptr.add(CHUNK_8);
      }
      let rem = count % CHUNK_8;
      if rem > 0 {
        let b = *src.get_unchecked(full_bytes);
        for shift in 0..rem {
          let idx = ((b >> shift) & MASK_1BIT) as usize;
          *dst_ptr = *lut.get_unchecked(idx);
          dst_ptr = dst_ptr.add(1);
        }
      }

      return;
    } else if bit_width == BITS_2 {
      let lut = decoder.build_lut_2();
      let full_bytes = count / CHUNK_4;
      for &b in &src[..full_bytes] {
        *dst_ptr.add(0) = *lut.get_unchecked((b & MASK_2BIT) as usize);
        *dst_ptr.add(1) = *lut.get_unchecked(((b >> 2) & MASK_2BIT) as usize);
        *dst_ptr.add(2) = *lut.get_unchecked(((b >> 4) & MASK_2BIT) as usize);
        *dst_ptr.add(3) = *lut.get_unchecked(((b >> 6) & MASK_2BIT) as usize);
        dst_ptr = dst_ptr.add(CHUNK_4);
      }
      let rem = count % CHUNK_4;
      if rem > 0 {
        let b = *src.get_unchecked(full_bytes);
        for i in 0..rem {
          let idx = ((b >> (i * 2)) & MASK_2BIT) as usize;
          *dst_ptr = *lut.get_unchecked(idx);
          dst_ptr = dst_ptr.add(1);
        }
      }

      return;
    } else if bit_width == BITS_4 {
      let lut = decoder.build_lut_4();
      let full_bytes = count / CHUNK_2;
      let (byte_chunks, byte_rem) = src[..full_bytes].as_chunks::<CHUNK_2>();
      for chunk in byte_chunks {
        let b0 = chunk[0];
        let b1 = chunk[1];
        *dst_ptr.add(0) = *lut.get_unchecked((b0 & MASK_4BIT) as usize);
        *dst_ptr.add(1) = *lut.get_unchecked((b0 >> 4) as usize);
        *dst_ptr.add(2) = *lut.get_unchecked((b1 & MASK_4BIT) as usize);
        *dst_ptr.add(3) = *lut.get_unchecked((b1 >> 4) as usize);
        dst_ptr = dst_ptr.add(CHUNK_4);
      }
      for &b in byte_rem {
        *dst_ptr.add(0) = *lut.get_unchecked((b & MASK_4BIT) as usize);
        *dst_ptr.add(1) = *lut.get_unchecked((b >> 4) as usize);
        dst_ptr = dst_ptr.add(CHUNK_2);
      }
      if !count.is_multiple_of(CHUNK_2) {
        let b = *src.get_unchecked(full_bytes);
        *dst_ptr = *lut.get_unchecked((b & MASK_4BIT) as usize);
      }

      return;
    } else if bit_width == BITS_8 {
      let (chunks, rem) = src[..count].as_chunks::<CHUNK_8>();
      let mut idx = 0;
      for chunk in chunks {
        *dst_ptr.add(idx) = decoder.decode_offset(chunk[0] as u64);
        *dst_ptr.add(idx + 1) = decoder.decode_offset(chunk[1] as u64);
        *dst_ptr.add(idx + 2) = decoder.decode_offset(chunk[2] as u64);
        *dst_ptr.add(idx + 3) = decoder.decode_offset(chunk[3] as u64);
        *dst_ptr.add(idx + 4) = decoder.decode_offset(chunk[4] as u64);
        *dst_ptr.add(idx + 5) = decoder.decode_offset(chunk[5] as u64);
        *dst_ptr.add(idx + 6) = decoder.decode_offset(chunk[6] as u64);
        *dst_ptr.add(idx + 7) = decoder.decode_offset(chunk[7] as u64);
        idx += CHUNK_8;
      }
      for (i, &b) in rem.iter().enumerate() {
        *dst_ptr.add(idx + i) = decoder.decode_offset(b as u64);
      }

      return;
    } else if bit_width == BITS_16 {
      let src_ptr = src.as_ptr().cast::<u16>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i).read_unaligned()) as u64);
        *dst_ptr.add(i + 1) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 1).read_unaligned()) as u64);
        *dst_ptr.add(i + 2) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 2).read_unaligned()) as u64);
        *dst_ptr.add(i + 3) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 3).read_unaligned()) as u64);
        *dst_ptr.add(i + 4) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 4).read_unaligned()) as u64);
        *dst_ptr.add(i + 5) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 5).read_unaligned()) as u64);
        *dst_ptr.add(i + 6) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 6).read_unaligned()) as u64);
        *dst_ptr.add(i + 7) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i + 7).read_unaligned()) as u64);
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u16::from_le(src_ptr.add(i).read_unaligned()) as u64);
        i += 1;
      }

      return;
    } else if bit_width == BITS_32 {
      let src_ptr = src.as_ptr().cast::<u32>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i).read_unaligned()) as u64);
        *dst_ptr.add(i + 1) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 1).read_unaligned()) as u64);
        *dst_ptr.add(i + 2) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 2).read_unaligned()) as u64);
        *dst_ptr.add(i + 3) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 3).read_unaligned()) as u64);
        *dst_ptr.add(i + 4) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 4).read_unaligned()) as u64);
        *dst_ptr.add(i + 5) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 5).read_unaligned()) as u64);
        *dst_ptr.add(i + 6) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 6).read_unaligned()) as u64);
        *dst_ptr.add(i + 7) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i + 7).read_unaligned()) as u64);
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u32::from_le(src_ptr.add(i).read_unaligned()) as u64);
        i += 1;
      }

      return;
    } else if bit_width == BITS_64 {
      let src_ptr = src.as_ptr().cast::<u64>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = decoder.decode_offset(u64::from_le(src_ptr.add(i).read_unaligned()));
        *dst_ptr.add(i + 1) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 1).read_unaligned()));
        *dst_ptr.add(i + 2) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 2).read_unaligned()));
        *dst_ptr.add(i + 3) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 3).read_unaligned()));
        *dst_ptr.add(i + 4) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 4).read_unaligned()));
        *dst_ptr.add(i + 5) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 5).read_unaligned()));
        *dst_ptr.add(i + 6) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 6).read_unaligned()));
        *dst_ptr.add(i + 7) =
          decoder.decode_offset(u64::from_le(src_ptr.add(i + 7).read_unaligned()));
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) = decoder.decode_offset(u64::from_le(src_ptr.add(i).read_unaligned()));
        i += 1;
      }

      return;
    }

    let mask = bit_mask(bit_width);
    let bw = bit_width as usize;
    let src_ptr = src.as_ptr();

    let mut i = 0;
    if bit_width <= 16 {
      let safe_limit_16 = src.len().saturating_sub(16);
      let max_safe_groups = safe_limit_16 / bw;
      let fast_end_8 = (max_safe_groups * 8).min(count & !7);
      let mut byte_offset = 0;

      while i + 16 <= fast_end_8 {
        let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
        let chunk1 = u128::from_le(
          src_ptr
            .add(byte_offset + bw)
            .cast::<u128>()
            .read_unaligned(),
        );
        *dst_ptr.add(i) = decoder.decode_offset(chunk0 as u64 & mask);
        *dst_ptr.add(i + 1) = decoder.decode_offset((chunk0 >> bw) as u64 & mask);
        *dst_ptr.add(i + 2) = decoder.decode_offset((chunk0 >> (bw * 2)) as u64 & mask);
        *dst_ptr.add(i + 3) = decoder.decode_offset((chunk0 >> (bw * 3)) as u64 & mask);
        *dst_ptr.add(i + 4) = decoder.decode_offset((chunk0 >> (bw * 4)) as u64 & mask);
        *dst_ptr.add(i + 5) = decoder.decode_offset((chunk0 >> (bw * 5)) as u64 & mask);
        *dst_ptr.add(i + 6) = decoder.decode_offset((chunk0 >> (bw * 6)) as u64 & mask);
        *dst_ptr.add(i + 7) = decoder.decode_offset((chunk0 >> (bw * 7)) as u64 & mask);

        *dst_ptr.add(i + 8) = decoder.decode_offset(chunk1 as u64 & mask);
        *dst_ptr.add(i + 9) = decoder.decode_offset((chunk1 >> bw) as u64 & mask);
        *dst_ptr.add(i + 10) = decoder.decode_offset((chunk1 >> (bw * 2)) as u64 & mask);
        *dst_ptr.add(i + 11) = decoder.decode_offset((chunk1 >> (bw * 3)) as u64 & mask);
        *dst_ptr.add(i + 12) = decoder.decode_offset((chunk1 >> (bw * 4)) as u64 & mask);
        *dst_ptr.add(i + 13) = decoder.decode_offset((chunk1 >> (bw * 5)) as u64 & mask);
        *dst_ptr.add(i + 14) = decoder.decode_offset((chunk1 >> (bw * 6)) as u64 & mask);
        *dst_ptr.add(i + 15) = decoder.decode_offset((chunk1 >> (bw * 7)) as u64 & mask);
        byte_offset += bw * 2;
        i += 16;
      }

      while i + 8 <= fast_end_8 {
        let chunk = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
        *dst_ptr.add(i) = decoder.decode_offset(chunk as u64 & mask);
        *dst_ptr.add(i + 1) = decoder.decode_offset((chunk >> bw) as u64 & mask);
        *dst_ptr.add(i + 2) = decoder.decode_offset((chunk >> (bw * 2)) as u64 & mask);
        *dst_ptr.add(i + 3) = decoder.decode_offset((chunk >> (bw * 3)) as u64 & mask);
        *dst_ptr.add(i + 4) = decoder.decode_offset((chunk >> (bw * 4)) as u64 & mask);
        *dst_ptr.add(i + 5) = decoder.decode_offset((chunk >> (bw * 5)) as u64 & mask);
        *dst_ptr.add(i + 6) = decoder.decode_offset((chunk >> (bw * 6)) as u64 & mask);
        *dst_ptr.add(i + 7) = decoder.decode_offset((chunk >> (bw * 7)) as u64 & mask);
        byte_offset += bw;
        i += 8;
      }

      let safe_limit_bytes = src.len().saturating_sub(BYTES_U64);
      while i < count {
        let bit_pos = i * bw;
        let byte_offset = bit_pos >> 3;
        let word = if byte_offset <= safe_limit_bytes {
          u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
        } else {
          let mut buf = [0u8; 8];
          let available = src.len().saturating_sub(byte_offset).min(8);
          if available > 0 {
            copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
          }
          u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
        };
        let off = (word >> (bit_pos & 7)) & mask;
        *dst_ptr.add(i) = decoder.decode_offset(off);
        i += 1;
      }
    } else if bit_width <= 32 {
      let mid_byte = (4 * bw) / 8;
      let mid_shift = (4 * bw) & 7;
      let safe_limit_32 = src.len().saturating_sub(mid_byte + 16);
      let max_safe_groups = safe_limit_32 / bw;
      let fast_end_8 = (max_safe_groups * 8).min(count & !7);
      let mut byte_offset = 0;

      while i + 8 <= fast_end_8 {
        let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
        let chunk1 = u128::from_le(
          src_ptr
            .add(byte_offset + mid_byte)
            .cast::<u128>()
            .read_unaligned(),
        );
        *dst_ptr.add(i) = decoder.decode_offset(chunk0 as u64 & mask);
        *dst_ptr.add(i + 1) = decoder.decode_offset((chunk0 >> bw) as u64 & mask);
        *dst_ptr.add(i + 2) = decoder.decode_offset((chunk0 >> (bw * 2)) as u64 & mask);
        *dst_ptr.add(i + 3) = decoder.decode_offset((chunk0 >> (bw * 3)) as u64 & mask);
        *dst_ptr.add(i + 4) = decoder.decode_offset((chunk1 >> mid_shift) as u64 & mask);
        *dst_ptr.add(i + 5) = decoder.decode_offset((chunk1 >> (mid_shift + bw)) as u64 & mask);
        *dst_ptr.add(i + 6) = decoder.decode_offset((chunk1 >> (mid_shift + bw * 2)) as u64 & mask);
        *dst_ptr.add(i + 7) = decoder.decode_offset((chunk1 >> (mid_shift + bw * 3)) as u64 & mask);
        byte_offset += bw;
        i += 8;
      }

      let safe_limit_bytes = src.len().saturating_sub(BYTES_U64);
      while i < count {
        let bit_pos = i * bw;
        let byte_offset = bit_pos >> 3;
        let word = if byte_offset <= safe_limit_bytes {
          u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
        } else {
          let mut buf = [0u8; 8];
          let available = src.len().saturating_sub(byte_offset).min(8);
          if available > 0 {
            copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
          }
          u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
        };
        let off = (word >> (bit_pos & 7)) & mask;
        *dst_ptr.add(i) = decoder.decode_offset(off);
        i += 1;
      }
    } else if bit_width <= 56 {
      let safe_limit_16 = src.len().saturating_sub(16);
      let max_safe_i = (safe_limit_16 * 8) / bw;
      let fast_end_4 = max_safe_i.saturating_sub(3).min(count);
      let fast_limit = max_safe_i.min(count);

      while i + 4 <= fast_end_4 {
        let p0 = i * bw;
        let p1 = p0 + bw;
        let p2 = p1 + bw;
        let p3 = p2 + bw;
        let w0 =
          (u128::from_le(src_ptr.add(p0 >> 3).cast::<u128>().read_unaligned()) >> (p0 & 7)) as u64;
        let w1 =
          (u128::from_le(src_ptr.add(p1 >> 3).cast::<u128>().read_unaligned()) >> (p1 & 7)) as u64;
        let w2 =
          (u128::from_le(src_ptr.add(p2 >> 3).cast::<u128>().read_unaligned()) >> (p2 & 7)) as u64;
        let w3 =
          (u128::from_le(src_ptr.add(p3 >> 3).cast::<u128>().read_unaligned()) >> (p3 & 7)) as u64;
        *dst_ptr.add(i) = decoder.decode_offset(w0 & mask);
        *dst_ptr.add(i + 1) = decoder.decode_offset(w1 & mask);
        *dst_ptr.add(i + 2) = decoder.decode_offset(w2 & mask);
        *dst_ptr.add(i + 3) = decoder.decode_offset(w3 & mask);
        i += 4;
      }

      while i < fast_limit {
        let p = i * bw;
        let w =
          (u128::from_le(src_ptr.add(p >> 3).cast::<u128>().read_unaligned()) >> (p & 7)) as u64;
        *dst_ptr.add(i) = decoder.decode_offset(w & mask);
        i += 1;
      }

      let safe_limit_bytes = src.len().saturating_sub(BYTES_U64);
      while i < count {
        let bit_pos = i * bw;
        let byte_offset = bit_pos >> 3;
        let word = if byte_offset <= safe_limit_bytes {
          u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
        } else {
          let mut buf = [0u8; 8];
          let available = src.len().saturating_sub(byte_offset).min(8);
          if available > 0 {
            copy_nonoverlapping(src_ptr.add(byte_offset), buf.as_mut_ptr(), available);
          }
          u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
        };
        let off = (word >> (bit_pos & 7)) & mask;
        *dst_ptr.add(i) = decoder.decode_offset(off);
        i += 1;
      }
    } else {
      let safe_limit_bytes_16 = src.len().saturating_sub(16);
      let max_safe_i_128 = (safe_limit_bytes_16 * 8) / bw;
      let fast_end = max_safe_i_128.min(count);
      while i + 4 <= fast_end {
        let bit_pos0 = i * bw;
        let bit_pos1 = (i + 1) * bw;
        let bit_pos2 = (i + 2) * bw;
        let bit_pos3 = (i + 3) * bw;
        let w0 = (u128::from_le(src_ptr.add(bit_pos0 >> 3).cast::<u128>().read_unaligned())
          >> (bit_pos0 & 7)) as u64;
        let w1 = (u128::from_le(src_ptr.add(bit_pos1 >> 3).cast::<u128>().read_unaligned())
          >> (bit_pos1 & 7)) as u64;
        let w2 = (u128::from_le(src_ptr.add(bit_pos2 >> 3).cast::<u128>().read_unaligned())
          >> (bit_pos2 & 7)) as u64;
        let w3 = (u128::from_le(src_ptr.add(bit_pos3 >> 3).cast::<u128>().read_unaligned())
          >> (bit_pos3 & 7)) as u64;
        *dst_ptr.add(i) = decoder.decode_offset(w0 & mask);
        *dst_ptr.add(i + 1) = decoder.decode_offset(w1 & mask);
        *dst_ptr.add(i + 2) = decoder.decode_offset(w2 & mask);
        *dst_ptr.add(i + 3) = decoder.decode_offset(w3 & mask);
        i += 4;
      }
      while i < fast_end {
        let bit_pos = i * bw;
        let word = (u128::from_le(src_ptr.add(bit_pos >> 3).cast::<u128>().read_unaligned())
          >> (bit_pos & 7)) as u64;
        *dst_ptr.add(i) = decoder.decode_offset(word & mask);
        i += 1;
      }
      while i < count {
        let bit_pos = i * bw;
        let byte_offset = bit_pos >> 3;
        let mut buf = [0u8; 16];
        let available = src.len().saturating_sub(byte_offset).min(16);
        if available > 0 {
          buf[..available].copy_from_slice(&src[byte_offset..byte_offset + available]);
        }
        let word =
          (u128::from_le(buf.as_ptr().cast::<u128>().read_unaligned()) >> (bit_pos & 7)) as u64;
        *dst_ptr.add(i) = decoder.decode_offset(word & mask);
        i += 1;
      }
    }
  }
}
