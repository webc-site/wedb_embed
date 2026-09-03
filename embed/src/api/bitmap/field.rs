use std::{borrow::Borrow, collections::hash_map::Entry};

use rapidhash::RapidHashMap as HashMap;

use super::{bitops::*, key, meta::BitmapMeta};
use crate::{
  bitmap::opt::{BitfieldOpType, BitfieldOperation, BitfieldValue},
  engine::{Engine, Partition},
  error::{Error, Result},
  key::check_composite_meta_not_other_type,
  key_composer::{KeyComposer, KeyTag},
  meta::current_now_ms,
  string::{decode_string_value, encode_string_value, is_string_expired, key::raw},
  wedb::Db,
};

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

          let old_raw = if op.encoding.is_signed() {
            view.get_signed_bitfield(bit_offset, bits)? as u64
          } else {
            view.get_unsigned_bitfield(bit_offset, bits)?
          };

          let (ret, new_raw, _) = bitfield_op_calc(op, old_raw);
          results.push(ret);

          if op.op_type != BitfieldOpType::Get && !read_only && ret.is_some() {
            view.set_bitfield(bit_offset, bits, new_raw)?;
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

      let old_raw = if op.encoding.is_signed() {
        view.get_signed_bitfield(bit_offset, bits)? as u64
      } else {
        view.get_unsigned_bitfield(bit_offset, bits)?
      };

      let (ret, new_raw, _) = bitfield_op_calc(op, old_raw);
      results.push(ret);

      if op.op_type != BitfieldOpType::Get && !read_only && ret.is_some() {
        view.set_bitfield(bit_offset, bits, new_raw)?;

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
