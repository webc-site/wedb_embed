use crate::{
  constants::{BITS_PER_BYTE, BITS_U64, BYTES_U64},
  float::AlpFloat,
  params::bit_mask,
};

const BITS_64: u8 = 64;

const CHUNK_8: usize = 8;

/// Calculates total bytes required to pack N W-bit integers.
/// 计算 N 个 W-bit 整数打包所需的总字节数
#[inline(always)]
pub const fn packed_byte_size(count: usize, bit_width: u8) -> usize {
  (count * (bit_width as usize)).div_ceil(BITS_PER_BYTE)
}

/// Fast bit packing: packs `values` into `dst`.
/// 高速位打包：将 `values` 打包入 `dst`
pub fn bitpack_u64(values: &[u64], bit_width: u8, dst: &mut Vec<u8>) {
  if values.is_empty() || bit_width == 0 {
    return;
  }

  let total_bytes = packed_byte_size(values.len(), bit_width);
  let old_len = dst.len();
  dst.reserve(total_bytes + 16);

  if bit_width <= 16 {
    let stride = bit_width as usize;
    let mask = bit_mask(bit_width);
    let (chunks, rem) = values.as_chunks::<CHUNK_8>();

    // SAFETY: dst 已 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，余数在结尾填充，完全覆盖 total_bytes 且不越界。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        pack_8_unrolled(
          bit_width,
          chunk[0] & mask,
          chunk[1] & mask,
          chunk[2] & mask,
          chunk[3] & mask,
          chunk[4] & mask,
          chunk[5] & mask,
          chunk[6] & mask,
          chunk[7] & mask,
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
      }

      if !rem.is_empty() {
        pack_rem(rem.iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width <= 32 {
    let stride = bit_width as usize;
    let mask = bit_mask(bit_width);
    let (chunks, rem) = values.as_chunks::<CHUNK_8>();

    // SAFETY: dst 已 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，余数在结尾填充，完全覆盖 total_bytes 且不越界。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        pack_8_w17_to_w32(
          bit_width,
          chunk[0] & mask,
          chunk[1] & mask,
          chunk[2] & mask,
          chunk[3] & mask,
          chunk[4] & mask,
          chunk[5] & mask,
          chunk[6] & mask,
          chunk[7] & mask,
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
      }

      if !rem.is_empty() {
        pack_rem(rem.iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width <= 48 {
    let stride = bit_width as usize;
    let mask = bit_mask(bit_width);
    let (chunks, rem) = values.as_chunks::<CHUNK_8>();

    // SAFETY: dst 已 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，完全覆盖 total_bytes 且不越界。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        pack_8_w33_to_w48(
          bit_width,
          [
            chunk[0] & mask,
            chunk[1] & mask,
            chunk[2] & mask,
            chunk[3] & mask,
            chunk[4] & mask,
            chunk[5] & mask,
            chunk[6] & mask,
            chunk[7] & mask,
          ],
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
      }

      if !rem.is_empty() {
        pack_rem(rem.iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst 已 reserve(total_bytes + 16)，逐元素写入 8-byte 小端序列后安全更新长度。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in values {
        dst_ptr.cast::<u64>().write_unaligned(v.to_le());
        dst_ptr = dst_ptr.add(BYTES_U64);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  }

  let mask = bit_mask(bit_width);
  let mut acc: u128 = 0;
  let mut bits: u32 = 0;

  // SAFETY: dst 已预先 reserve(total_bytes + 16)，通过指针直接写入累加器中的完整 64 位或剩余字节，完全覆盖 total_bytes。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);
    for &val in values {
      acc |= ((val & mask) as u128) << bits;
      bits += bit_width as u32;
      if bits >= BITS_U64 as u32 {
        dst_ptr.cast::<u64>().write_unaligned((acc as u64).to_le());
        dst_ptr = dst_ptr.add(BYTES_U64);
        acc >>= BITS_U64;
        bits -= BITS_U64 as u32;
      }
    }

    while bits > 0 {
      *dst_ptr = acc as u8;
      dst_ptr = dst_ptr.add(1);
      acc >>= BITS_PER_BYTE;
      bits = bits.saturating_sub(BITS_PER_BYTE as u32);
    }

    dst.set_len(old_len + total_bytes);
  }
}

/// Generic fast bit packing of encoded floating-point frame-of-reference deltas into `dst`.
/// 通用高速位打包已编码的浮点差值整数并直接写入 `dst`
pub fn bitpack_encoded<F: AlpFloat>(
  encoded_ints: &[F::Int],
  base: F::Int,
  bit_width: u8,
  dst: &mut Vec<u8>,
) {
  if encoded_ints.is_empty() || bit_width == 0 {
    return;
  }

  let total_bytes = packed_byte_size(encoded_ints.len(), bit_width);
  let old_len = dst.len();
  dst.reserve(total_bytes + 16);

  if bit_width <= 16 {
    let stride = bit_width as usize;
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_8>();

    // SAFETY: dst 已预先 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，完全覆盖 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        pack_8_unrolled(
          bit_width,
          F::int_diff_to_u64(chunk[0], base),
          F::int_diff_to_u64(chunk[1], base),
          F::int_diff_to_u64(chunk[2], base),
          F::int_diff_to_u64(chunk[3], base),
          F::int_diff_to_u64(chunk[4], base),
          F::int_diff_to_u64(chunk[5], base),
          F::int_diff_to_u64(chunk[6], base),
          F::int_diff_to_u64(chunk[7], base),
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
      }

      if !rem.is_empty() {
        pack_rem(
          rem.iter().map(|&val| F::int_diff_to_u64(val, base)),
          bit_width,
          dst_ptr,
        );
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width <= 32 {
    let stride = bit_width as usize;
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_8>();

    // SAFETY: dst 已预先 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，完全覆盖 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        pack_8_w17_to_w32(
          bit_width,
          F::int_diff_to_u64(chunk[0], base),
          F::int_diff_to_u64(chunk[1], base),
          F::int_diff_to_u64(chunk[2], base),
          F::int_diff_to_u64(chunk[3], base),
          F::int_diff_to_u64(chunk[4], base),
          F::int_diff_to_u64(chunk[5], base),
          F::int_diff_to_u64(chunk[6], base),
          F::int_diff_to_u64(chunk[7], base),
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
      }

      if !rem.is_empty() {
        pack_rem(
          rem.iter().map(|&val| F::int_diff_to_u64(val, base)),
          bit_width,
          dst_ptr,
        );
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width <= 48 {
    let stride = bit_width as usize;
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_8>();

    // SAFETY: dst 已预先 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，完全覆盖 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        pack_8_w33_to_w48(
          bit_width,
          [
            F::int_diff_to_u64(chunk[0], base),
            F::int_diff_to_u64(chunk[1], base),
            F::int_diff_to_u64(chunk[2], base),
            F::int_diff_to_u64(chunk[3], base),
            F::int_diff_to_u64(chunk[4], base),
            F::int_diff_to_u64(chunk[5], base),
            F::int_diff_to_u64(chunk[6], base),
            F::int_diff_to_u64(chunk[7], base),
          ],
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
      }

      if !rem.is_empty() {
        pack_rem(
          rem.iter().map(|&val| F::int_diff_to_u64(val, base)),
          bit_width,
          dst_ptr,
        );
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst 已 reserve(total_bytes + 16)，逐元素写入 8-byte 小端序列。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in encoded_ints {
        dst_ptr
          .cast::<u64>()
          .write_unaligned(F::int_diff_to_u64(v, base).to_le());
        dst_ptr = dst_ptr.add(BYTES_U64);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  }

  let mut acc: u128 = 0;
  let mut bits: u32 = 0;

  // SAFETY: dst 已预先 reserve(total_bytes + 16)，通过指针直接写入累加器中的完整 64 位或剩余字节，完全覆盖 total_bytes。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);
    for &val in encoded_ints {
      let offset = F::int_diff_to_u64(val, base);
      acc |= (offset as u128) << bits;
      bits += bit_width as u32;
      if bits >= BITS_U64 as u32 {
        dst_ptr.cast::<u64>().write_unaligned((acc as u64).to_le());
        dst_ptr = dst_ptr.add(BYTES_U64);
        acc >>= BITS_U64;
        bits -= BITS_U64 as u32;
      }
    }

    while bits > 0 {
      *dst_ptr = acc as u8;
      dst_ptr = dst_ptr.add(1);
      acc >>= BITS_PER_BYTE;
      bits = bits.saturating_sub(BITS_PER_BYTE as u32);
    }

    dst.set_len(old_len + total_bytes);
  }
}

/// Fused delta computation and bitpacking directly into `dst`.
/// 熔合一阶差分计算与位打包：直接从原始整型数组提取相邻差分并打包，完全省去原地差分内存写入。
pub fn bitpack_fused_delta<F: AlpFloat>(
  raw_ints: &[F::Int],
  min_delta: F::Int,
  bit_width: u8,
  dst: &mut Vec<u8>,
) {
  if raw_ints.len() <= 1 || bit_width == 0 {
    return;
  }

  let deltas_len = raw_ints.len() - 1;
  let total_bytes = packed_byte_size(deltas_len, bit_width);
  let old_len = dst.len();
  dst.reserve(total_bytes + 16);

  let num_chunks = deltas_len / CHUNK_8;
  let rem_start = 1 + num_chunks * CHUNK_8;

  if bit_width <= 16 {
    let stride = bit_width as usize;
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      let mut raw_ptr = raw_ints.as_ptr();
      for _ in 0..num_chunks {
        let prev = *raw_ptr;
        let x0 = *raw_ptr.add(1);
        let x1 = *raw_ptr.add(2);
        let x2 = *raw_ptr.add(3);
        let x3 = *raw_ptr.add(4);
        let x4 = *raw_ptr.add(5);
        let x5 = *raw_ptr.add(6);
        let x6 = *raw_ptr.add(7);
        let x7 = *raw_ptr.add(8);

        pack_8_unrolled(
          bit_width,
          F::int_diff_to_u64(F::int_sub(x0, prev), min_delta),
          F::int_diff_to_u64(F::int_sub(x1, x0), min_delta),
          F::int_diff_to_u64(F::int_sub(x2, x1), min_delta),
          F::int_diff_to_u64(F::int_sub(x3, x2), min_delta),
          F::int_diff_to_u64(F::int_sub(x4, x3), min_delta),
          F::int_diff_to_u64(F::int_sub(x5, x4), min_delta),
          F::int_diff_to_u64(F::int_sub(x6, x5), min_delta),
          F::int_diff_to_u64(F::int_sub(x7, x6), min_delta),
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
        raw_ptr = raw_ptr.add(CHUNK_8);
      }

      if rem_start < raw_ints.len() {
        let mut rem_deltas = [0u64; CHUNK_8];
        let rem_count = raw_ints.len() - rem_start;
        for (i, dst_val) in rem_deltas[..rem_count].iter_mut().enumerate() {
          let curr = *raw_ints.get_unchecked(rem_start + i);
          let p = *raw_ints.get_unchecked(rem_start + i - 1);
          *dst_val = F::int_diff_to_u64(F::int_sub(curr, p), min_delta);
        }
        pack_rem(rem_deltas[..rem_count].iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width <= 32 {
    let stride = bit_width as usize;
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      let mut raw_ptr = raw_ints.as_ptr();
      for _ in 0..num_chunks {
        let prev = *raw_ptr;
        let x0 = *raw_ptr.add(1);
        let x1 = *raw_ptr.add(2);
        let x2 = *raw_ptr.add(3);
        let x3 = *raw_ptr.add(4);
        let x4 = *raw_ptr.add(5);
        let x5 = *raw_ptr.add(6);
        let x6 = *raw_ptr.add(7);
        let x7 = *raw_ptr.add(8);

        pack_8_w17_to_w32(
          bit_width,
          F::int_diff_to_u64(F::int_sub(x0, prev), min_delta),
          F::int_diff_to_u64(F::int_sub(x1, x0), min_delta),
          F::int_diff_to_u64(F::int_sub(x2, x1), min_delta),
          F::int_diff_to_u64(F::int_sub(x3, x2), min_delta),
          F::int_diff_to_u64(F::int_sub(x4, x3), min_delta),
          F::int_diff_to_u64(F::int_sub(x5, x4), min_delta),
          F::int_diff_to_u64(F::int_sub(x6, x5), min_delta),
          F::int_diff_to_u64(F::int_sub(x7, x6), min_delta),
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
        raw_ptr = raw_ptr.add(CHUNK_8);
      }

      if rem_start < raw_ints.len() {
        let mut rem_deltas = [0u64; CHUNK_8];
        let rem_count = raw_ints.len() - rem_start;
        for (i, dst_val) in rem_deltas[..rem_count].iter_mut().enumerate() {
          let curr = *raw_ints.get_unchecked(rem_start + i);
          let p = *raw_ints.get_unchecked(rem_start + i - 1);
          *dst_val = F::int_diff_to_u64(F::int_sub(curr, p), min_delta);
        }
        pack_rem(rem_deltas[..rem_count].iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width <= 48 {
    let stride = bit_width as usize;
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      let mut raw_ptr = raw_ints.as_ptr();
      for _ in 0..num_chunks {
        let prev = *raw_ptr;
        let x0 = *raw_ptr.add(1);
        let x1 = *raw_ptr.add(2);
        let x2 = *raw_ptr.add(3);
        let x3 = *raw_ptr.add(4);
        let x4 = *raw_ptr.add(5);
        let x5 = *raw_ptr.add(6);
        let x6 = *raw_ptr.add(7);
        let x7 = *raw_ptr.add(8);

        pack_8_w33_to_w48(
          bit_width,
          [
            F::int_diff_to_u64(F::int_sub(x0, prev), min_delta),
            F::int_diff_to_u64(F::int_sub(x1, x0), min_delta),
            F::int_diff_to_u64(F::int_sub(x2, x1), min_delta),
            F::int_diff_to_u64(F::int_sub(x3, x2), min_delta),
            F::int_diff_to_u64(F::int_sub(x4, x3), min_delta),
            F::int_diff_to_u64(F::int_sub(x5, x4), min_delta),
            F::int_diff_to_u64(F::int_sub(x6, x5), min_delta),
            F::int_diff_to_u64(F::int_sub(x7, x6), min_delta),
          ],
          dst_ptr,
        );
        dst_ptr = dst_ptr.add(stride);
        raw_ptr = raw_ptr.add(CHUNK_8);
      }

      if rem_start < raw_ints.len() {
        let mut rem_deltas = [0u64; CHUNK_8];
        let rem_count = raw_ints.len() - rem_start;
        for (i, dst_val) in rem_deltas[..rem_count].iter_mut().enumerate() {
          let curr = *raw_ints.get_unchecked(rem_start + i);
          let p = *raw_ints.get_unchecked(rem_start + i - 1);
          *dst_val = F::int_diff_to_u64(F::int_sub(curr, p), min_delta);
        }
        pack_rem(rem_deltas[..rem_count].iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  }

  // Fallback for > 48 bits
  let mut acc: u128 = 0;
  let mut bits: u32 = 0;
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);
    for i in 1..raw_ints.len() {
      let delta = F::int_sub(raw_ints[i], raw_ints[i - 1]);
      let offset = F::int_diff_to_u64(delta, min_delta);
      acc |= (offset as u128) << bits;
      bits += bit_width as u32;
      while bits >= BITS_U64 as u32 {
        dst_ptr.cast::<u64>().write_unaligned((acc as u64).to_le());
        dst_ptr = dst_ptr.add(BYTES_U64);
        acc >>= BITS_U64;
        bits -= BITS_U64 as u32;
      }
    }
    while bits > 0 {
      *dst_ptr = acc as u8;
      dst_ptr = dst_ptr.add(1);
      acc >>= BITS_PER_BYTE;
      bits = bits.saturating_sub(BITS_PER_BYTE as u32);
    }
    dst.set_len(old_len + total_bytes);
  }
}

/// Safely packs remainder integers (< 8) without 128-bit shift overflow.
/// 安全打包余数整数（少于 8 个），绝无 128-bit 位移溢出
#[inline(always)]
unsafe fn pack_rem(rem: impl IntoIterator<Item = u64>, bit_width: u8, mut dst_ptr: *mut u8) {
  let mask = bit_mask(bit_width);
  let mut acc: u128 = 0;
  let mut bits: u32 = 0;
  unsafe {
    for val in rem {
      acc |= ((val & mask) as u128) << bits;
      bits += bit_width as u32;
      while bits >= 64 {
        dst_ptr.cast::<u64>().write_unaligned((acc as u64).to_le());
        dst_ptr = dst_ptr.add(8);
        acc >>= 64;
        bits -= 64;
      }
    }
    while bits > 0 {
      *dst_ptr = acc as u8;
      dst_ptr = dst_ptr.add(1);
      acc >>= BITS_PER_BYTE;
      bits = bits.saturating_sub(BITS_PER_BYTE as u32);
    }
  }
}

/// Unrolls 8 integers into packed bytes for any bit width from 1 to 16.
/// 针对比特位宽 1~16 的 8 整数向量化就地循环展开打包
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn pack_8_unrolled(
  w: u8,
  o0: u64,
  o1: u64,
  o2: u64,
  o3: u64,
  o4: u64,
  o5: u64,
  o6: u64,
  o7: u64,
  dst_ptr: *mut u8,
) {
  unsafe {
    match w {
      1 => {
        let b = (o0 as u8 & 1)
          | ((o1 as u8 & 1) << 1)
          | ((o2 as u8 & 1) << 2)
          | ((o3 as u8 & 1) << 3)
          | ((o4 as u8 & 1) << 4)
          | ((o5 as u8 & 1) << 5)
          | ((o6 as u8 & 1) << 6)
          | ((o7 as u8 & 1) << 7);
        *dst_ptr = b;
      }
      2 => {
        let b0 =
          (o0 as u8 & 3) | ((o1 as u8 & 3) << 2) | ((o2 as u8 & 3) << 4) | ((o3 as u8 & 3) << 6);
        let b1 =
          (o4 as u8 & 3) | ((o5 as u8 & 3) << 2) | ((o6 as u8 & 3) << 4) | ((o7 as u8 & 3) << 6);
        dst_ptr
          .cast::<u16>()
          .write_unaligned(u16::from_le_bytes([b0, b1]));
      }
      3 => {
        let word = (o0
          | (o1 << 3)
          | (o2 << 6)
          | (o3 << 9)
          | (o4 << 12)
          | (o5 << 15)
          | (o6 << 18)
          | (o7 << 21)) as u32;
        dst_ptr.cast::<u32>().write_unaligned(word.to_le());
      }
      4 => {
        let b0 = (o0 as u8 & 0xf) | ((o1 as u8 & 0xf) << 4);
        let b1 = (o2 as u8 & 0xf) | ((o3 as u8 & 0xf) << 4);
        let b2 = (o4 as u8 & 0xf) | ((o5 as u8 & 0xf) << 4);
        let b3 = (o6 as u8 & 0xf) | ((o7 as u8 & 0xf) << 4);
        dst_ptr
          .cast::<u32>()
          .write_unaligned(u32::from_le_bytes([b0, b1, b2, b3]));
      }
      5 => {
        let word = o0
          | (o1 << 5)
          | (o2 << 10)
          | (o3 << 15)
          | (o4 << 20)
          | (o5 << 25)
          | (o6 << 30)
          | (o7 << 35);
        dst_ptr.cast::<u64>().write_unaligned(word.to_le());
      }
      6 => {
        let word = o0
          | (o1 << 6)
          | (o2 << 12)
          | (o3 << 18)
          | (o4 << 24)
          | (o5 << 30)
          | (o6 << 36)
          | (o7 << 42);
        dst_ptr.cast::<u64>().write_unaligned(word.to_le());
      }
      7 => {
        let word = o0
          | (o1 << 7)
          | (o2 << 14)
          | (o3 << 21)
          | (o4 << 28)
          | (o5 << 35)
          | (o6 << 42)
          | (o7 << 49);
        dst_ptr.cast::<u64>().write_unaligned(word.to_le());
      }
      8 => {
        let word = o0
          | (o1 << 8)
          | (o2 << 16)
          | (o3 << 24)
          | (o4 << 32)
          | (o5 << 40)
          | (o6 << 48)
          | (o7 << 56);
        dst_ptr.cast::<u64>().write_unaligned(word.to_le());
      }
      9 => {
        let w0 = o0 | (o1 << 9) | (o2 << 18) | (o3 << 27);
        let w1 = o4 | (o5 << 9) | (o6 << 18) | (o7 << 27);
        let low = w0 | (w1 << 36);
        let high = (w1 >> 28) as u8;
        dst_ptr.cast::<u64>().write_unaligned(low.to_le());
        *dst_ptr.add(8) = high;
      }
      10 => {
        let w0 = o0 | (o1 << 10) | (o2 << 20) | (o3 << 30);
        let w1 = o4 | (o5 << 10) | (o6 << 20) | (o7 << 30);
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(5).cast::<u64>().write_unaligned(w1.to_le());
      }
      11 => {
        let w0 = o0 | (o1 << 11) | (o2 << 22) | (o3 << 33);
        let w1 = o4 | (o5 << 11) | (o6 << 22) | (o7 << 33);
        let low = w0 | (w1 << 44);
        let high = (w1 >> 20) as u32;
        dst_ptr.cast::<u64>().write_unaligned(low.to_le());
        dst_ptr.add(8).cast::<u32>().write_unaligned(high.to_le());
      }
      12 => {
        let w0 = o0 | (o1 << 12) | (o2 << 24) | (o3 << 36);
        let w1 = o4 | (o5 << 12) | (o6 << 24) | (o7 << 36);
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(6).cast::<u64>().write_unaligned(w1.to_le());
      }
      13 => {
        let w0 = o0 | (o1 << 13) | (o2 << 26) | (o3 << 39);
        let w1 = o4 | (o5 << 13) | (o6 << 26) | (o7 << 39);
        let low = w0 | (w1 << 52);
        let high = w1 >> 12;
        dst_ptr.cast::<u64>().write_unaligned(low.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(high.to_le());
      }
      14 => {
        let w0 = o0 | (o1 << 14) | (o2 << 28) | (o3 << 42);
        let w1 = o4 | (o5 << 14) | (o6 << 28) | (o7 << 42);
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(7).cast::<u64>().write_unaligned(w1.to_le());
      }
      15 => {
        let w0 = o0 | (o1 << 15) | (o2 << 30) | (o3 << 45);
        let w1 = o4 | (o5 << 15) | (o6 << 30) | (o7 << 45);
        let low = w0 | (w1 << 60);
        let high = w1 >> 4;
        dst_ptr.cast::<u64>().write_unaligned(low.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(high.to_le());
      }
      16 => {
        let w0 = o0 | (o1 << 16) | (o2 << 32) | (o3 << 48);
        let w1 = o4 | (o5 << 16) | (o6 << 32) | (o7 << 48);
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(w1.to_le());
      }
      _ => {}
    }
  }
}

#[inline(always)]
unsafe fn write_pair(p: u64, w2: u32, b: usize, dst_ptr: *mut u8) {
  let byte_idx = b / 8;
  let shift = (b % 8) as u32;
  unsafe {
    if shift == 0 {
      dst_ptr
        .add(byte_idx)
        .cast::<u64>()
        .write_unaligned(p.to_le());
    } else {
      let existing = *dst_ptr.add(byte_idx);
      let combined = (p << shift) | (existing as u64);
      dst_ptr
        .add(byte_idx)
        .cast::<u64>()
        .write_unaligned(combined.to_le());
      if w2 + shift > 64 {
        *dst_ptr.add(byte_idx + 8) = (p >> (64 - shift)) as u8;
      }
    }
  }
}

/// Packs 8 integers of width 17..=32 into bytes.
/// 打包位宽在 17~32 之间的 8 个整数
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn pack_8_w17_to_w32(
  w: u8,
  o0: u64,
  o1: u64,
  o2: u64,
  o3: u64,
  o4: u64,
  o5: u64,
  o6: u64,
  o7: u64,
  dst_ptr: *mut u8,
) {
  let w_u32 = w as u32;
  let w2 = w_u32 * 2;
  let p0 = o0 | (o1 << w_u32);
  let p1 = o2 | (o3 << w_u32);
  let p2 = o4 | (o5 << w_u32);
  let p3 = o6 | (o7 << w_u32);

  unsafe {
    write_pair(p0, w2, 0, dst_ptr);
    write_pair(p1, w2, w as usize * 2, dst_ptr);
    write_pair(p2, w2, w as usize * 4, dst_ptr);
    write_pair(p3, w2, w as usize * 6, dst_ptr);
  }
}

/// Packs 8 integers of width 33..=48 into bytes.
/// 打包位宽在 33~48 之间的 8 个整数
#[inline(always)]
unsafe fn pack_8_w33_to_w48(w: u8, o: [u64; 8], dst_ptr: *mut u8) {
  unsafe {
    for (i, &val) in o.iter().enumerate() {
      let b = i * (w as usize);
      let byte_idx = b / 8;
      let shift = (b % 8) as u32;
      if shift == 0 {
        dst_ptr
          .add(byte_idx)
          .cast::<u64>()
          .write_unaligned(val.to_le());
      } else {
        let existing = *dst_ptr.add(byte_idx);
        let combined = (val << shift) | (existing as u64);
        dst_ptr
          .add(byte_idx)
          .cast::<u64>()
          .write_unaligned(combined.to_le());
      }
    }
  }
}
