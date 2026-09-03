use super::{bitops::*, key, meta::BitmapMeta};
use crate::{
  bitmap::opt::BitOp,
  engine::{Engine, Partition},
  error::{ERR_WRONG_TYPE, Error, Result},
  meta::current_now_ms,
  string::{decode_string_value, is_string_expired, key::raw},
  wedb::{Db, DbBatch},
};

/// Bitmap bitwise operations interface (BITOP).
/// 位图按位逻辑运算操作实现（AND, OR, XOR, NOT）
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
