pub use super::r#const::*;
use crate::{
  bitmap::opt::BitOp,
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
