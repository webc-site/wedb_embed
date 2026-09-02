use crate::{
  constants::{BITS_PER_BYTE, BITS_U64, BYTES_U16, BYTES_U32, BYTES_U64},
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
  dst.reserve(total_bytes);

  if bit_width == BITS_1 {
    let (chunks, rem) = values.as_chunks::<CHUNK_8>();
    // SAFETY: dst 已 reserve(total_bytes)，按 8 个整数一组打包写入 chunks.len() 字节，余数最多写入 1 字节，刚好填满 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        let o0 = (chunk[0] as u8) & MASK_1BIT;
        let o1 = (chunk[1] as u8) & MASK_1BIT;
        let o2 = (chunk[2] as u8) & MASK_1BIT;
        let o3 = (chunk[3] as u8) & MASK_1BIT;
        let o4 = (chunk[4] as u8) & MASK_1BIT;
        let o5 = (chunk[5] as u8) & MASK_1BIT;
        let o6 = (chunk[6] as u8) & MASK_1BIT;
        let o7 = (chunk[7] as u8) & MASK_1BIT;
        *dst_ptr =
          o0 | (o1 << 1) | (o2 << 2) | (o3 << 3) | (o4 << 4) | (o5 << 5) | (o6 << 6) | (o7 << 7);
        dst_ptr = dst_ptr.add(1);
      }
      if !rem.is_empty() {
        let mut b = 0u8;
        for (i, &val) in rem.iter().enumerate() {
          let o = (val as u8) & MASK_1BIT;
          b |= o << i;
        }
        *dst_ptr = b;
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_2 {
    let (chunks, rem) = values.as_chunks::<CHUNK_4>();
    // SAFETY: dst 已 reserve(total_bytes)，按 4 个整数一组打包写入 chunks.len() 字节，余数最多写入 1 字节，刚好填满 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        let o0 = (chunk[0] as u8) & MASK_2BIT;
        let o1 = (chunk[1] as u8) & MASK_2BIT;
        let o2 = (chunk[2] as u8) & MASK_2BIT;
        let o3 = (chunk[3] as u8) & MASK_2BIT;
        *dst_ptr = o0 | (o1 << 2) | (o2 << 4) | (o3 << 6);
        dst_ptr = dst_ptr.add(1);
      }
      if !rem.is_empty() {
        let mut b = 0u8;
        for (i, &val) in rem.iter().enumerate() {
          let o = (val as u8) & MASK_2BIT;
          b |= o << (i * 2);
        }
        *dst_ptr = b;
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_4 {
    let (chunks, rem) = values.as_chunks::<CHUNK_2>();
    // SAFETY: dst 已 reserve(total_bytes)，按 2 个整数一组打包写入 chunks.len() 字节，余数最多写入 1 字节，刚好填满 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        let o0 = (chunk[0] as u8) & MASK_4BIT;
        let o1 = (chunk[1] as u8) & MASK_4BIT;
        *dst_ptr = o0 | (o1 << 4);
        dst_ptr = dst_ptr.add(1);
      }
      if let Some(&last) = rem.first() {
        let o0 = (last as u8) & MASK_4BIT;
        *dst_ptr = o0;
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_8 {
    // SAFETY: dst 已 reserve(total_bytes)，且循环严格写入 values.len() 个字节，写入完成后调用 set_len 确保内存全部初始化完毕。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in values {
        *dst_ptr = v as u8;
        dst_ptr = dst_ptr.add(1);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_16 {
    // SAFETY: dst 已 reserve(total_bytes)，逐元素写入 2-byte 小端序列后安全更新长度。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in values {
        dst_ptr.cast::<u16>().write_unaligned((v as u16).to_le());
        dst_ptr = dst_ptr.add(BYTES_U16);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_32 {
    // SAFETY: dst 已 reserve(total_bytes)，逐元素写入 4-byte 小端序列后安全更新长度。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in values {
        dst_ptr.cast::<u32>().write_unaligned((v as u32).to_le());
        dst_ptr = dst_ptr.add(BYTES_U32);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst 已 reserve(total_bytes)，逐元素写入 8-byte 小端序列后安全更新长度。
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

  // SAFETY: dst 已预先 reserve(total_bytes)，通过指针直接写入累加器中的完整 64 位或剩余字节，完全覆盖 total_bytes。
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
  dst.reserve(total_bytes);

  if bit_width == BITS_1 {
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_8>();
    // SAFETY: dst 已 reserve(total_bytes)，按 8 个整数一组打包写入 chunks.len() 字节，余数最多写入 1 字节，刚好填满 total_bytes，无越界与未初始化。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        let o0 = (F::int_diff_to_u64(chunk[0], base) as u8) & MASK_1BIT;
        let o1 = (F::int_diff_to_u64(chunk[1], base) as u8) & MASK_1BIT;
        let o2 = (F::int_diff_to_u64(chunk[2], base) as u8) & MASK_1BIT;
        let o3 = (F::int_diff_to_u64(chunk[3], base) as u8) & MASK_1BIT;
        let o4 = (F::int_diff_to_u64(chunk[4], base) as u8) & MASK_1BIT;
        let o5 = (F::int_diff_to_u64(chunk[5], base) as u8) & MASK_1BIT;
        let o6 = (F::int_diff_to_u64(chunk[6], base) as u8) & MASK_1BIT;
        let o7 = (F::int_diff_to_u64(chunk[7], base) as u8) & MASK_1BIT;
        *dst_ptr =
          o0 | (o1 << 1) | (o2 << 2) | (o3 << 3) | (o4 << 4) | (o5 << 5) | (o6 << 6) | (o7 << 7);
        dst_ptr = dst_ptr.add(1);
      }
      if !rem.is_empty() {
        let mut b = 0u8;
        for (i, &val) in rem.iter().enumerate() {
          let o = (F::int_diff_to_u64(val, base) as u8) & MASK_1BIT;
          b |= o << i;
        }
        *dst_ptr = b;
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_2 {
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_4>();
    // SAFETY: dst 已 reserve(total_bytes)，按 4 个整数一组打包写入 chunks.len() 字节，余数最多写入 1 字节，刚好填满 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        let o0 = (F::int_diff_to_u64(chunk[0], base) as u8) & MASK_2BIT;
        let o1 = (F::int_diff_to_u64(chunk[1], base) as u8) & MASK_2BIT;
        let o2 = (F::int_diff_to_u64(chunk[2], base) as u8) & MASK_2BIT;
        let o3 = (F::int_diff_to_u64(chunk[3], base) as u8) & MASK_2BIT;
        *dst_ptr = o0 | (o1 << 2) | (o2 << 4) | (o3 << 6);
        dst_ptr = dst_ptr.add(1);
      }
      if !rem.is_empty() {
        let mut b = 0u8;
        for (i, &val) in rem.iter().enumerate() {
          let o = (F::int_diff_to_u64(val, base) as u8) & MASK_2BIT;
          b |= o << (i * 2);
        }
        *dst_ptr = b;
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_4 {
    let (chunks, rem) = encoded_ints.as_chunks::<CHUNK_2>();
    // SAFETY: dst 已 reserve(total_bytes)，按 2 个整数一组打包写入 chunks.len() 字节，余数最多写入 1 字节，刚好填满 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for chunk in chunks {
        let o0 = (F::int_diff_to_u64(chunk[0], base) as u8) & MASK_4BIT;
        let o1 = (F::int_diff_to_u64(chunk[1], base) as u8) & MASK_4BIT;
        *dst_ptr = o0 | (o1 << 4);
        dst_ptr = dst_ptr.add(1);
      }
      if let Some(&last) = rem.first() {
        let o0 = (F::int_diff_to_u64(last, base) as u8) & MASK_4BIT;
        *dst_ptr = o0;
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_8 {
    // SAFETY: dst 已 reserve(encoded_ints.len())，逐元素写入 u8，完全覆盖 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in encoded_ints {
        *dst_ptr = F::int_diff_to_u64(v, base) as u8;
        dst_ptr = dst_ptr.add(1);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_16 {
    // SAFETY: dst 已 reserve(total_bytes)，逐元素写入 2-byte 小端序列，完全覆盖 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in encoded_ints {
        dst_ptr
          .cast::<u16>()
          .write_unaligned((F::int_diff_to_u64(v, base) as u16).to_le());
        dst_ptr = dst_ptr.add(BYTES_U16);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_32 {
    // SAFETY: dst 已 reserve(total_bytes)，逐元素写入 4-byte 小端序列，完全覆盖 total_bytes。
    unsafe {
      let mut dst_ptr = dst.as_mut_ptr().add(old_len);
      for &v in encoded_ints {
        dst_ptr
          .cast::<u32>()
          .write_unaligned((F::int_diff_to_u64(v, base) as u32).to_le());
        dst_ptr = dst_ptr.add(BYTES_U32);
      }
      dst.set_len(old_len + total_bytes);
    }
    return;
  } else if bit_width == BITS_64 {
    // SAFETY: dst 已 reserve(total_bytes)，逐元素写入 8-byte 小端序列，完全覆盖 total_bytes。
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

  // SAFETY: dst 已预先 reserve(total_bytes)，通过指针直接写入累加器中的完整 64 位或剩余字节，完全覆盖 total_bytes。
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
