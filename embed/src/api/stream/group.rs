use std::str;

use rapidhash::{HashMapExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::{
    key::clear_prefix_in_batch,
    stream::{
      StreamEntry,
      r#const::*,
      decode_stream_entry_fields,
      r#impl::{clean_stream_residue, get_stream_meta, stream_range_with_options},
      key,
      meta::{
        StreamAutoClaimResult, StreamClaimResult, StreamConsumerGroupMeta, StreamConsumerMeta,
        StreamGetPendingEntryResult, StreamId, StreamMeta, StreamNack, StreamPelEntry,
        StreamReadResult,
      },
      opt::{StreamAutoClaim, StreamClaim, StreamPending, StreamRange},
      parse_stream_id_from_subkey,
    },
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::{current_now_ms, generate_version},
  wedb::Db,
};

/// Calculates consumer group lag (aligned with Kvrocks CheckLagValid).
/// 对标 Kvrocks CheckLagValid 计算消费者组 Lag
pub fn check_lag_valid(stream_meta: &StreamMeta, group_meta: &mut StreamConsumerGroupMeta) {
  let mut valid = false;
  if stream_meta.entries_added == 0 {
    group_meta.lag = 0;
    valid = true;
  } else if group_meta.entries_read != -1
    && !stream_range_has_tombstones(stream_meta, group_meta.last_delivered_id)
  {
    group_meta.lag = stream_meta
      .entries_added
      .saturating_sub(group_meta.entries_read as u64);
    valid = true;
  } else {
    let entries_read =
      stream_estimate_distance_from_first_ever_entry(stream_meta, group_meta.last_delivered_id);
    if entries_read != -1 {
      group_meta.lag = stream_meta
        .entries_added
        .saturating_sub(entries_read as u64);
      valid = true;
    }
  }
  if !valid {
    group_meta.lag = u64::MAX;
  }
}

/// Checks whether stream range contains tombstone entries aligned with Kvrocks StreamRangeHasTombstones.
/// 对标 Kvrocks StreamRangeHasTombstones
fn stream_range_has_tombstones(meta: &StreamMeta, start_id: StreamId) -> bool {
  let end_id = StreamId::max();
  if meta.base.size == 0 || meta.max_deleted_entry_id.is_min() {
    return false;
  }
  if meta.first_entry_id > meta.max_deleted_entry_id {
    return false;
  }
  start_id <= meta.max_deleted_entry_id && meta.max_deleted_entry_id <= end_id
}

/// Estimates distance from first ever entry aligned with Kvrocks StreamEstimateDistanceFromFirstEverEntry.
/// 对标 Kvrocks StreamEstimateDistanceFromFirstEverEntry
fn stream_estimate_distance_from_first_ever_entry(meta: &StreamMeta, id: StreamId) -> i64 {
  if meta.entries_added == 0 {
    return 0;
  }
  if meta.base.size == 0 && id < meta.last_entry_id {
    return meta.entries_added as i64;
  }
  if id == meta.last_entry_id {
    return meta.entries_added as i64;
  } else if id > meta.last_entry_id {
    return -1;
  }
  if meta.max_deleted_entry_id.is_min() || meta.max_deleted_entry_id < meta.first_entry_id {
    if id < meta.first_entry_id {
      return meta.entries_added.saturating_sub(meta.base.size) as i64;
    } else if id == meta.first_entry_id {
      return (meta.entries_added.saturating_sub(meta.base.size) + 1) as i64;
    }
  }
  -1
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn xgroup_create<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    last_id: &str,
    mkstream: bool,
    entries_read: Option<i64>,
  ) -> Result<()> {
    stream_group_create(self, key, group_name, last_id, mkstream, entries_read)
  }

  #[inline]
  pub fn xgroup_destroy<K: AsRef<[u8]>>(&self, key: K, group_name: &str) -> Result<bool> {
    stream_group_destroy(self, key, group_name)
  }

  #[inline]
  pub fn xgroup_create_consumer<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
  ) -> Result<i32> {
    stream_group_create_consumer(self, key, group_name, consumer_name)
  }

  #[inline]
  pub fn xgroup_del_consumer<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
  ) -> Result<u64> {
    stream_group_del_consumer(self, key, group_name, consumer_name)
  }

  #[inline]
  pub fn xgroup_set_id<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    last_id: &str,
    entries_read: Option<i64>,
  ) -> Result<()> {
    stream_group_set_id(self, key, group_name, last_id, entries_read)
  }

  #[inline]
  pub fn xack<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    entry_ids: &[StreamId],
  ) -> Result<u64> {
    stream_ack(self, key, group_name, entry_ids)
  }

  #[inline]
  pub fn xclaim<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
    min_idle_time_ms: u64,
    entry_ids: &[StreamId],
    options: StreamClaim,
  ) -> Result<StreamClaimResult> {
    stream_claim(
      self,
      key,
      group_name,
      consumer_name,
      min_idle_time_ms,
      entry_ids,
      options,
    )
  }

  #[inline]
  pub fn xautoclaim<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
    options: StreamAutoClaim,
  ) -> Result<StreamAutoClaimResult> {
    stream_autoclaim(self, key, group_name, consumer_name, options)
  }

  #[inline]
  pub fn xpending_summary<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
  ) -> Result<StreamGetPendingEntryResult> {
    stream_pending_summary(self, key, group_name)
  }

  #[inline]
  pub fn xpending_range<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    options: StreamPending,
  ) -> Result<Vec<StreamNack>> {
    stream_pending_range(self, key, group_name, options)
  }

  #[inline]
  pub fn xread<K: AsRef<[u8]>>(
    &self,
    key: K,
    start_id: StreamId,
    count: Option<usize>,
  ) -> Result<Vec<StreamEntry>> {
    stream_read(self, key, start_id, count)
  }

  #[inline]
  pub fn xread_streams(
    &self,
    streams: &[(&str, StreamId)],
    count: Option<usize>,
  ) -> Result<Vec<StreamReadResult>> {
    stream_read_streams(self, streams, count)
  }

  #[inline]
  pub fn xreadgroup<K: AsRef<[u8]>>(
    &self,
    key: K,
    group_name: &str,
    consumer_name: &str,
    start_id_str: &str,
    count: Option<usize>,
    noack: bool,
  ) -> Result<Vec<StreamEntry>> {
    stream_readgroup(
      self,
      key,
      group_name,
      consumer_name,
      start_id_str,
      count,
      noack,
    )
  }

  #[inline]
  pub fn xreadgroup_streams(
    &self,
    group_name: &str,
    consumer_name: &str,
    streams: &[(&str, &str)],
    count: Option<usize>,
    noack: bool,
  ) -> Result<Vec<StreamReadResult>> {
    stream_readgroup_streams(self, group_name, consumer_name, streams, count, noack)
  }
}

/// Creates a stream consumer group (XGROUP CREATE) aligned with Apache Kvrocks Stream::CreateGroup.
/// XGROUP CREATE key groupname id|$ [MKSTREAM] [ENTRIESREAD entries_read]（对标 Apache Kvrocks Stream::CreateGroup）
pub fn stream_group_create<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  last_id: &str,
  mkstream: bool,
  entries_read: Option<i64>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let _meta_ks = db.meta();
  let data_ks = db.data();

  let now_ms = current_now_ms();
  let opt_m = get_stream_meta(db, key_bytes, now_ms)?;
  let mut need_clean_residue = false;

  let mut meta = match opt_m {
    Some(decoded) => decoded,
    None => {
      if !mkstream {
        return Err(Error::invalid_data(ERR_XGROUP_KEY_REQUIRE_EXIST));
      }
      need_clean_residue = true;
      StreamMeta::new(0, generate_version())
    }
  };

  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  if data_ks.contains_key(&group_k)? {
    return Err(Error::redis(ERR_GROUP_BUSY));
  }

  let last_delivered_id = if last_id == "$" {
    meta.last_entry_id
  } else {
    StreamId::parse(last_id)?
  };

  let group_meta = StreamConsumerGroupMeta {
    consumer_number: 0,
    pending_number: 0,
    last_delivered_id,
    entries_read: entries_read.unwrap_or(-1),
    lag: 0,
  };

  let mut batch = db.batch();

  if need_clean_residue {
    clean_stream_residue(db, key_bytes, &mut batch)?;
  }

  batch.insert_data(&group_k, &group_meta.encode());
  meta.group_number += 1;
  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;

  Ok(())
}

/// Destroys a stream consumer group (XGROUP DESTROY) aligned with Apache Kvrocks Stream::DestroyGroup.
/// XGROUP DESTROY key groupname（对标 Apache Kvrocks Stream::DestroyGroup）
pub fn stream_group_destroy<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
) -> Result<bool>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let meta_k = key::meta(&kc, key_bytes);
  let _meta_ks = db.meta();
  let data_ks = db.data();

  let now_ms = current_now_ms();
  let mut meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
    }
  };

  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  if !data_ks.contains_key(&group_k)? {
    return Ok(false);
  }

  let mut batch = db.batch();
  batch.rm_data(&group_k);

  let c_prefix = key::consumer_prefix(&kc, key_bytes, group_name.as_bytes());
  clear_prefix_in_batch(data_ks, &c_prefix, &mut batch)?;

  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  clear_prefix_in_batch(data_ks, &p_prefix, &mut batch)?;

  meta.group_number = meta.group_number.saturating_sub(1);
  batch.insert_meta(&meta_k, &meta.encode());
  batch.commit()?;

  Ok(true)
}

/// Creates a consumer in consumer group (XGROUP CREATECONSUMER) aligned with Kvrocks.
/// XGROUP CREATECONSUMER key groupname consumername（对标 Apache Kvrocks Stream::CreateConsumer）
pub fn stream_group_create_consumer<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
) -> Result<i32>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_KEY_GROUP_MUST_EXIST));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  if data_ks.contains_key(&consumer_k)? {
    return Ok(0);
  }

  let consumer_meta = StreamConsumerMeta {
    pending_number: 0,
    last_attempted_interaction_ms: now_ms,
    last_successful_interaction_ms: now_ms,
  };

  group_meta.consumer_number += 1;
  let mut batch = db.batch();
  batch.insert_data(&consumer_k, &consumer_meta.encode());
  batch.insert_data(&group_k, &group_meta.encode());
  batch.commit()?;

  Ok(1)
}

/// Deletes a consumer from consumer group (XGROUP DELCONSUMER) aligned with Kvrocks.
/// XGROUP DELCONSUMER key groupname consumername（对标 Apache Kvrocks Stream::DeleteConsumer）
pub fn stream_group_del_consumer<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_GROUP_MUST_EXIST));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let consumer_bytes = match data_ks.get(&consumer_k)? {
    Some(b) => b,
    None => return Ok(0),
  };
  let consumer_meta = StreamConsumerMeta::decode(&consumer_bytes).unwrap_or_default();
  let deleted_pel = consumer_meta.pending_number;

  let mut batch = db.batch();
  batch.rm_data(&consumer_k);

  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if k.starts_with(&p_prefix)
      && let Some(pel) = StreamPelEntry::decode(v)
      && pel.consumer_name == consumer_name
    {
      batch.rm_data(k);
    }
  }

  group_meta.consumer_number = group_meta.consumer_number.saturating_sub(1);
  group_meta.pending_number = group_meta.pending_number.saturating_sub(deleted_pel);
  batch.insert_data(&group_k, &group_meta.encode());
  batch.commit()?;

  Ok(deleted_pel)
}

/// Sets consumer group last delivered ID (XGROUP SETID) aligned with Kvrocks.
/// XGROUP SETID key groupname id|$ [ENTRIESREAD entries_read]（对标 Apache Kvrocks Stream::GroupSetId）
pub fn stream_group_set_id<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  last_id: &str,
  entries_read: Option<i64>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(meta) => meta,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_KEY_MUST_EXIST));
    }
  };

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::invalid_data(ERR_XGROUP_GROUP_MUST_EXIST));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let parsed_id = if last_id == "$" {
    meta.last_entry_id
  } else {
    StreamId::parse(last_id)?
  };

  group_meta.last_delivered_id = parsed_id;
  if let Some(er) = entries_read {
    group_meta.entries_read = er;
  }

  data_ks.insert(&group_k, &group_meta.encode())?;
  Ok(())
}

/// Acknowledges pending stream messages (XACK) aligned with Apache Kvrocks Stream::DeletePelEntries.
/// XACK key groupname id [id ...]（对标 Apache Kvrocks Stream::DeletePelEntries）
pub fn stream_ack<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  entry_ids: &[StreamId],
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());

  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => return Ok(0),
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let mut acknowledged = 0u64;
  let mut consumer_acks: HashMap<String, u64> = HashMap::new();
  let mut seen_ids: HashSet<StreamId> = HashSet::default();
  let mut batch = db.batch();

  for &id in entry_ids {
    if !seen_ids.insert(id) {
      continue;
    }
    let pel_k = key::pel_item(&kc, key_bytes, group_name.as_bytes(), id.ms, id.seq);
    if let Some(pel_bytes) = data_ks.get(&pel_k)? {
      if let Some(pel_entry) = StreamPelEntry::decode(&pel_bytes) {
        *consumer_acks.entry(pel_entry.consumer_name).or_insert(0) += 1;
      }
      acknowledged += 1;
      batch.rm_data(&pel_k);
    }
  }

  if acknowledged > 0 {
    group_meta.pending_number = group_meta.pending_number.saturating_sub(acknowledged);
    batch.insert_data(&group_k, &group_meta.encode());

    for (consumer_name, ack_cnt) in consumer_acks {
      let consumer_k = key::consumer_meta(
        &kc,
        key_bytes,
        group_name.as_bytes(),
        consumer_name.as_bytes(),
      );
      if let Some(c_bytes) = data_ks.get(&consumer_k)?
        && let Some(mut c_meta) = StreamConsumerMeta::decode(&c_bytes)
      {
        c_meta.pending_number = c_meta.pending_number.saturating_sub(ack_cnt);
        batch.insert_data(&consumer_k, &c_meta.encode());
      }
    }
    batch.commit()?;
  }

  Ok(acknowledged)
}

/// Changes the ownership of pending stream messages (XCLAIM).
/// 转移待处理流消息的所有权
pub fn stream_claim<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
  min_idle_time_ms: u64,
  entry_ids: &[StreamId],
  options: StreamClaim,
) -> Result<StreamClaimResult>
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
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::not_found(ERR_GROUP_NOT_FOUND));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let mut consumer_meta = match data_ks.get(&consumer_k)? {
    Some(b) => StreamConsumerMeta::decode(&b).unwrap_or_default(),
    None => {
      group_meta.consumer_number += 1;
      StreamConsumerMeta::default()
    }
  };

  consumer_meta.last_attempted_interaction_ms = now_ms;
  consumer_meta.last_successful_interaction_ms = now_ms;

  let mut result = StreamClaimResult::default();
  let mut seen_ids: HashSet<StreamId> = HashSet::default();
  let mut batch = db.batch();

  for &id in entry_ids {
    if !seen_ids.insert(id) {
      continue;
    }
    let item_k = key::item(&kc, key_bytes, id.ms, id.seq);
    let raw_item_val = data_ks.get(&item_k)?;
    if raw_item_val.is_none() {
      continue;
    }

    let pel_k = key::pel_item(&kc, key_bytes, group_name.as_bytes(), id.ms, id.seq);
    let raw_pel_val = data_ks.get(&pel_k)?;

    let is_forced = raw_pel_val.is_none() && options.force;
    let mut pel_entry = if let Some(b) = raw_pel_val {
      StreamPelEntry::decode(&b).unwrap_or(StreamPelEntry {
        last_delivery_time_ms: 0,
        last_delivery_count: 0,
        consumer_name: String::new(),
      })
    } else if is_forced {
      StreamPelEntry {
        last_delivery_time_ms: 0,
        last_delivery_count: 0,
        consumer_name: String::new(),
      }
    } else {
      continue;
    };

    if now_ms.saturating_sub(pel_entry.last_delivery_time_ms) < min_idle_time_ms {
      continue;
    }

    if is_forced {
      group_meta.pending_number += 1;
    }

    if options.just_id {
      result.ids.push(id);
    } else if let Some(val_bytes) = raw_item_val {
      let fields = decode_stream_entry_fields(&val_bytes).unwrap_or_default();
      result.entries.push((id, fields));
    }

    if !pel_entry.consumer_name.is_empty() && pel_entry.consumer_name != consumer_name {
      let orig_k = key::consumer_meta(
        &kc,
        key_bytes,
        group_name.as_bytes(),
        pel_entry.consumer_name.as_bytes(),
      );
      if let Some(orig_b) = data_ks.get(&orig_k)?
        && let Some(mut orig_c_meta) = StreamConsumerMeta::decode(&orig_b)
      {
        orig_c_meta.pending_number = orig_c_meta.pending_number.saturating_sub(1);
        batch.insert_data(&orig_k, &orig_c_meta.encode());
      }
    }

    if pel_entry.consumer_name != consumer_name {
      consumer_meta.pending_number += 1;
      pel_entry.consumer_name = consumer_name.to_string();
    }

    if options.with_time {
      pel_entry.last_delivery_time_ms = options.last_delivery_time_ms;
    } else {
      pel_entry.last_delivery_time_ms = now_ms.saturating_sub(options.idle_time_ms);
    }
    if pel_entry.last_delivery_time_ms > now_ms {
      pel_entry.last_delivery_time_ms = now_ms;
    }

    if options.with_retry_count {
      pel_entry.last_delivery_count = options.last_delivery_count;
    } else if !options.just_id {
      pel_entry.last_delivery_count += 1;
    }

    batch.insert_data(&pel_k, &pel_entry.encode());
  }

  if let Some(last_deliv) = options.last_delivered_id
    && last_deliv > group_meta.last_delivered_id
  {
    group_meta.last_delivered_id = last_deliv;
  }

  batch.insert_data(&consumer_k, &consumer_meta.encode());
  batch.insert_data(&group_k, &group_meta.encode());
  batch.commit()?;

  Ok(result)
}

/// Automatically claims pending stream messages (XAUTOCLAIM) aligned with Apache Kvrocks Stream::AutoClaim.
/// XAUTOCLAIM key group consumer min-idle-time start [COUNT count] [JUSTID]（对标 Apache Kvrocks Stream::AutoClaim）
pub fn stream_autoclaim<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
  options: StreamAutoClaim,
) -> Result<StreamAutoClaimResult>
where
  Error: From<E::Error>,
{
  if options.exclude_start && options.start_id.is_max() {
    return Err(Error::invalid_data(ERR_INVALID_START_ID_INTERVAL));
  }

  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();

  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Err(Error::not_found(ERR_STREAM_NOT_FOUND));
  }

  let kc = db.kc();
  let data_ks = db.data();
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::not_found(ERR_GROUP_NOT_FOUND));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let mut consumer_meta = match data_ks.get(&consumer_k)? {
    Some(b) => StreamConsumerMeta::decode(&b).unwrap_or_default(),
    None => {
      group_meta.consumer_number += 1;
      StreamConsumerMeta::default()
    }
  };

  consumer_meta.last_attempted_interaction_ms = now_ms;
  consumer_meta.last_successful_interaction_ms = now_ms;

  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  let mut count = options.count;
  let mut attempts = options.attempts_factors * count;

  let mut deleted_entries = Vec::new();
  let mut claimed_entries = Vec::new();
  let mut next_claim_id = StreamId::min();
  let mut has_next = false;
  let mut batch = db.batch();

  let mut claimed_from_others: HashMap<String, u64> = HashMap::new();
  let mut deleted_from_consumers: HashMap<String, u64> = HashMap::new();

  let mut iter = data_ks.prefix(&p_prefix);
  while let Some(g) = iter.next() {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&p_prefix) {
      break;
    }
    let sid = match parse_stream_id_from_subkey(&k[p_prefix.len()..]) {
      Some(s) => s,
      None => continue,
    };
    if sid < options.start_id || (options.exclude_start && sid == options.start_id) {
      continue;
    }

    if count == 0 || attempts == 0 {
      next_claim_id = sid;
      has_next = true;
      break;
    }

    attempts -= 1;

    if let Some(mut pel_entry) = StreamPelEntry::decode(v) {
      if now_ms.saturating_sub(pel_entry.last_delivery_time_ms) < options.min_idle_time_ms {
        continue;
      }

      let item_k = key::item(&kc, key_bytes, sid.ms, sid.seq);
      let raw_item_val = data_ks.get(&item_k)?;

      if raw_item_val.is_none() {
        deleted_entries.push(sid);
        batch.rm_weak_data(k);
        *deleted_from_consumers
          .entry(pel_entry.consumer_name.clone())
          .or_insert(0) += 1;
        count -= 1;
      } else {
        let fields = if options.just_id {
          Vec::new()
        } else if let Some(ref val_slice) = raw_item_val {
          decode_stream_entry_fields(val_slice).unwrap_or_default()
        } else {
          Vec::new()
        };

        claimed_entries.push((sid, fields));
        count -= 1;

        if pel_entry.consumer_name != consumer_name {
          *claimed_from_others
            .entry(pel_entry.consumer_name.clone())
            .or_insert(0) += 1;
          pel_entry.consumer_name = consumer_name.to_string();
          pel_entry.last_delivery_time_ms = now_ms;
          if !options.just_id {
            pel_entry.last_delivery_count += 1;
          }
          batch.insert_data(k, &pel_entry.encode());
        }
      }

      if count == 0 || attempts == 0 {
        for next_g in iter.by_ref() {
          let next_entry = next_g?;
          let nk = next_entry.key();
          if !nk.starts_with(&p_prefix) {
            break;
          }
          if let Some(nsid) = parse_stream_id_from_subkey(&nk[p_prefix.len()..]) {
            next_claim_id = nsid;
            has_next = true;
            break;
          }
        }
        break;
      }
    }
  }

  if !has_next {
    next_claim_id = StreamId::min();
  }

  let total_claimed: u64 = claimed_from_others.values().sum();
  let total_deleted = deleted_entries.len() as u64;

  if total_claimed > 0 || total_deleted > 0 {
    consumer_meta.pending_number += total_claimed;
    if let Some(dec) = deleted_from_consumers.get(consumer_name) {
      consumer_meta.pending_number = consumer_meta.pending_number.saturating_sub(*dec);
    }
    batch.insert_data(&consumer_k, &consumer_meta.encode());

    let mut other_decrements: HashMap<String, u64> = claimed_from_others;
    for (c, cnt) in &deleted_from_consumers {
      if c != consumer_name {
        *other_decrements.entry(c.clone()).or_insert(0) += *cnt;
      }
    }

    for (other_consumer, dec_cnt) in other_decrements {
      let other_k = key::consumer_meta(
        &kc,
        key_bytes,
        group_name.as_bytes(),
        other_consumer.as_bytes(),
      );
      if let Some(other_b) = data_ks.get(&other_k)?
        && let Some(mut other_meta) = StreamConsumerMeta::decode(&other_b)
      {
        other_meta.pending_number = other_meta.pending_number.saturating_sub(dec_cnt);
        batch.insert_data(&other_k, &other_meta.encode());
      }
    }

    if total_deleted > 0 {
      group_meta.pending_number = group_meta.pending_number.saturating_sub(total_deleted);
      batch.insert_data(&group_k, &group_meta.encode());
    }
  }

  batch.commit()?;

  Ok(StreamAutoClaimResult {
    next_claim_id,
    entries: claimed_entries,
    deleted_ids: deleted_entries,
  })
}

/// Retrieves pending entries summary (XPENDING summary) aligned with Kvrocks.
/// XPENDING summary（对标 Apache Kvrocks Stream::GetPendingEntries summary）
pub fn stream_pending_summary<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
) -> Result<StreamGetPendingEntryResult>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  let data_ks = db.data();

  let mut first_id = StreamId::max();
  let mut last_id = StreamId::min();
  let mut total_pending = 0u64;
  let mut consumer_counts: HashMap<String, u64> = HashMap::new();
  let mut consumer_order: Vec<String> = Vec::new();

  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&p_prefix) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[p_prefix.len()..])
      && let Some(pel) = StreamPelEntry::decode(v)
    {
      total_pending += 1;
      if sid < first_id {
        first_id = sid;
      }
      if sid > last_id {
        last_id = sid;
      }
      if !consumer_counts.contains_key(&pel.consumer_name) {
        consumer_order.push(pel.consumer_name.clone());
      }
      *consumer_counts.entry(pel.consumer_name).or_insert(0) += 1;
    }
  }

  if total_pending == 0 {
    return Ok(StreamGetPendingEntryResult {
      pending_number: 0,
      first_entry_id: StreamId::min(),
      last_entry_id: StreamId::min(),
      consumer_infos: Vec::new(),
    });
  }

  let consumer_infos = consumer_order
    .into_iter()
    .map(|name| {
      let cnt = consumer_counts.get(&name).copied().unwrap_or(0);
      (name, cnt)
    })
    .collect();
  Ok(StreamGetPendingEntryResult {
    pending_number: total_pending,
    first_entry_id: first_id,
    last_entry_id: last_id,
    consumer_infos,
  })
}

/// Retrieves pending entries within range (XPENDING range) aligned with Kvrocks.
/// XPENDING range（对标 Apache Kvrocks Stream::GetPendingEntries range）
pub fn stream_pending_range<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  options: StreamPending,
) -> Result<Vec<StreamNack>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let kc = db.kc();
  let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
  let data_ks = db.data();

  let max_count = options.count.unwrap_or(usize::MAX);
  let mut results = Vec::new();
  let now_ms = current_now_ms();

  for g in data_ks.prefix(&p_prefix) {
    let entry = g?;
    let (k, v) = (entry.key(), entry.value());
    if !k.starts_with(&p_prefix) {
      break;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[p_prefix.len()..]) {
      if options.exclude_end {
        if sid >= options.end_id {
          break;
        }
      } else if sid > options.end_id {
        break;
      }

      let within_start = if options.exclude_start {
        sid > options.start_id
      } else {
        sid >= options.start_id
      };
      let within_end = if options.exclude_end {
        sid < options.end_id
      } else {
        sid <= options.end_id
      };

      if within_start
        && within_end
        && let Some(pel) = StreamPelEntry::decode(v)
      {
        if options.with_time && now_ms.saturating_sub(pel.last_delivery_time_ms) < options.idle_time
        {
          continue;
        }
        if let Some(ref target_c) = options.consumer
          && &pel.consumer_name != target_c
        {
          continue;
        }
        results.push(StreamNack {
          id: sid,
          pel_entry: pel,
        });
        if results.len() >= max_count {
          break;
        }
      }
    }
  }

  Ok(results)
}

/// Reads entries from a stream starting from a given ID (XREAD).
/// 从流中读取指定 ID 之后的新条目
pub fn stream_read<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  start_id: StreamId,
  count: Option<usize>,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let options = StreamRange {
    start: start_id,
    end: StreamId::max(),
    count,
    reverse: false,
    exclude_start: true,
    exclude_end: false,
  };
  stream_range_with_options(db, key, options)
}

/// Reads entries across multiple streams (multi-key XREAD).
/// XREAD 多流联合查询
pub fn stream_read_streams<E: Engine>(
  db: &Db<E>,
  streams: &[(&str, StreamId)],
  count: Option<usize>,
) -> Result<Vec<StreamReadResult>>
where
  Error: From<E::Error>,
{
  let mut results = Vec::with_capacity(streams.len());
  for &(stream_name, start_id) in streams {
    let entries = stream_read(db, stream_name, start_id, count)?;
    if !entries.is_empty() {
      results.push(StreamReadResult {
        name: stream_name.to_string(),
        entries,
      });
    }
  }
  Ok(results)
}

/// Reads entries via consumer group (XREADGROUP) aligned with Apache Kvrocks Stream::RangeWithPending.
/// XREADGROUP GROUP group consumer [COUNT count] [NOACK] STREAMS key ID（对标 Apache Kvrocks Stream::RangeWithPending）
pub fn stream_readgroup<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  group_name: &str,
  consumer_name: &str,
  start_id_str: &str,
  count: Option<usize>,
  noack: bool,
) -> Result<Vec<StreamEntry>>
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
  let group_k = key::group_meta(&kc, key_bytes, group_name.as_bytes());
  let group_bytes = match data_ks.get(&group_k)? {
    Some(b) => b,
    None => {
      return Err(Error::not_found(ERR_GROUP_NOT_FOUND));
    }
  };
  let mut group_meta = StreamConsumerGroupMeta::decode(&group_bytes).unwrap_or_default();

  let consumer_k = key::consumer_meta(
    &kc,
    key_bytes,
    group_name.as_bytes(),
    consumer_name.as_bytes(),
  );
  let mut consumer_meta = match data_ks.get(&consumer_k)? {
    Some(b) => StreamConsumerMeta::decode(&b).unwrap_or_default(),
    None => {
      group_meta.consumer_number += 1;
      StreamConsumerMeta::default()
    }
  };

  consumer_meta.last_attempted_interaction_ms = now_ms;
  consumer_meta.last_successful_interaction_ms = now_ms;

  let max_count = count.unwrap_or(usize::MAX);
  let mut batch = db.batch();

  if start_id_str == ">" {
    let start_id = group_meta.last_delivered_id;
    let options = StreamRange {
      start: start_id,
      end: StreamId::max(),
      count: Some(max_count),
      reverse: false,
      exclude_start: true,
      exclude_end: false,
    };
    let entries = stream_range_with_options(db, key.as_ref(), options)?;
    let mut max_id = StreamId::min();

    for (sid, _) in &entries {
      if *sid > max_id {
        max_id = *sid;
      }
      if !noack {
        let pel_k = key::pel_item(&kc, key_bytes, group_name.as_bytes(), sid.ms, sid.seq);
        let pel_entry = StreamPelEntry {
          last_delivery_time_ms: now_ms,
          last_delivery_count: 1,
          consumer_name: consumer_name.to_string(),
        };
        batch.insert_data(&pel_k, &pel_entry.encode());
        group_meta.entries_read += 1;
        group_meta.pending_number += 1;
        consumer_meta.pending_number += 1;
      }
    }

    if max_id > group_meta.last_delivered_id {
      group_meta.last_delivered_id = max_id;
    }

    batch.insert_data(&group_k, &group_meta.encode());
    batch.insert_data(&consumer_k, &consumer_meta.encode());
    batch.commit()?;

    Ok(entries)
  } else {
    let start_id = StreamId::parse(start_id_str)?;
    let p_prefix = key::pel_prefix(&kc, key_bytes, group_name.as_bytes());
    let mut entries = Vec::new();

    for g in data_ks.prefix(&p_prefix) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(&p_prefix) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[p_prefix.len()..])
        && sid >= start_id
        && let Some(mut pel) = StreamPelEntry::decode(v)
        && pel.consumer_name == consumer_name
      {
        let item_k = key::item(&kc, key_bytes, sid.ms, sid.seq);
        if let Some(item_v) = data_ks.get(&item_k)? {
          let fields = decode_stream_entry_fields(&item_v).unwrap_or_default();
          entries.push((sid, fields));
          pel.last_delivery_count += 1;
          pel.last_delivery_time_ms = now_ms;
          batch.insert_data(k, &pel.encode());

          if entries.len() >= max_count {
            break;
          }
        }
      }
    }

    batch.insert_data(&group_k, &group_meta.encode());
    batch.insert_data(&consumer_k, &consumer_meta.encode());
    batch.commit()?;

    Ok(entries)
  }
}

/// Reads entries across multiple streams via consumer group (multi-key XREADGROUP).
/// XREADGROUP 多流联合消费
pub fn stream_readgroup_streams<E: Engine>(
  db: &Db<E>,
  group_name: &str,
  consumer_name: &str,
  streams: &[(&str, &str)],
  count: Option<usize>,
  noack: bool,
) -> Result<Vec<StreamReadResult>>
where
  Error: From<E::Error>,
{
  let mut results = Vec::with_capacity(streams.len());
  for &(stream_name, start_id_str) in streams {
    let entries = stream_readgroup(
      db,
      stream_name,
      group_name,
      consumer_name,
      start_id_str,
      count,
      noack,
    )?;
    if !entries.is_empty() {
      results.push(StreamReadResult {
        name: stream_name.to_string(),
        entries,
      });
    }
  }
  Ok(results)
}
