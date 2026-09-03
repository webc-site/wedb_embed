use std::ops::Bound;

use crate::{
  api::stream::{
    StreamEntry,
    r#const::{ERR_INVALID_END_ID_INTERVAL, ERR_INVALID_START_ID_INTERVAL},
    decode_stream_entry_fields,
    r#impl::{get_stream_entry, get_stream_meta},
    key,
    meta::StreamId,
    opt::StreamRange,
    parse_stream_id_from_subkey,
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
  let meta = match get_stream_meta(db, key_bytes, now_ms)? {
    Some(m) => m,
    None => return Ok(Vec::new()),
  };

  if meta.base.size == 0 {
    return Ok(Vec::new());
  }

  // O(1) 内存级快速边界校验：若查询区间完全落在当前流的 [first_entry_id, last_entry_id] 之外，直接短路返回
  if !options.reverse {
    if options.start > meta.last_entry_id || options.end < meta.first_entry_id {
      return Ok(Vec::new());
    }
  } else if options.start < meta.first_entry_id || options.end > meta.last_entry_id {
    return Ok(Vec::new());
  }

  // 单点查询快速路径（O(1) 直接点查）
  if options.start == options.end {
    if options.exclude_start || options.exclude_end {
      return Ok(Vec::new());
    }
    return Ok(
      get_stream_entry(db, key_bytes, options.start)?
        .into_iter()
        .collect(),
    );
  }

  let kc = db.kc();
  let data_ks = db.data();
  let prefix = key::prefix_stack(&kc, key_bytes);
  let max_count = options.count.unwrap_or(usize::MAX);
  let mut results = Vec::with_capacity(max_count.min(meta.base.size as usize));

  let mut process_entry = |sid: StreamId, v: &[u8]| -> bool {
    if !options.reverse {
      if options.exclude_start && sid <= options.start {
        return true;
      }
      if (options.exclude_end && sid >= options.end) || sid > options.end {
        return false;
      }
    } else {
      if options.exclude_start && sid >= options.start {
        return true;
      }
      if (options.exclude_end && sid <= options.end) || sid < options.end {
        return false;
      }
    }
    let fields = decode_stream_entry_fields(v).unwrap_or_default();
    results.push((sid, fields));
    results.len() < max_count
  };

  let mut scan_entry = |k: &[u8], v: &[u8]| -> bool {
    if !k.starts_with(prefix.as_slice()) {
      return false;
    }
    if let Some(sid) = parse_stream_id_from_subkey(&k[prefix.len()..])
      && !process_entry(sid, v)
    {
      return false;
    }
    true
  };

  if !options.reverse {
    let start_item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);
    let end_item_k = key::item(&kc, key_bytes, options.end.ms, options.end.seq);

    for g in data_ks.range((
      Bound::Included(start_item_k.as_slice()),
      Bound::Included(end_item_k.as_slice()),
    )) {
      let entry = g?;
      if !scan_entry(entry.key(), entry.value()) {
        break;
      }
    }
  } else {
    let low_item_k = key::item(&kc, key_bytes, options.end.ms, options.end.seq);
    let high_item_k = key::item(&kc, key_bytes, options.start.ms, options.start.seq);

    for g in data_ks
      .range((
        Bound::Included(low_item_k.as_slice()),
        Bound::Included(high_item_k.as_slice()),
      ))
      .rev()
    {
      let entry = g?;
      if !scan_entry(entry.key(), entry.value()) {
        break;
      }
    }
  }

  Ok(results)
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
