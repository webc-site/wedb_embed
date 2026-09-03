use crate::{
  api::key::{cleanup_all_composite_data_with_buf, exists_impl, find_active_composite_meta},
  engine::{Engine, KvEntry, Partition},
  error::{ERR_NO_SUCH_KEY, Error, Result},
  key_composer::KeyTag,
  meta::{KeyMeta, current_now_ms, generate_version},
  string::{compose_string_key as raw, decode_string_value, is_string_expired},
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn copy<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2, nx: bool) -> Result<bool> {
    copy_impl(self, src.as_ref(), dst.as_ref(), nx, false)
  }

  #[inline]
  pub fn copy_replace<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2) -> Result<bool> {
    copy_impl(self, src.as_ref(), dst.as_ref(), false, false)
  }

  #[inline]
  pub fn rename<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2) -> Result<()> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    if !self.exists_one(src_bytes)? {
      return Err(Error::not_found(ERR_NO_SUCH_KEY));
    }
    if src_bytes == dst_bytes {
      return Ok(());
    }
    copy_impl(self, src_bytes, dst_bytes, false, true)?;
    Ok(())
  }

  #[inline]
  pub fn renamenx<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(&self, src: K1, dst: K2) -> Result<bool> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    if !self.exists_one(src_bytes)? {
      return Err(Error::not_found(ERR_NO_SUCH_KEY));
    }
    if src_bytes == dst_bytes {
      return Ok(false);
    }
    copy_impl(self, src_bytes, dst_bytes, true, true)
  }
}

/// Copies or moves a key and all its associated subkeys (COPY / RENAME / RENAMENX).
/// 复制或原子移动键及其所有关联子键（对标 Kvrocks Database::Copy / Rename）
pub fn copy_impl<E: Engine>(
  db: &Db<E>,
  src: &[u8],
  dst: &[u8],
  nx: bool,
  delete_old: bool,
) -> Result<bool>
where
  Error: From<E::Error>,
{
  if src == dst {
    let exists = exists_impl(db, &[src])? > 0;
    if !exists {
      return Ok(false);
    }
    return Ok(!nx);
  }

  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  if nx && exists_impl(db, &[dst])? > 0 {
    return Ok(false);
  }

  let src_raw_k = raw(&kc, src);

  // 1. 检查原生 String
  if let Some(src_val) = data_ks.get(&src_raw_k)? {
    let (exp, _) = decode_string_value(&src_val);
    if !is_string_expired(exp, now_ms) {
      let mut batch = db.batch();
      if !nx {
        let dst_raw_k = raw(&kc, dst);
        batch.rm_data(&dst_raw_k);
        let mut buf = Vec::new();
        cleanup_all_composite_data_with_buf(db, dst, &mut batch, &mut buf)?;
      }
      let dst_raw_k = raw(&kc, dst);
      batch.insert_data(&dst_raw_k, &src_val);
      if delete_old {
        batch.rm_data(&src_raw_k);
      }
      batch.commit()?;
      return Ok(true);
    }
  }

  if meta_ks.is_empty()? {
    return Ok(false);
  }

  // 2. 检查复合类型元数据
  let mut buf = Vec::new();
  if let Some((tag_u8, _base_meta, raw_guard)) =
    find_active_composite_meta(db, src, now_ms, &mut buf)?
  {
    let mut batch = db.batch();
    if !nx {
      let dst_raw_k = raw(&kc, dst);
      batch.rm_data(&dst_raw_k);
      cleanup_all_composite_data_with_buf(db, dst, &mut batch, &mut buf)?;
    }

    let new_version = generate_version();
    let mut dst_meta_val = raw_guard.to_vec();
    let is_kvrocks = (dst_meta_val[0] & KeyMeta::META_64BIT_ENCODING_MASK) != 0
      || (dst_meta_val.len() >= KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE && dst_meta_val[0] > 14);
    let ver_offset = if is_kvrocks { 9 } else { 10 };
    if dst_meta_val.len() >= ver_offset + 8 {
      dst_meta_val[ver_offset..ver_offset + 8].copy_from_slice(&new_version.to_be_bytes());
    }

    kc.compose_meta_key_into(&[tag_u8], dst, &mut buf);
    batch.insert_meta(&buf, &dst_meta_val);

    let Some(key_tag) = KeyTag::from_u8(tag_u8) else {
      return Ok(false);
    };

    let data_tags: &'static [KeyTag] = match key_tag {
      KeyTag::HashMeta => &[KeyTag::HashData],
      KeyTag::ListMeta => &[KeyTag::ListData],
      KeyTag::SetMeta => &[KeyTag::SetData],
      KeyTag::ZSetMeta => &[KeyTag::ZSetData, KeyTag::ZSetScore],
      KeyTag::BitmapMeta => &[KeyTag::BitmapData],
      KeyTag::BloomMeta => &[KeyTag::BloomData],
      KeyTag::CuckooMeta => &[KeyTag::CuckooData],
      KeyTag::SortedIntMeta => &[KeyTag::SortedIntData],
      KeyTag::TimeSeriesMeta => &[KeyTag::TimeSeriesData],
      KeyTag::StreamMeta => &[
        KeyTag::StreamData,
        KeyTag::StreamGroup,
        KeyTag::StreamConsumer,
        KeyTag::StreamPel,
      ],
      KeyTag::JsonMeta => &[KeyTag::JsonData],
      KeyTag::TDigestMeta => &[KeyTag::TDigestData],
      KeyTag::HllMeta => &[KeyTag::HllRaw],
      _ => &[],
    };

    let mut src_prefix = Vec::with_capacity(32 + src.len());
    let mut dst_prefix = Vec::with_capacity(32 + dst.len());
    let mut sub_buf = Vec::with_capacity(64);

    for &dtag in data_tags {
      if dtag == KeyTag::HllRaw {
        kc.compose_meta_key_into(dtag.as_slice(), src, &mut src_prefix);
        kc.compose_meta_key_into(dtag.as_slice(), dst, &mut dst_prefix);
        if let Some(val) = data_ks.get(&src_prefix)? {
          batch.insert_data(&dst_prefix, &val);
          if delete_old {
            batch.rm_data(&src_prefix);
          }
        }
      } else {
        kc.compose_prefix_into(dtag.as_slice(), src, &mut src_prefix);
        kc.compose_prefix_into(dtag.as_slice(), dst, &mut dst_prefix);

        for item in data_ks.prefix(&src_prefix) {
          let entry = item?;
          let k = entry.key();
          if !k.starts_with(&src_prefix) {
            break;
          }
          let remain = &k[src_prefix.len()..];
          sub_buf.clear();
          sub_buf.extend_from_slice(&dst_prefix);
          sub_buf.extend_from_slice(remain);

          batch.insert_data(&sub_buf, entry.value());
          if delete_old {
            batch.rm_data(k);
          }
        }
      }
    }

    if delete_old {
      kc.compose_meta_key_into(&[tag_u8], src, &mut buf);
      batch.rm_meta(&buf);
    }

    batch.commit()?;
    return Ok(true);
  }

  Ok(false)
}
