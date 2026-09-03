use crate::{
  api::{
    bitmap::{
      MAX_BITMAP_TO_STRING_BYTES,
      bitops::{
        BITMAP_SEGMENT_BITS, BITMAP_SEGMENT_BYTES, expand_bitmap_segment, get_bit_from_bytes,
        get_bit_lsb, segment_byte_offset_for_bit, segment_index_for_bit, set_bit_in_bytes,
        set_bit_lsb,
      },
      key,
      meta::BitmapMeta,
    },
    key::{check_composite_meta_not_other_type, clear_prefix_in_batch},
    string::key::raw,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key_composer::KeyTag,
  meta::current_now_ms,
  string::{decode_string_value, encode_string_value, is_string_expired},
  wedb::Db,
};
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
