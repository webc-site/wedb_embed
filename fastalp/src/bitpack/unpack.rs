use crate::{
  bitpack::packed_byte_size,
  constants::{
    BITS_PER_BYTE, BITS_U64, BYTES_U64, LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT, LUT_SIZE_8BIT,
  },
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
  assert!(
    dst.len() >= count,
    "destination buffer too small: dst.len()={} < count={}",
    dst.len(),
    count
  );
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

    let mask = bit_mask(bit_width);
    let bw = bit_width as usize;
    let safe_limit_bytes = src.len().saturating_sub(BYTES_U64);
    let src_ptr = src.as_ptr();

    let mut i = 0;
    while i + 4 <= count {
      let bit_pos3 = (i + 3) * bw;
      if (bit_pos3 >> 3) <= safe_limit_bytes {
        let bit_pos0 = i * bw;
        let bit_pos1 = (i + 1) * bw;
        let bit_pos2 = (i + 2) * bw;
        let w0 = u64::from_le(src_ptr.add(bit_pos0 >> 3).cast::<u64>().read_unaligned());
        let w1 = u64::from_le(src_ptr.add(bit_pos1 >> 3).cast::<u64>().read_unaligned());
        let w2 = u64::from_le(src_ptr.add(bit_pos2 >> 3).cast::<u64>().read_unaligned());
        let w3 = u64::from_le(src_ptr.add(bit_pos3 >> 3).cast::<u64>().read_unaligned());
        *dst_ptr.add(i) = (w0 >> (bit_pos0 & 7)) & mask;
        *dst_ptr.add(i + 1) = (w1 >> (bit_pos1 & 7)) & mask;
        *dst_ptr.add(i + 2) = (w2 >> (bit_pos2 & 7)) & mask;
        *dst_ptr.add(i + 3) = (w3 >> (bit_pos3 & 7)) & mask;
        i += 4;
      } else {
        break;
      }
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
      *dst_ptr.add(i) = (word >> (bit_pos & 7)) & mask;
      i += 1;
    }
  }

  Ok(())
}

/// Fast bit unpacking: unpacks `count` integers of `bit_width` from `src` into `dst`.
/// 高速位解包：从 `src` 解包出 `count` 个 `bit_width` 位的整数至 `dst`
#[inline]
pub fn bitunpack_u64(src: &[u8], count: usize, bit_width: u8, dst: &mut Vec<u64>) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.resize(old_len + count, 0);
  bitunpack_u64_slice(src, count, bit_width, &mut dst[old_len..])
}

/// Generic zero-copy direct bit unpacking and floating-point reconstruction into `dst`.
/// 通用零拷贝直接解包并重构浮点数据至 `dst`
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

  let required_bytes = packed_byte_size(count, bit_width);
  if src.len() < required_bytes {
    return Err(Error::UnexpectedEof {
      needed: required_bytes,
      available: src.len(),
    });
  }

  if bit_width == 0 {
    let val = F::decode_from_offset(0, base, fac_int, frac_flt);
    dst.resize(dst.len() + count, val);
    return Ok(());
  }

  let old_len = dst.len();
  dst.reserve(count);

  // SAFETY:
  // 1. 上方已校验 src.len() >= required_bytes，保证读指针与各 bit_width 分支的 read_unaligned / 查表访问严格在合法内存范围内；
  // 2. dst 已预分配 dst.reserve(count)，写入 old_len..old_len+count 空间完全充足且无越界风险；
  // 3. 循环严格解码并初始化 count 个浮点元素后，调用 dst.set_len(old_len + count) 安全更新长度。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);

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
      dst.set_len(old_len + count);
      return Ok(());
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
      dst.set_len(old_len + count);
      return Ok(());
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
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_8 {
      let src_ptr = src.as_ptr();
      if fac_int == 1 {
        for i in 0..count {
          let off = *src_ptr.add(i) as u64;
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
        }
      } else {
        for i in 0..count {
          let off = *src_ptr.add(i) as u64;
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
        }
      }
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_16 {
      let src_ptr = src.as_ptr().cast::<u16>();
      if fac_int == 1 {
        for i in 0..count {
          let off = u16::from_le(src_ptr.add(i).read_unaligned()) as u64;
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
        }
      } else {
        for i in 0..count {
          let off = u16::from_le(src_ptr.add(i).read_unaligned()) as u64;
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
        }
      }
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_32 {
      let src_ptr = src.as_ptr().cast::<u32>();
      if fac_int == 1 {
        for i in 0..count {
          let off = u32::from_le(src_ptr.add(i).read_unaligned()) as u64;
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
        }
      } else {
        for i in 0..count {
          let off = u32::from_le(src_ptr.add(i).read_unaligned()) as u64;
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
        }
      }
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_64 {
      let src_ptr = src.as_ptr().cast::<u64>();
      if fac_int == 1 {
        for i in 0..count {
          let off = u64::from_le(src_ptr.add(i).read_unaligned());
          *dst_ptr.add(i) = F::decode_from_offset_fac1(off, base, frac_flt);
        }
      } else {
        for i in 0..count {
          let off = u64::from_le(src_ptr.add(i).read_unaligned());
          *dst_ptr.add(i) = F::decode_from_offset(off, base, fac_int, frac_flt);
        }
      }
      dst.set_len(old_len + count);
      return Ok(());
    }

    if bit_width < BITS_8 {
      let lut = F::build_lut::<LUT_SIZE_8BIT>(base, fac_int, frac_flt);
      let mask = bit_mask(bit_width);
      let mut acc: u128 = 0;
      let mut bits_in_acc: u32 = 0;
      let mut src_ptr = src.as_ptr();
      let src_end = src.as_ptr().add(src.len());

      let mut i = 0;
      while i < count && src_end.offset_from(src_ptr) >= BYTES_U64 as isize {
        if bits_in_acc < bit_width as u32 {
          let chunk = u64::from_le(src_ptr.cast::<u64>().read_unaligned());
          acc |= (chunk as u128) << bits_in_acc;
          bits_in_acc += BITS_U64 as u32;
          src_ptr = src_ptr.add(BYTES_U64);
        }
        let off = (acc as usize) & (mask as usize);
        acc >>= bit_width;
        bits_in_acc -= bit_width as u32;
        *dst_ptr = *lut.get_unchecked(off);
        dst_ptr = dst_ptr.add(1);
        i += 1;
      }

      while i < count {
        while bits_in_acc < bit_width as u32 && src_ptr < src_end {
          acc |= (*src_ptr as u128) << bits_in_acc;
          bits_in_acc += BITS_PER_BYTE as u32;
          src_ptr = src_ptr.add(1);
        }
        let off = (acc as usize) & (mask as usize);
        acc >>= bit_width;
        bits_in_acc = bits_in_acc.saturating_sub(bit_width as u32);
        *dst_ptr = *lut.get_unchecked(off);
        dst_ptr = dst_ptr.add(1);
        i += 1;
      }

      dst.set_len(old_len + count);
      return Ok(());
    }

    let mask = bit_mask(bit_width);
    let mut acc: u128 = 0;
    let mut bits_in_acc: u32 = 0;
    let mut src_ptr = src.as_ptr();
    let src_end = src.as_ptr().add(src.len());

    let mut i = 0;
    while i < count && src_end.offset_from(src_ptr) >= BYTES_U64 as isize {
      if bits_in_acc < bit_width as u32 {
        let chunk = u64::from_le(src_ptr.cast::<u64>().read_unaligned());
        acc |= (chunk as u128) << bits_in_acc;
        bits_in_acc += BITS_U64 as u32;
        src_ptr = src_ptr.add(BYTES_U64);
      }
      let off = (acc as u64) & mask;
      acc >>= bit_width;
      bits_in_acc -= bit_width as u32;
      *dst_ptr = F::decode_from_offset(off, base, fac_int, frac_flt);
      dst_ptr = dst_ptr.add(1);
      i += 1;
    }

    while i < count {
      while bits_in_acc < bit_width as u32 && src_ptr < src_end {
        acc |= (*src_ptr as u128) << bits_in_acc;
        bits_in_acc += BITS_PER_BYTE as u32;
        src_ptr = src_ptr.add(1);
      }
      let off = (acc as u64) & mask;
      acc >>= bit_width;
      bits_in_acc = bits_in_acc.saturating_sub(bit_width as u32);
      *dst_ptr = F::decode_from_offset(off, base, fac_int, frac_flt);
      dst_ptr = dst_ptr.add(1);
      i += 1;
    }

    dst.set_len(old_len + count);
  }

  Ok(())
}

/// Generic zero-copy direct bit unpacking and decimal division floating-point reconstruction into `dst`.
/// 通用零拷贝直接解包并采用十进制除法重构浮点数据至 `dst`
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

  let required_bytes = packed_byte_size(count, bit_width);
  if src.len() < required_bytes {
    return Err(Error::UnexpectedEof {
      needed: required_bytes,
      available: src.len(),
    });
  }

  if bit_width == 0 {
    let val = F::decode_from_offset_div(0, base, exp_factor);
    dst.resize(dst.len() + count, val);
    return Ok(());
  }

  let old_len = dst.len();
  dst.reserve(count);

  // SAFETY:
  // 1. 上方已校验 src.len() >= required_bytes，保证读指针在合法内存范围内；
  // 2. dst 已预分配 dst.reserve(count)，写入空间充足且无越界风险；
  // 3. 循环严格初始化 count 个元素后安全更新长度。
  unsafe {
    let mut dst_ptr = dst.as_mut_ptr().add(old_len);

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
      dst.set_len(old_len + count);
      return Ok(());
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
      dst.set_len(old_len + count);
      return Ok(());
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
      dst.set_len(old_len + count);
      return Ok(());
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
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_16 {
      let src_ptr = src.as_ptr().cast::<u16>();
      for i in 0..count {
        let off = u16::from_le(src_ptr.add(i).read_unaligned()) as u64;
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
      }
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_32 {
      let src_ptr = src.as_ptr().cast::<u32>();
      for i in 0..count {
        let off = u32::from_le(src_ptr.add(i).read_unaligned()) as u64;
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
      }
      dst.set_len(old_len + count);
      return Ok(());
    } else if bit_width == BITS_64 {
      let src_ptr = src.as_ptr().cast::<u64>();
      for i in 0..count {
        let off = u64::from_le(src_ptr.add(i).read_unaligned());
        *dst_ptr.add(i) = F::decode_from_offset_div(off, base, exp_factor);
      }
      dst.set_len(old_len + count);
      return Ok(());
    }

    if bit_width < BITS_8 {
      let lut = F::build_lut_div::<LUT_SIZE_8BIT>(base, exp_factor);
      let mask = bit_mask(bit_width);
      let mut acc: u128 = 0;
      let mut bits_in_acc: u32 = 0;
      let mut src_ptr = src.as_ptr();
      let src_end = src.as_ptr().add(src.len());

      let mut i = 0;
      while i < count && src_end.offset_from(src_ptr) >= BYTES_U64 as isize {
        if bits_in_acc < bit_width as u32 {
          let chunk = u64::from_le(src_ptr.cast::<u64>().read_unaligned());
          acc |= (chunk as u128) << bits_in_acc;
          bits_in_acc += BITS_U64 as u32;
          src_ptr = src_ptr.add(BYTES_U64);
        }
        let off = (acc as usize) & (mask as usize);
        acc >>= bit_width;
        bits_in_acc -= bit_width as u32;
        *dst_ptr = *lut.get_unchecked(off);
        dst_ptr = dst_ptr.add(1);
        i += 1;
      }

      while i < count {
        while bits_in_acc < bit_width as u32 && src_ptr < src_end {
          acc |= (*src_ptr as u128) << bits_in_acc;
          bits_in_acc += BITS_PER_BYTE as u32;
          src_ptr = src_ptr.add(1);
        }
        let off = (acc as usize) & (mask as usize);
        acc >>= bit_width;
        bits_in_acc = bits_in_acc.saturating_sub(bit_width as u32);
        *dst_ptr = *lut.get_unchecked(off);
        dst_ptr = dst_ptr.add(1);
        i += 1;
      }

      dst.set_len(old_len + count);
      return Ok(());
    }

    let mask = bit_mask(bit_width);
    let mut acc: u128 = 0;
    let mut bits_in_acc: u32 = 0;
    let mut src_ptr = src.as_ptr();
    let src_end = src.as_ptr().add(src.len());

    let mut i = 0;
    while i < count && src_end.offset_from(src_ptr) >= BYTES_U64 as isize {
      if bits_in_acc < bit_width as u32 {
        let chunk = u64::from_le(src_ptr.cast::<u64>().read_unaligned());
        acc |= (chunk as u128) << bits_in_acc;
        bits_in_acc += BITS_U64 as u32;
        src_ptr = src_ptr.add(BYTES_U64);
      }
      let off = (acc as u64) & mask;
      acc >>= bit_width;
      bits_in_acc -= bit_width as u32;
      *dst_ptr = F::decode_from_offset_div(off, base, exp_factor);
      dst_ptr = dst_ptr.add(1);
      i += 1;
    }

    while i < count {
      while bits_in_acc < bit_width as u32 && src_ptr < src_end {
        acc |= (*src_ptr as u128) << bits_in_acc;
        bits_in_acc += BITS_PER_BYTE as u32;
        src_ptr = src_ptr.add(1);
      }
      let off = (acc as u64) & mask;
      acc >>= bit_width;
      bits_in_acc = bits_in_acc.saturating_sub(bit_width as u32);
      *dst_ptr = F::decode_from_offset_div(off, base, exp_factor);
      dst_ptr = dst_ptr.add(1);
      i += 1;
    }

    dst.set_len(old_len + count);
  }

  Ok(())
}
