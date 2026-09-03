use std::result::Result as StdResult;

use rapidhash::{HashSetExt, RapidHashSet as HashSet};

use crate::{
  api::stream::{
    r#const::{
      ERR_EMPTY_STREAM_ENTRIES_ADDED, ERR_EMPTY_STREAM_MAX_DELETED,
      ERR_SET_ID_ENTRIES_ADDED_SMALLER, ERR_SET_ID_MAX_DEL_GREATER, ERR_SET_ID_SMALLER_THAN_TOP,
    },
    r#impl::get_stream_meta,
    key,
    meta::{StreamId, StreamMeta},
    opt::{StreamTrim, StreamTrimStrategy},
    parse_stream_id_from_subkey,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::{current_now_ms, generate_version},
  wedb::{Db, DbBatch},
};

/// Internal stream trimming logic aligned with Apache Kvrocks Stream::trim.
/// 内部流裁剪逻辑（对标 Apache Kvrocks Stream::trim）
pub(crate) fn trim_stream_internal<E: Engine>(
  db: &Db<E>,
  meta: &mut StreamMeta,
  key: &[u8],
  options: StreamTrim,
  batch: &mut DbBatch<E>,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  if meta.base.size == 0 {
    return Ok(0);
  }
  if options.strategy == StreamTrimStrategy::MaxLen && meta.base.size <= options.max_len {
    return Ok(0);
  }
  if options.strategy == StreamTrimStrategy::MinId && meta.first_entry_id >= options.min_id {
    return Ok(0);
  }

  let kc = db.kc();
  let prefix = key::prefix_stack(&kc, key);
  let data_ks = db.data();
  let mut delete_cnt = 0u64;
  let mut last_deleted_id = StreamId::min();
  let mut new_first_id = StreamId::min();
  let mut found_next_first = false;

  let mut iter = data_ks.prefix(&prefix);
  while let Some(g) = iter.next() {
    let entry = g?;
    let k = entry.key();
    if !k.starts_with(prefix.as_slice()) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
      if options.strategy == StreamTrimStrategy::MaxLen
        && (meta.base.size.saturating_sub(delete_cnt)) <= options.max_len
      {
        new_first_id = sid;
        found_next_first = true;
        break;
      }
      if options.strategy == StreamTrimStrategy::MinId && sid >= options.min_id {
        new_first_id = sid;
        found_next_first = true;
        break;
      }

      delete_cnt += 1;
      last_deleted_id = sid;
      batch.rm_weak_data(k);

      if let Some(lim) = options.limit
        && delete_cnt as usize >= lim
      {
        for next_g in iter.by_ref() {
          let next_entry = next_g?;
          let nk = next_entry.key();
          if !nk.starts_with(prefix.as_slice()) {
            break;
          }
          if let Some(nsid) = parse_stream_id_from_subkey(&nk[prefix.len()..]) {
            new_first_id = nsid;
            found_next_first = true;
            break;
          }
        }
        break;
      }
    }
  }

  if delete_cnt > 0 {
    meta.base.size = meta.base.size.saturating_sub(delete_cnt);
    if last_deleted_id > meta.max_deleted_entry_id {
      meta.max_deleted_entry_id = last_deleted_id;
    }
    if meta.base.size == 0 || !found_next_first {
      meta.base.size = 0;
      meta.first_entry_id.clear();
      meta.last_entry_id.clear();
      meta.recorded_first_entry_id.clear();
    } else if found_next_first {
      meta.first_entry_id = new_first_id;
      meta.recorded_first_entry_id = new_first_id;
    }
  }

  Ok(delete_cnt)
}

/// Trims stream entries (XTRIM) aligned with Apache Kvrocks Stream::Trim.
/// XTRIM key [MAXLEN|MINID ...]（对标 Apache Kvrocks Stream::Trim）
pub fn stream_trim<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  options: StreamTrim,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(m) => m,
    None => return Ok(0),
  };

  let mut batch = db.batch();
  let delete_cnt = trim_stream_internal(db, &mut meta, key_bytes, options, &mut batch)?;
  if delete_cnt > 0 {
    let meta_k = key::meta(&db.kc(), key_bytes);
    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
  }

  Ok(delete_cnt)
}

#[inline]
fn find_stream_boundary_id<I, E, Item>(
  iter: I,
  prefix: &[u8],
  deleted_ids: &HashSet<StreamId>,
) -> Result<Option<StreamId>>
where
  I: Iterator<Item = StdResult<Item, E>>,
  Error: From<E>,
  Item: KvEntry,
{
  for g in iter {
    let entry = g?;
    let k = entry.key();
    if !k.starts_with(prefix) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..])
      && !deleted_ids.contains(&sid)
    {
      return Ok(Some(sid));
    }
  }
  Ok(None)
}

/// Deletes stream entries by ID (XDEL) aligned with Apache Kvrocks Stream::DeleteEntries.
/// XDEL key id [id ...]（对标 Apache Kvrocks Stream::DeleteEntries）
pub fn stream_del<E: Engine, K: AsRef<[u8]>>(db: &Db<E>, key: K, ids: &[StreamId]) -> Result<u64>
where
  Error: From<E::Error>,
{
  if ids.is_empty() {
    return Ok(0);
  }

  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  let meta_k = key::meta(&db.kc(), key_bytes);

  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(m) => m,
    None => return Ok(0),
  };

  if meta.base.size == 0 {
    return Ok(0);
  }

  let kc = db.kc();
  let data_ks = db.data();
  let mut batch = db.batch();
  let mut deleted_cnt = 0u64;
  let mut deleted_ids = HashSet::with_capacity(ids.len());

  for &id in ids {
    if !deleted_ids.insert(id) {
      continue;
    }
    let item_k = key::item(&kc, key_bytes, id.ms, id.seq);
    if data_ks.contains_key(&item_k)? {
      batch.rm_weak_data(&item_k);
      deleted_cnt += 1;
      meta.max_deleted_entry_id = meta.max_deleted_entry_id.max(id);
    }
  }

  if deleted_cnt > 0 {
    meta.base.size = meta.base.size.saturating_sub(deleted_cnt);
    if meta.base.size == 0 {
      meta.first_entry_id.clear();
      meta.last_entry_id.clear();
      meta.recorded_first_entry_id.clear();
    } else {
      let need_new_first = deleted_ids.contains(&meta.first_entry_id);
      let need_new_last = deleted_ids.contains(&meta.last_entry_id);

      if need_new_first || need_new_last {
        let prefix = key::prefix_stack(&kc, key_bytes);
        if need_new_first
          && let Some(sid) =
            find_stream_boundary_id(data_ks.prefix(&prefix), prefix.as_slice(), &deleted_ids)?
        {
          meta.first_entry_id = sid;
          meta.recorded_first_entry_id = sid;
        }
        if need_new_last
          && let Some(sid) = find_stream_boundary_id(
            data_ks.prefix(&prefix).rev(),
            prefix.as_slice(),
            &deleted_ids,
          )?
        {
          meta.last_entry_id = sid;
        }
      }
    }

    batch.insert_meta(&meta_k, &meta.encode());
    batch.commit()?;
  }

  Ok(deleted_cnt)
}

/// Sets stream last-id and statistics (XSETID) aligned with Apache Kvrocks Stream::SetId.
/// XSETID key last-id [ENTRIESADDED entries_added] [MAXDELETEDID max_deleted_id]（对标 Apache Kvrocks Stream::SetId）
pub fn stream_setid<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  last_generated_id: StreamId,
  entries_added: Option<u64>,
  max_deleted_id: Option<StreamId>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  if let Some(max_del) = max_deleted_id
    && last_generated_id < max_del
  {
    return Err(Error::invalid_data(ERR_SET_ID_MAX_DEL_GREATER));
  }

  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let meta_ks = db.meta();

  let now_ms = current_now_ms();
  let opt_m = get_stream_meta(db, key_bytes, now_ms)?;
  let is_empty = opt_m.is_none();

  if is_empty {
    if entries_added.is_none() || entries_added == Some(0) {
      return Err(Error::invalid_data(ERR_EMPTY_STREAM_ENTRIES_ADDED));
    }
    if max_deleted_id.is_none() || max_deleted_id == Some(StreamId::min()) {
      return Err(Error::invalid_data(ERR_EMPTY_STREAM_MAX_DELETED));
    }
  }

  let mut meta = match opt_m {
    Some(decoded) => decoded,
    None => StreamMeta::new(0, generate_version()),
  };

  if meta.base.size > 0 && last_generated_id < meta.last_generated_id {
    return Err(Error::invalid_data(ERR_SET_ID_SMALLER_THAN_TOP));
  }

  if meta.base.size > 0
    && let Some(ea) = entries_added
    && ea < meta.base.size
  {
    return Err(Error::invalid_data(ERR_SET_ID_ENTRIES_ADDED_SMALLER));
  }

  meta.last_generated_id = last_generated_id;
  if let Some(ea) = entries_added {
    meta.entries_added = ea;
  }
  if let Some(max_del) = max_deleted_id
    && !max_del.is_min()
  {
    meta.max_deleted_entry_id = max_del;
  }

  meta_ks.insert(&meta_k, &meta.encode())?;
  Ok(())
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn xtrim<K: AsRef<[u8]>>(&self, key: K, options: StreamTrim) -> Result<u64> {
    stream_trim(self, key, options)
  }

  #[inline]
  pub fn xdel<K: AsRef<[u8]>>(&self, key: K, ids: &[StreamId]) -> Result<u64> {
    stream_del(self, key, ids)
  }

  #[inline]
  pub fn xsetid<K: AsRef<[u8]>>(
    &self,
    key: K,
    last_generated_id: StreamId,
    entries_added: Option<u64>,
    max_deleted_id: Option<StreamId>,
  ) -> Result<()> {
    stream_setid(self, key, last_generated_id, entries_added, max_deleted_id)
  }
}
