use core::{ptr::copy_nonoverlapping, slice::from_raw_parts_mut};

use crate::{
  bitpack::packed_byte_size,
  constants::{BYTES_U64, LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT, LUT_SIZE_8BIT},
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

/// Fast bit unpacking directly into slice: unpacks `count` integers of `bit_width` from `src` into `dst`.
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
  // 1. 上方已校验 src.len() >= required_bytes，保证读指针与 read_unaligned 严格在 src 有效内存边界内；
  // 2. dst.len() >= count，写入 0..count 空间完全充足且无越界风险。
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
      for (i, &b) in src[..count].iter().enumerate() {
        *dst_ptr.add(i) = b as u64;
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
      let src_ptr = src.as_ptr().cast::<u64>();
      for i in 0..count {
        *dst_ptr.add(i) = u64::from_le(src_ptr.add(i).read_unaligned());
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

  // SAFETY: Caller guarantees src_len bytes readable from src_ptr, count elements writable to dst_ptr
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

  // SAFETY: Caller guarantees src_len bytes readable from src_ptr, count elements writable to dst_ptr
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

  // SAFETY: Caller guarantees src_len bytes readable from src_ptr, count elements writable to dst_ptr
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

  // SAFETY: Caller guarantees src_len bytes readable from src_ptr, count elements writable to dst_ptr
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

    let safe_limit_bytes = src_len.saturating_sub(BYTES_U64);
    while i < count {
      let bit_pos = i * bw;
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

/// Fast bit unpacking: unpacks `count` integers of `bit_width` from `src` into `dst`.
/// 高速位解包：从 `src` 解包出 `count` 个 `bit_width` 位的整数至 `dst`（零双重初始化开销）
#[inline]
pub fn bitunpack_u64(src: &[u8], count: usize, bit_width: u8, dst: &mut Vec<u64>) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst 已预分配 count 空间，通过切片零堆分配安全解包，消除 resize 双重写零开销
  let slice = unsafe { from_raw_parts_mut(dst.as_mut_ptr().add(old_len), count) };
  bitunpack_u64_slice(src, count, bit_width, slice)?;
  unsafe {
    dst.set_len(old_len + count);
  }
  Ok(())
}

/// Core bit unpacking inner function writing directly to raw pointer.
/// 内部核心位解包逻辑：直接写入目标裸指针
///
/// # Safety
/// 1. `src` 必须至少包含 `packed_byte_size(count, bit_width)` 字节有效数据；
/// 2. `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
pub unsafe fn bitunpack_core<F: AlpFloat>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  base: F::Int,
  fac_int: i64,
  frac_flt: F,
  mut dst_ptr: *mut F,
) {
  // SAFETY: Caller guarantees src has at least packed_byte_size bytes and dst_ptr can hold count elements
  unsafe {
    if bit_width == BITS_1 {
      let lut = F::build_lut::<LUT_SIZE_1BIT>(base, fac_int, frac_flt);
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
      let lut = F::build_lut::<LUT_SIZE_2BIT>(base, fac_int, frac_flt);
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
      let lut = F::build_lut::<LUT_SIZE_4BIT>(base, fac_int, frac_flt);
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
      if fac_int == 1 {
        for chunk in chunks {
          *dst_ptr.add(idx) = F::decode_from_offset_fac1(chunk[0] as u64, base, frac_flt);
          *dst_ptr.add(idx + 1) = F::decode_from_offset_fac1(chunk[1] as u64, base, frac_flt);
          *dst_ptr.add(idx + 2) = F::decode_from_offset_fac1(chunk[2] as u64, base, frac_flt);
          *dst_ptr.add(idx + 3) = F::decode_from_offset_fac1(chunk[3] as u64, base, frac_flt);
          *dst_ptr.add(idx + 4) = F::decode_from_offset_fac1(chunk[4] as u64, base, frac_flt);
          *dst_ptr.add(idx + 5) = F::decode_from_offset_fac1(chunk[5] as u64, base, frac_flt);
          *dst_ptr.add(idx + 6) = F::decode_from_offset_fac1(chunk[6] as u64, base, frac_flt);
          *dst_ptr.add(idx + 7) = F::decode_from_offset_fac1(chunk[7] as u64, base, frac_flt);
          idx += CHUNK_8;
        }
        for (i, &b) in rem.iter().enumerate() {
          *dst_ptr.add(idx + i) = F::decode_from_offset_fac1(b as u64, base, frac_flt);
        }
      } else {
        for chunk in chunks {
          *dst_ptr.add(idx) = F::decode_from_offset(chunk[0] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 1) = F::decode_from_offset(chunk[1] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 2) = F::decode_from_offset(chunk[2] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 3) = F::decode_from_offset(chunk[3] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 4) = F::decode_from_offset(chunk[4] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 5) = F::decode_from_offset(chunk[5] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 6) = F::decode_from_offset(chunk[6] as u64, base, fac_int, frac_flt);
          *dst_ptr.add(idx + 7) = F::decode_from_offset(chunk[7] as u64, base, fac_int, frac_flt);
          idx += CHUNK_8;
        }
        for (i, &b) in rem.iter().enumerate() {
          *dst_ptr.add(idx + i) = F::decode_from_offset(b as u64, base, fac_int, frac_flt);
        }
      }

      return;
    } else if bit_width == BITS_16 {
      let src_ptr = src.as_ptr().cast::<u16>();
      let mut i = 0;
      if fac_int == 1 {
        while i + 8 <= count {
          *dst_ptr.add(i) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 1) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 1).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 2) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 2).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 3) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 3).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 4) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 4).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 5) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 5).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 6).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i + 7).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          i += 8;
        }
        while i < count {
          *dst_ptr.add(i) = F::decode_from_offset_fac1(
            u16::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          i += 1;
        }
      } else {
        while i + 8 <= count {
          *dst_ptr.add(i) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 1) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 1).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 2) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 2).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 3) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 3).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 4) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 4).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 5) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 5).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 6).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i + 7).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          i += 8;
        }
        while i < count {
          *dst_ptr.add(i) = F::decode_from_offset(
            u16::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          i += 1;
        }
      }

      return;
    } else if bit_width == BITS_32 {
      let src_ptr = src.as_ptr().cast::<u32>();
      let mut i = 0;
      if fac_int == 1 {
        while i + 8 <= count {
          *dst_ptr.add(i) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 1) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 1).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 2) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 2).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 3) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 3).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 4) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 4).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 5) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 5).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 6).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i + 7).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          i += 8;
        }
        while i < count {
          *dst_ptr.add(i) = F::decode_from_offset_fac1(
            u32::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            frac_flt,
          );
          i += 1;
        }
      } else {
        while i + 8 <= count {
          *dst_ptr.add(i) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 1) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 1).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 2) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 2).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 3) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 3).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 4) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 4).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 5) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 5).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 6).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i + 7).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          i += 8;
        }
        while i < count {
          *dst_ptr.add(i) = F::decode_from_offset(
            u32::from_le(src_ptr.add(i).read_unaligned()) as u64,
            base,
            fac_int,
            frac_flt,
          );
          i += 1;
        }
      }

      return;
    } else if bit_width == BITS_64 {
      let src_ptr = src.as_ptr().cast::<u64>();
      let mut i = 0;
      if fac_int == 1 {
        while i + 8 <= count {
          *dst_ptr.add(i) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 1) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 1).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 2) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 2).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 3) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 3).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 4) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 4).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 5) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 5).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 6).read_unaligned()),
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i + 7).read_unaligned()),
            base,
            frac_flt,
          );
          i += 8;
        }
        while i < count {
          *dst_ptr.add(i) = F::decode_from_offset_fac1(
            u64::from_le(src_ptr.add(i).read_unaligned()),
            base,
            frac_flt,
          );
          i += 1;
        }
      } else {
        while i + 8 <= count {
          *dst_ptr.add(i) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 1) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 1).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 2) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 2).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 3) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 3).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 4) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 4).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 5) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 5).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 6).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i + 7).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          i += 8;
        }
        while i < count {
          *dst_ptr.add(i) = F::decode_from_offset(
            u64::from_le(src_ptr.add(i).read_unaligned()),
            base,
            fac_int,
            frac_flt,
          );
          i += 1;
        }
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

      if fac_int == 1 {
        while i + 16 <= fast_end_8 {
          let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
          let chunk1 = u128::from_le(
            src_ptr
              .add(byte_offset + bw)
              .cast::<u128>()
              .read_unaligned(),
          );
          *dst_ptr.add(i) = F::decode_from_offset_fac1(chunk0 as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 1) =
            F::decode_from_offset_fac1((chunk0 >> bw) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 2) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 2)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 3) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 3)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 4) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 4)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 5) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 5)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 6) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 6)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 7) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 7)) as u64 & mask, base, frac_flt);

          *dst_ptr.add(i + 8) = F::decode_from_offset_fac1(chunk1 as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 9) =
            F::decode_from_offset_fac1((chunk1 >> bw) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 10) =
            F::decode_from_offset_fac1((chunk1 >> (bw * 2)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 11) =
            F::decode_from_offset_fac1((chunk1 >> (bw * 3)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 12) =
            F::decode_from_offset_fac1((chunk1 >> (bw * 4)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 13) =
            F::decode_from_offset_fac1((chunk1 >> (bw * 5)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 14) =
            F::decode_from_offset_fac1((chunk1 >> (bw * 6)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 15) =
            F::decode_from_offset_fac1((chunk1 >> (bw * 7)) as u64 & mask, base, frac_flt);
          byte_offset += bw * 2;
          i += 16;
        }

        while i + 8 <= fast_end_8 {
          let chunk = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
          *dst_ptr.add(i) = F::decode_from_offset_fac1(chunk as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 1) =
            F::decode_from_offset_fac1((chunk >> bw) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 2) =
            F::decode_from_offset_fac1((chunk >> (bw * 2)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 3) =
            F::decode_from_offset_fac1((chunk >> (bw * 3)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 4) =
            F::decode_from_offset_fac1((chunk >> (bw * 4)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 5) =
            F::decode_from_offset_fac1((chunk >> (bw * 5)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 6) =
            F::decode_from_offset_fac1((chunk >> (bw * 6)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 7) =
            F::decode_from_offset_fac1((chunk >> (bw * 7)) as u64 & mask, base, frac_flt);
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
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
          i += 1;
        }
      } else {
        while i + 16 <= fast_end_8 {
          let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
          let chunk1 = u128::from_le(
            src_ptr
              .add(byte_offset + bw)
              .cast::<u128>()
              .read_unaligned(),
          );
          *dst_ptr.add(i) = F::decode_from_offset(chunk0 as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 1) =
            F::decode_from_offset((chunk0 >> bw) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 2) =
            F::decode_from_offset((chunk0 >> (bw * 2)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 3) =
            F::decode_from_offset((chunk0 >> (bw * 3)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 4) =
            F::decode_from_offset((chunk0 >> (bw * 4)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 5) =
            F::decode_from_offset((chunk0 >> (bw * 5)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 6) =
            F::decode_from_offset((chunk0 >> (bw * 6)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 7) =
            F::decode_from_offset((chunk0 >> (bw * 7)) as u64 & mask, base, fac_int, frac_flt);

          *dst_ptr.add(i + 8) =
            F::decode_from_offset(chunk1 as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 9) =
            F::decode_from_offset((chunk1 >> bw) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 10) =
            F::decode_from_offset((chunk1 >> (bw * 2)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 11) =
            F::decode_from_offset((chunk1 >> (bw * 3)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 12) =
            F::decode_from_offset((chunk1 >> (bw * 4)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 13) =
            F::decode_from_offset((chunk1 >> (bw * 5)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 14) =
            F::decode_from_offset((chunk1 >> (bw * 6)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 15) =
            F::decode_from_offset((chunk1 >> (bw * 7)) as u64 & mask, base, fac_int, frac_flt);
          byte_offset += bw * 2;
          i += 16;
        }

        while i + 8 <= fast_end_8 {
          let chunk = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
          *dst_ptr.add(i) = F::decode_from_offset(chunk as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 1) =
            F::decode_from_offset((chunk >> bw) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 2) =
            F::decode_from_offset((chunk >> (bw * 2)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 3) =
            F::decode_from_offset((chunk >> (bw * 3)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 4) =
            F::decode_from_offset((chunk >> (bw * 4)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 5) =
            F::decode_from_offset((chunk >> (bw * 5)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 6) =
            F::decode_from_offset((chunk >> (bw * 6)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 7) =
            F::decode_from_offset((chunk >> (bw * 7)) as u64 & mask, base, fac_int, frac_flt);
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
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
          i += 1;
        }
      }
    } else if bit_width <= 32 {
      let mid_byte = (4 * bw) / 8;
      let mid_shift = (4 * bw) & 7;
      let safe_limit_32 = src.len().saturating_sub(mid_byte + 16);
      let max_safe_groups = safe_limit_32 / bw;
      let fast_end_8 = (max_safe_groups * 8).min(count & !7);
      let mut byte_offset = 0;

      if fac_int == 1 {
        while i + 8 <= fast_end_8 {
          let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
          let chunk1 = u128::from_le(
            src_ptr
              .add(byte_offset + mid_byte)
              .cast::<u128>()
              .read_unaligned(),
          );
          *dst_ptr.add(i) = F::decode_from_offset_fac1(chunk0 as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 1) =
            F::decode_from_offset_fac1((chunk0 >> bw) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 2) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 2)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 3) =
            F::decode_from_offset_fac1((chunk0 >> (bw * 3)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 4) =
            F::decode_from_offset_fac1((chunk1 >> mid_shift) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 5) =
            F::decode_from_offset_fac1((chunk1 >> (mid_shift + bw)) as u64 & mask, base, frac_flt);
          *dst_ptr.add(i + 6) = F::decode_from_offset_fac1(
            (chunk1 >> (mid_shift + bw * 2)) as u64 & mask,
            base,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset_fac1(
            (chunk1 >> (mid_shift + bw * 3)) as u64 & mask,
            base,
            frac_flt,
          );
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
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
          i += 1;
        }
      } else {
        while i + 8 <= fast_end_8 {
          let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
          let chunk1 = u128::from_le(
            src_ptr
              .add(byte_offset + mid_byte)
              .cast::<u128>()
              .read_unaligned(),
          );
          *dst_ptr.add(i) = F::decode_from_offset(chunk0 as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 1) =
            F::decode_from_offset((chunk0 >> bw) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 2) =
            F::decode_from_offset((chunk0 >> (bw * 2)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 3) =
            F::decode_from_offset((chunk0 >> (bw * 3)) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 4) =
            F::decode_from_offset((chunk1 >> mid_shift) as u64 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 5) = F::decode_from_offset(
            (chunk1 >> (mid_shift + bw)) as u64 & mask,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 6) = F::decode_from_offset(
            (chunk1 >> (mid_shift + bw * 2)) as u64 & mask,
            base,
            fac_int,
            frac_flt,
          );
          *dst_ptr.add(i + 7) = F::decode_from_offset(
            (chunk1 >> (mid_shift + bw * 3)) as u64 & mask,
            base,
            fac_int,
            frac_flt,
          );
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
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
          i += 1;
        }
      }
    } else if bit_width <= 56 {
      let safe_limit_16 = src.len().saturating_sub(16);
      let max_safe_i = (safe_limit_16 * 8) / bw;
      let fast_end_4 = max_safe_i.saturating_sub(3).min(count);
      let fast_limit = max_safe_i.min(count);

      if fac_int == 1 {
        while i + 4 <= fast_end_4 {
          let p0 = i * bw;
          let p1 = p0 + bw;
          let p2 = p1 + bw;
          let p3 = p2 + bw;
          let w0 = (u128::from_le(src_ptr.add(p0 >> 3).cast::<u128>().read_unaligned()) >> (p0 & 7))
            as u64;
          let w1 = (u128::from_le(src_ptr.add(p1 >> 3).cast::<u128>().read_unaligned()) >> (p1 & 7))
            as u64;
          let w2 = (u128::from_le(src_ptr.add(p2 >> 3).cast::<u128>().read_unaligned()) >> (p2 & 7))
            as u64;
          let w3 = (u128::from_le(src_ptr.add(p3 >> 3).cast::<u128>().read_unaligned()) >> (p3 & 7))
            as u64;
          *dst_ptr.add(i) = F::decode_from_offset_fac1(w0 & mask, base, frac_flt);
          *dst_ptr.add(i + 1) = F::decode_from_offset_fac1(w1 & mask, base, frac_flt);
          *dst_ptr.add(i + 2) = F::decode_from_offset_fac1(w2 & mask, base, frac_flt);
          *dst_ptr.add(i + 3) = F::decode_from_offset_fac1(w3 & mask, base, frac_flt);
          i += 4;
        }

        while i < fast_limit {
          let p = i * bw;
          let w =
            (u128::from_le(src_ptr.add(p >> 3).cast::<u128>().read_unaligned()) >> (p & 7)) as u64;
          *dst_ptr.add(i) = F::decode_from_offset_fac1(w & mask, base, frac_flt);
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
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
          i += 1;
        }
      } else {
        while i + 4 <= fast_end_4 {
          let p0 = i * bw;
          let p1 = p0 + bw;
          let p2 = p1 + bw;
          let p3 = p2 + bw;
          let w0 = (u128::from_le(src_ptr.add(p0 >> 3).cast::<u128>().read_unaligned()) >> (p0 & 7))
            as u64;
          let w1 = (u128::from_le(src_ptr.add(p1 >> 3).cast::<u128>().read_unaligned()) >> (p1 & 7))
            as u64;
          let w2 = (u128::from_le(src_ptr.add(p2 >> 3).cast::<u128>().read_unaligned()) >> (p2 & 7))
            as u64;
          let w3 = (u128::from_le(src_ptr.add(p3 >> 3).cast::<u128>().read_unaligned()) >> (p3 & 7))
            as u64;
          *dst_ptr.add(i) = F::decode_from_offset(w0 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 1) = F::decode_from_offset(w1 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 2) = F::decode_from_offset(w2 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 3) = F::decode_from_offset(w3 & mask, base, fac_int, frac_flt);
          i += 4;
        }

        while i < fast_limit {
          let p = i * bw;
          let w =
            (u128::from_le(src_ptr.add(p >> 3).cast::<u128>().read_unaligned()) >> (p & 7)) as u64;
          *dst_ptr.add(i) = F::decode_from_offset(w & mask, base, fac_int, frac_flt);
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
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
          i += 1;
        }
      }
    } else {
      let safe_limit_bytes_16 = src.len().saturating_sub(16);
      let max_safe_i_128 = (safe_limit_bytes_16 * 8) / bw;
      let fast_end = max_safe_i_128.min(count);
      if fac_int == 1 {
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
          *dst_ptr.add(i) = F::decode_from_offset_fac1(w0 & mask, base, frac_flt);
          *dst_ptr.add(i + 1) = F::decode_from_offset_fac1(w1 & mask, base, frac_flt);
          *dst_ptr.add(i + 2) = F::decode_from_offset_fac1(w2 & mask, base, frac_flt);
          *dst_ptr.add(i + 3) = F::decode_from_offset_fac1(w3 & mask, base, frac_flt);
          i += 4;
        }
        while i < fast_end {
          let bit_pos = i * bw;
          let word = (u128::from_le(src_ptr.add(bit_pos >> 3).cast::<u128>().read_unaligned())
            >> (bit_pos & 7)) as u64;
          *dst_ptr.add(i) = F::decode_from_offset_fac1(word & mask, base, frac_flt);
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
          *dst_ptr.add(i) = F::decode_from_offset_fac1(word & mask, base, frac_flt);
          i += 1;
        }
      } else {
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
          *dst_ptr.add(i) = F::decode_from_offset(w0 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 1) = F::decode_from_offset(w1 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 2) = F::decode_from_offset(w2 & mask, base, fac_int, frac_flt);
          *dst_ptr.add(i + 3) = F::decode_from_offset(w3 & mask, base, fac_int, frac_flt);
          i += 4;
        }
        while i < fast_end {
          let bit_pos = i * bw;
          let word = (u128::from_le(src_ptr.add(bit_pos >> 3).cast::<u128>().read_unaligned())
            >> (bit_pos & 7)) as u64;
          *dst_ptr.add(i) = F::decode_from_offset(word & mask, base, fac_int, frac_flt);
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
          *dst_ptr.add(i) = F::decode_from_offset(word & mask, base, fac_int, frac_flt);
          i += 1;
        }
      }
    }
  }
}

/// Direct bit unpacking and floating-point reconstruction into a destination slice.
/// 直接解包并重构浮点数据至目标切片（零堆分配、零内存拷贝）
#[inline(always)]
pub fn bitunpack_slice<F: AlpFloat>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  base: F::Int,
  fac_int: i64,
  frac_flt: F,
  dst: &mut [F],
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  if dst.len() < count {
    return Err(Error::BufferTooSmall {
      needed: count,
      available: dst.len(),
    });
  }
  let required_bytes = packed_byte_size(count, bit_width);
  if src.len() < required_bytes {
    return Err(Error::UnexpectedEof {
      needed: required_bytes,
      available: src.len(),
    });
  }
  if bit_width == 0 {
    let val = F::decode_from_offset(0, base, fac_int, frac_flt);
    dst[..count].fill(val);
    return Ok(());
  }
  // SAFETY: 上方已检验可用字节充足且 dst.len() >= count
  unsafe {
    bitunpack_core(
      src,
      count,
      bit_width,
      base,
      fac_int,
      frac_flt,
      dst.as_mut_ptr(),
    );
  }
  Ok(())
}

/// Generic zero-copy direct bit unpacking and floating-point reconstruction into `dst`.
/// 通用直接解包并重构浮点数据至 `dst`（委托切片内核，消除冗余校验）
#[inline(always)]
pub fn bitunpack_into<F: AlpFloat>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  base: F::Int,
  fac_int: i64,
  frac_flt: F,
  dst: &mut Vec<F>,
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst 已预分配 count 空间，通过切片零堆分配安全解包
  let slice = unsafe { from_raw_parts_mut(dst.as_mut_ptr().add(old_len), count) };
  bitunpack_slice(src, count, bit_width, base, fac_int, frac_flt, slice)?;
  unsafe {
    dst.set_len(old_len + count);
  }
  Ok(())
}

/// Core decimal division bit unpacking inner function writing directly to raw pointer.
/// 内部核心十进制除法位解包逻辑：直接写入目标裸指针
///
/// # Safety
/// 1. `src` 必须至少包含 `packed_byte_size(count, bit_width)` 字节有效数据；
/// 2. `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
pub unsafe fn bitunpack_core_div<F: AlpFloat>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  base: F::Int,
  exp_factor: F,
  mut dst_ptr: *mut F,
) {
  unsafe {
    if bit_width == BITS_1 {
      let lut = F::build_lut_div::<LUT_SIZE_1BIT>(base, exp_factor);
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
      let lut = F::build_lut_div::<LUT_SIZE_2BIT>(base, exp_factor);
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
      let lut = F::build_lut_div::<LUT_SIZE_4BIT>(base, exp_factor);
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
      let lut = F::build_lut_div::<LUT_SIZE_8BIT>(base, exp_factor);
      let (chunks, rem) = src[..count].as_chunks::<CHUNK_8>();
      let mut idx = 0;
      for chunk in chunks {
        *dst_ptr.add(idx) = *lut.get_unchecked(chunk[0] as usize);
        *dst_ptr.add(idx + 1) = *lut.get_unchecked(chunk[1] as usize);
        *dst_ptr.add(idx + 2) = *lut.get_unchecked(chunk[2] as usize);
        *dst_ptr.add(idx + 3) = *lut.get_unchecked(chunk[3] as usize);
        *dst_ptr.add(idx + 4) = *lut.get_unchecked(chunk[4] as usize);
        *dst_ptr.add(idx + 5) = *lut.get_unchecked(chunk[5] as usize);
        *dst_ptr.add(idx + 6) = *lut.get_unchecked(chunk[6] as usize);
        *dst_ptr.add(idx + 7) = *lut.get_unchecked(chunk[7] as usize);
        idx += CHUNK_8;
      }
      for (i, &b) in rem.iter().enumerate() {
        *dst_ptr.add(idx + i) = *lut.get_unchecked(b as usize);
      }

      return;
    } else if bit_width == BITS_16 {
      let src_ptr = src.as_ptr().cast::<u16>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 1) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 1).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 2) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 2).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 3) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 3).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 4) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 4).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 5) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 5).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 6) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 6).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 7) = F::decode_from_offset_div(
          u16::from_le(src_ptr.add(i + 7).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        i += 8;
      }
      while i < count {
        let off = u16::from_le(src_ptr.add(i).read_unaligned()) as u64;
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
        i += 1;
      }

      return;
    } else if bit_width == BITS_32 {
      let src_ptr = src.as_ptr().cast::<u32>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 1) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 1).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 2) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 2).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 3) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 3).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 4) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 4).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 5) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 5).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 6) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 6).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 7) = F::decode_from_offset_div(
          u32::from_le(src_ptr.add(i + 7).read_unaligned()) as u64,
          base,
          exp_factor,
        );
        i += 8;
      }
      while i < count {
        let off = u32::from_le(src_ptr.add(i).read_unaligned()) as u64;
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
        i += 1;
      }

      return;
    } else if bit_width == BITS_64 {
      let src_ptr = src.as_ptr().cast::<u64>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 1) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 1).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 2) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 2).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 3) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 3).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 4) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 4).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 5) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 5).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 6) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 6).read_unaligned()),
          base,
          exp_factor,
        );
        *dst_ptr.add(i + 7) = F::decode_from_offset_div(
          u64::from_le(src_ptr.add(i + 7).read_unaligned()),
          base,
          exp_factor,
        );
        i += 8;
      }
      while i < count {
        let off = u64::from_le(src_ptr.add(i).read_unaligned());
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
        i += 1;
      }

      return;
    }

    let mask = bit_mask(bit_width);
    let bw = bit_width as usize;
    let safe_limit_bytes = src.len().saturating_sub(BYTES_U64);
    let src_ptr = src.as_ptr();

    let mut i = 0;
    if bit_width <= 56 {
      let max_safe_i = (safe_limit_bytes * 8) / bw;
      let fast_end_8 = max_safe_i.saturating_sub(7).min(count);
      let fast_end_4 = max_safe_i.saturating_sub(3).min(count);
      let fast_limit = max_safe_i.min(count);
      let mut bit_pos = 0;

      while i + 8 <= fast_end_8 {
        let p0 = bit_pos;
        let p1 = bit_pos + bw;
        let p2 = bit_pos + bw * 2;
        let p3 = bit_pos + bw * 3;
        let p4 = bit_pos + bw * 4;
        let p5 = bit_pos + bw * 5;
        let p6 = bit_pos + bw * 6;
        let p7 = bit_pos + bw * 7;
        let w0 = u64::from_le(src_ptr.add(p0 >> 3).cast::<u64>().read_unaligned());
        let w1 = u64::from_le(src_ptr.add(p1 >> 3).cast::<u64>().read_unaligned());
        let w2 = u64::from_le(src_ptr.add(p2 >> 3).cast::<u64>().read_unaligned());
        let w3 = u64::from_le(src_ptr.add(p3 >> 3).cast::<u64>().read_unaligned());
        let w4 = u64::from_le(src_ptr.add(p4 >> 3).cast::<u64>().read_unaligned());
        let w5 = u64::from_le(src_ptr.add(p5 >> 3).cast::<u64>().read_unaligned());
        let w6 = u64::from_le(src_ptr.add(p6 >> 3).cast::<u64>().read_unaligned());
        let w7 = u64::from_le(src_ptr.add(p7 >> 3).cast::<u64>().read_unaligned());
        *dst_ptr.add(i) = F::decode_from_offset_div((w0 >> (p0 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 1) = F::decode_from_offset_div((w1 >> (p1 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 2) = F::decode_from_offset_div((w2 >> (p2 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 3) = F::decode_from_offset_div((w3 >> (p3 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 4) = F::decode_from_offset_div((w4 >> (p4 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 5) = F::decode_from_offset_div((w5 >> (p5 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 6) = F::decode_from_offset_div((w6 >> (p6 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 7) = F::decode_from_offset_div((w7 >> (p7 & 7)) & mask, base, exp_factor);
        bit_pos += bw * 8;
        i += 8;
      }

      while i + 4 <= fast_end_4 {
        let p0 = bit_pos;
        let p1 = bit_pos + bw;
        let p2 = bit_pos + bw * 2;
        let p3 = bit_pos + bw * 3;
        let w0 = u64::from_le(src_ptr.add(p0 >> 3).cast::<u64>().read_unaligned());
        let w1 = u64::from_le(src_ptr.add(p1 >> 3).cast::<u64>().read_unaligned());
        let w2 = u64::from_le(src_ptr.add(p2 >> 3).cast::<u64>().read_unaligned());
        let w3 = u64::from_le(src_ptr.add(p3 >> 3).cast::<u64>().read_unaligned());
        *dst_ptr.add(i) = F::decode_from_offset_div((w0 >> (p0 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 1) = F::decode_from_offset_div((w1 >> (p1 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 2) = F::decode_from_offset_div((w2 >> (p2 & 7)) & mask, base, exp_factor);
        *dst_ptr.add(i + 3) = F::decode_from_offset_div((w3 >> (p3 & 7)) & mask, base, exp_factor);
        bit_pos += bw * 4;
        i += 4;
      }

      while i < fast_limit {
        let p0 = bit_pos;
        let w = u64::from_le(src_ptr.add(p0 >> 3).cast::<u64>().read_unaligned());
        *dst_ptr.add(i) = F::decode_from_offset_div((w >> (p0 & 7)) & mask, base, exp_factor);
        bit_pos += bw;
        i += 1;
      }

      while i < count {
        let bit_pos = i * bw;
        let byte_offset = bit_pos >> 3;
        let word = if byte_offset <= safe_limit_bytes {
          u64::from_le(src_ptr.add(byte_offset).cast::<u64>().read_unaligned())
        } else {
          let mut buf = [0u8; 8];
          let available = src.len().saturating_sub(byte_offset).min(8);
          if available > 0 {
            buf[..available].copy_from_slice(&src[byte_offset..byte_offset + available]);
          }
          u64::from_le(buf.as_ptr().cast::<u64>().read_unaligned())
        };
        let off = (word >> (bit_pos & 7)) & mask;
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
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
        *dst_ptr.add(i) = F::decode_from_offset_div(w0 & mask, base, exp_factor);
        *dst_ptr.add(i + 1) = F::decode_from_offset_div(w1 & mask, base, exp_factor);
        *dst_ptr.add(i + 2) = F::decode_from_offset_div(w2 & mask, base, exp_factor);
        *dst_ptr.add(i + 3) = F::decode_from_offset_div(w3 & mask, base, exp_factor);
        i += 4;
      }
      while i < fast_end {
        let bit_pos = i * bw;
        let word = (u128::from_le(src_ptr.add(bit_pos >> 3).cast::<u128>().read_unaligned())
          >> (bit_pos & 7)) as u64;
        *dst_ptr.add(i) = F::decode_from_offset_div(word & mask, base, exp_factor);
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
        *dst_ptr.add(i) = F::decode_from_offset_div(word & mask, base, exp_factor);
        i += 1;
      }
    }
  }
}

/// Direct bit unpacking and decimal division floating-point reconstruction into a destination slice.
/// 直接解包并采用十进制除法重构浮点数据至目标切片（零堆分配、零内存拷贝）
#[inline(always)]
pub fn bitunpack_slice_div<F: AlpFloat>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  base: F::Int,
  exp_factor: F,
  dst: &mut [F],
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  if dst.len() < count {
    return Err(Error::BufferTooSmall {
      needed: count,
      available: dst.len(),
    });
  }
  let required_bytes = packed_byte_size(count, bit_width);
  if src.len() < required_bytes {
    return Err(Error::UnexpectedEof {
      needed: required_bytes,
      available: src.len(),
    });
  }
  if bit_width == 0 {
    let val = F::decode_from_offset_div(0, base, exp_factor);
    dst[..count].fill(val);
    return Ok(());
  }
  // SAFETY: 上方已检验可用字节充足且 dst.len() >= count
  unsafe {
    bitunpack_core_div(src, count, bit_width, base, exp_factor, dst.as_mut_ptr());
  }
  Ok(())
}

/// Generic zero-copy direct bit unpacking and decimal division floating-point reconstruction into `dst`.
/// 通用直接解包并采用十进制除法重构浮点数据至 `dst`（委托切片内核，消除冗余校验）
#[inline(always)]
pub fn bitunpack_into_div<F: AlpFloat>(
  src: &[u8],
  count: usize,
  bit_width: u8,
  base: F::Int,
  exp_factor: F,
  dst: &mut Vec<F>,
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst 已预分配 count 空间，通过切片零堆分配安全解包
  let slice = unsafe { from_raw_parts_mut(dst.as_mut_ptr().add(old_len), count) };
  bitunpack_slice_div(src, count, bit_width, base, exp_factor, slice)?;
  unsafe {
    dst.set_len(old_len + count);
  }
  Ok(())
}
