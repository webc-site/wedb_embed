use std::ops::Bound;

use crate::{
  api::list::{
    ListMeta,
    key::{
      ItemKeyComposer as ListItemKeyComposer, item as compose_list_item,
      meta as compose_list_meta_key, prefix_stack as compose_list_prefix_stack,
    },
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{clear_prefix_in_batch, get_meta_checked},
  meta::current_now_ms,
  wedb::Db,
};

/// List insertion and removal operations (LINSERT, LREM).
/// 列表元素插入与元素移除实现（对标 Redis / Kvrocks LINSERT 与 LREM）
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
    let data_ks = self.data();

    let actual_start = meta.head;
    let actual_end = meta.tail.wrapping_sub(1);
    let mut pivot_offset = None;

    if actual_start <= actual_end {
      let start_k = compose_list_item(&kc, key_bytes, actual_start);
      let end_k = compose_list_item(&kc, key_bytes, actual_end);
      for (offset, g) in data_ks
        .range((
          Bound::Included(start_k.as_slice()),
          Bound::Included(end_k.as_slice()),
        ))
        .enumerate()
      {
        let entry = g?;
        if entry.value().as_ref() == pivot_bytes {
          pivot_offset = Some(offset);
          break;
        }
      }
    } else {
      let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
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

    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
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

    let actual_start = meta.head;
    let actual_end = meta.tail.wrapping_sub(1);

    if actual_start <= actual_end {
      let start_k = compose_list_item(&kc, key_bytes, actual_start);
      let end_k = compose_list_item(&kc, key_bytes, actual_end);
      if count >= 0 {
        for (offset, g) in data_ks
          .range((
            Bound::Included(start_k.as_slice()),
            Bound::Included(end_k.as_slice()),
          ))
          .enumerate()
        {
          let entry = g?;
          if entry.value().as_ref() == elem_bytes {
            to_delete_offsets.push(offset);
            if to_delete_offsets.len() >= target_del_limit {
              break;
            }
          }
        }
      } else {
        for (i, g) in data_ks
          .range((
            Bound::Included(start_k.as_slice()),
            Bound::Included(end_k.as_slice()),
          ))
          .rev()
          .enumerate()
        {
          if i >= len {
            break;
          }
          let entry = g?;
          if entry.value().as_ref() == elem_bytes {
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

    let first_del = to_delete_offsets[0];
    let mut write_idx = first_del;
    let mut del_idx = 1usize;

    for read_idx in (first_del + 1)..len {
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
}
