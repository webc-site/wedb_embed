use core::{ptr::copy_nonoverlapping, slice::from_raw_parts};

use super::consumer::AlpConsumer;
use crate::constants::BYTES_U64;

const MASK_1BIT: u8 = 0x01;
const MASK_2BIT: u8 = 0x03;
const MASK_4BIT: u8 = 0x0f;

const CHUNK_8: usize = 8;
const CHUNK_4: usize = 4;
const CHUNK_2: usize = 2;

#[inline(always)]
pub(crate) unsafe fn unpack_1<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  mut dst_ptr: *mut T,
) {
  unsafe {
    if let Some(lut) = consumer.use_lut_1() {
      let full_bytes = count / CHUNK_8;
      for &b in from_raw_parts(src_ptr, full_bytes) {
        write_8!(dst_ptr, k => *lut.get_unchecked(((b >> k) & MASK_1BIT) as usize));
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
    } else {
      let full_bytes = count / CHUNK_8;
      for (b_idx, &b) in from_raw_parts(src_ptr, full_bytes).iter().enumerate() {
        consumer.consume_8(
          arr_8!(k => ((b >> k) & MASK_1BIT) as u64),
          dst_ptr.add(b_idx * CHUNK_8),
        );
      }
      let rem = count % CHUNK_8;
      if rem > 0 {
        let b = *src_ptr.add(full_bytes);
        let rem_start = full_bytes * CHUNK_8;
        for shift in 0..rem {
          consumer.consume_1(
            ((b >> shift) & MASK_1BIT) as u64,
            dst_ptr.add(rem_start + shift),
          );
        }
      }
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_2<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  mut dst_ptr: *mut T,
) {
  unsafe {
    if let Some(lut) = consumer.use_lut_2() {
      let full_bytes = count / CHUNK_4;
      for &b in from_raw_parts(src_ptr, full_bytes) {
        write_4!(dst_ptr, k => *lut.get_unchecked(((b >> (k * 2)) & MASK_2BIT) as usize));
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
    } else {
      let full_groups = count / CHUNK_8;
      for g in 0..full_groups {
        let w = u16::from_le(src_ptr.add(g * 2).cast::<u16>().read_unaligned()) as u64;
        consumer.consume_8(
          arr_8!(k => (w >> (k * 2)) & (MASK_2BIT as u64)),
          dst_ptr.add(g * CHUNK_8),
        );
      }
      let rem_start = full_groups * CHUNK_8;
      let mut rem_i = rem_start;
      while rem_i < count {
        let bit_pos = (rem_i - rem_start) * 2;
        let byte_idx = full_groups * 2 + (bit_pos >> 3);
        let b = *src_ptr.add(byte_idx);
        consumer.consume_1(
          ((b >> (bit_pos & 7)) & MASK_2BIT) as u64,
          dst_ptr.add(rem_i),
        );
        rem_i += 1;
      }
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_4<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  mut dst_ptr: *mut T,
) {
  unsafe {
    if let Some(lut) = consumer.use_lut_4() {
      let full_bytes = count / CHUNK_2;
      let (byte_chunks, byte_rem) = from_raw_parts(src_ptr, full_bytes).as_chunks::<CHUNK_2>();
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
    } else {
      let full_groups = count / CHUNK_8;
      for g in 0..full_groups {
        let w = u32::from_le(src_ptr.add(g * 4).cast::<u32>().read_unaligned()) as u64;
        consumer.consume_8(
          arr_8!(k => (w >> (k * 4)) & (MASK_4BIT as u64)),
          dst_ptr.add(g * CHUNK_8),
        );
      }
      let rem_start = full_groups * CHUNK_8;
      let mut rem_i = rem_start;
      while rem_i < count {
        let bit_pos = (rem_i - rem_start) * 4;
        let byte_idx = full_groups * 4 + (bit_pos >> 3);
        let b = *src_ptr.add(byte_idx);
        consumer.consume_1(
          ((b >> (bit_pos & 7)) & MASK_4BIT) as u64,
          dst_ptr.add(rem_i),
        );
        rem_i += 1;
      }
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_8<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
) {
  let (chunks, rem) = unsafe { from_raw_parts(src_ptr, count).as_chunks::<CHUNK_8>() };
  let mut idx = 0;
  for chunk in chunks {
    unsafe {
      consumer.consume_8(chunk.map(|b| b as u64), dst_ptr.add(idx));
    }
    idx += CHUNK_8;
  }
  for (i, &b) in rem.iter().enumerate() {
    unsafe {
      consumer.consume_1(b as u64, dst_ptr.add(idx + i));
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_16<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
) {
  let src_ptr16 = src_ptr.cast::<u16>();
  let mut i = 0;
  while i + 8 <= count {
    unsafe {
      let chunk = u128::from_le(src_ptr.add(i * 2).cast::<u128>().read_unaligned());
      consumer.consume_8(
        arr_8!(k => ((chunk >> (k * 16)) as u16) as u64),
        dst_ptr.add(i),
      );
    }
    i += 8;
  }
  while i < count {
    unsafe {
      consumer.consume_1(
        u16::from_le(src_ptr16.add(i).read_unaligned()) as u64,
        dst_ptr.add(i),
      );
    }
    i += 1;
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_32<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
) {
  let src_ptr32 = src_ptr.cast::<u32>();
  let mut i = 0;
  while i + 8 <= count {
    unsafe {
      consumer.consume_8(
        arr_8!(k => u32::from_le(src_ptr32.add(i + k).read_unaligned()) as u64),
        dst_ptr.add(i),
      );
    }
    i += 8;
  }
  while i < count {
    unsafe {
      consumer.consume_1(
        u32::from_le(src_ptr32.add(i).read_unaligned()) as u64,
        dst_ptr.add(i),
      );
    }
    i += 1;
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_64<T: Copy, C: AlpConsumer<T>>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
) {
  unsafe {
    if consumer.consume_bulk_64(src_ptr, count, dst_ptr) {
      return;
    }
    let src_ptr64 = src_ptr.cast::<u64>();
    let mut i = 0;
    while i + 8 <= count {
      consumer.consume_8(
        arr_8!(k => u64::from_le(src_ptr64.add(i + k).read_unaligned())),
        dst_ptr.add(i),
      );
      i += 8;
    }
    while i < count {
      consumer.consume_1(
        u64::from_le(src_ptr64.add(i).read_unaligned()),
        dst_ptr.add(i),
      );
      i += 1;
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_le16<T: Copy, C: AlpConsumer<T>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
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
      consumer.consume_8(
        [
          w0 & mask,
          (w0 >> BW) & mask,
          (w0 >> (BW * 2)) & mask,
          (w0 >> (BW * 3)) & mask,
          w1 & mask,
          (w1 >> BW) & mask,
          (w1 >> (BW * 2)) & mask,
          (w1 >> (BW * 3)) & mask,
        ],
        dst_ptr.add(i),
      );

      let w2 = chunk1 as u64;
      let w3 = (chunk1 >> (BW * 4)) as u64;
      consumer.consume_8(
        [
          w2 & mask,
          (w2 >> BW) & mask,
          (w2 >> (BW * 2)) & mask,
          (w2 >> (BW * 3)) & mask,
          w3 & mask,
          (w3 >> BW) & mask,
          (w3 >> (BW * 2)) & mask,
          (w3 >> (BW * 3)) & mask,
        ],
        dst_ptr.add(i + 8),
      );

      byte_offset += BW * 2;
      i += 16;
    }

    while i + 8 <= fast_end_8 {
      let chunk = u128::from_le(src_ptr.add(byte_offset).cast::<u128>().read_unaligned());
      let w0 = chunk as u64;
      let w1 = (chunk >> (BW * 4)) as u64;
      consumer.consume_8(
        [
          w0 & mask,
          (w0 >> BW) & mask,
          (w0 >> (BW * 2)) & mask,
          (w0 >> (BW * 3)) & mask,
          w1 & mask,
          (w1 >> BW) & mask,
          (w1 >> (BW * 2)) & mask,
          (w1 >> (BW * 3)) & mask,
        ],
        dst_ptr.add(i),
      );
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
      consumer.consume_1((word >> (bit_pos & 7)) & mask, dst_ptr.add(i));
      i += 1;
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_17_to_32<T: Copy, C: AlpConsumer<T>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
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
      consumer.consume_8(
        [
          (chunk0 as u64) & mask,
          ((chunk0 >> BW) as u64) & mask,
          ((chunk0 >> (BW * 2)) as u64) & mask,
          ((chunk0 >> (BW * 3)) as u64) & mask,
          ((chunk1 >> mid_shift) as u64) & mask,
          ((chunk1 >> (mid_shift + BW)) as u64) & mask,
          ((chunk1 >> (mid_shift + BW * 2)) as u64) & mask,
          ((chunk1 >> (mid_shift + BW * 3)) as u64) & mask,
        ],
        dst_ptr.add(i),
      );
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
      consumer.consume_1((word >> (bit_pos & 7)) & mask, dst_ptr.add(i));
      i += 1;
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn unpack_33_to_64<T: Copy, C: AlpConsumer<T>, const BW: usize>(
  src_ptr: *const u8,
  count: usize,
  consumer: &mut C,
  dst_ptr: *mut T,
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

      consumer.consume_8([v0, v1, v2, v3, v4, v5, v6, v7], dst_ptr.add(i));

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
      consumer.consume_1(word & mask, dst_ptr.add(i));
      i += 1;
    }
  }
}

macro_rules! match_bw {
  (
    $bw:expr, ($src:expr, $count:expr, $consumer:expr, $dst:expr, $len:expr),
    le16: [$($w_le16:literal),*],
    w17_32: [$($w_17_32:literal),*],
    w33_64: [$($w_33_64:literal),*]
  ) => {
    match $bw {
      1 => $crate::bitpack::unpack::kernel::unpack_1($src, $count, $consumer, $dst),
      2 => $crate::bitpack::unpack::kernel::unpack_2($src, $count, $consumer, $dst),
      4 => $crate::bitpack::unpack::kernel::unpack_4($src, $count, $consumer, $dst),
      8 => $crate::bitpack::unpack::kernel::unpack_8($src, $count, $consumer, $dst),
      16 => $crate::bitpack::unpack::kernel::unpack_16($src, $count, $consumer, $dst),
      32 => $crate::bitpack::unpack::kernel::unpack_32($src, $count, $consumer, $dst),
      64 => $crate::bitpack::unpack::kernel::unpack_64($src, $count, $consumer, $dst),

      $(
        $w_le16 => {
          $crate::bitpack::unpack::kernel::unpack_le16::<_, _, $w_le16>(
            $src, $count, $consumer, $dst, $len,
          )
        }
      )*
      $(
        $w_17_32 => {
          $crate::bitpack::unpack::kernel::unpack_17_to_32::<_, _, $w_17_32>(
            $src, $count, $consumer, $dst, $len,
          )
        }
      )*
      $(
        $w_33_64 => {
          $crate::bitpack::unpack::kernel::unpack_33_to_64::<_, _, $w_33_64>(
            $src, $count, $consumer, $dst, $len,
          )
        }
      )*
      _ => core::hint::unreachable_unchecked(),
    }
  };
}

macro_rules! dispatch_bw {
  ($bw:expr, $src:expr, $count:expr, $consumer:expr, $dst:expr, $len:expr) => {
    $crate::bitpack::unpack::kernel::match_bw!(
      $bw, ($src, $count, $consumer, $dst, $len),
      le16: [3, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15],
      w17_32: [17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
      w33_64: [
        33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48,
        49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63
      ]
    )
  };
}

pub(crate) use dispatch_bw;
pub(crate) use match_bw;
