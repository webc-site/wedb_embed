use crate::{
  api::stream::{
    r#const::*,
    group::check_lag_valid,
    r#impl::{get_stream_entry, get_stream_meta},
    key,
    meta::{StreamConsumerGroupMeta, StreamConsumerMeta, StreamId, StreamInfo},
    range::stream_range,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn xinfo_stream<K: AsRef<[u8]>>(
    &self,
    key: K,
    full: bool,
    count: Option<usize>,
  ) -> Result<StreamInfo> {
    stream_info_stream(self, key, full, count)
  }

  #[inline]
  pub fn xinfo_groups<K: AsRef<[u8]>>(
    &self,
    key: K,
  ) -> Result<Vec<(String, StreamConsumerGroupMeta)>> {
    stream_info_groups(self, key)
  }

  #[inline]
  pub fn xinfo_consumers<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
  ) -> Result<Vec<(String, StreamConsumerMeta)>> {
    stream_info_consumers(self, key, group_name)
  }
}

/// Returns stream information (XINFO STREAM) aligned with Apache Kvrocks Stream::GetStreamInfo.
/// XINFO STREAM key [FULL [COUNT count]]（对标 Apache Kvrocks Stream::GetStreamInfo）
pub fn stream_info_stream<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  full: bool,
  count: Option<usize>,
) -> Result<StreamInfo>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
    }
  };

  let mut first_entry = None;
  let mut last_entry = None;

  if meta.base.size > 0 && !meta.first_entry_id.is_min() {
    first_entry = get_stream_entry(db, key_bytes, meta.first_entry_id)?;
  }

  if meta.base.size > 0 && !meta.last_entry_id.is_min() {
    last_entry = get_stream_entry(db, key_bytes, meta.last_entry_id)?;
  }

  let entries = if full {
    let count_limit = match count {
      Some(0) | None => None,
      Some(c) => Some(c),
    };
    stream_range(
      db,
      key.as_ref(),
      StreamId::min(),
      StreamId::max(),
      count_limit,
    )?
  } else {
    Vec::new()
  };

  Ok(StreamInfo {
    size: meta.base.size,
    entries_added: meta.entries_added,
    last_generated_id: meta.last_generated_id,
    max_deleted_entry_id: meta.max_deleted_entry_id,
    recorded_first_entry_id: meta.recorded_first_entry_id,
    first_entry,
    last_entry,
    groups: meta.group_number,
    entries,
  })
}

/// Returns consumer groups information (XINFO GROUPS) aligned with Apache Kvrocks Stream::GetGroupInfo.
/// XINFO GROUPS key（对标 Apache Kvrocks Stream::GetGroupInfo）
pub fn stream_info_groups<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
) -> Result<Vec<(String, StreamConsumerGroupMeta)>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
    }
  };

  let kc = db.kc();
  let data_ks = db.data();
  let g_prefix = key::group_prefix_stack(&kc, key_bytes);
  let mut groups = Vec::new();

  for g in data_ks.prefix(&g_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(g_prefix.as_slice()) {
      break;
    }
    let group_name = str::from_utf8(&k[g_prefix.len()..])
      .unwrap_or("")
      .to_string();
    if let Some(mut g_meta) = StreamConsumerGroupMeta::decode(v) {
      check_lag_valid(&meta, &mut g_meta);
      groups.push((group_name, g_meta));
    }
  }

  Ok(groups)
}

/// Returns consumer information (XINFO CONSUMERS) aligned with Apache Kvrocks Stream::GetConsumerInfo.
/// XINFO CONSUMERS key groupname（对标 Apache Kvrocks Stream::GetConsumerInfo）
pub fn stream_info_consumers<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
) -> Result<Vec<(String, StreamConsumerMeta)>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let c_prefix = key::consumer_prefix(&kc, key_bytes, group_name.as_bytes());
  let mut consumers = Vec::new();
  for g in data_ks.prefix(&c_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&c_prefix) {
      break;
    }
    let consumer_name = str::from_utf8(&k[c_prefix.len()..])
      .unwrap_or("")
      .to_string();
    if let Some(c_meta) = StreamConsumerMeta::decode(v) {
      consumers.push((consumer_name, c_meta));
    }
  }

  Ok(consumers)
}
