use core::ptr::copy_nonoverlapping;

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

  if bit_width <= 48 {
    let (chunks, rem) = values.as_chunks::<CHUNK_8>();

    // SAFETY: dst reserved (total_bytes + 16), writes stride bytes per 8 integers, trailing remainder filled, bounds safe
    // SAFETY: dst 已 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，余数在结尾填充，完全覆盖 total_bytes 且不越界。
    unsafe {
      let dst_start = dst.as_mut_ptr().add(old_len);
      let dst_ptr = match_pack_23!(
        bit_width,
        fallback => {
          let stride = bit_width as usize;
          let mask = bit_mask(bit_width);
          let mut p = dst_start;
          for chunk in chunks {
            pack_chunk_8(bit_width, mask_chunk_8(chunk, mask), p);
            p = p.add(stride);
          }
          p
        },
        |W| pack_u64_chunks::<W>(chunks, dst_start)
      );

      if !rem.is_empty() {
        pack_rem(rem.iter().copied(), bit_width, dst_ptr);
      }

      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == 56 {
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in values {
        dst_ptr.cast::<u64>().write_unaligned(v.to_le());
        dst_ptr = dst_ptr.add(7);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == 52 {
    let (pairs, rem) = values.as_chunks::<2>();
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &[v0, v1] in pairs {
        let w0 = v0 | (v1 << 52);
        let w1 = v1 >> 12;
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(w1.to_le());
        dst_ptr = dst_ptr.add(13);
      }
      if let Some(&last) = rem.first() {
        let acc = (last as u128).to_le();
        copy_nonoverlapping((&acc as *const u128).cast::<u8>(), dst_ptr, 7);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst reserved (total_bytes + 16), writes 8-byte LE sequence per element and safely updates length
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

  // SAFETY: dst pre-reserved (total_bytes + 16), writes 64-bit word or remainder directly, covers total_bytes
  // SAFETY: dst 已预先 reserve(total_bytes + 16)，通过指针直接写入累加器中的完整 64 位或剩余字节，完全覆盖 total_bytes。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);
    for &val in values {
      push_bits_to_stream(&mut acc, &mut bits, val & mask, bit_width, &mut dst_ptr);
    }
    flush_remaining_bits(acc, bits, dst_ptr);
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

  if bit_width <= 48 {
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_8>();

    // SAFETY: dst pre-reserved (total_bytes + 16), writes stride bytes per 8 integers, covers total_bytes
    // SAFETY: dst 已预先 reserve(total_bytes + 16)，按 8 个整数一组打包写入 stride 字节，完全覆盖 total_bytes。
    unsafe {
      let dst_start = dst.as_mut_ptr().add(old_len);
      let dst_ptr = match_pack_23!(
        bit_width,
        fallback => {
          let stride = bit_width as usize;
          let mut p = dst_start;
          for chunk in chunks {
            pack_chunk_8(bit_width, diff_chunk_8::<F>(chunk, base), p);
            p = p.add(stride);
          }
          p
        },
        |W| pack_encoded_chunks::<F, W>(chunks, base, dst_start)
      );

      pack_encoded_rem::<F>(rem, base, bit_width, dst_ptr);
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == 56 {
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in encoded_ints {
        let offset = F::int_diff_to_u64(v, base);
        dst_ptr.cast::<u64>().write_unaligned(offset.to_le());
        dst_ptr = dst_ptr.add(7);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == 52 {
    let (pairs, rem) = encoded_ints.as_chunks::<2>();
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &[v0, v1] in pairs {
        let off0 = F::int_diff_to_u64(v0, base);
        let off1 = F::int_diff_to_u64(v1, base);
        let w0 = off0 | (off1 << 52);
        let w1 = off1 >> 12;
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(w1.to_le());
        dst_ptr = dst_ptr.add(13);
      }
      if let Some(&last) = rem.first() {
        let off = F::int_diff_to_u64(last, base);
        let acc = (off as u128).to_le();
        copy_nonoverlapping((&acc as *const u128).cast::<u8>(), dst_ptr, 7);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst reserved (total_bytes + 16), writes 8-byte LE sequence per element
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

  // SAFETY: dst pre-reserved (total_bytes + 16), writes 64-bit word or remainder directly, covers total_bytes
  // SAFETY: dst 已预先 reserve(total_bytes + 16)，通过指针直接写入累加器中的完整 64 位或剩余字节，完全覆盖 total_bytes。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);
    for &val in encoded_ints {
      let offset = F::int_diff_to_u64(val, base);
      push_bits_to_stream(&mut acc, &mut bits, offset, bit_width, &mut dst_ptr);
    }
    flush_remaining_bits(acc, bits, dst_ptr);
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

  if bit_width <= 48 {
    unsafe {
      let dst_start = dst.as_mut_ptr().add(old_len);
      let raw_ptr = raw_ints.as_ptr();
      let dst_ptr = match_pack_23!(
        bit_width,
        fallback => {
          let stride = bit_width as usize;
          let mut p = dst_start;
          let mut r = raw_ptr;
          for _ in 0..num_chunks {
            pack_chunk_8(bit_width, delta_chunk_8::<F>(r, min_delta), p);
            p = p.add(stride);
            r = r.add(CHUNK_8);
          }
          p
        },
        |W| pack_fused_delta_chunks::<F, W>(raw_ptr, min_delta, num_chunks, dst_start)
      );

      pack_fused_delta_rem::<F>(raw_ints, rem_start, min_delta, bit_width, dst_ptr);
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst reserved (total_bytes + 16), writes 8-byte LE sequence per element
    // SAFETY: dst 已 reserve(total_bytes + 16)，逐元素写入 8-byte 小端序列。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for i in 1..raw_ints.len() {
        let delta = F::int_sub(*raw_ints.get_unchecked(i), *raw_ints.get_unchecked(i - 1));
        let offset = F::int_diff_to_u64(delta, min_delta);
        dst_ptr.cast::<u64>().write_unaligned(offset.to_le());
        dst_ptr = dst_ptr.add(BYTES_U64);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  }

  // Fallback for > 48 bits (except 64)
  let mut acc: u128 = 0;
  let mut bits: u32 = 0;
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);
    for i in 1..raw_ints.len() {
      let delta = F::int_sub(*raw_ints.get_unchecked(i), *raw_ints.get_unchecked(i - 1));
      let offset = F::int_diff_to_u64(delta, min_delta);
      push_bits_to_stream(&mut acc, &mut bits, offset, bit_width, &mut dst_ptr);
    }
    flush_remaining_bits(acc, bits, dst_ptr);
    dst.set_len(old_len + total_bytes);
  }
}

/// Dispatches packing of an 8-integer chunk across width tiers (1..=16, 17..=32, 33..=48).
/// 统一分发 8 整数块打包至不同位宽层级（零开销内联函数）
#[inline(always)]
unsafe fn pack_chunk_8(bit_width: u8, chunk: [u64; CHUNK_8], dst_ptr: *mut u8) {
  unsafe {
    match bit_width {
      1..=16 => pack_8_unrolled(bit_width, chunk, dst_ptr),
      20 => {
        let [o0, o1, o2, o3, o4, o5, o6, o7] = chunk;
        let p0 = o0 | (o1 << 20);
        let p1 = o2 | (o3 << 20);
        let p2 = o4 | (o5 << 20);
        let p3 = o6 | (o7 << 20);
        dst_ptr.cast::<u64>().write_unaligned(p0.to_le());
        dst_ptr.add(5).cast::<u64>().write_unaligned(p1.to_le());
        dst_ptr.add(10).cast::<u64>().write_unaligned(p2.to_le());
        dst_ptr.add(15).cast::<u64>().write_unaligned(p3.to_le());
      }
      24 => {
        let [o0, o1, o2, o3, o4, o5, o6, o7] = chunk;
        let w0 = o0 | (o1 << 24) | (o2 << 48);
        let w1 = (o2 >> 16) | (o3 << 8) | (o4 << 32) | (o5 << 56);
        let w2 = (o5 >> 8) | (o6 << 16) | (o7 << 40);
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(w1.to_le());
        dst_ptr.add(16).cast::<u64>().write_unaligned(w2.to_le());
      }
      28 => {
        let [o0, o1, o2, o3, o4, o5, o6, o7] = chunk;
        let p0 = o0 | (o1 << 28);
        let p1 = o2 | (o3 << 28);
        let p2 = o4 | (o5 << 28);
        let p3 = o6 | (o7 << 28);
        dst_ptr.cast::<u64>().write_unaligned(p0.to_le());
        dst_ptr.add(7).cast::<u64>().write_unaligned(p1.to_le());
        dst_ptr.add(14).cast::<u64>().write_unaligned(p2.to_le());
        dst_ptr.add(21).cast::<u64>().write_unaligned(p3.to_le());
      }
      32 => {
        let [o0, o1, o2, o3, o4, o5, o6, o7] = chunk;
        let w0 = o0 | (o1 << 32);
        let w1 = o2 | (o3 << 32);
        let w2 = o4 | (o5 << 32);
        let w3 = o6 | (o7 << 32);
        dst_ptr.cast::<u64>().write_unaligned(w0.to_le());
        dst_ptr.add(8).cast::<u64>().write_unaligned(w1.to_le());
        dst_ptr.add(16).cast::<u64>().write_unaligned(w2.to_le());
        dst_ptr.add(24).cast::<u64>().write_unaligned(w3.to_le());
      }
      17..=31 => pack_8_w17_to_w32(bit_width, chunk, dst_ptr),
      _ => pack_8_w33_to_w48(bit_width, chunk, dst_ptr),
    }
  }
}

#[inline(always)]
unsafe fn pack_encoded_chunks<F: AlpFloat, const W: u8>(
  chunks: &[[F::Int; CHUNK_8]],
  base: F::Int,
  mut dst_ptr: *mut u8,
) -> *mut u8 {
  let stride = W as usize;
  for chunk in chunks {
    unsafe {
      pack_chunk_8(W, diff_chunk_8::<F>(chunk, base), dst_ptr);
      dst_ptr = dst_ptr.add(stride);
    }
  }
  dst_ptr
}

#[inline(always)]
unsafe fn pack_u64_chunks<const W: u8>(chunks: &[[u64; CHUNK_8]], mut dst_ptr: *mut u8) -> *mut u8 {
  let stride = W as usize;
  let mask = bit_mask(W);
  for chunk in chunks {
    unsafe {
      pack_chunk_8(W, mask_chunk_8(chunk, mask), dst_ptr);
      dst_ptr = dst_ptr.add(stride);
    }
  }
  dst_ptr
}

#[inline(always)]
unsafe fn pack_fused_delta_chunks<F: AlpFloat, const W: u8>(
  raw_ptr: *const F::Int,
  min_delta: F::Int,
  num_chunks: usize,
  mut dst_ptr: *mut u8,
) -> *mut u8 {
  let stride = W as usize;
  let mut p = raw_ptr;
  for _ in 0..num_chunks {
    unsafe {
      pack_chunk_8(W, delta_chunk_8::<F>(p, min_delta), dst_ptr);
      dst_ptr = dst_ptr.add(stride);
      p = p.add(CHUNK_8);
    }
  }
  dst_ptr
}

/// Pushes an integer into the 128-bit bitpacking accumulator and flushes complete 64-bit words.
/// 将单个整数压入 128 位位打包累加器，当满 64 位时以小端字节序写入目标指针
#[inline(always)]
unsafe fn push_bits_to_stream(
  acc: &mut u128,
  bits: &mut u32,
  val: u64,
  bit_width: u8,
  dst_ptr: &mut *mut u8,
) {
  unsafe {
    *acc |= (val as u128) << *bits;
    *bits += bit_width as u32;
    if *bits >= BITS_U64 as u32 {
      dst_ptr.cast::<u64>().write_unaligned((*acc as u64).to_le());
      *dst_ptr = dst_ptr.add(BYTES_U64);
      *acc >>= BITS_U64;
      *bits -= BITS_U64 as u32;
    }
  }
}

/// Flushes remaining bits from accumulator into byte stream.
/// 将累加器中剩余未写出的比特逐字节刷出至目标流
#[inline(always)]
unsafe fn flush_remaining_bits(mut acc: u128, mut bits: u32, mut dst_ptr: *mut u8) {
  unsafe {
    while bits > 0 {
      *dst_ptr = acc as u8;
      dst_ptr = dst_ptr.add(1);
      acc >>= BITS_PER_BYTE;
      bits = bits.saturating_sub(BITS_PER_BYTE as u32);
    }
  }
}

/// Applies bitmask to an 8-element u64 array.
/// 为 8 元素 u64 数组批量施加位掩码
#[inline(always)]
fn mask_chunk_8(chunk: &[u64; CHUNK_8], mask: u64) -> [u64; CHUNK_8] {
  chunk.map(|x| x & mask)
}

/// Computes differences between an 8-element integer chunk and base as u64 array.
/// 计算 8 元素整数块相对于基准值的差值数组
#[inline(always)]
fn diff_chunk_8<F: AlpFloat>(chunk: &[F::Int; CHUNK_8], base: F::Int) -> [u64; CHUNK_8] {
  chunk.map(|v| F::int_diff_to_u64(v, base))
}

/// Computes fused 8-element delta offsets directly from raw pointer.
/// 从原始指针就地快速计算 8 个相邻一阶差分偏移量
#[inline(always)]
unsafe fn delta_chunk_8<F: AlpFloat>(raw_ptr: *const F::Int, min_delta: F::Int) -> [u64; CHUNK_8] {
  // SAFETY: Caller guarantees raw_ptr has readable range of at least 9 elements (prev + 8 values)
  // SAFETY: 调用方保证 raw_ptr 具有至少 9 个元素的有效读取范围 (prev + 8 values)
  unsafe {
    arr_8!(k => {
      let curr = *raw_ptr.add(k + 1);
      let prev = *raw_ptr.add(k);
      F::int_diff_to_u64(F::int_sub(curr, prev), min_delta)
    })
  }
}

/// Packs remainder encoded integers into destination buffer.
/// 打包剩余的差值编码整数
#[inline(always)]
unsafe fn pack_encoded_rem<F: AlpFloat>(
  rem: &[F::Int],
  base: F::Int,
  bit_width: u8,
  dst_ptr: *mut u8,
) {
  if !rem.is_empty() {
    unsafe {
      pack_rem(
        rem.iter().map(|&val| F::int_diff_to_u64(val, base)),
        bit_width,
        dst_ptr,
      );
    }
  }
}

/// Packs remainder fused delta integers into destination buffer.
/// 打包剩余的一阶差分整数
#[inline(always)]
unsafe fn pack_fused_delta_rem<F: AlpFloat>(
  raw_ints: &[F::Int],
  rem_start: usize,
  min_delta: F::Int,
  bit_width: u8,
  dst_ptr: *mut u8,
) {
  if rem_start < raw_ints.len() {
    let mut rem_deltas = [0u64; CHUNK_8];
    let rem_count = raw_ints.len() - rem_start;
    // SAFETY: rem_start + i < raw_ints.len()
    unsafe {
      for (i, dst_val) in rem_deltas[..rem_count].iter_mut().enumerate() {
        let curr = *raw_ints.get_unchecked(rem_start + i);
        let p = *raw_ints.get_unchecked(rem_start + i - 1);
        *dst_val = F::int_diff_to_u64(F::int_sub(curr, p), min_delta);
      }
      pack_rem(rem_deltas[..rem_count].iter().copied(), bit_width, dst_ptr);
    }
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
unsafe fn pack_8_unrolled(w: u8, o: [u64; 8], dst_ptr: *mut u8) {
  let [o0, o1, o2, o3, o4, o5, o6, o7] = o;
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
unsafe fn pack_8_w17_to_w32(w: u8, o: [u64; 8], dst_ptr: *mut u8) {
  let [o0, o1, o2, o3, o4, o5, o6, o7] = o;
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
