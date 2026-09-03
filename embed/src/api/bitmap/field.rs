use std::{borrow::Borrow, collections::hash_map::Entry};

use rapidhash::RapidHashMap as HashMap;

use super::{bitops::*, key, meta::BitmapMeta};
use crate::{
  bitmap::opt::{
    BitfieldEncoding, BitfieldOpType, BitfieldOperation, BitfieldOverflow, BitfieldValue,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::check_composite_meta_not_other_type,
  key_composer::{KeyComposer, KeyTag},
  meta::current_now_ms,
  string::{decode_string_value, encode_string_value, is_string_expired, key::raw},
  wedb::Db,
};

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

  #[inline]
  pub fn apply_op(
    &mut self,
    op: &BitfieldOperation,
    read_only: bool,
  ) -> Result<Option<BitfieldValue>> {
    let bit_offset = op.offset;
    let bits = op.encoding.bits();
    let old_raw = if op.encoding.is_signed() {
      self.get_signed_bitfield(bit_offset, bits)? as u64
    } else {
      self.get_unsigned_bitfield(bit_offset, bits)?
    };

    let (ret, new_raw, _) = bitfield_op_calc(op, old_raw);

    if op.op_type != BitfieldOpType::Get && !read_only && ret.is_some() {
      self.set_bitfield(bit_offset, bits, new_raw)?;
    }

    Ok(ret)
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

struct SegmentCacheStore<'a, E: Engine> {
  db: &'a Db<E>,
  kc: KeyComposer,
  key: &'a [u8],
  cache: HashMap<u32, (bool, Vec<u8>)>,
}

impl<'a, E: Engine> SegmentCacheStore<'a, E>
where
  Error: From<E::Error>,
{
  #[inline]
  fn new(db: &'a Db<E>, kc: KeyComposer, key: &'a [u8]) -> Self {
    Self {
      db,
      kc,
      key,
      cache: HashMap::default(),
    }
  }

  #[inline]
  fn get(&mut self, seg_idx: u32) -> Result<&[u8]> {
    let entry = match self.cache.entry(seg_idx) {
      Entry::Occupied(e) => e.into_mut(),
      Entry::Vacant(e) => {
        let seg_k = key::segment(&self.kc, self.key, seg_idx);
        let bytes = self
          .db
          .data()
          .get(&seg_k)?
          .map(|v| v.to_vec())
          .unwrap_or_default();
        e.insert((false, bytes))
      }
    };
    Ok(&entry.1)
  }

  #[inline]
  fn get_segment_mut(&mut self, seg_idx: u32, min_bytes: usize) -> Result<&mut Vec<u8>> {
    let entry = match self.cache.entry(seg_idx) {
      Entry::Occupied(o) => o.into_mut(),
      Entry::Vacant(v) => {
        let seg_k = key::segment(&self.kc, self.key, seg_idx);
        let data = self
          .db
          .data()
          .get(&seg_k)?
          .map(|b| b.to_vec())
          .unwrap_or_default();
        v.insert((false, data))
      }
    };
    expand_bitmap_segment(&mut entry.1, min_bytes);
    entry.0 = true;
    Ok(&mut entry.1)
  }

  #[inline]
  fn dirty_segments(&self) -> impl Iterator<Item = (u32, &[u8])> {
    self
      .cache
      .iter()
      .filter_map(|(&seg_idx, (dirty, seg_data))| {
        if *dirty {
          Some((seg_idx, seg_data.as_slice()))
        } else {
          None
        }
      })
  }
}

/// Bitfield operations interface (BITFIELD, BITFIELD_RO).
/// 位字段操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn bitfield<K: AsRef<[u8]>, O: Borrow<BitfieldOperation>>(
    &self,
    key: K,
    ops: impl IntoIterator<Item = O>,
  ) -> Result<Vec<Option<BitfieldValue>>> {
    let ops_vec: Vec<BitfieldOperation> = ops.into_iter().map(|o| *o.borrow()).collect();
    self.exec_bitfield(key, &ops_vec, false)
  }

  #[inline]
  pub fn bitfield_read_only<K: AsRef<[u8]>, O: Borrow<BitfieldOperation>>(
    &self,
    key: K,
    ops: impl IntoIterator<Item = O>,
  ) -> Result<Vec<Option<BitfieldValue>>> {
    let mut ops_vec = Vec::new();
    for op in ops {
      let op = op.borrow();
      if op.op_type != BitfieldOpType::Get {
        return Err(Error::invalid_data(
          "ERR BITFIELD_RO only supports the GET subcmd",
        ));
      }
      ops_vec.push(*op);
    }
    self.exec_bitfield(key, &ops_vec, true)
  }

  #[inline]
  pub(crate) fn exec_bitfield<K: AsRef<[u8]>>(
    &self,
    key: K,
    ops: &[BitfieldOperation],
    read_only: bool,
  ) -> Result<Vec<Option<BitfieldValue>>> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();

    // 1. 检查 String 模式
    let raw_k = raw(&kc, key_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, val) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        let mut str_bytes = val.to_vec();
        let mut results = Vec::with_capacity(ops.len());
        let mut modified = false;
        let mut view = ArrayBitfieldBitmap::default();

        for op in ops {
          let bit_offset = op.offset;
          let bits = op.encoding.bits();
          let first_byte = (bit_offset / 8) as usize;
          let last_byte = ((bit_offset + bits as u64 - 1) / 8) as usize;

          if op.op_type != BitfieldOpType::Get && last_byte >= str_bytes.len() {
            str_bytes.resize(last_byte + 1, 0);
            modified = true;
          }

          view.set_byte_offset(first_byte as u32);
          view.reset();
          let copy_len = if first_byte < str_bytes.len() {
            (str_bytes.len() - first_byte).min(ArrayBitfieldBitmap::SIZE)
          } else {
            0
          };
          if copy_len > 0 {
            view.set(
              first_byte as u32,
              &str_bytes[first_byte..first_byte + copy_len],
            )?;
          }

          let ret = view.apply_op(op, read_only)?;
          results.push(ret);

          if op.op_type != BitfieldOpType::Get && !read_only && ret.is_some() {
            let write_bytes = (last_byte - first_byte + 1).min(ArrayBitfieldBitmap::SIZE);
            view.get(
              first_byte as u32,
              &mut str_bytes[first_byte..first_byte + write_bytes],
            )?;
            modified = true;
          }
        }

        if modified && !read_only {
          let enc_val = encode_string_value(&str_bytes, expire_at);
          data_ks.insert(&raw_k, &enc_val)?;
        }

        return Ok(results);
      }
    }

    // 2. Segment 模式
    let bm_meta_k = key::meta(&kc, key_bytes);
    let cur_meta_opt = meta_ks
      .get(&bm_meta_k)?
      .and_then(|b| BitmapMeta::decode(&b));

    if cur_meta_opt.is_none() {
      check_composite_meta_not_other_type(self, key_bytes, KeyTag::BitmapMeta.as_slice(), now_ms)?;
    }

    let mut meta = match cur_meta_opt {
      Some(m) if !m.is_expired(now_ms) => m,
      _ => BitmapMeta::new_with_version(0, 0),
    };

    let mut store = SegmentCacheStore::new(self, kc, key_bytes);
    let mut results = Vec::with_capacity(ops.len());
    let mut max_bytes = meta.base.size;
    let mut has_changes = false;

    let mut view = ArrayBitfieldBitmap::default();

    for op in ops {
      let bit_offset = op.offset;
      let bits = op.encoding.bits();

      let first_byte = (bit_offset / 8) as u32;
      let last_byte = ((bit_offset + bits as u64 - 1) / 8) as u32;
      let req_bytes = (last_byte + 1) as u64;

      if op.op_type != BitfieldOpType::Get {
        max_bytes = max_bytes.max(req_bytes);
      }

      let first_seg = first_byte / (BITMAP_SEGMENT_BYTES as u32);
      let last_seg = last_byte / (BITMAP_SEGMENT_BYTES as u32);

      view.set_byte_offset(first_byte);
      view.reset();

      // 读取涉及的 Segment 到 view 中（LSB 转换为 MSB）
      for s_idx in first_seg..=last_seg {
        let seg_slice = store.get(s_idx)?;
        let seg_base_byte = s_idx * (BITMAP_SEGMENT_BYTES as u32);
        let seg_end_byte = seg_base_byte + (BITMAP_SEGMENT_BYTES as u32);

        let inter_start = first_byte.max(seg_base_byte);
        let inter_end = (last_byte + 1).min(seg_end_byte);

        if inter_start < inter_end {
          let seg_rel_start = (inter_start - seg_base_byte) as usize;
          let seg_rel_end = (inter_end - seg_base_byte) as usize;
          let slice_len = seg_rel_end - seg_rel_start;

          let mut msb_slice = [0u8; ArrayBitfieldBitmap::SIZE];
          if seg_rel_start < seg_slice.len() {
            let avail_end = seg_rel_end.min(seg_slice.len());
            let copy_cnt = avail_end - seg_rel_start;
            for (dst, &src) in msb_slice[..copy_cnt]
              .iter_mut()
              .zip(&seg_slice[seg_rel_start..avail_end])
            {
              *dst = src.reverse_bits();
            }
          }
          view.set(inter_start, &msb_slice[..slice_len])?;
        }
      }

      let ret = view.apply_op(op, read_only)?;
      results.push(ret);

      if op.op_type != BitfieldOpType::Get && !read_only && ret.is_some() {
        for s_idx in first_seg..=last_seg {
          let seg_base_byte = s_idx * (BITMAP_SEGMENT_BYTES as u32);
          let seg_end_byte = seg_base_byte + (BITMAP_SEGMENT_BYTES as u32);

          let inter_start = first_byte.max(seg_base_byte);
          let inter_end = (last_byte + 1).min(seg_end_byte);

          if inter_start < inter_end {
            let seg_rel_start = (inter_start - seg_base_byte) as usize;
            let seg_rel_end = (inter_end - seg_base_byte) as usize;
            let slice_len = seg_rel_end - seg_rel_start;

            let mut msb_slice = [0u8; ArrayBitfieldBitmap::SIZE];
            view.get(inter_start, &mut msb_slice[..slice_len])?;

            let seg = store.get_segment_mut(s_idx, seg_rel_end)?;
            for (dst, &src) in seg[seg_rel_start..seg_rel_end]
              .iter_mut()
              .zip(&msb_slice[..slice_len])
            {
              *dst = src.reverse_bits();
            }
            has_changes = true;
          }
        }
      }
    }

    // 提交变更
    if !read_only && (has_changes || max_bytes > meta.base.size) {
      let mut batch = self.batch();
      for (seg_idx, seg_data) in store.dirty_segments() {
        let seg_k = key::segment(&kc, key_bytes, seg_idx);
        batch.insert_data(&seg_k, seg_data);
      }
      meta.base.size = max_bytes;
      batch.insert_meta(&bm_meta_k, &meta.encode());
      batch.commit()?;
    }

    Ok(results)
  }
}
