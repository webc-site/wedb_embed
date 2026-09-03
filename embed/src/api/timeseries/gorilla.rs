use std::{
  cmp::Ordering,
  fmt::{self, Display},
};

use crate::error::{Error, Result};

/// Time series sample data point aligned with Apache Kvrocks TSSample.
/// 时序采样点（对标 Apache Kvrocks TSSample）
#[derive(Debug, Clone, Copy, PartialEq, Default, bitcode::Encode, bitcode::Decode)]
#[repr(C)]
pub struct TSSample {
  pub ts: u64,
  pub v: f64,
}

impl TSSample {
  pub const MAX_TIMESTAMP: u64 = u64::MAX;
  pub const NAN_VALUE: f64 = f64::NAN;

  #[inline]
  pub const fn new(ts: u64, v: f64) -> Self {
    Self { ts, v }
  }
}

impl Display for TSSample {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "({}, {})", self.ts, self.v)
  }
}

impl Eq for TSSample {}

impl PartialOrd for TSSample {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for TSSample {
  #[inline]
  fn cmp(&self, other: &Self) -> Ordering {
    self.ts.cmp(&other.ts)
  }
}

/// High-performance bitstream writer with branchless buffer management.
/// 高性能比特流写入器
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
  buf: Vec<u8>,
  current_byte: u8,
  bit_count: u8,
}

impl BitWriter {
  #[inline]
  pub fn new() -> Self {
    Self::with_capacity(64)
  }

  #[inline]
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      buf: Vec::with_capacity(capacity),
      current_byte: 0,
      bit_count: 0,
    }
  }

  #[inline(always)]
  pub fn write_bit(&mut self, bit: bool) {
    if bit {
      self.current_byte |= 1 << (7 - self.bit_count);
    }
    self.bit_count += 1;
    if self.bit_count == 8 {
      self.buf.push(self.current_byte);
      self.current_byte = 0;
      self.bit_count = 0;
    }
  }

  #[inline(always)]
  pub fn write_bits(&mut self, val: u64, mut num_bits: u8) {
    if num_bits == 0 {
      return;
    }
    while num_bits > 0 {
      let space_in_byte = 8 - self.bit_count;
      if num_bits <= space_in_byte {
        let shift = space_in_byte - num_bits;
        let mask = if num_bits == 64 {
          u64::MAX
        } else {
          (1u64 << num_bits) - 1
        };
        self.current_byte |= ((val & mask) as u8) << shift;
        self.bit_count += num_bits;
        if self.bit_count == 8 {
          self.buf.push(self.current_byte);
          self.current_byte = 0;
          self.bit_count = 0;
        }
        break;
      } else {
        let shift = num_bits - space_in_byte;
        let chunk = (val >> shift) & ((1u64 << space_in_byte) - 1);
        self.current_byte |= chunk as u8;
        self.buf.push(self.current_byte);
        self.current_byte = 0;
        self.bit_count = 0;
        num_bits -= space_in_byte;
      }
    }
  }

  #[inline]
  pub fn finish(mut self) -> Vec<u8> {
    if self.bit_count > 0 {
      self.buf.push(self.current_byte);
      self.current_byte = 0;
      self.bit_count = 0;
    }
    self.buf
  }
}

/// High-performance zero-copy bitstream reader.
/// 高性能比特流读取器（零拷贝切片遍历）
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
  data: &'a [u8],
  byte_offset: usize,
  bit_offset: u8,
}

impl<'a> BitReader<'a> {
  #[inline]
  pub const fn new(data: &'a [u8]) -> Self {
    Self {
      data,
      byte_offset: 0,
      bit_offset: 0,
    }
  }

  #[inline(always)]
  pub fn read_bit(&mut self) -> Option<bool> {
    if self.byte_offset >= self.data.len() {
      return None;
    }
    let b = self.data[self.byte_offset];
    let bit = (b & (1 << (7 - self.bit_offset))) != 0;
    self.bit_offset += 1;
    if self.bit_offset == 8 {
      self.byte_offset += 1;
      self.bit_offset = 0;
    }
    Some(bit)
  }

  #[inline(always)]
  pub fn read_bits(&mut self, mut num_bits: u8) -> Option<u64> {
    if num_bits == 0 {
      return Some(0);
    }
    let mut result = 0u64;
    while num_bits > 0 {
      if self.byte_offset >= self.data.len() {
        return None;
      }
      let space_in_byte = 8 - self.bit_offset;
      let b = self.data[self.byte_offset];
      if num_bits <= space_in_byte {
        let shift = space_in_byte - num_bits;
        let chunk = (b >> shift) & (((1u16 << num_bits) - 1) as u8);
        result = (result << num_bits) | (chunk as u64);
        self.bit_offset += num_bits;
        if self.bit_offset == 8 {
          self.byte_offset += 1;
          self.bit_offset = 0;
        }
        return Some(result);
      } else {
        let chunk = b & (((1u16 << space_in_byte) - 1) as u8);
        result = (result << space_in_byte) | (chunk as u64);
        num_bits -= space_in_byte;
        self.byte_offset += 1;
        self.bit_offset = 0;
      }
    }
    Some(result)
  }
}

/// Variable-length bitstream compression for timestamps using Delta-of-Delta.
/// 时间戳 Delta-of-Delta 变长比特流压缩
pub fn compress_timestamps(timestamps: &[u64]) -> Vec<u8> {
  if timestamps.is_empty() {
    return Vec::new();
  }
  let mut writer = BitWriter::with_capacity(timestamps.len() * 4 + 16);
  let t0 = timestamps[0];
  writer.write_bits(t0, 64);
  if timestamps.len() == 1 {
    return writer.finish();
  }
  let delta0 = timestamps[1].saturating_sub(t0);
  writer.write_bits(delta0, 32);

  let mut prev_delta = delta0;
  let mut prev_ts = timestamps[1];

  for &ts in &timestamps[2..] {
    let cur_delta = ts.saturating_sub(prev_ts);
    let dod = (cur_delta as i64) - (prev_delta as i64);

    if dod == 0 {
      writer.write_bit(false);
    } else if (-63..=64).contains(&dod) {
      writer.write_bits(0b10, 2);
      writer.write_bits((dod + 63) as u64, 7);
    } else if (-255..=256).contains(&dod) {
      writer.write_bits(0b110, 3);
      writer.write_bits((dod + 255) as u64, 9);
    } else if (-2047..=2048).contains(&dod) {
      writer.write_bits(0b1110, 4);
      writer.write_bits((dod + 2047) as u64, 12);
    } else {
      writer.write_bits(0b1111, 4);
      writer.write_bits(dod as u32 as u64, 32);
    }

    prev_delta = cur_delta;
    prev_ts = ts;
  }

  writer.finish()
}

#[inline(always)]
fn read_dod(reader: &mut BitReader<'_>) -> Option<i64> {
  let bit = reader.read_bit()?;
  if !bit {
    return Some(0);
  }
  let b2 = reader.read_bit()?;
  if !b2 {
    let val = reader.read_bits(7)?;
    Some((val as i64) - 63)
  } else {
    let b3 = reader.read_bit()?;
    if !b3 {
      let val = reader.read_bits(9)?;
      Some((val as i64) - 255)
    } else {
      let b4 = reader.read_bit()?;
      if !b4 {
        let val = reader.read_bits(12)?;
        Some((val as i64) - 2047)
      } else {
        let val = reader.read_bits(32)?;
        Some(val as u32 as i32 as i64)
      }
    }
  }
}

/// Variable-length bitstream decompression for Delta-of-Delta timestamps.
/// 时间戳 Delta-of-Delta 变长比特流解压（直接写入目标缓冲区）
pub fn decompress_timestamps_into(data: &[u8], count: usize, out: &mut Vec<u64>) -> Result<()> {
  if count == 0 || data.is_empty() {
    return Ok(());
  }
  let mut reader = BitReader::new(data);
  out.reserve(count);

  let t0 = reader
    .read_bits(64)
    .ok_or_else(|| Error::invalid_data("ERR TSDB: corrupted timestamp stream (missing t0)"))?;
  out.push(t0);
  if count == 1 {
    return Ok(());
  }

  let delta0 = reader
    .read_bits(32)
    .ok_or_else(|| Error::invalid_data("ERR TSDB: corrupted timestamp stream (missing delta0)"))?;
  let mut prev_ts = t0.saturating_add(delta0);
  let mut prev_delta = delta0;
  out.push(prev_ts);

  for _ in 2..count {
    let dod = read_dod(&mut reader)
      .ok_or_else(|| Error::invalid_data("ERR TSDB: corrupted timestamp dod bit"))?;
    let cur_delta = ((prev_delta as i64) + dod).max(0) as u64;
    prev_ts = prev_ts.saturating_add(cur_delta);
    prev_delta = cur_delta;
    out.push(prev_ts);
  }

  Ok(())
}

/// Fast extraction of last timestamp from compressed bitstream with zero heap allocation.
/// 快速提取压缩时间戳流的末尾时间戳（零堆分配）
pub fn decompress_last_timestamp(data: &[u8], count: usize) -> Option<u64> {
  if count == 0 || data.is_empty() {
    return None;
  }
  let mut reader = BitReader::new(data);
  let t0 = reader.read_bits(64)?;
  if count == 1 {
    return Some(t0);
  }
  let delta0 = reader.read_bits(32)?;
  let mut prev_ts = t0.saturating_add(delta0);
  let mut prev_delta = delta0;
  if count == 2 {
    return Some(prev_ts);
  }

  for _ in 2..count {
    let dod = read_dod(&mut reader)?;
    let cur_delta = ((prev_delta as i64) + dod).max(0) as u64;
    prev_ts = prev_ts.saturating_add(cur_delta);
    prev_delta = cur_delta;
  }

  Some(prev_ts)
}
