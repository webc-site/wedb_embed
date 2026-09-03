use crate::{
  api::list::{
    ListMeta, ListPopResult,
    r#impl::prepare_list_meta_for_write,
    key::{ItemKeyComposer as ListItemKeyComposer, meta as compose_list_meta_key},
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

/// List element movement and multi-key pop operations (LMOVE, RPOPLPUSH, LMPOP).
/// 列表元素移动与多列表原子弹出实现（对标 Redis 6.2+ / Kvrocks LMOVE 与 LMPOP）
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn rpoplpush<S: AsRef<[u8]>, D: AsRef<[u8]>>(
    &self,
    src: S,
    dst: D,
  ) -> Result<Option<Vec<u8>>> {
    self.lmove(src, dst, false, true)
  }

  #[inline]
  pub fn lmove<S: AsRef<[u8]>, D: AsRef<[u8]>>(
    &self,
    src: S,
    dst: D,
    src_left: bool,
    dst_left: bool,
  ) -> Result<Option<Vec<u8>>> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    let now_ms = current_now_ms();
    let kc = self.kc();

    let src_meta_k = compose_list_meta_key(&kc, src_bytes);
    let mut src_meta = match get_meta_checked::<ListMeta, _>(self, src_bytes, &src_meta_k, now_ms)?
    {
      Some(m) => m,
      None => return Ok(None),
    };

    if src_meta.base.size == 0 {
      return Ok(None);
    }

    let data_ks = self.data();
    let _meta_ks = self.meta();

    if src_bytes == dst_bytes {
      let mut composer = ListItemKeyComposer::new(&kc, src_bytes);
      let curr_idx = if src_left {
        src_meta.head
      } else {
        src_meta.tail.wrapping_sub(1)
      };
      let elem = match data_ks.get(composer.key_for_idx(curr_idx))? {
        Some(v) => v.to_vec(),
        None => return Ok(None),
      };

      if src_left == dst_left || src_meta.base.size == 1 {
        return Ok(Some(elem));
      }

      let mut batch = self.batch();
      batch.rm_data(composer.key_for_idx(curr_idx));

      let _ = src_meta.pop_index(src_left);
      let target_idx = src_meta.push_index(dst_left);

      batch.insert_data(composer.key_for_idx(target_idx), &elem);
      batch.insert_meta(&src_meta_k, &src_meta.encode());
      batch.commit()?;

      return Ok(Some(elem));
    }

    // 跨列表移动
    let mut src_composer = ListItemKeyComposer::new(&kc, src_bytes);
    let curr_src_idx = if src_left {
      src_meta.head
    } else {
      src_meta.tail.wrapping_sub(1)
    };
    let elem = match data_ks.get(src_composer.key_for_idx(curr_src_idx))? {
      Some(v) => v.to_vec(),
      None => return Ok(None),
    };

    let dst_meta_k = compose_list_meta_key(&kc, dst_bytes);
    let mut batch = self.batch();

    let (mut dst_meta, _) =
      prepare_list_meta_for_write(self, dst_bytes, &dst_meta_k, now_ms, &mut batch)?;

    batch.rm_data(src_composer.key_for_idx(curr_src_idx));
    src_meta.base.size -= 1;
    let _ = src_meta.pop_index(src_left);

    if src_meta.base.size == 0 {
      batch.rm_meta(&src_meta_k);
    } else {
      batch.insert_meta(&src_meta_k, &src_meta.encode());
    }

    let mut dst_composer = ListItemKeyComposer::new(&kc, dst_bytes);
    let target_dst_idx = dst_meta.push_index(dst_left);

    dst_meta.base.size += 1;
    batch.insert_data(dst_composer.key_for_idx(target_dst_idx), &elem);
    batch.insert_meta(&dst_meta_k, &dst_meta.encode());
    batch.commit()?;

    Ok(Some(elem))
  }

  #[inline]
  pub fn lmpop<K: AsRef<[u8]>>(
    &self,
    keys: &[K],
    left: bool,
    count: usize,
  ) -> Result<Option<ListPopResult>> {
    if count == 0 || keys.is_empty() {
      return Ok(None);
    }
    for k in keys {
      let popped = if left {
        self.lpop(k, count)?
      } else {
        self.rpop(k, count)?
      };
      if !popped.is_empty() {
        return Ok(Some((k.as_ref().to_vec(), popped)));
      }
    }
    Ok(None)
  }
}
