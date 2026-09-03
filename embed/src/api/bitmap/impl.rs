use std::{borrow::Borrow, collections::hash_map::Entry};

use rapidhash::RapidHashMap as HashMap;

use crate::{
  api::{
    bitmap::{
      MAX_BITMAP_TO_STRING_BYTES,
      bitops::{
        ArrayBitfieldBitmap, BITMAP_SEGMENT_BITS, BITMAP_SEGMENT_BYTES, bit_op_exec_into,
        bitfield_op_calc, expand_bitmap_segment, find_bit_in_byte_lsb, get_bit_from_bytes,
        get_bit_lsb, normalize_range, normalize_to_byte_range_with_padding_mask, raw_bitpos_lsb,
        raw_popcount, segment_byte_offset_for_bit, segment_index_for_bit, set_bit_in_bytes,
        set_bit_lsb, string_bitcount, string_bitpos,
      },
      key,
      meta::BitmapMeta,
      opt::{BitCount, BitOp, BitPos, BitUnit, BitfieldOpType, BitfieldOperation, BitfieldValue},
    },
    key::{check_composite_meta_not_other_type, clear_prefix_in_batch},
    string::key::raw,
  },
  engine::{Engine, Partition},
  error::{ERR_WRONG_TYPE, Error, Result},
  key_composer::{KeyComposer, KeyTag},
  meta::current_now_ms,
  string::{decode_string_value, encode_string_value, is_string_expired},
  wedb::{Db, DbBatch},
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

/// Bitmap operations interface (Bitmaps).
/// 位图结构操作接口 (Bitmaps)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn setbit<K: AsRef<[u8]>>(&self, key: K, offset: u64, bit: u8) -> Result<u8> {
    let kc = self.kc();
    if bit > 1 {
      return Err(Error::invalid_data(
        "ERR bit is out of range, must be 0 or 1",
      ));
    }
    if offset > u32::MAX as u64 {
      return Err(Error::invalid_data(
        "ERR bit offset is not an integer or out of range",
      ));
    }

    let key_bytes = key.as_ref();
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();
    check_composite_meta_not_other_type(self, key_bytes, KeyTag::BitmapMeta.as_slice(), now_ms)?;

    // 1. 优先检查元数据（Segment 分段模式）
    let bm_meta_k = key::meta(&kc, key_bytes);
    let cur_meta_opt = meta_ks
      .get(&bm_meta_k)?
      .and_then(|b| BitmapMeta::decode(&b));

    if let Some(mut meta) = cur_meta_opt
      && !meta.is_expired(now_ms)
    {
      let seg_idx = segment_index_for_bit(offset);
      let bit_offset_in_seg = (offset % (BITMAP_SEGMENT_BITS as u64)) as usize;
      let byte_idx_in_seg = bit_offset_in_seg >> 3;

      let seg_k = key::segment(&kc, key_bytes, seg_idx);
      let seg_slice_opt = data_ks.get(&seg_k)?;
      let old_bit = seg_slice_opt
        .as_deref()
        .map(|s| get_bit_lsb(s, bit_offset_in_seg))
        .unwrap_or(0);

      let used_size = segment_byte_offset_for_bit(offset) as u64 + byte_idx_in_seg as u64 + 1;
      let bitmap_size = meta.base.size.max(used_size);

      if let Some(ref seg_slice) = seg_slice_opt
        && old_bit == bit
        && meta.base.size == bitmap_size
        && byte_idx_in_seg < seg_slice.len()
      {
        return Ok(old_bit);
      }

      let mut seg = seg_slice_opt.map(|v| v.to_vec()).unwrap_or_default();
      expand_bitmap_segment(&mut seg, byte_idx_in_seg + 1);
      set_bit_lsb(&mut seg, bit_offset_in_seg, bit);
      meta.base.size = bitmap_size;

      let mut batch = self.batch();
      batch.insert_data(seg_k.as_slice(), &seg);
      batch.insert_meta(bm_meta_k.as_slice(), &meta.encode());
      batch.commit()?;

      return Ok(old_bit);
    }

    // 2. 检查普通 String 模式
    let raw_k = raw(&kc, key_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, val) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        let byte_idx = (offset >> 3) as usize;
        let old_bit = get_bit_from_bytes(val, offset as usize);

        if old_bit == bit && byte_idx < val.len() {
          return Ok(old_bit);
        }

        let mut str_bytes = val.to_vec();
        if byte_idx >= str_bytes.len() {
          str_bytes.resize(byte_idx + 1, 0);
        }
        set_bit_in_bytes(&mut str_bytes, offset as usize, bit);
        let enc_val = encode_string_value(&str_bytes, expire_at);
        data_ks.insert(&raw_k, &enc_val)?;
        return Ok(old_bit);
      }
    }

    // 3. 不存在任何未过期键时，默认初始化为 Segment 模式
    let seg_idx = segment_index_for_bit(offset);
    let bit_offset_in_seg = (offset % (BITMAP_SEGMENT_BITS as u64)) as usize;
    let byte_idx_in_seg = bit_offset_in_seg >> 3;

    let mut seg = Vec::new();
    expand_bitmap_segment(&mut seg, byte_idx_in_seg + 1);
    let old_bit = set_bit_lsb(&mut seg, bit_offset_in_seg, bit);

    let used_size = segment_byte_offset_for_bit(offset) as u64 + byte_idx_in_seg as u64 + 1;
    let meta = BitmapMeta::new_with_version(0, used_size);

    let seg_k = key::segment(&kc, key_bytes, seg_idx);
    let mut batch = self.batch();
    if cur_meta_opt.is_some() {
      let bm_prefix = key::prefix_stack(&kc, key_bytes);
      clear_prefix_in_batch(self.data(), bm_prefix.as_slice(), &mut batch)?;
    }
    batch.insert_data(seg_k.as_slice(), &seg);
    batch.insert_meta(bm_meta_k.as_slice(), &meta.encode());
    batch.commit()?;

    Ok(old_bit)
  }

  #[inline]
  pub fn getbit<K: AsRef<[u8]>>(&self, key: K, offset: u64) -> Result<u8> {
    let kc = self.kc();
    if offset > u32::MAX as u64 {
      return Ok(0);
    }

    let key_bytes = key.as_ref();
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();

    // 1. 优先检查 Segment 模式
    let bm_meta_k = key::meta(&kc, key_bytes);
    if let Some(m_bytes) = meta_ks.get(&bm_meta_k)?
      && let Some(meta) = BitmapMeta::decode(&m_bytes)
    {
      if meta.is_expired(now_ms) {
        return Ok(0);
      }
      let seg_idx = segment_index_for_bit(offset);
      let bit_offset_in_seg = (offset % (BITMAP_SEGMENT_BITS as u64)) as usize;
      let seg_k = key::segment(&kc, key_bytes, seg_idx);

      if let Some(seg) = data_ks.get(&seg_k)? {
        return Ok(get_bit_lsb(&seg, bit_offset_in_seg));
      }
      return Ok(0);
    }

    // 2. 检查 String 模式
    let raw_k = raw(&kc, key_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, val) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        return Ok(get_bit_from_bytes(val, offset as usize));
      }
    }

    check_composite_meta_not_other_type(self, key_bytes, KeyTag::BitmapMeta.as_slice(), now_ms)?;

    Ok(0)
  }

  #[inline]
  pub fn bitcount<K: AsRef<[u8]>>(
    &self,
    key: K,
    opt_li: impl IntoIterator<Item = BitCount>,
  ) -> Result<u64> {
    let mut start = None;
    let mut end = None;
    let mut is_bit_index = false;
    for opt in opt_li {
      match opt {
        BitCount::Range(s, e) => {
          start = Some(s);
          end = Some(e);
        }
        BitCount::Start(s) => start = Some(s),
        BitCount::End(e) => end = Some(e),
        BitCount::Unit(BitUnit::Bit) => is_bit_index = true,
        BitCount::Unit(BitUnit::Byte) => is_bit_index = false,
      }
    }
    self.raw_bitcount(key, start, end, is_bit_index)
  }

  #[inline]
  pub(crate) fn raw_bitcount<K: AsRef<[u8]>>(
    &self,
    key: K,
    start: Option<i64>,
    end: Option<i64>,
    is_bit_index: bool,
  ) -> Result<u64> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();

    // 1. Segment 模式
    let bm_meta_k = key::meta(&kc, key_bytes);
    if let Some(m_bytes) = meta_ks.get(&bm_meta_k)?
      && let Some(meta) = BitmapMeta::decode(&m_bytes)
    {
      if meta.is_expired(now_ms) || meta.is_empty() {
        return Ok(0);
      }

      let length = if is_bit_index {
        (meta.base.size * 8) as i64
      } else {
        meta.base.size as i64
      };

      let s = start.unwrap_or(0);
      let e = end.unwrap_or(-1);
      if s < 0 && e < 0 && s > e {
        return Ok(0);
      }

      let (norm_s, norm_e) = normalize_range(s, e, length);
      if norm_s > norm_e {
        return Ok(0);
      }

      let (start_byte, stop_byte, first_mask, last_mask) =
        normalize_to_byte_range_with_padding_mask(is_bit_index, norm_s, norm_e);

      let first_seg = start_byte / BITMAP_SEGMENT_BYTES;
      let last_seg = stop_byte / BITMAP_SEGMENT_BYTES;

      let mut total_cnt = 0u64;

      for seg_idx in first_seg..=last_seg {
        let seg_k = key::segment(&kc, key_bytes, seg_idx as u32);
        if let Some(seg) = data_ks.get(&seg_k)? {
          let seg_offset = seg_idx * BITMAP_SEGMENT_BYTES;

          let seg_start = start_byte.saturating_sub(seg_offset);
          let seg_stop = if stop_byte < seg_offset + BITMAP_SEGMENT_BYTES {
            stop_byte - seg_offset
          } else {
            BITMAP_SEGMENT_BYTES - 1
          };

          if seg_start < seg.len() {
            let actual_stop = seg_stop.min(seg.len() - 1);
            if seg_start <= actual_stop {
              let bytes = &seg[seg_start..=actual_stop];
              let cnt = raw_popcount(bytes);
              let mut mask_cnt = 0u64;
              if first_seg == last_seg && seg_idx == first_seg && seg_start == actual_stop {
                let combined_mask = (first_mask | last_mask).reverse_bits();
                if combined_mask != 0 {
                  mask_cnt += (seg[seg_start] & combined_mask).count_ones() as u64;
                }
              } else {
                if first_mask != 0 && seg_idx == first_seg && seg_start < seg.len() {
                  let reversed_first_mask = first_mask.reverse_bits();
                  mask_cnt += (seg[seg_start] & reversed_first_mask).count_ones() as u64;
                }
                if last_mask != 0
                  && seg_idx == last_seg
                  && actual_stop == seg_stop
                  && actual_stop < seg.len()
                {
                  let reversed_last_mask = last_mask.reverse_bits();
                  mask_cnt += (seg[actual_stop] & reversed_last_mask).count_ones() as u64;
                }
              }

              total_cnt += cnt.saturating_sub(mask_cnt);
            }
          }
        }
      }

      return Ok(total_cnt);
    }

    // 2. String 模式
    let raw_k = raw(&kc, key_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, val) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        return Ok(string_bitcount(val, start, end, is_bit_index));
      }
    }

    check_composite_meta_not_other_type(self, key_bytes, KeyTag::BitmapMeta.as_slice(), now_ms)?;

    Ok(0)
  }

  #[inline]
  pub fn bitpos<K: AsRef<[u8]>>(
    &self,
    key: K,
    bit: u8,
    opt_li: impl IntoIterator<Item = BitPos>,
  ) -> Result<i64> {
    let mut start = None;
    let mut end = None;
    let mut is_bit_index = false;
    for opt in opt_li {
      match opt {
        BitPos::Range(s, e) => {
          start = Some(s);
          end = Some(e);
        }
        BitPos::Start(s) => start = Some(s),
        BitPos::End(e) => end = Some(e),
        BitPos::Unit(BitUnit::Bit) => is_bit_index = true,
        BitPos::Unit(BitUnit::Byte) => is_bit_index = false,
      }
    }
    self.raw_bitpos(key, bit, start, end, is_bit_index)
  }

  #[inline]
  pub(crate) fn raw_bitpos<K: AsRef<[u8]>>(
    &self,
    key: K,
    bit: u8,
    start: Option<i64>,
    end: Option<i64>,
    is_bit_index: bool,
  ) -> Result<i64> {
    let stop_given = end.is_some();
    let kc = self.kc();
    if bit > 1 {
      return Err(Error::invalid_data(
        "ERR bit is out of range, must be 0 or 1",
      ));
    }

    let key_bytes = key.as_ref();
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();

    // 1. Segment 模式
    let bm_meta_k = key::meta(&kc, key_bytes);
    if let Some(m_bytes) = meta_ks.get(&bm_meta_k)?
      && let Some(meta) = BitmapMeta::decode(&m_bytes)
    {
      if meta.is_expired(now_ms) || meta.is_empty() {
        return Ok(if bit == 0 { 0 } else { -1 });
      }

      let length = if is_bit_index {
        (meta.base.size * 8) as i64
      } else {
        meta.base.size as i64
      };

      let s = start.unwrap_or(0);
      let e = end.unwrap_or(-1);
      let (norm_s, norm_e) = normalize_range(s, e, length);
      if norm_s > norm_e {
        return Ok(-1);
      }

      let u_start = norm_s as usize;
      let u_stop = norm_e as usize;
      let byte_start = if is_bit_index { u_start / 8 } else { u_start };
      let byte_stop = if is_bit_index { u_stop / 8 } else { u_stop };
      let start_seg = byte_start / BITMAP_SEGMENT_BYTES;
      let stop_seg = byte_stop / BITMAP_SEGMENT_BYTES;

      for seg_idx in start_seg..=stop_seg {
        let seg_k = key::segment(&kc, key_bytes, seg_idx as u32);
        let seg_opt = data_ks.get(&seg_k)?;
        let seg_offset_bytes = seg_idx * BITMAP_SEGMENT_BYTES;
        let seg_start_byte = byte_start.saturating_sub(seg_offset_bytes);
        let seg_stop_byte = if byte_stop < seg_offset_bytes + BITMAP_SEGMENT_BYTES {
          byte_stop - seg_offset_bytes
        } else {
          BITMAP_SEGMENT_BYTES - 1
        };

        if let Some(seg) = seg_opt {
          let seg_slice = &seg[..];
          if seg_start_byte < seg_slice.len() {
            let actual_stop = seg_stop_byte.min(seg_slice.len() - 1);
            if is_bit_index {
              for (b_idx, &b) in seg_slice[..=actual_stop]
                .iter()
                .enumerate()
                .skip(seg_start_byte)
              {
                let start_bit = if seg_idx == start_seg && b_idx == seg_start_byte {
                  u_start % 8
                } else {
                  0
                };
                let stop_bit = if seg_idx == stop_seg && b_idx == seg_stop_byte {
                  u_stop % 8
                } else {
                  7
                };
                if let Some(bit_idx) = find_bit_in_byte_lsb(b, bit, start_bit, stop_bit) {
                  let abs_pos = ((seg_offset_bytes + b_idx) * 8 + bit_idx) as i64;
                  return Ok(abs_pos);
                }
              }
            } else if let Some(rel_pos) =
              raw_bitpos_lsb(&seg_slice[seg_start_byte..=actual_stop], bit)
            {
              let abs_pos = ((seg_offset_bytes + seg_start_byte) * 8 + rel_pos) as i64;
              return Ok(abs_pos);
            }
          }

          if bit == 0 && seg_slice.len() <= seg_stop_byte {
            let start_byte_in_seg = seg_start_byte.max(seg_slice.len());
            let first_zero_bit = (seg_offset_bytes + start_byte_in_seg) * 8;
            let abs_pos = if seg_idx == start_seg {
              u_start.max(first_zero_bit) as i64
            } else {
              first_zero_bit as i64
            };
            if is_bit_index && abs_pos > norm_e {
              return Ok(-1);
            }
            return Ok(abs_pos);
          }
        } else if bit == 0 {
          let pos_in_seg = if seg_idx == start_seg {
            if is_bit_index {
              u_start.saturating_sub(seg_offset_bytes * 8)
            } else {
              seg_start_byte * 8
            }
          } else {
            0
          };
          let abs_pos = (seg_offset_bytes * 8 + pos_in_seg) as i64;
          if is_bit_index && abs_pos > norm_e {
            return Ok(-1);
          }
          return Ok(abs_pos);
        }
      }

      return Ok(if stop_given && bit == 0 {
        -1
      } else if bit == 0 {
        (meta.base.size * 8) as i64
      } else {
        -1
      });
    }

    // 2. String 模式
    let raw_k = raw(&kc, key_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, val) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        return Ok(string_bitpos(
          val,
          bit,
          start,
          end,
          stop_given,
          is_bit_index,
        ));
      }
    }

    check_composite_meta_not_other_type(self, key_bytes, KeyTag::BitmapMeta.as_slice(), now_ms)?;

    Ok(if bit == 0 { 0 } else { -1 })
  }

  #[inline]
  pub fn bitop<K: AsRef<[u8]>, S: AsRef<[u8]>>(
    &self,
    op: BitOp,
    dest_key: K,
    src_keys: &[S],
  ) -> Result<usize> {
    let kc = self.kc();
    if src_keys.is_empty() || (op == BitOp::Not && src_keys.len() != 1) {
      return Err(Error::invalid_data(
        "ERR syntax error in BITOP or wrong number of arguments",
      ));
    }

    let dest_bytes = dest_key.as_ref();
    let dest_meta_k = key::meta(&kc, dest_bytes);
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();

    let mut src_metas: Vec<(&[u8], BitmapMeta)> = Vec::with_capacity(src_keys.len());
    let mut max_bitmap_size = 0u64;

    for sk in src_keys {
      let sk_bytes = sk.as_ref();
      let raw_sk = raw(&kc, sk_bytes);

      // 检查源 key 是否是 String 类型，Kvrocks 中 Bitmap 与 String 间不支持 BITOP
      if let Some(raw) = data_ks.get(&raw_sk)? {
        let (expire_at, _) = decode_string_value(&raw);
        if !is_string_expired(expire_at, now_ms) {
          return Err(Error::wrong_type(ERR_WRONG_TYPE));
        }
      }

      let sm_k = key::meta(&kc, sk_bytes);
      if let Some(m_bytes) = meta_ks.get(&sm_k)?
        && let Some(m) = BitmapMeta::decode(&m_bytes)
        && !m.is_expired(now_ms)
      {
        max_bitmap_size = max_bitmap_size.max(m.base.size);
        src_metas.push((sk_bytes, m));
      }
    }

    let mut batch = self.batch();

    let old_dest_meta = meta_ks
      .get(&dest_meta_k)?
      .and_then(|b| BitmapMeta::decode(&b));

    let clean_old_dest_segments = |batch: &mut DbBatch<E>| {
      if let Some(ref old_m) = old_dest_meta {
        let old_stop_seg = (old_m.base.size.saturating_sub(1) as usize) / BITMAP_SEGMENT_BYTES;
        for seg_idx in 0..=old_stop_seg {
          let seg_k = key::segment(&kc, dest_bytes, seg_idx as u32);
          batch.rm_weak_data(seg_k.as_slice());
        }
      }
    };

    if max_bitmap_size == 0 {
      // 清理目标 bitmap
      batch.rm_meta(dest_meta_k.as_slice());
      clean_old_dest_segments(&mut batch);
      batch.commit()?;
      return Ok(0);
    }

    let can_skip_op = op == BitOp::And && src_metas.len() != src_keys.len();
    if can_skip_op {
      // AND 运算中只要任一源键为空，结果即全为 0，但记录目标元数据大小为 max_bitmap_size
      clean_old_dest_segments(&mut batch);
      let dest_meta = BitmapMeta::new_with_version(0, max_bitmap_size);
      batch.insert_meta(dest_meta_k.as_slice(), &dest_meta.encode());
      batch.commit()?;
      return Ok(max_bitmap_size as usize);
    }

    let stop_seg_index = (max_bitmap_size.saturating_sub(1) as usize) / BITMAP_SEGMENT_BYTES;
    let mut frag_res = [0u8; BITMAP_SEGMENT_BYTES];
    let mut fragments: Vec<Option<<E::Partition as Partition>::Value>> =
      Vec::with_capacity(src_metas.len());

    for frag_idx in 0..=stop_seg_index {
      fragments.clear();
      let mut frag_maxlen = 0usize;

      for (sk_bytes, _) in &src_metas {
        let sub_k = key::segment(&kc, sk_bytes, frag_idx as u32);
        let frag_opt = data_ks.get(sub_k.as_slice())?;

        if let Some(ref frag) = frag_opt {
          if frag.is_empty() {
            if op == BitOp::And {
              frag_maxlen = 0;
              break;
            }
          } else {
            frag_maxlen = frag_maxlen.max(frag.len());
          }
        } else if op == BitOp::And {
          frag_maxlen = 0;
          break;
        }
        fragments.push(frag_opt);
      }

      let dest_sub_k = key::segment(&kc, dest_bytes, frag_idx as u32);
      if frag_maxlen != 0 || op == BitOp::Not {
        let write_len = if op == BitOp::Not {
          if frag_idx == stop_seg_index {
            if max_bitmap_size.is_multiple_of(BITMAP_SEGMENT_BYTES as u64) {
              BITMAP_SEGMENT_BYTES
            } else {
              (max_bitmap_size % (BITMAP_SEGMENT_BYTES as u64)) as usize
            }
          } else {
            BITMAP_SEGMENT_BYTES
          }
        } else {
          frag_maxlen
        };

        let frag_slices: Vec<&[u8]> = fragments
          .iter()
          .map(|f| f.as_deref().unwrap_or(&[]))
          .collect();
        bit_op_exec_into(op, &frag_slices, &mut frag_res[..write_len])?;

        batch.insert_data(dest_sub_k.as_slice(), &frag_res[..write_len]);
      } else {
        batch.rm_weak_data(dest_sub_k.as_slice());
      }
    }

    // 清理旧目标键中超出当前大小的多余分段
    if let Some(old_m) = old_dest_meta {
      let old_stop_seg = (old_m.base.size.saturating_sub(1) as usize) / BITMAP_SEGMENT_BYTES;
      if old_stop_seg > stop_seg_index {
        for seg_idx in (stop_seg_index + 1)..=old_stop_seg {
          let seg_k = key::segment(&kc, dest_bytes, seg_idx as u32);
          batch.rm_weak_data(seg_k.as_slice());
        }
      }
    }

    let dest_meta = BitmapMeta::new_with_version(0, max_bitmap_size);
    batch.insert_meta(dest_meta_k.as_slice(), &dest_meta.encode());
    batch.commit()?;

    Ok(max_bitmap_size as usize)
  }

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

  #[inline]
  pub fn get_bitmap_bytes<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let meta_ks = self.meta();

    // 1. 优先检查 Segment 模式
    let bm_meta_k = key::meta(&kc, key_bytes);
    if let Some(m_bytes) = meta_ks.get(&bm_meta_k)?
      && let Some(meta) = BitmapMeta::decode(&m_bytes)
    {
      if meta.is_expired(now_ms) || meta.is_empty() {
        return Ok(None);
      }

      let total_size = meta.base.size as usize;
      if total_size > MAX_BITMAP_TO_STRING_BYTES {
        return Err(Error::invalid_data(
          "The size of the bitmap string exceeds configuration max-bitmap-to-string-mb (512MB)",
        ));
      }
      let mut out = vec![0u8; total_size];
      let stop_seg = (total_size.saturating_sub(1)) / BITMAP_SEGMENT_BYTES;

      for seg_idx in 0..=stop_seg {
        let seg_k = key::segment(&kc, key_bytes, seg_idx as u32);
        if let Some(seg_bytes) = data_ks.get(&seg_k)? {
          let seg_start = seg_idx * BITMAP_SEGMENT_BYTES;
          let copy_len = seg_bytes.len().min(total_size.saturating_sub(seg_start));
          for (dst, &src) in out[seg_start..seg_start + copy_len]
            .iter_mut()
            .zip(&seg_bytes[..copy_len])
          {
            *dst = src.reverse_bits();
          }
        }
      }

      return Ok(Some(out));
    }

    // 2. 检查 String 模式
    let raw_k = raw(&kc, key_bytes);
    if let Some(raw) = data_ks.get(&raw_k)? {
      let (expire_at, val) = decode_string_value(&raw);
      if !is_string_expired(expire_at, now_ms) {
        return Ok(Some(val.to_vec()));
      }
    }

    check_composite_meta_not_other_type(self, key_bytes, KeyTag::BitmapMeta.as_slice(), now_ms)?;

    Ok(None)
  }

  /// Retrieves bitmap as bytes string (aligned with Apache Kvrocks Bitmap::GetString).
  /// 将位图导出为连续字节字符串（对标 Apache Kvrocks Bitmap::GetString）
  #[inline]
  pub fn get_bitmap_string<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    self.get_bitmap_bytes(key)
  }
}
