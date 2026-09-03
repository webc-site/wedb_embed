pub use super::r#const::*;
use crate::{
  bitmap::opt::{
    BitOp, BitfieldEncoding, BitfieldOpType, BitfieldOperation, BitfieldOverflow, BitfieldValue,
  },
  error::{Error, Result},
};

/// Computes segment index for the given bit offset (aligned with Kvrocks SegmentSubKeyIndexForBit).
/// 计算指定位偏移所属的分段索引（对标 Kvrocks SegmentSubKeyIndexForBit / kBitmapSegmentBits）
#[inline]
pub const fn segment_index_for_bit(bit_offset: u64) -> u32 {
  (bit_offset / (BITMAP_SEGMENT_BITS as u64)) as u32
}

/// Computes starting byte offset of segment for the given bit offset.
/// 计算指定位偏移所属分段的字节起点偏移
#[inline]
pub const fn segment_byte_offset_for_bit(bit_offset: u64) -> u32 {
  segment_index_for_bit(bit_offset) * (BITMAP_SEGMENT_BYTES as u32)
}

/// Expands bitmap segment capacity up to min_bytes (aligned with Kvrocks ExpandBitmapSegment).
/// 扩展分段字节容量至 min_bytes（按需倍增至 1024 字节，对标 Kvrocks ExpandBitmapSegment）
#[inline]
pub fn expand_bitmap_segment(segment: &mut Vec<u8>, min_bytes: usize) {
  debug_assert!(min_bytes <= BITMAP_SEGMENT_BYTES);
  let old_size = segment.len();
  if min_bytes > old_size {
    let new_size = (old_size * 2).clamp(min_bytes, BITMAP_SEGMENT_BYTES);
    segment.resize(new_size, 0);
  }
}

/// Gets bit value in segment using LSB order (aligned with Kvrocks util::lsb::GetBit).
/// 获取分段中指定位的值（LSB 顺序，对标 Apache Kvrocks util::lsb::GetBit）
#[inline]
pub fn get_bit_lsb(segment: &[u8], bit_offset_in_segment: usize) -> u8 {
  let byte_idx = bit_offset_in_segment >> 3;
  if byte_idx < segment.len() {
    (segment[byte_idx] >> (bit_offset_in_segment & 7)) & 1
  } else {
    0
  }
}

/// Sets bit value in segment using LSB order, returning old bit (aligned with Kvrocks SetBitTo).
/// 设置分段中指定位的值（LSB 顺序，返回原位值，对标 Apache Kvrocks util::lsb::SetBitTo）
#[inline]
pub fn set_bit_lsb(segment: &mut [u8], bit_offset_in_segment: usize, bit: u8) -> u8 {
  let byte_idx = bit_offset_in_segment >> 3;
  let shift = bit_offset_in_segment & 7;
  let old = (segment[byte_idx] >> shift) & 1;
  if bit != 0 {
    segment[byte_idx] |= 1 << shift;
  } else {
    segment[byte_idx] &= !(1 << shift);
  }
  old
}

/// Gets bit value in byte slice using MSB order.
/// 获取连续字节中指定位的值（MSB 顺序，用于标准 Redis 兼容格式）
#[inline]
pub fn get_bit_from_bytes(bytes: &[u8], bit_offset: usize) -> u8 {
  let byte_idx = bit_offset >> 3;
  if byte_idx < bytes.len() {
    (bytes[byte_idx] >> (7 - (bit_offset & 7))) & 1
  } else {
    0
  }
}

/// Sets specific bit in contiguous bytes using MSB ordering for standard Redis compatibility.
/// 设置连续字节中指定位的值（MSB 顺序，用于标准 Redis 兼容格式）
#[inline]
pub fn set_bit_in_bytes(bytes: &mut Vec<u8>, bit_offset: usize, bit: u8) -> u8 {
  let byte_idx = bit_offset >> 3;
  if byte_idx >= bytes.len() {
    bytes.resize(byte_idx + 1, 0);
  }
  let shift = 7 - (bit_offset & 7);
  let old = (bytes[byte_idx] >> shift) & 1;
  if bit != 0 {
    bytes[byte_idx] |= 1 << shift;
  } else {
    bytes[byte_idx] &= !(1 << shift);
  }
  old
}

/// High-performance MSB bit position search aligned with Apache Kvrocks util::msb::RawBitpos.
/// 高性能大端序 MSB 位检索（对标 Apache Kvrocks util::msb::RawBitpos）
#[inline]
pub fn raw_bitpos(bytes: &[u8], bit: u8) -> Option<usize> {
  let mut offset = 0usize;
  let (chunks, remainder) = bytes.as_chunks::<8>();
  if bit == 1 {
    for chunk in chunks {
      let word = u64::from_be_bytes(*chunk);
      if word != 0 {
        return Some(offset + word.leading_zeros() as usize);
      }
      offset += 64;
    }
    for &b in remainder {
      if b != 0 {
        return Some(offset + b.leading_zeros() as usize);
      }
      offset += 8;
    }
  } else {
    for chunk in chunks {
      let word = u64::from_be_bytes(*chunk);
      if word != u64::MAX {
        return Some(offset + (!word).leading_zeros() as usize);
      }
      offset += 64;
    }
    for &b in remainder {
      if b != 0xFF {
        return Some(offset + (!b).leading_zeros() as usize);
      }
      offset += 8;
    }
  }
  None
}

/// High-performance LSB bit position search for bitmap segment storage acceleration aligned with util::lsb.
/// 高性能小端序 LSB 位检索（用于 Kvrocks Bitmap 分段原生存储加速，对标 util::lsb）
#[inline]
pub fn raw_bitpos_lsb(bytes: &[u8], bit: u8) -> Option<usize> {
  let mut offset = 0usize;
  let (chunks, remainder) = bytes.as_chunks::<8>();
  if bit == 1 {
    for chunk in chunks {
      let word = u64::from_le_bytes(*chunk);
      if word != 0 {
        return Some(offset + word.trailing_zeros() as usize);
      }
      offset += 64;
    }
    for &b in remainder {
      if b != 0 {
        return Some(offset + b.trailing_zeros() as usize);
      }
      offset += 8;
    }
  } else {
    for chunk in chunks {
      let word = u64::from_le_bytes(*chunk);
      if word != u64::MAX {
        return Some(offset + (!word).trailing_zeros() as usize);
      }
      offset += 64;
    }
    for &b in remainder {
      if b != 0xFF {
        return Some(offset + (!b).trailing_zeros() as usize);
      }
      offset += 8;
    }
  }
  None
}

/// Finds first target bit within [start_bit, stop_bit] in a single byte using LSB order with O(1) branchless bitwise operations.
/// 在单字节中按 LSB 顺序查找指定区间 [start_bit, stop_bit] 内首个目标位（O(1) 零分支快速位运算）
#[inline]
pub const fn find_bit_in_byte_lsb(
  b: u8,
  bit: u8,
  start_bit: usize,
  stop_bit: usize,
) -> Option<usize> {
  debug_assert!(start_bit <= stop_bit && stop_bit < 8);
  let mask = (((1u16 << (stop_bit - start_bit + 1)) - 1) as u8) << start_bit;
  let target = if bit == 1 { b } else { !b };
  let masked = target & mask;
  if masked != 0 {
    Some(masked.trailing_zeros() as usize)
  } else {
    None
  }
}

/// Finds first target bit within [start_bit, stop_bit] in a single byte using MSB order with O(1) branchless bitwise operations.
/// 在单字节中按 MSB 顺序查找指定区间 [start_bit, stop_bit] 内首个目标位（O(1) 零分支快速位运算）
#[inline]
pub const fn find_bit_in_byte_msb(
  b: u8,
  bit: u8,
  start_bit: usize,
  stop_bit: usize,
) -> Option<usize> {
  debug_assert!(start_bit <= stop_bit && stop_bit < 8);
  let mask = (((1u16 << (stop_bit - start_bit + 1)) - 1) as u8) << (7 - stop_bit);
  let target = if bit == 1 { b } else { !b };
  let masked = target & mask;
  if masked != 0 {
    Some(masked.leading_zeros() as usize)
  } else {
    None
  }
}

/// High-performance 64-bit native CPU popcount bit counting aligned with Apache Kvrocks util::RawPopcount.
/// 高性能 64 位原生 CPU POPCNT 位统计（对标 Apache Kvrocks util::RawPopcount）
#[inline]
pub fn raw_popcount(bytes: &[u8]) -> u64 {
  let mut count = 0u64;
  let (chunks, remainder) = bytes.as_chunks::<8>();
  for chunk in chunks {
    let word = u64::from_ne_bytes(*chunk);
    count += word.count_ones() as u64;
  }
  for &b in remainder {
    count += b.count_ones() as u64;
  }
  count
}

pub use crate::meta::normalize_bitmap_range as normalize_range;

/// Normalizes bit index into byte range and padding masks aligned with Kvrocks NormalizeToByteRangeWithPaddingMask.
/// 位图位索引标准化为字节范围与位掩码（对标 Kvrocks NormalizeToByteRangeWithPaddingMask）
#[inline]
pub const fn normalize_bit_range_to_byte_mask(
  start_bit: i64,
  end_bit: i64,
) -> (usize, usize, u8, u8) {
  debug_assert!(start_bit <= end_bit);
  let first_byte_neg_mask = (!((1u16 << (8 - (start_bit & 7))) - 1)) as u8;
  let last_byte_neg_mask = ((1u16 << (7 - (end_bit & 7))) - 1) as u8;
  let start_byte = (start_bit >> 3) as usize;
  let end_byte = (end_bit >> 3) as usize;
  (
    start_byte,
    end_byte,
    first_byte_neg_mask,
    last_byte_neg_mask,
  )
}

/// Range and mask normalization supporting byte and bit indices aligned with Kvrocks.
/// 支持字节与位索引的范围与掩码归一化（对标 Kvrocks）
#[inline]
pub const fn normalize_to_byte_range_with_padding_mask(
  is_bit_index: bool,
  start: i64,
  end: i64,
) -> (usize, usize, u8, u8) {
  if is_bit_index {
    normalize_bit_range_to_byte_mask(start, end)
  } else {
    (start as usize, end as usize, 0, 0)
  }
}

/// Small 9-byte local buffer for cross-segment high-precision bitfield operations aligned with Kvrocks ArrayBitfieldBitmap.
/// 9 字节局部小缓冲结构，用于跨分段高精度读取和写入 Bitfield（对标 Kvrocks ArrayBitfieldBitmap）
#[derive(Debug, Clone)]
pub struct ArrayBitfieldBitmap {
  pub buf: [u8; 9],
  pub byte_offset: u32,
}

impl Default for ArrayBitfieldBitmap {
  fn default() -> Self {
    Self::new(0)
  }
}

impl ArrayBitfieldBitmap {
  pub const SIZE: usize = 9;

  #[inline]
  pub const fn new(byte_offset: u32) -> Self {
    Self {
      buf: [0u8; Self::SIZE],
      byte_offset,
    }
  }

  #[inline]
  pub fn set_byte_offset(&mut self, byte_offset: u32) {
    self.byte_offset = byte_offset;
  }

  #[inline]
  pub fn reset(&mut self) {
    self.buf.fill(0);
  }

  #[inline]
  pub fn set(&mut self, byte_offset: u32, src: &[u8]) -> Result<()> {
    let bytes = src.len();
    if byte_offset < self.byte_offset
      || (byte_offset + bytes as u32) > (self.byte_offset + Self::SIZE as u32)
    {
      return Err(Error::invalid_data(
        "The range [offset, offset + bytes) is out of bitfield buffer",
      ));
    }
    let rel_offset = (byte_offset - self.byte_offset) as usize;
    self.buf[rel_offset..rel_offset + bytes].copy_from_slice(src);
    Ok(())
  }

  #[inline]
  pub fn get(&self, byte_offset: u32, dst: &mut [u8]) -> Result<()> {
    let bytes = dst.len();
    if byte_offset < self.byte_offset
      || (byte_offset + bytes as u32) > (self.byte_offset + Self::SIZE as u32)
    {
      return Err(Error::invalid_data(
        "The range [offset, offset + bytes) is out of bitfield buffer",
      ));
    }
    let rel_offset = (byte_offset - self.byte_offset) as usize;
    dst.copy_from_slice(&self.buf[rel_offset..rel_offset + bytes]);
    Ok(())
  }

  #[inline]
  pub fn get_unsigned_bitfield(&self, bit_offset: u64, bits: u8) -> Result<u64> {
    if bits == 0 || bits > 63 {
      return Err(Error::invalid_data("Invalid unsigned bits (1..=63)"));
    }
    self.read_raw_bitfield(bit_offset, bits)
  }

  #[inline]
  pub fn get_signed_bitfield(&self, bit_offset: u64, bits: u8) -> Result<i64> {
    if bits == 0 || bits > 64 {
      return Err(Error::invalid_data("Invalid signed bits (1..=64)"));
    }
    let raw = self.read_raw_bitfield(bit_offset, bits)?;
    let shift = 64 - bits;
    let val = ((raw as i64) << shift) >> shift;
    Ok(val)
  }

  #[inline]
  fn read_raw_bitfield(&self, bit_offset: u64, bits: u8) -> Result<u64> {
    let first_byte = (bit_offset / 8) as u32;
    let last_byte = ((bit_offset + bits as u64 - 1) / 8 + 1) as u32;
    let bytes = (last_byte - first_byte) as usize;

    if first_byte < self.byte_offset
      || (first_byte + bytes as u32) > (self.byte_offset + Self::SIZE as u32)
    {
      return Err(Error::invalid_data("Bitfield range out of buffer"));
    }

    let rel_bit_offset = (bit_offset - (self.byte_offset as u64 * 8)) as usize;
    let mut word_bytes = [0u8; 16];
    word_bytes[7..16].copy_from_slice(&self.buf);
    let word = u128::from_be_bytes(word_bytes);
    let shift = 72 - rel_bit_offset - (bits as usize);
    let mask = if bits == 64 {
      u64::MAX
    } else {
      (1u64 << bits) - 1
    };
    Ok(((word >> shift) as u64) & mask)
  }

  #[inline]
  pub fn set_bitfield(&mut self, bit_offset: u64, bits: u8, value: u64) -> Result<()> {
    let first_byte = (bit_offset / 8) as u32;
    let last_byte = ((bit_offset + bits as u64 - 1) / 8 + 1) as u32;
    let bytes = (last_byte - first_byte) as usize;

    if first_byte < self.byte_offset
      || (first_byte + bytes as u32) > (self.byte_offset + Self::SIZE as u32)
    {
      return Err(Error::invalid_data("Bitfield range out of buffer"));
    }

    let rel_bit_offset = (bit_offset - (self.byte_offset as u64 * 8)) as usize;
    let mut word_bytes = [0u8; 16];
    word_bytes[7..16].copy_from_slice(&self.buf);
    let mut word = u128::from_be_bytes(word_bytes);
    let shift = 72 - rel_bit_offset - (bits as usize);
    let bit_mask = if bits == 64 {
      u64::MAX as u128
    } else {
      (1u128 << bits) - 1
    };
    let mask = bit_mask << shift;
    let val = ((value as u128) & bit_mask) << shift;
    word = (word & !mask) | val;
    let updated_bytes = word.to_be_bytes();
    self.buf.copy_from_slice(&updated_bytes[7..16]);
    Ok(())
  }
}

/// Signed bitfield addition with overflow handling aligned with Kvrocks detail::SignedBitfieldPlus.
/// 有符号 BITFIELD 溢出加法运算（对标 Kvrocks detail::SignedBitfieldPlus）
#[inline]
pub fn signed_bitfield_plus(
  value: u64,
  incr: i64,
  bits: u8,
  overflow: BitfieldOverflow,
) -> (u64, bool) {
  let max = if bits == 64 {
    i64::MAX
  } else {
    (1i64 << (bits - 1)) - 1
  };
  let min = -max - 1;

  let signed_val = value as i64;
  let max_incr = (max as u64).wrapping_sub(value) as i64;
  let min_incr = min.wrapping_sub(signed_val);

  if signed_val > max
    || (bits != 64 && incr > max_incr)
    || (signed_val >= 0 && incr >= 0 && incr > max_incr)
  {
    match overflow {
      BitfieldOverflow::Wrap => (wrapped_signed_bitfield_plus(value, incr, bits), true),
      BitfieldOverflow::Sat => (max as u64, true),
      BitfieldOverflow::Fail => (0, true),
    }
  } else if signed_val < min
    || (bits != 64 && incr < min_incr)
    || (signed_val < 0 && incr < 0 && incr < min_incr)
  {
    match overflow {
      BitfieldOverflow::Wrap => (wrapped_signed_bitfield_plus(value, incr, bits), true),
      BitfieldOverflow::Sat => (min as u64, true),
      BitfieldOverflow::Fail => (0, true),
    }
  } else {
    (signed_val.wrapping_add(incr) as u64, false)
  }
}

#[inline]
const fn wrapped_signed_bitfield_plus(value: u64, incr: i64, bits: u8) -> u64 {
  let res = value.wrapping_add(incr as u64);
  if bits < 64 {
    let mask = u64::MAX << bits;
    if (res & (1u64 << (bits - 1))) != 0 {
      res | mask
    } else {
      res & !mask
    }
  } else {
    res
  }
}

/// Unsigned bitfield addition with overflow handling aligned with Kvrocks detail::UnsignedBitfieldPlus.
/// 无符号 BITFIELD 溢出加法运算（对标 Kvrocks detail::UnsignedBitfieldPlus）
#[inline]
pub fn unsigned_bitfield_plus(
  value: u64,
  incr: i64,
  bits: u8,
  overflow: BitfieldOverflow,
) -> (u64, bool) {
  let max = if bits == 64 {
    u64::MAX
  } else {
    (1u64 << bits) - 1
  };
  let max_incr = max.wrapping_sub(value) as i64;
  let min_incr = (!value).wrapping_add(1) as i64;

  if value > max || (incr > 0 && incr > max_incr) {
    match overflow {
      BitfieldOverflow::Wrap => (wrapped_unsigned_bitfield_plus(value, incr, bits), true),
      BitfieldOverflow::Sat => (max, true),
      BitfieldOverflow::Fail => (0, true),
    }
  } else if incr < 0 && incr < min_incr {
    match overflow {
      BitfieldOverflow::Wrap => (wrapped_unsigned_bitfield_plus(value, incr, bits), true),
      BitfieldOverflow::Sat => (0, true),
      BitfieldOverflow::Fail => (0, true),
    }
  } else {
    (value.wrapping_add(incr as u64), false)
  }
}

#[inline]
const fn wrapped_unsigned_bitfield_plus(value: u64, incr: i64, bits: u8) -> u64 {
  let mask = if bits == 64 { 0 } else { u64::MAX << bits };
  let res = value.wrapping_add(incr as u64);
  res & !mask
}

/// Executes a single bitfield logical operation aligned with Kvrocks BitfieldOp.
/// 执行单步 BITFIELD 逻辑运算（对标 Kvrocks BitfieldOp）
#[inline]
pub fn bitfield_op_calc(
  op: &BitfieldOperation,
  old_value: u64,
) -> (Option<BitfieldValue>, u64, bool) {
  if op.op_type == BitfieldOpType::Get {
    let val = if op.encoding.is_signed() {
      BitfieldValue::Signed(old_value as i64)
    } else {
      BitfieldValue::Unsigned(old_value)
    };
    return (Some(val), old_value, false);
  }

  let (new_value, is_overflow) = match op.encoding {
    BitfieldEncoding::Signed(bits) => {
      let input_val = if op.op_type == BitfieldOpType::Set {
        op.value as u64
      } else {
        old_value
      };
      let incr = if op.op_type == BitfieldOpType::Set {
        0
      } else {
        op.value
      };
      signed_bitfield_plus(input_val, incr, bits, op.overflow)
    }
    BitfieldEncoding::Unsigned(bits) => {
      let input_val = if op.op_type == BitfieldOpType::Set {
        op.value as u64
      } else {
        old_value
      };
      let incr = if op.op_type == BitfieldOpType::Set {
        0
      } else {
        op.value
      };
      unsigned_bitfield_plus(input_val, incr, bits, op.overflow)
    }
  };

  if op.overflow == BitfieldOverflow::Fail && is_overflow {
    return (None, old_value, true);
  }

  let returned_val = if op.op_type == BitfieldOpType::Set {
    if op.encoding.is_signed() {
      BitfieldValue::Signed(old_value as i64)
    } else {
      BitfieldValue::Unsigned(old_value)
    }
  } else if op.encoding.is_signed() {
    BitfieldValue::Signed(new_value as i64)
  } else {
    BitfieldValue::Unsigned(new_value)
  };

  (Some(returned_val), new_value, false)
}

/// 64-bit word vectorized bitwise AND operation.
/// 64 位原生字向量化位与操作
#[inline]
pub fn bitwise_and(dst: &mut [u8], src: &[u8]) {
  let common_len = dst.len().min(src.len());
  let (dst_chunks, dst_rem) = dst[..common_len].as_chunks_mut::<8>();
  let (src_chunks, src_rem) = src[..common_len].as_chunks::<8>();

  for (d, s) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
    let dw = u64::from_ne_bytes(*d);
    let sw = u64::from_ne_bytes(*s);
    *d = (dw & sw).to_ne_bytes();
  }
  for (d, s) in dst_rem.iter_mut().zip(src_rem.iter()) {
    *d &= *s;
  }
  dst[common_len..].fill(0);
}

/// 64-bit word vectorized bitwise OR operation.
/// 64 位原生字向量化位或操作
#[inline]
pub fn bitwise_or(dst: &mut [u8], src: &[u8]) {
  let len = dst.len().min(src.len());
  let (dst_chunks, dst_rem) = dst[..len].as_chunks_mut::<8>();
  let (src_chunks, src_rem) = src[..len].as_chunks::<8>();

  for (d, s) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
    let dw = u64::from_ne_bytes(*d);
    let sw = u64::from_ne_bytes(*s);
    *d = (dw | sw).to_ne_bytes();
  }
  for (d, s) in dst_rem.iter_mut().zip(src_rem.iter()) {
    *d |= *s;
  }
}

/// 64-bit word vectorized bitwise XOR operation.
/// 64 位原生字向量化位异或操作
#[inline]
pub fn bitwise_xor(dst: &mut [u8], src: &[u8]) {
  let len = dst.len().min(src.len());
  let (dst_chunks, dst_rem) = dst[..len].as_chunks_mut::<8>();
  let (src_chunks, src_rem) = src[..len].as_chunks::<8>();

  for (d, s) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
    let dw = u64::from_ne_bytes(*d);
    let sw = u64::from_ne_bytes(*s);
    *d = (dw ^ sw).to_ne_bytes();
  }
  for (d, s) in dst_rem.iter_mut().zip(src_rem.iter()) {
    *d ^= *s;
  }
}

/// 64-bit word vectorized bitwise NOT operation.
/// 64 位原生字向量化位非操作
#[inline]
pub fn bitwise_not(dst: &mut [u8], src: &[u8]) {
  let len = dst.len().min(src.len());
  let (dst_chunks, dst_rem) = dst[..len].as_chunks_mut::<8>();
  let (src_chunks, src_rem) = src[..len].as_chunks::<8>();

  for (d, s) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
    let sw = u64::from_ne_bytes(*s);
    *d = (!sw).to_ne_bytes();
  }
  for (d, s) in dst_rem.iter_mut().zip(src_rem.iter()) {
    *d = !*s;
  }
}

/// High-performance 64-bit word slice bitmap operation writing into buffer aligned with Kvrocks Bitmap::BitOp.
/// 高性能 64 位字切片位图操作（零堆分配写入给定缓冲，对标 Kvrocks Bitmap::BitOp）
#[inline]
pub fn bit_op_exec_into(op: BitOp, src_slices: &[&[u8]], out: &mut [u8]) -> Result<usize> {
  let out_len = out.len();
  match op {
    BitOp::And => {
      if let Some((first, rest)) = src_slices.split_first() {
        let copy_len = first.len().min(out_len);
        out[..copy_len].copy_from_slice(&first[..copy_len]);
        out[copy_len..out_len].fill(0);
        for &src in rest {
          bitwise_and(out, src);
        }
      } else {
        out.fill(0);
      }
    }
    BitOp::Or => {
      if let Some((first, rest)) = src_slices.split_first() {
        let copy_len = first.len().min(out_len);
        out[..copy_len].copy_from_slice(&first[..copy_len]);
        out[copy_len..out_len].fill(0);
        for &src in rest {
          bitwise_or(out, src);
        }
      } else {
        out.fill(0);
      }
    }
    BitOp::Xor => {
      if let Some((first, rest)) = src_slices.split_first() {
        let copy_len = first.len().min(out_len);
        out[..copy_len].copy_from_slice(&first[..copy_len]);
        out[copy_len..out_len].fill(0);
        for &src in rest {
          bitwise_xor(out, src);
        }
      } else {
        out.fill(0);
      }
    }
    BitOp::Not => {
      if src_slices.len() != 1 {
        return Err(Error::invalid_data(
          "ERR BITOP NOT takes exactly one source key",
        ));
      }
      let src = src_slices[0];
      let src_len = src.len().min(out_len);
      bitwise_not(&mut out[..src_len], &src[..src_len]);
      if out_len > src_len {
        out[src_len..out_len].fill(0xFF);
      }
    }
  }

  Ok(out_len)
}

/// High-performance 64-bit word slice bitmap operation for AND / OR / XOR / NOT.
/// 高性能 64 位字切片位图操作（AND / OR / XOR / NOT）
pub fn bit_op_exec(op: &str, src_slices: &[&[u8]]) -> Result<Vec<u8>> {
  let bit_op = op.parse::<BitOp>()?;
  let max_len = src_slices.iter().map(|s| s.len()).max().unwrap_or(0);
  let mut out = vec![0u8; max_len];
  let written = bit_op_exec_into(bit_op, src_slices, &mut out)?;
  out.truncate(written);
  Ok(out)
}

/// String-mode BITCOUNT in MSB order aligned with Apache Kvrocks BitmapString::BitCount.
/// 字符串模式 BITCOUNT（MSB 顺序，对标 Apache Kvrocks BitmapString::BitCount）
pub fn string_bitcount(
  val: &[u8],
  start: Option<i64>,
  end: Option<i64>,
  is_bit_index: bool,
) -> u64 {
  let strlen = val.len() as i64;
  let totlen = if is_bit_index { strlen << 3 } else { strlen };
  let s = start.unwrap_or(0);
  let e = end.unwrap_or(-1);
  if s < 0 && e < 0 && s > e {
    return 0;
  }
  let (norm_s, norm_e) = normalize_range(s, e, totlen);
  if norm_s > norm_e {
    return 0;
  }

  let (start_byte, stop_byte, first_mask, last_mask) =
    normalize_to_byte_range_with_padding_mask(is_bit_index, norm_s, norm_e);

  if start_byte >= val.len() {
    return 0;
  }
  let actual_stop = stop_byte.min(val.len().saturating_sub(1));
  if start_byte > actual_stop {
    return 0;
  }

  let bytes = &val[start_byte..=actual_stop];
  let cnt = raw_popcount(bytes);

  let mut mask_cnt = 0u64;
  if first_mask != 0 && start_byte < val.len() {
    mask_cnt += (val[start_byte] & first_mask).count_ones() as u64;
  }
  if last_mask != 0 && actual_stop == stop_byte && actual_stop < val.len() {
    mask_cnt += (val[actual_stop] & last_mask).count_ones() as u64;
  }
  cnt.saturating_sub(mask_cnt)
}

/// String-mode BITPOS in MSB order aligned with Apache Kvrocks BitmapString::BitPos.
/// 字符串模式 BITPOS（MSB 顺序，对标 Apache Kvrocks BitmapString::BitPos）
pub fn string_bitpos(
  val: &[u8],
  bit: u8,
  start: Option<i64>,
  end: Option<i64>,
  stop_given: bool,
  is_bit_index: bool,
) -> i64 {
  let strlen = val.len() as i64;
  let length = if is_bit_index { strlen * 8 } else { strlen };
  let s = start.unwrap_or(0);
  let e = end.unwrap_or(-1);
  let (norm_s, norm_e) = normalize_range(s, e, length);
  if norm_s > norm_e {
    return -1;
  }

  let mut byte_start = (if is_bit_index { norm_s / 8 } else { norm_s }) as usize;
  let byte_stop = (if is_bit_index { norm_e / 8 } else { norm_e }) as usize;
  let bit_in_start_byte = if is_bit_index {
    (norm_s % 8) as usize
  } else {
    0
  };
  let bit_in_stop_byte = if is_bit_index {
    (norm_e % 8) as usize
  } else {
    7
  };

  if is_bit_index && byte_start == byte_stop {
    if byte_start < val.len()
      && let Some(bit_idx) =
        find_bit_in_byte_msb(val[byte_start], bit, bit_in_start_byte, bit_in_stop_byte)
    {
      return (byte_start * 8 + bit_idx) as i64;
    }
    return -1;
  }

  if is_bit_index && bit_in_start_byte != 0 {
    if byte_start < val.len()
      && let Some(bit_idx) = find_bit_in_byte_msb(val[byte_start], bit, bit_in_start_byte, 7)
    {
      return (byte_start * 8 + bit_idx) as i64;
    }
    byte_start += 1;
  }

  if byte_start > byte_stop || byte_start >= val.len() {
    return if stop_given && bit == 0 {
      -1
    } else if bit == 0 {
      strlen * 8
    } else {
      -1
    };
  }

  let actual_stop = byte_stop.min(val.len() - 1);
  let bytes_cnt = actual_stop - byte_start + 1;
  let pos_opt = raw_bitpos(&val[byte_start..byte_start + bytes_cnt], bit);

  match pos_opt {
    Some(pos) => {
      let abs_pos = (pos + byte_start * 8) as i64;
      if is_bit_index && abs_pos > norm_e {
        return -1;
      }
      abs_pos
    }
    None => {
      if stop_given && bit == 0 {
        -1
      } else if bit == 0 {
        strlen * 8
      } else {
        -1
      }
    }
  }
}
