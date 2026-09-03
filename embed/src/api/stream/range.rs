use std::ops::Bound;

use crate::{
  api::stream::{
    StreamEntry,
    decode_stream_entry_fields, key,
    meta::StreamId,
    opt::StreamRange,
    parse_stream_id_from_subkey,
    r#const::{ERR_INVALID_END_ID_INTERVAL, ERR_INVALID_START_ID_INTERVAL},
    r#impl::get_stream_meta,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  meta::current_now_ms,
  wedb::Db,
};

/// Returns stream entries within a range (XRANGE).
/// 获取指定 ID 范围内的流条目
pub fn stream_range<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  start: StreamId,
  end: StreamId,
  count: Option<usize>,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let options = StreamRange {
    start,
    end,
    count,
    reverse: false,
    exclude_start: false,
    exclude_end: false,
  };
  stream_range_with_options(db, key, options)
}

/// Returns stream entries in reverse order within a range (XREVRANGE).
/// 逆序获取指定 ID 范围内的流条目
pub fn stream_revrange<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  end: StreamId,
  start: StreamId,
  count: Option<usize>,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  let options = StreamRange {
    start: end,
    end: start,
    count,
    reverse: true,
    exclude_start: false,
    exclude_end: false,
  };
  stream_range_with_options(db, key, options)
}

/// Scans stream entries within range aligned with Apache Kvrocks Stream::Range.
/// XRANGE / XREVRANGE 配置扫描（对标 Apache Kvrocks Stream::Range）
pub fn stream_range_with_options<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  options: StreamRange,
) -> Result<Vec<StreamEntry>>
where
  Error: From<E::Error>,
{
  if options.exclude_start && options.start.is_max() {
    return Err(Error::invalid_data(ERR_INVALID_START_ID_INTERVAL));
  }
  if options.exclude_end && options.end.is_min() {
    return Err(Error::invalid_data(ERR_INVALID_END_ID_INTERVAL));
  }
  if let Some(0) = options.count {
    return Ok(Vec::new());
  }

  if (!options.reverse && options.end < options.start)
    || (options.reverse && options.start < options.end)
  {
    return Ok(Vec::new());
  }

  let key_bytes = key.as_ref();
  let now_ms = current_now_ms();
  if get_stream_meta(db, key_bytes, now_ms)?.is_none() {
    return Ok(Vec::new());
  }

  let kc = db.kc();
  let data_ks = db.data();

  // 单点查询快速路径（O(1) 直接点查）
  if options.start == options.end {
    if options.exclude_start || options.exclude_end {
      return Ok(Vec::new());
    }
    let item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    if let Some(v) = data_ks.get(&item_k)? {
      let fields = decode_stream_entry_fields(&v).unwrap_or_default();
      return Ok(vec![(options.start, fields)]);
    }
    return Ok(Vec::new());
  }

  let prefix = key::prefix_stack(&kc, key_bytes);
  let max_count = options.count.unwrap_or(usize::MAX);

  if !options.reverse {
    let start_item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    let end_item_k = key::item(&kc, key_bytes, options.end.ms, options.end.seq);
    let mut results = Vec::with_capacity(max_count.min(1024));

    for g in data_ks.range((
      Bound::Included(start_item_k.as_slice()),
      Bound::Included(end_item_k.as_slice()),
    )) {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
        if options.exclude_start && sid <= options.start {
          continue;
        }
        if options.exclude_end && sid >= options.end {
          break;
        }
        if sid > options.end {
          break;
        }

        let fields = decode_stream_entry_fields(v).unwrap_or_default();
        results.push((sid, fields));
        if results.len() >= max_count {
          break;
        }
      }
    }
    Ok(results)
  } else {
    let low_item_k = key::item(&kc, key_bytes, options.end.ms, options.end.seq);
    let high_item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    let mut results = Vec::with_capacity(max_count.min(1024));

    for g in data_ks
      .range((
        Bound::Included(low_item_k.as_slice()),
        Bound::Included(high_item_k.as_slice()),
      ))
      .rev()
    {
      let entry = g?;
      let (k, v) = (entry.key(), entry.value());
      if !k.starts_with(prefix.as_slice()) {
        break;
      }
      if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..]) {
        if options.exclude_start && sid >= options.start {
          continue;
        }
        if sid > options.start {
          continue;
        }
        if options.exclude_end && sid <= options.end {
          break;
        }
        if sid < options.end {
          break;
        }

        let fields = decode_stream_entry_fields(v).unwrap_or_default();
        results.push((sid, fields));
        if results.len() >= max_count {
          break;
        }
      }
    }
    Ok(results)
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn xrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    options: impl Into<StreamRange>,
  ) -> Result<Vec<StreamEntry>> {
    let mut opts = options.into();
    opts.reverse = false;
    stream_range_with_options(self, key, opts)
  }

  #[inline]
  pub fn xrevrange<K: AsRef<[u8]>>(
    &self,
    key: K,
    options: impl Into<StreamRange>,
  ) -> Result<Vec<StreamEntry>> {
    let mut opts = options.into();
    opts.reverse = true;
    stream_range_with_options(self, key, opts)
  }
}

