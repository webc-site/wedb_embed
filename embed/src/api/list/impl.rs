use crate::{
  IntoIndexRange,
  api::list::{
    ListItemKeyComposer, compose_list_item, compose_list_meta_key, compose_list_prefix_stack,
    r#const::{ERR_INDEX_OUT_OF_RANGE, ERR_RANK_ZERO},
    meta::ListMeta,
    opt::LPos,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::current_now_ms,
  normalize_range,
  wedb::{Db, DbBatch},
};

/// List structure operations interface (Lists).
/// 列表结构操作接口 (Lists)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn lpush_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<u64> {
    list_push_internal(self, key.as_ref(), &[value], true, true)
  }

  #[inline]
  pub fn lpush<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, values: &[V]) -> Result<u64> {
    list_push_internal(self, key.as_ref(), values, true, true)
  }

  #[inline]
  pub fn rpush_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<u64> {
    list_push_internal(self, key.as_ref(), &[value], true, false)
  }

  #[inline]
  pub fn rpush<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, values: &[V]) -> Result<u64> {
    list_push_internal(self, key.as_ref(), values, true, false)
  }

  #[inline]
  pub fn lpushx_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<u64> {
    list_push_internal(self, key.as_ref(), &[value], false, true)
  }

  #[inline]
  pub fn lpushx<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, values: &[V]) -> Result<u64> {
    list_push_internal(self, key.as_ref(), values, false, true)
  }

  #[inline]
  pub fn rpushx_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<u64> {
    list_push_internal(self, key.as_ref(), &[value], false, false)
  }

  #[inline]
  pub fn rpushx<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, values: &[V]) -> Result<u64> {
    list_push_internal(self, key.as_ref(), values, false, false)
  }

  #[inline]
  pub fn lpop_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let mut res = list_pop_internal(self, key.as_ref(), 1, true)?;
    Ok(res.pop())
  }

  #[inline]
  pub fn lpop<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<Vec<u8>>> {
    list_pop_internal(self, key.as_ref(), count, true)
  }

  #[inline]
  pub fn rpop_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let mut res = list_pop_internal(self, key.as_ref(), 1, false)?;
    Ok(res.pop())
  }

  #[inline]
  pub fn rpop<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<Vec<u8>>> {
    list_pop_internal(self, key.as_ref(), count, false)
  }

  #[inline]
  pub fn llen<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => Ok(m.base.size),
      None => Ok(0),
    }
  }

  #[inline]
  pub fn lrange<K: AsRef<[u8]>>(&self, key: K, range: impl IntoIndexRange) -> Result<Vec<Vec<u8>>> {
    let (start, stop) = range.into_index_range();
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(Vec::new()),
    };

    if meta.base.size == 0 {
      return Ok(Vec::new());
    }

    let len = meta.base.size as i64;
    let (s, e) = normalize_range(start, stop, len);
    if s > e {
      return Ok(Vec::new());
    }

    let num_elems = (e - s + 1) as usize;
    let mut results = Vec::with_capacity(num_elems);
    let actual_start = meta.head.wrapping_add(s as u64);
    let actual_end = meta.head.wrapping_add(e as u64);

    if actual_start <= actual_end {
      let start_k = compose_list_item(&kc, key_bytes, actual_start);
      let end_k = compose_list_item(&kc, key_bytes, actual_end);
      for g in self.data().range(start_k.as_slice()..=end_k.as_slice()) {
        let entry = g?;
        results.push(entry.value().to_vec());
      }
    } else {
      let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
      let data_ks = self.data();
      for idx in s..=e {
        let actual_idx = meta.head.wrapping_add(idx as u64);
        let item_k = composer.key_for_idx(actual_idx);
        if let Some(val) = data_ks.get(item_k)? {
          results.push(val.to_vec());
        }
      }
    }
    Ok(results)
  }

  #[inline]
  pub fn with_lindex<K: AsRef<[u8]>, R>(
    &self,
    key: K,
    index: i64,
    f: impl FnOnce(&[u8]) -> R,
  ) -> Result<Option<R>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(None),
    };

    let len = meta.base.size as i64;
    let actual_offset = if index < 0 {
      len.checked_add(index).unwrap_or(i64::MIN)
    } else {
      index
    };

    if actual_offset < 0 || actual_offset >= len {
      return Ok(None);
    }

    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
    let actual_idx = meta.head.wrapping_add(actual_offset as u64);
    let item_k = composer.key_for_idx(actual_idx);
    let val = self.data().get(item_k)?;
    Ok(val.as_deref().map(f))
  }

  #[inline]
  pub fn lindex<K: AsRef<[u8]>>(&self, key: K, index: i64) -> Result<Option<Vec<u8>>> {
    self.with_lindex(key, index, |v| v.to_vec())
  }

  #[inline]
  pub fn lset<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, index: i64, value: V) -> Result<()> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Err(Error::invalid_data(ERR_INDEX_OUT_OF_RANGE)),
    };

    if meta.base.size == 0 {
      return Err(Error::invalid_data(ERR_INDEX_OUT_OF_RANGE));
    }

    let len = meta.base.size as i64;
    let actual_offset = if index < 0 {
      len.checked_add(index).unwrap_or(i64::MIN)
    } else {
      index
    };

    if actual_offset < 0 || actual_offset >= len {
      return Err(Error::invalid_data(ERR_INDEX_OUT_OF_RANGE));
    }

    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
    let actual_idx = meta.head.wrapping_add(actual_offset as u64);
    let item_k = composer.key_for_idx(actual_idx);

    let mut batch = self.batch_with_capacity(1);
    batch.insert_data(item_k, value.as_ref());
    batch.commit()?;

    Ok(())
  }

  #[inline]
  pub fn ltrim<K: AsRef<[u8]>>(&self, key: K, range: impl IntoIndexRange) -> Result<()> {
    let (start, stop) = range.into_index_range();
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(()),
    };

    if meta.base.size == 0 {
      return Ok(());
    }

    let len = meta.base.size as i64;
    let (s, e) = normalize_range(start, stop, len);

    let mut batch = self.batch_with_capacity(32);
    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);

    if s > e {
      for offset in 0..meta.base.size {
        let idx = meta.head.wrapping_add(offset);
        let item_k = composer.key_for_idx(idx);
        batch.rm_weak_data(item_k);
      }
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(());
    }

    for offset in 0..(s as u64) {
      let idx = meta.head.wrapping_add(offset);
      let item_k = composer.key_for_idx(idx);
      batch.rm_weak_data(item_k);
    }

    for offset in ((e + 1) as u64)..meta.base.size {
      let idx = meta.head.wrapping_add(offset);
      let item_k = composer.key_for_idx(idx);
      batch.rm_weak_data(item_k);
    }

    let new_size = (e - s + 1) as u64;
    let new_head = meta.head.wrapping_add(s as u64);
    let new_tail = new_head.wrapping_add(new_size);

    meta.base.size = new_size;
    meta.head = new_head;
    meta.tail = new_tail;

    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
    Ok(())
  }

  #[inline]
  pub fn linsert<K: AsRef<[u8]>, P: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    before: bool,
    pivot: P,
    elem: V,
  ) -> Result<i64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    if meta.base.size == 0 {
      return Ok(0);
    }

    let len = meta.base.size as usize;
    let pivot_bytes = pivot.as_ref();
    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
    let data_ks = self.data();
    let _meta_ks = self.meta();

    let actual_start = meta.head;
    let actual_end = meta.tail.wrapping_sub(1);
    let mut pivot_offset = None;

    if actual_start <= actual_end {
      let start_k = compose_list_item(&kc, key_bytes, actual_start);
      let end_k = compose_list_item(&kc, key_bytes, actual_end);
      for (offset, g) in data_ks.range(start_k.as_slice()..=end_k.as_slice()).enumerate() {
        let entry = g?;
        if entry.value() == pivot_bytes {
          pivot_offset = Some(offset);
          break;
        }
      }
    } else {
      for offset in 0..len {
        let idx = meta.head.wrapping_add(offset as u64);
        let item_k = composer.key_for_idx(idx);
        if let Some(v) = data_ks.get(item_k)?
          && v.as_ref() == pivot_bytes
        {
          pivot_offset = Some(offset);
          break;
        }
      }
    }

    let pivot_offset = match pivot_offset {
      Some(o) => o,
      None => return Ok(-1),
    };

    let insert_offset = if before {
      pivot_offset
    } else {
      pivot_offset + 1
    };

    let mut batch = self.batch();

    if insert_offset < len / 2 {
      let new_head = meta.head.wrapping_sub(1);
      for offset in 0..insert_offset {
        let from_idx = meta.head.wrapping_add(offset as u64);
        let to_idx = new_head.wrapping_add(offset as u64);
        if let Some(val) = data_ks.get(composer.key_for_idx(from_idx))? {
          batch.insert_data(composer.key_for_idx(to_idx), val.as_ref());
        }
      }
      let target_idx = new_head.wrapping_add(insert_offset as u64);
      batch.insert_data(composer.key_for_idx(target_idx), elem.as_ref());
      meta.head = new_head;
    } else {
      let old_tail = meta.tail;
      let new_tail = old_tail.wrapping_add(1);
      for offset in (insert_offset..len).rev() {
        let from_idx = meta.head.wrapping_add(offset as u64);
        let to_idx = from_idx.wrapping_add(1);
        if let Some(val) = data_ks.get(composer.key_for_idx(from_idx))? {
          batch.insert_data(composer.key_for_idx(to_idx), val.as_ref());
        }
      }
      let target_idx = meta.head.wrapping_add(insert_offset as u64);
      batch.insert_data(composer.key_for_idx(target_idx), elem.as_ref());
      meta.tail = new_tail;
    }

    meta.base.size += 1;
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;

    Ok(meta.base.size as i64)
  }

  #[inline]
  pub fn lrem<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, count: i64, elem: V) -> Result<u64> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    if meta.base.size == 0 {
      return Ok(0);
    }

    let len = meta.base.size as usize;
    let target_del_limit = if count == 0 {
      usize::MAX
    } else {
      count.unsigned_abs() as usize
    };

    let elem_bytes = elem.as_ref();
    let mut to_delete_offsets = Vec::new();
    let data_ks = self.data();
    let _meta_ks = self.meta();

    let actual_start = meta.head;
    let actual_end = meta.tail.wrapping_sub(1);

    if actual_start <= actual_end {
      let start_k = compose_list_item(&kc, key_bytes, actual_start);
      let end_k = compose_list_item(&kc, key_bytes, actual_end);
      if count >= 0 {
        for (offset, g) in data_ks.range(start_k.as_slice()..=end_k.as_slice()).enumerate() {
          let entry = g?;
          if entry.value() == elem_bytes {
            to_delete_offsets.push(offset);
            if to_delete_offsets.len() >= target_del_limit {
              break;
            }
          }
        }
      } else {
        for (i, g) in data_ks.range(start_k.as_slice()..=end_k.as_slice()).rev().enumerate() {
          let entry = g?;
          if entry.value() == elem_bytes {
            let offset = len - 1 - i;
            to_delete_offsets.push(offset);
            if to_delete_offsets.len() >= target_del_limit {
              break;
            }
          }
        }
        to_delete_offsets.reverse();
      }
    } else {
      let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
      let mut check_match = |offset: usize| -> Result<bool> {
        let idx = meta.head.wrapping_add(offset as u64);
        let item_k = composer.key_for_idx(idx);
        if let Some(v) = data_ks.get(item_k)?
          && v.as_ref() == elem_bytes
        {
          to_delete_offsets.push(offset);
          if to_delete_offsets.len() >= target_del_limit {
            return Ok(false);
          }
        }
        Ok(true)
      };

      if count >= 0 {
        for offset in 0..len {
          if !check_match(offset)? {
            break;
          }
        }
      } else {
        for offset in (0..len).rev() {
          if !check_match(offset)? {
            break;
          }
        }
        to_delete_offsets.reverse();
      }
    }

    if to_delete_offsets.is_empty() {
      return Ok(0);
    }

    let deleted_count = to_delete_offsets.len() as u64;
    let mut batch = self.batch();

    if deleted_count == meta.base.size {
      let prefix = compose_list_prefix_stack(&kc, key_bytes);
      clear_prefix_in_batch(self.data(), &prefix, &mut batch)?;
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(deleted_count);
    }

    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);

    let mut write_idx = 0usize;
    let mut del_idx = 0usize;

    for read_idx in 0..len {
      if del_idx < to_delete_offsets.len() && to_delete_offsets[del_idx] == read_idx {
        del_idx += 1;
        continue;
      }
      if write_idx != read_idx {
        let from_key_idx = meta.head.wrapping_add(read_idx as u64);
        let to_key_idx = meta.head.wrapping_add(write_idx as u64);
        if let Some(val) = data_ks.get(composer.key_for_idx(from_key_idx))? {
          batch.insert_data(composer.key_for_idx(to_key_idx), val.as_ref());
        }
      }
      write_idx += 1;
    }

    for extra_idx in write_idx..len {
      let key_idx = meta.head.wrapping_add(extra_idx as u64);
      batch.rm_data(composer.key_for_idx(key_idx));
    }

    meta.base.size -= deleted_count;
    meta.tail = meta.head.wrapping_add(meta.base.size);

    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;

    Ok(deleted_count)
  }

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

      if src_left == dst_left {
        return Ok(Some(elem));
      }

      let mut batch = self.batch();
      batch.rm_data(composer.key_for_idx(curr_idx));

      if src_left {
        src_meta.head = src_meta.head.wrapping_add(1);
      } else {
        src_meta.tail = src_meta.tail.wrapping_sub(1);
      }

      let target_idx = if dst_left {
        src_meta.head = src_meta.head.wrapping_sub(1);
        src_meta.head
      } else {
        let t = src_meta.tail;
        src_meta.tail = src_meta.tail.wrapping_add(1);
        t
      };

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
    if src_left {
      src_meta.head = src_meta.head.wrapping_add(1);
    } else {
      src_meta.tail = src_meta.tail.wrapping_sub(1);
    }

    if src_meta.base.size == 0 {
      batch.rm_meta(&src_meta_k);
    } else {
      batch.insert_meta(&src_meta_k, &src_meta.encode());
    }

    let mut dst_composer = ListItemKeyComposer::new(&kc, dst_bytes);
    let target_dst_idx = if dst_left {
      dst_meta.head = dst_meta.head.wrapping_sub(1);
      dst_meta.head
    } else {
      let t = dst_meta.tail;
      dst_meta.tail = dst_meta.tail.wrapping_add(1);
      t
    };

    dst_meta.base.size += 1;
    batch.insert_data(dst_composer.key_for_idx(target_dst_idx), &elem);
    batch.insert_meta(&dst_meta_k, &dst_meta.encode());
    batch.commit()?;

    Ok(Some(elem))
  }

  #[inline]
  pub fn lpos_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    elem: V,
    opt_li: impl IntoIterator<Item = LPos>,
  ) -> Result<Option<i64>> {
    let res = self.lpos(key, elem, opt_li)?;
    Ok(res.into_iter().next())
  }

  #[inline]
  pub fn lpos<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    elem: V,
    opt_li: impl IntoIterator<Item = LPos>,
  ) -> Result<Vec<i64>> {
    let mut rank = 1i64;
    let mut count = None;
    let mut max_len = None;
    for opt in opt_li {
      match opt {
        LPos::Rank(r) => rank = r,
        LPos::Count(c) => count = Some(c),
        LPos::MaxLen(m) => max_len = Some(m),
      }
    }
    if rank == 0 {
      return Err(Error::invalid_data(ERR_RANK_ZERO));
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(Vec::new()),
    };

    if meta.base.size == 0 {
      return Ok(Vec::new());
    }

    let len = meta.base.size as usize;
    let reversed = rank < 0;
    let target_rank = rank.unsigned_abs() as usize;
    let limit = max_len
      .map(|m| if m == 0 { len } else { m.min(len) })
      .unwrap_or(len);

    let elem_bytes = elem.as_ref();
    let count_limit = match count {
      Some(0) => usize::MAX,
      Some(c) => c,
      None => 1,
    };
    let is_multi_count = count.is_some();
    let mut matches = match count {
      Some(c) if c > 0 => Vec::with_capacity(c.min(limit)),
      _ => Vec::with_capacity(limit.min(16)),
    };

    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
    let data_ks = self.data();
    let mut rank_count = 0usize;

    let mut check_offset = |offset: usize| -> Result<bool> {
      let idx = meta.head.wrapping_add(offset as u64);
      let item_k = composer.key_for_idx(idx);
      if let Some(val) = data_ks.get(item_k)?
        && val.as_ref() == elem_bytes
      {
        rank_count += 1;
        if rank_count >= target_rank {
          matches.push(offset as i64);
          if (is_multi_count && matches.len() >= count_limit) || !is_multi_count {
            return Ok(false);
          }
        }
      }
      Ok(true)
    };

    if !reversed {
      for offset in 0..limit {
        if !check_offset(offset)? {
          break;
        }
      }
    } else {
      let start_offset = len - 1;
      let end_offset = len.saturating_sub(limit);
      for offset in (end_offset..=start_offset).rev() {
        if !check_offset(offset)? {
          break;
        }
      }
    }

    Ok(matches)
  }
}

#[inline]
pub fn prepare_list_meta_for_write<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
  batch: &mut DbBatch<E>,
) -> Result<(ListMeta, bool)>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  match get_meta_checked::<ListMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) => Ok((meta, true)),
    None => {
      let prefix = compose_list_prefix_stack(&kc, k_bytes);
      clear_prefix_in_batch(db.data(), &prefix, batch)?;
      Ok((ListMeta::new_with_version(0), false))
    }
  }
}

fn list_push_internal<E: Engine, V: AsRef<[u8]>>(
  db: &Db<E>,
  key_bytes: &[u8],
  values: &[V],
  create_if_missing: bool,
  push_left: bool,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  if values.is_empty() {
    return Ok(0);
  }
  let kc = db.kc();
  let meta_k = compose_list_meta_key(&kc, key_bytes);
  let now_ms = current_now_ms();

  let mut batch = db.batch_with_capacity(values.len() + 1);
  let (mut meta, metadata_existed) =
    prepare_list_meta_for_write(db, key_bytes, &meta_k, now_ms, &mut batch)?;

  if !create_if_missing && (!metadata_existed || meta.base.size == 0) {
    return Ok(0);
  }

  let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
  let _data_ks = db.data();
  let _meta_ks = db.meta();

  for v in values {
    let v_bytes = v.as_ref();
    let target_idx = if push_left {
      meta.head = meta.head.wrapping_sub(1);
      meta.head
    } else {
      let t = meta.tail;
      meta.tail = meta.tail.wrapping_add(1);
      t
    };
    let item_k = composer.key_for_idx(target_idx);
    batch.insert_data(item_k, v_bytes);
    meta.base.size += 1;
  }

  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;
  Ok(meta.base.size)
}

fn list_pop_internal<E: Engine>(
  db: &Db<E>,
  key_bytes: &[u8],
  count: usize,
  pop_left: bool,
) -> Result<Vec<Vec<u8>>>
where
  Error: From<E::Error>,
{
  if count == 0 {
    return Ok(Vec::new());
  }
  let kc = db.kc();
  let meta_k = compose_list_meta_key(&kc, key_bytes);
  let now_ms = current_now_ms();

  let mut meta = match get_meta_checked::<ListMeta, _>(db, key_bytes, &meta_k, now_ms)? {
    Some(m) if m.base.size > 0 => m,
    _ => return Ok(Vec::new()),
  };

  let actual_count = (count as u64).min(meta.base.size);
  let mut results = Vec::with_capacity(actual_count as usize);

  let mut batch = db.batch_with_capacity(actual_count as usize + 1);
  let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
  let data_ks = db.data();
  let _meta_ks = db.meta();

  for _ in 0..actual_count {
    let target_idx = if pop_left {
      let h = meta.head;
      meta.head = meta.head.wrapping_add(1);
      h
    } else {
      meta.tail = meta.tail.wrapping_sub(1);
      meta.tail
    };
    let item_k = composer.key_for_idx(target_idx);
    if let Some(val) = data_ks.get(item_k)? {
      results.push(val.to_vec());
      batch.rm_weak_data(item_k);
    }
  }

  meta.base.size -= actual_count;
  if meta.base.size == 0 {
    batch.rm_meta(&meta_k);
  } else {
    batch.insert_meta(&meta_k, &meta.encode());
  }
  batch.commit()?;

  Ok(results)
}
