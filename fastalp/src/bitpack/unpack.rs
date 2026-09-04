use core::ptr::copy_nonoverlapping;

use crate::{
  bitpack::packed_byte_size,
  constants::{BYTES_U64, LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT, MAX_DICT_ENTRIES},
  error::{Error, Result},
  float::AlpFloat,
};

const MASK_1BIT: u8 = 0x01;
const MASK_2BIT: u8 = 0x03;
const MASK_4BIT: u8 = 0x0f;

const CHUNK_8: usize = 8;
const CHUNK_4: usize = 4;
const CHUNK_2: usize = 2;

macro_rules! dispatch_bw {
  ($bw:expr, $BW:ident => $body:expr) => {
    match $bw {
      1 => {
        const $BW: usize = 1;
        $body
      }
      2 => {
        const $BW: usize = 2;
        $body
      }
      3 => {
        const $BW: usize = 3;
        $body
      }
      4 => {
        const $BW: usize = 4;
        $body
      }
      5 => {
        const $BW: usize = 5;
        $body
      }
      6 => {
        const $BW: usize = 6;
        $body
      }
      7 => {
        const $BW: usize = 7;
        $body
      }
      8 => {
        const $BW: usize = 8;
        $body
      }
      9 => {
        const $BW: usize = 9;
        $body
      }
      10 => {
        const $BW: usize = 10;
        $body
      }
      11 => {
        const $BW: usize = 11;
        $body
      }
      12 => {
        const $BW: usize = 12;
        $body
      }
      13 => {
        const $BW: usize = 13;
        $body
      }
      14 => {
        const $BW: usize = 14;
        $body
      }
      15 => {
        const $BW: usize = 15;
        $body
      }
      16 => {
        const $BW: usize = 16;
        $body
      }
      17 => {
        const $BW: usize = 17;
        $body
      }
      18 => {
        const $BW: usize = 18;
        $body
      }
      19 => {
        const $BW: usize = 19;
        $body
      }
      20 => {
        const $BW: usize = 20;
        $body
      }
      21 => {
        const $BW: usize = 21;
        $body
      }
      22 => {
        const $BW: usize = 22;
        $body
      }
      23 => {
        const $BW: usize = 23;
        $body
      }
      24 => {
        const $BW: usize = 24;
        $body
      }
      25 => {
        const $BW: usize = 25;
        $body
      }
      26 => {
        const $BW: usize = 26;
        $body
      }
      27 => {
        const $BW: usize = 27;
        $body
      }
      28 => {
        const $BW: usize = 28;
        $body
      }
      29 => {
        const $BW: usize = 29;
        $body
      }
      30 => {
        const $BW: usize = 30;
        $body
      }
      31 => {
        const $BW: usize = 31;
        $body
      }
      32 => {
        const $BW: usize = 32;
        $body
      }
      33 => {
        const $BW: usize = 33;
        $body
      }
      34 => {
        const $BW: usize = 34;
        $body
      }
      35 => {
        const $BW: usize = 35;
        $body
      }
      36 => {
        const $BW: usize = 36;
        $body
      }
      37 => {
        const $BW: usize = 37;
        $body
      }
      38 => {
        const $BW: usize = 38;
        $body
      }
      39 => {
        const $BW: usize = 39;
        $body
      }
      40 => {
        const $BW: usize = 40;
        $body
      }
      41 => {
        const $BW: usize = 41;
        $body
      }
      42 => {
        const $BW: usize = 42;
        $body
      }
      43 => {
        const $BW: usize = 43;
        $body
      }
      44 => {
        const $BW: usize = 44;
        $body
      }
      45 => {
        const $BW: usize = 45;
        $body
      }
      46 => {
        const $BW: usize = 46;
        $body
      }
      47 => {
        const $BW: usize = 47;
        $body
      }
      48 => {
        const $BW: usize = 48;
        $body
      }
      49 => {
        const $BW: usize = 49;
        $body
      }
      50 => {
        const $BW: usize = 50;
        $body
      }
      51 => {
        const $BW: usize = 51;
        $body
      }
      52 => {
        const $BW: usize = 52;
        $body
      }
      53 => {
        const $BW: usize = 53;
        $body
      }
      54 => {
        const $BW: usize = 54;
        $body
      }
      55 => {
        const $BW: usize = 55;
        $body
      }
      56 => {
        const $BW: usize = 56;
        $body
      }
      57 => {
        const $BW: usize = 57;
        $body
      }
      58 => {
        const $BW: usize = 58;
        $body
      }
      59 => {
        const $BW: usize = 59;
        $body
      }
      60 => {
        const $BW: usize = 60;
        $body
      }
      61 => {
        const $BW: usize = 61;
        $body
      }
      62 => {
        const $BW: usize = 62;
        $body
      }
      63 => {
        const $BW: usize = 63;
        $body
      }
      64 => {
        const $BW: usize = 64;
        $body
      }
      _ => core::hint::unreachable_unchecked(),
    }
  };
}

/// Direct pointer bit-unpacking: unpacks `count` integers of `bit_width` from `src` directly to `dst_ptr` (zero-heap allocation, zero slice init).
/// 底层直接写入裸指针的解包逻辑（零堆分配、零未初始化切片构造）
///
/// # Safety
///
/// Caller must ensure `dst_ptr` has valid writable memory for at least `count` continuous `u64` elements.
#[inline(always)]
pub(crate) unsafe fn bitunpack_u64_raw(
  src: &[u8],
  count: usize,
  bit_width: u8,
  dst_ptr: *mut u64,
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  if bit_width == 0 {
    // SAFETY: Caller guarantees dst_ptr has count writable elements
    unsafe {
      core::ptr::write_bytes(dst_ptr, 0, count);
    }
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

  // SAFETY: Available bytes verified above, dst_ptr has count writable space guaranteed by caller
  unsafe {
    dispatch_bw!(bit_width, BW => {
      unpack_u64_const::<BW>(src.as_ptr(), count, dst_ptr, src.len());
    });
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
unsafe fn unpack_u64_33_to_64<const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  dst_ptr: *mut u64,
  src_len: usize,
) {
  let mask: u64 = if BW == 64 { u64::MAX } else { (1u64 << BW) - 1 };
  let safe_limit = src_len.saturating_sub(BW + 16);
  let fast_end = count & !7;
  let mut byte_off = 0;
  let mut i = 0;

  const fn off(k: usize, bw: usize) -> usize {
    (k * bw) / 8
  }
  const fn sh(k: usize, bw: usize) -> usize {
    (k * bw) & 7
  }

  unsafe {
    while i < fast_end && byte_off <= safe_limit {
      let p = src_ptr.add(byte_off);
      let (v0, v1, v2, v3, v4, v5, v6, v7) = if BW <= 56 {
        (
          (u64::from_le(p.add(off(0, BW)).cast::<u64>().read_unaligned()) >> sh(0, BW)) & mask,
          (u64::from_le(p.add(off(1, BW)).cast::<u64>().read_unaligned()) >> sh(1, BW)) & mask,
          (u64::from_le(p.add(off(2, BW)).cast::<u64>().read_unaligned()) >> sh(2, BW)) & mask,
          (u64::from_le(p.add(off(3, BW)).cast::<u64>().read_unaligned()) >> sh(3, BW)) & mask,
          (u64::from_le(p.add(off(4, BW)).cast::<u64>().read_unaligned()) >> sh(4, BW)) & mask,
          (u64::from_le(p.add(off(5, BW)).cast::<u64>().read_unaligned()) >> sh(5, BW)) & mask,
          (u64::from_le(p.add(off(6, BW)).cast::<u64>().read_unaligned()) >> sh(6, BW)) & mask,
          (u64::from_le(p.add(off(7, BW)).cast::<u64>().read_unaligned()) >> sh(7, BW)) & mask,
        )
      } else {
        (
          ((u128::from_le(p.add(off(0, BW)).cast::<u128>().read_unaligned()) >> sh(0, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(1, BW)).cast::<u128>().read_unaligned()) >> sh(1, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(2, BW)).cast::<u128>().read_unaligned()) >> sh(2, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(3, BW)).cast::<u128>().read_unaligned()) >> sh(3, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(4, BW)).cast::<u128>().read_unaligned()) >> sh(4, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(5, BW)).cast::<u128>().read_unaligned()) >> sh(5, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(6, BW)).cast::<u128>().read_unaligned()) >> sh(6, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(7, BW)).cast::<u128>().read_unaligned()) >> sh(7, BW)) as u64)
            & mask,
        )
      };

      *dst_ptr.add(i) = v0;
      *dst_ptr.add(i + 1) = v1;
      *dst_ptr.add(i + 2) = v2;
      *dst_ptr.add(i + 3) = v3;
      *dst_ptr.add(i + 4) = v4;
      *dst_ptr.add(i + 5) = v5;
      *dst_ptr.add(i + 6) = v6;
      *dst_ptr.add(i + 7) = v7;

      byte_off += BW;
      i += 8;
    }

    let safe_limit_bytes = src_len.saturating_sub(16);
    while i < count {
      let bit_pos = i * BW;
      let b_offset = bit_pos >> 3;
      let off_shift = bit_pos & 7;
      let word = if b_offset <= safe_limit_bytes {
        (u128::from_le(src_ptr.add(b_offset).cast::<u128>().read_unaligned()) >> off_shift) as u64
      } else {
        let mut buf = [0u8; 16];
        let available = src_len.saturating_sub(b_offset).min(16);
        if available > 0 {
          copy_nonoverlapping(src_ptr.add(b_offset), buf.as_mut_ptr(), available);
        }
        (u128::from_le(buf.as_ptr().cast::<u128>().read_unaligned()) >> off_shift) as u64
      };
      *dst_ptr.add(i) = word & mask;
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_u64_const<const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  mut dst_ptr: *mut u64,
  src_len: usize,
) {
  unsafe {
    if BW == 1 {
      let full_bytes = count / CHUNK_8;
      for &b in core::slice::from_raw_parts(src_ptr, full_bytes) {
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
        let b = *src_ptr.add(full_bytes);
        for shift in 0..rem {
          *dst_ptr = ((b >> shift) & MASK_1BIT) as u64;
          dst_ptr = dst_ptr.add(1);
        }
      }
      return;
    }
    if BW == 2 {
      let full_bytes = count / CHUNK_4;
      for &b in core::slice::from_raw_parts(src_ptr, full_bytes) {
        *dst_ptr.add(0) = (b & MASK_2BIT) as u64;
        *dst_ptr.add(1) = ((b >> 2) & MASK_2BIT) as u64;
        *dst_ptr.add(2) = ((b >> 4) & MASK_2BIT) as u64;
        *dst_ptr.add(3) = ((b >> 6) & MASK_2BIT) as u64;
        dst_ptr = dst_ptr.add(CHUNK_4);
      }
      let rem = count % CHUNK_4;
      if rem > 0 {
        let b = *src_ptr.add(full_bytes);
        for i in 0..rem {
          *dst_ptr = ((b >> (i * 2)) & MASK_2BIT) as u64;
          dst_ptr = dst_ptr.add(1);
        }
      }
      return;
    }
    if BW == 4 {
      let full_bytes = count / CHUNK_2;
      let (byte_chunks, byte_rem) =
        core::slice::from_raw_parts(src_ptr, full_bytes).as_chunks::<CHUNK_2>();
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
        let b = *src_ptr.add(full_bytes);
        *dst_ptr = (b & MASK_4BIT) as u64;
      }
      return;
    }
    if BW == 8 {
      let (chunks, rem) = core::slice::from_raw_parts(src_ptr, count).as_chunks::<CHUNK_8>();
      let mut idx = 0;
      for chunk in chunks {
        *dst_ptr.add(idx) = chunk[0] as u64;
        *dst_ptr.add(idx + 1) = chunk[1] as u64;
        *dst_ptr.add(idx + 2) = chunk[2] as u64;
        *dst_ptr.add(idx + 3) = chunk[3] as u64;
        *dst_ptr.add(idx + 4) = chunk[4] as u64;
        *dst_ptr.add(idx + 5) = chunk[5] as u64;
        *dst_ptr.add(idx + 6) = chunk[6] as u64;
        *dst_ptr.add(idx + 7) = chunk[7] as u64;
        idx += CHUNK_8;
      }
      for (i, &b) in rem.iter().enumerate() {
        *dst_ptr.add(idx + i) = b as u64;
      }
      return;
    }
    if BW == 16 {
      let src16 = src_ptr.cast::<u16>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = u16::from_le(src16.add(i).read_unaligned()) as u64;
        *dst_ptr.add(i + 1) = u16::from_le(src16.add(i + 1).read_unaligned()) as u64;
        *dst_ptr.add(i + 2) = u16::from_le(src16.add(i + 2).read_unaligned()) as u64;
        *dst_ptr.add(i + 3) = u16::from_le(src16.add(i + 3).read_unaligned()) as u64;
        *dst_ptr.add(i + 4) = u16::from_le(src16.add(i + 4).read_unaligned()) as u64;
        *dst_ptr.add(i + 5) = u16::from_le(src16.add(i + 5).read_unaligned()) as u64;
        *dst_ptr.add(i + 6) = u16::from_le(src16.add(i + 6).read_unaligned()) as u64;
        *dst_ptr.add(i + 7) = u16::from_le(src16.add(i + 7).read_unaligned()) as u64;
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) = u16::from_le(src16.add(i).read_unaligned()) as u64;
        i += 1;
      }
      return;
    }
    if BW == 32 {
      let src32 = src_ptr.cast::<u32>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = u32::from_le(src32.add(i).read_unaligned()) as u64;
        *dst_ptr.add(i + 1) = u32::from_le(src32.add(i + 1).read_unaligned()) as u64;
        *dst_ptr.add(i + 2) = u32::from_le(src32.add(i + 2).read_unaligned()) as u64;
        *dst_ptr.add(i + 3) = u32::from_le(src32.add(i + 3).read_unaligned()) as u64;
        *dst_ptr.add(i + 4) = u32::from_le(src32.add(i + 4).read_unaligned()) as u64;
        *dst_ptr.add(i + 5) = u32::from_le(src32.add(i + 5).read_unaligned()) as u64;
        *dst_ptr.add(i + 6) = u32::from_le(src32.add(i + 6).read_unaligned()) as u64;
        *dst_ptr.add(i + 7) = u32::from_le(src32.add(i + 7).read_unaligned()) as u64;
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) = u32::from_le(src32.add(i).read_unaligned()) as u64;
        i += 1;
      }
      return;
    }
    if BW == 64 {
      if cfg!(target_endian = "little") {
        copy_nonoverlapping(src_ptr, dst_ptr.cast::<u8>(), count * BYTES_U64);
      } else {
        let src64 = src_ptr.cast::<u64>();
        for i in 0..count {
          *dst_ptr.add(i) = u64::from_le(src64.add(i).read_unaligned());
        }
      }
      return;
    }

    if BW <= 16 {
      unpack_u64_le16::<BW>(src_ptr, count, dst_ptr, src_len);
      return;
    }
    if BW <= 32 {
      unpack_u64_17_to_32::<BW>(src_ptr, count, dst_ptr, src_len);
      return;
    }
    unpack_u64_33_to_64::<BW>(src_ptr, count, dst_ptr, src_len);
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
  // SAFETY: dst has reserved count space, safely unpack into pointer without constructing uninitialized slice
  // SAFETY: dst 已预分配 count 空间，直接写入裸指针，消除未初始化切片构造的 UB 隐患
  unsafe {
    bitunpack_u64_raw(src, count, bit_width, dst.as_mut_ptr().add(old_len))?;
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

/// Real Doubles (ALP-RD) constant high-bits fused decoder.
/// 针对高位阶码恒定的 ALP-RD 融合单趟解码器
#[derive(Copy, Clone)]
pub struct AlpRdConstantDecoder<F: AlpFloat> {
  pub high_bits: u64,
  pub _phantom: core::marker::PhantomData<F>,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpRdConstantDecoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::from_u64_raw(self.high_bits | off)
  }

  #[inline(always)]
  fn decode_int(&self, _val: F::Int) -> F {
    F::ZERO
  }

  #[inline(always)]
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT] {
    [
      F::from_u64_raw(self.high_bits),
      F::from_u64_raw(self.high_bits | 1),
    ]
  }

  #[inline(always)]
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT] {
    [
      F::from_u64_raw(self.high_bits),
      F::from_u64_raw(self.high_bits | 1),
      F::from_u64_raw(self.high_bits | 2),
      F::from_u64_raw(self.high_bits | 3),
    ]
  }

  #[inline(always)]
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT] {
    let mut lut = [F::ZERO; LUT_SIZE_4BIT];
    for (i, slot) in lut.iter_mut().enumerate() {
      *slot = F::from_u64_raw(self.high_bits | (i as u64));
    }
    lut
  }
}

/// Compact low-cardinality dictionary fused decoder.
/// 低基数紧凑字典融合单趟解码器（持有轻量 8 字节引用，消除 512B 栈复制）
#[derive(Copy, Clone)]
pub struct AlpDictDecoder<'a, F: AlpFloat> {
  pub dict: &'a [F; MAX_DICT_ENTRIES],
}

impl<'a, F: AlpFloat> AlpDecoder<F> for AlpDictDecoder<'a, F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    // SAFETY: off & (MAX_DICT_ENTRIES - 1) 严格限制在 [0, 63] 内，dict 具备 64 项有效元素
    unsafe {
      *self
        .dict
        .get_unchecked((off as usize) & (MAX_DICT_ENTRIES - 1))
    }
  }

  #[inline(always)]
  fn decode_int(&self, _val: F::Int) -> F {
    F::ZERO
  }

  #[inline(always)]
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT] {
    [self.dict[0], self.dict[1]]
  }

  #[inline(always)]
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT] {
    [self.dict[0], self.dict[1], self.dict[2], self.dict[3]]
  }

  #[inline(always)]
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT] {
    let mut lut = [self.dict[0]; LUT_SIZE_4BIT];
    lut.copy_from_slice(&self.dict[..LUT_SIZE_4BIT]);
    lut
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
unsafe fn unpack_and_decode_le16<F: AlpFloat, D: AlpDecoder<F>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  decoder: D,
  dst_ptr: *mut F,
  src_len: usize,
) {
  let mask: u64 = (1u64 << BW) - 1;
  let safe_limit_16 = src_len.saturating_sub(16);
  let max_safe_groups = safe_limit_16 / BW;
  let fast_end_8 = (max_safe_groups * 8).min(count & !7);
  let mut byte_offset = 0;
  let mut i = 0;

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
      *dst_ptr.add(i) = decoder.decode_offset(w0 & mask);
      *dst_ptr.add(i + 1) = decoder.decode_offset((w0 >> BW) & mask);
      *dst_ptr.add(i + 2) = decoder.decode_offset((w0 >> (BW * 2)) & mask);
      *dst_ptr.add(i + 3) = decoder.decode_offset((w0 >> (BW * 3)) & mask);
      *dst_ptr.add(i + 4) = decoder.decode_offset(w1 & mask);
      *dst_ptr.add(i + 5) = decoder.decode_offset((w1 >> BW) & mask);
      *dst_ptr.add(i + 6) = decoder.decode_offset((w1 >> (BW * 2)) & mask);
      *dst_ptr.add(i + 7) = decoder.decode_offset((w1 >> (BW * 3)) & mask);

      let w2 = chunk1 as u64;
      let w3 = (chunk1 >> (BW * 4)) as u64;
      *dst_ptr.add(i + 8) = decoder.decode_offset(w2 & mask);
      *dst_ptr.add(i + 9) = decoder.decode_offset((w2 >> BW) & mask);
      *dst_ptr.add(i + 10) = decoder.decode_offset((w2 >> (BW * 2)) & mask);
      *dst_ptr.add(i + 11) = decoder.decode_offset((w2 >> (BW * 3)) & mask);
      *dst_ptr.add(i + 12) = decoder.decode_offset(w3 & mask);
      *dst_ptr.add(i + 13) = decoder.decode_offset((w3 >> BW) & mask);
      *dst_ptr.add(i + 14) = decoder.decode_offset((w3 >> (BW * 2)) & mask);
      *dst_ptr.add(i + 15) = decoder.decode_offset((w3 >> (BW * 3)) & mask);

      byte_offset += BW * 2;
      i += 16;
    }

    while i + 8 <= fast_end_8 {
      let chunk = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
      let w0 = chunk as u64;
      let w1 = (chunk >> (BW * 4)) as u64;
      *dst_ptr.add(i) = decoder.decode_offset(w0 & mask);
      *dst_ptr.add(i + 1) = decoder.decode_offset((w0 >> BW) & mask);
      *dst_ptr.add(i + 2) = decoder.decode_offset((w0 >> (BW * 2)) & mask);
      *dst_ptr.add(i + 3) = decoder.decode_offset((w0 >> (BW * 3)) & mask);
      *dst_ptr.add(i + 4) = decoder.decode_offset(w1 & mask);
      *dst_ptr.add(i + 5) = decoder.decode_offset((w1 >> BW) & mask);
      *dst_ptr.add(i + 6) = decoder.decode_offset((w1 >> (BW * 2)) & mask);
      *dst_ptr.add(i + 7) = decoder.decode_offset((w1 >> (BW * 3)) & mask);
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
      *dst_ptr.add(i) = decoder.decode_offset((word >> (bit_pos & 7)) & mask);
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_and_decode_17_to_32<F: AlpFloat, D: AlpDecoder<F>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  decoder: D,
  dst_ptr: *mut F,
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

  unsafe {
    while i + 8 <= fast_end_8 {
      let chunk0 = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
      let chunk1 = u128::from_le(
        src_ptr
          .add(byte_offset + mid_byte)
          .cast::<u128>()
          .read_unaligned(),
      );
      *dst_ptr.add(i) = decoder.decode_offset((chunk0 as u64) & mask);
      *dst_ptr.add(i + 1) = decoder.decode_offset(((chunk0 >> BW) as u64) & mask);
      *dst_ptr.add(i + 2) = decoder.decode_offset(((chunk0 >> (BW * 2)) as u64) & mask);
      *dst_ptr.add(i + 3) = decoder.decode_offset(((chunk0 >> (BW * 3)) as u64) & mask);
      *dst_ptr.add(i + 4) = decoder.decode_offset(((chunk1 >> mid_shift) as u64) & mask);
      *dst_ptr.add(i + 5) = decoder.decode_offset(((chunk1 >> (mid_shift + BW)) as u64) & mask);
      *dst_ptr.add(i + 6) = decoder.decode_offset(((chunk1 >> (mid_shift + BW * 2)) as u64) & mask);
      *dst_ptr.add(i + 7) = decoder.decode_offset(((chunk1 >> (mid_shift + BW * 3)) as u64) & mask);
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
      *dst_ptr.add(i) = decoder.decode_offset((word >> (bit_pos & 7)) & mask);
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_and_decode_33_to_64<F: AlpFloat, D: AlpDecoder<F>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  decoder: D,
  dst_ptr: *mut F,
  src_len: usize,
) {
  let mask: u64 = if BW == 64 { u64::MAX } else { (1u64 << BW) - 1 };
  let safe_limit = src_len.saturating_sub(BW + 16);
  let fast_end = count & !7;
  let mut byte_off = 0;
  let mut i = 0;

  const fn off(k: usize, bw: usize) -> usize {
    (k * bw) / 8
  }
  const fn sh(k: usize, bw: usize) -> usize {
    (k * bw) & 7
  }

  unsafe {
    while i < fast_end && byte_off <= safe_limit {
      let p = src_ptr.add(byte_off);
      let (v0, v1, v2, v3, v4, v5, v6, v7) = if BW <= 56 {
        (
          (u64::from_le(p.add(off(0, BW)).cast::<u64>().read_unaligned()) >> sh(0, BW)) & mask,
          (u64::from_le(p.add(off(1, BW)).cast::<u64>().read_unaligned()) >> sh(1, BW)) & mask,
          (u64::from_le(p.add(off(2, BW)).cast::<u64>().read_unaligned()) >> sh(2, BW)) & mask,
          (u64::from_le(p.add(off(3, BW)).cast::<u64>().read_unaligned()) >> sh(3, BW)) & mask,
          (u64::from_le(p.add(off(4, BW)).cast::<u64>().read_unaligned()) >> sh(4, BW)) & mask,
          (u64::from_le(p.add(off(5, BW)).cast::<u64>().read_unaligned()) >> sh(5, BW)) & mask,
          (u64::from_le(p.add(off(6, BW)).cast::<u64>().read_unaligned()) >> sh(6, BW)) & mask,
          (u64::from_le(p.add(off(7, BW)).cast::<u64>().read_unaligned()) >> sh(7, BW)) & mask,
        )
      } else {
        (
          ((u128::from_le(p.add(off(0, BW)).cast::<u128>().read_unaligned()) >> sh(0, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(1, BW)).cast::<u128>().read_unaligned()) >> sh(1, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(2, BW)).cast::<u128>().read_unaligned()) >> sh(2, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(3, BW)).cast::<u128>().read_unaligned()) >> sh(3, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(4, BW)).cast::<u128>().read_unaligned()) >> sh(4, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(5, BW)).cast::<u128>().read_unaligned()) >> sh(5, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(6, BW)).cast::<u128>().read_unaligned()) >> sh(6, BW)) as u64)
            & mask,
          ((u128::from_le(p.add(off(7, BW)).cast::<u128>().read_unaligned()) >> sh(7, BW)) as u64)
            & mask,
        )
      };

      *dst_ptr.add(i) = decoder.decode_offset(v0);
      *dst_ptr.add(i + 1) = decoder.decode_offset(v1);
      *dst_ptr.add(i + 2) = decoder.decode_offset(v2);
      *dst_ptr.add(i + 3) = decoder.decode_offset(v3);
      *dst_ptr.add(i + 4) = decoder.decode_offset(v4);
      *dst_ptr.add(i + 5) = decoder.decode_offset(v5);
      *dst_ptr.add(i + 6) = decoder.decode_offset(v6);
      *dst_ptr.add(i + 7) = decoder.decode_offset(v7);

      byte_off += BW;
      i += 8;
    }

    let safe_limit_bytes = src_len.saturating_sub(16);
    while i < count {
      let bit_pos = i * BW;
      let b_offset = bit_pos >> 3;
      let off_shift = bit_pos & 7;
      let word = if b_offset <= safe_limit_bytes {
        (u128::from_le(src_ptr.add(b_offset).cast::<u128>().read_unaligned()) >> off_shift) as u64
      } else {
        let mut buf = [0u8; 16];
        let available = src_len.saturating_sub(b_offset).min(16);
        if available > 0 {
          copy_nonoverlapping(src_ptr.add(b_offset), buf.as_mut_ptr(), available);
        }
        (u128::from_le(buf.as_ptr().cast::<u128>().read_unaligned()) >> off_shift) as u64
      };
      *dst_ptr.add(i) = decoder.decode_offset(word & mask);
      i += 1;
    }
  }
}

#[inline(always)]
unsafe fn unpack_and_decode_const<F: AlpFloat, D: AlpDecoder<F>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  decoder: D,
  mut dst_ptr: *mut F,
  src_len: usize,
) {
  unsafe {
    if BW == 1 {
      let lut = decoder.build_lut_1();
      let full_bytes = count / CHUNK_8;
      for &b in core::slice::from_raw_parts(src_ptr, full_bytes) {
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
        let b = *src_ptr.add(full_bytes);
        for shift in 0..rem {
          *dst_ptr = *lut.get_unchecked(((b >> shift) & MASK_1BIT) as usize);
          dst_ptr = dst_ptr.add(1);
        }
      }
      return;
    }
    if BW == 2 {
      let lut = decoder.build_lut_2();
      let full_bytes = count / CHUNK_4;
      for &b in core::slice::from_raw_parts(src_ptr, full_bytes) {
        *dst_ptr.add(0) = *lut.get_unchecked((b & MASK_2BIT) as usize);
        *dst_ptr.add(1) = *lut.get_unchecked(((b >> 2) & MASK_2BIT) as usize);
        *dst_ptr.add(2) = *lut.get_unchecked(((b >> 4) & MASK_2BIT) as usize);
        *dst_ptr.add(3) = *lut.get_unchecked(((b >> 6) & MASK_2BIT) as usize);
        dst_ptr = dst_ptr.add(CHUNK_4);
      }
      let rem = count % CHUNK_4;
      if rem > 0 {
        let b = *src_ptr.add(full_bytes);
        for i in 0..rem {
          *dst_ptr = *lut.get_unchecked(((b >> (i * 2)) & MASK_2BIT) as usize);
          dst_ptr = dst_ptr.add(1);
        }
      }
      return;
    }
    if BW == 4 {
      let lut = decoder.build_lut_4();
      let full_bytes = count / CHUNK_2;
      let (byte_chunks, byte_rem) =
        core::slice::from_raw_parts(src_ptr, full_bytes).as_chunks::<CHUNK_2>();
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
        let b = *src_ptr.add(full_bytes);
        *dst_ptr = *lut.get_unchecked((b & MASK_4BIT) as usize);
      }
      return;
    }
    if BW == 8 {
      let (chunks, rem) = core::slice::from_raw_parts(src_ptr, count).as_chunks::<CHUNK_8>();
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
    }
    if BW == 16 {
      let src_ptr16 = src_ptr.cast::<u16>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i).read_unaligned()) as u64);
        *dst_ptr.add(i + 1) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 1).read_unaligned()) as u64);
        *dst_ptr.add(i + 2) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 2).read_unaligned()) as u64);
        *dst_ptr.add(i + 3) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 3).read_unaligned()) as u64);
        *dst_ptr.add(i + 4) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 4).read_unaligned()) as u64);
        *dst_ptr.add(i + 5) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 5).read_unaligned()) as u64);
        *dst_ptr.add(i + 6) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 6).read_unaligned()) as u64);
        *dst_ptr.add(i + 7) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i + 7).read_unaligned()) as u64);
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u16::from_le(src_ptr16.add(i).read_unaligned()) as u64);
        i += 1;
      }
      return;
    }
    if BW == 32 {
      let src_ptr32 = src_ptr.cast::<u32>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i).read_unaligned()) as u64);
        *dst_ptr.add(i + 1) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 1).read_unaligned()) as u64);
        *dst_ptr.add(i + 2) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 2).read_unaligned()) as u64);
        *dst_ptr.add(i + 3) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 3).read_unaligned()) as u64);
        *dst_ptr.add(i + 4) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 4).read_unaligned()) as u64);
        *dst_ptr.add(i + 5) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 5).read_unaligned()) as u64);
        *dst_ptr.add(i + 6) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 6).read_unaligned()) as u64);
        *dst_ptr.add(i + 7) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i + 7).read_unaligned()) as u64);
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) =
          decoder.decode_offset(u32::from_le(src_ptr32.add(i).read_unaligned()) as u64);
        i += 1;
      }
      return;
    }
    if BW == 64 {
      let src_ptr64 = src_ptr.cast::<u64>();
      let mut i = 0;
      while i + 8 <= count {
        *dst_ptr.add(i) = decoder.decode_offset(u64::from_le(src_ptr64.add(i).read_unaligned()));
        *dst_ptr.add(i + 1) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 1).read_unaligned()));
        *dst_ptr.add(i + 2) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 2).read_unaligned()));
        *dst_ptr.add(i + 3) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 3).read_unaligned()));
        *dst_ptr.add(i + 4) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 4).read_unaligned()));
        *dst_ptr.add(i + 5) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 5).read_unaligned()));
        *dst_ptr.add(i + 6) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 6).read_unaligned()));
        *dst_ptr.add(i + 7) =
          decoder.decode_offset(u64::from_le(src_ptr64.add(i + 7).read_unaligned()));
        i += 8;
      }
      while i < count {
        *dst_ptr.add(i) = decoder.decode_offset(u64::from_le(src_ptr64.add(i).read_unaligned()));
        i += 1;
      }
      return;
    }

    if BW <= 16 {
      unpack_and_decode_le16::<F, D, BW>(src_ptr, count, decoder, dst_ptr, src_len);
      return;
    }
    if BW <= 32 {
      unpack_and_decode_17_to_32::<F, D, BW>(src_ptr, count, decoder, dst_ptr, src_len);
      return;
    }
    unpack_and_decode_33_to_64::<F, D, BW>(src_ptr, count, decoder, dst_ptr, src_len);
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
  dst_ptr: *mut F,
) {
  if count == 0 {
    return;
  }
  if bit_width == 0 {
    let val = decoder.decode_offset(0);
    // SAFETY: dst_ptr has capacity for count elements
    unsafe {
      core::slice::from_raw_parts_mut(dst_ptr, count).fill(val);
    }
    return;
  }
  // SAFETY: Caller guarantees src has at least packed_byte_size bytes, and dst_ptr has capacity for count elements
  unsafe {
    dispatch_bw!(bit_width, BW => {
      unpack_and_decode_const::<F, D, BW>(src.as_ptr(), count, decoder, dst_ptr, src.len());
    });
  }
}
