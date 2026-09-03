use std::ops::Bound;

use crate::{
  IntoIndexRange,
  api::list::{
    ListItemKeyComposer, compose_list_item, compose_list_meta_key, compose_list_prefix_stack,
    r#const::ERR_INDEX_OUT_OF_RANGE, meta::ListMeta,
  },
  engine::{Engine, KvEntry, Partition},
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
    list_pop_one_internal(self, key.as_ref(), true)
  }

  #[inline]
  pub fn lpop<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<Vec<u8>>> {
    list_pop_internal(self, key.as_ref(), count, true)
  }

  #[inline]
  pub fn rpop_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    list_pop_one_internal(self, key.as_ref(), false)
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
      for g in self.data().range((
        Bound::Included(start_k.as_slice()),
        Bound::Included(end_k.as_slice()),
      )) {
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

    if s > e {
      let prefix = compose_list_prefix_stack(&kc, key_bytes);
      clear_prefix_in_batch(self.data(), &prefix, &mut batch)?;
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(());
    }

    let mut composer = ListItemKeyComposer::new(&kc, key_bytes);

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

  if values.len() == 1 {
    let target_idx = meta.push_index(push_left);
    let item_k = compose_list_item(&kc, key_bytes, target_idx);
    batch.insert_data(item_k.as_slice(), values[0].as_ref());
    meta.base.size += 1;
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
    return Ok(meta.base.size);
  }

  let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
  let _data_ks = db.data();
  let _meta_ks = db.meta();

  for v in values {
    let v_bytes = v.as_ref();
    let target_idx = meta.push_index(push_left);
    let item_k = composer.key_for_idx(target_idx);
    batch.insert_data(item_k, v_bytes);
    meta.base.size += 1;
  }

  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;
  Ok(meta.base.size)
}

fn list_pop_one_internal<E: Engine>(
  db: &Db<E>,
  key_bytes: &[u8],
  pop_left: bool,
) -> Result<Option<Vec<u8>>>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let meta_k = compose_list_meta_key(&kc, key_bytes);
  let now_ms = current_now_ms();

  let mut meta = match get_meta_checked::<ListMeta, _>(db, key_bytes, &meta_k, now_ms)? {
    Some(m) if m.base.size > 0 => m,
    _ => return Ok(None),
  };

  let target_idx = meta.pop_index(pop_left);

  let item_k = compose_list_item(&kc, key_bytes, target_idx);
  let data_ks = db.data();
  let val = match data_ks.get(item_k.as_slice())? {
    Some(v) => v.to_vec(),
    None => return Ok(None),
  };

  let mut batch = db.batch_with_capacity(2);
  batch.rm_weak_data(item_k.as_slice());

  meta.base.size -= 1;
  if meta.base.size == 0 {
    batch.rm_meta(&meta_k);
  } else {
    batch.insert_meta(&meta_k, &meta.encode());
  }
  batch.commit()?;

  Ok(Some(val))
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
    let target_idx = meta.pop_index(pop_left);
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
