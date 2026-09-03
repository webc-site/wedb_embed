use super::{bitops::*, key, meta::BitmapMeta};
use crate::{
  bitmap::opt::{BitCount, BitPos, BitUnit},
  engine::{Engine, Partition},
  error::{Error, Result},
  key::check_composite_meta_not_other_type,
  key_composer::KeyTag,
  meta::current_now_ms,
  string::{decode_string_value, is_string_expired, key::raw},
  wedb::Db,
};

/// Bitmap range count and position search operations (BITCOUNT, BITPOS).
/// 位图范围统计与位查找接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
