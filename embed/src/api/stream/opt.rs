use std::ops::{Bound, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

use super::meta::{NextStreamEntryIdStrategy, StreamId};

/// Stream trim strategy (aligned with Apache Kvrocks StreamTrimStrategy).
/// Stream 裁剪策略（对标 Apache Kvrocks StreamTrimStrategy）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
#[repr(u8)]
pub enum StreamTrimStrategy {
  #[default]
  None = 0,
  MaxLen = 1,
  MinId = 2,
}

/// Stream trim configuration (aligned with Apache Kvrocks StreamTrimOpt).
/// Stream 裁剪配置（对标 Apache Kvrocks StreamTrimOpt）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamTrim {
  pub strategy: StreamTrimStrategy,
  pub max_len: u64,
  pub min_id: StreamId,
  pub limit: Option<usize>,
}

/// XTRIM command options enumeration.
/// XTRIM 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XTrim {
  MaxLen(u64),
  MinId(StreamId),
  Limit(usize),
}

impl StreamTrim {
  pub fn none() -> Self {
    Self::default()
  }

  pub fn maxlen(max_len: u64) -> Self {
    Self {
      strategy: StreamTrimStrategy::MaxLen,
      max_len,
      min_id: StreamId::min(),
      limit: None,
    }
  }

  pub fn minid(min_id: StreamId) -> Self {
    Self {
      strategy: StreamTrimStrategy::MinId,
      max_len: 0,
      min_id,
      limit: None,
    }
  }

  pub fn with_limit(mut self, limit: usize) -> Self {
    self.limit = Some(limit);
    self
  }

  pub fn from_options(options: impl IntoIterator<Item = XTrim>) -> Self {
    let mut trim = Self::default();
    for opt in options {
      match opt {
        XTrim::MaxLen(len) => {
          trim.strategy = StreamTrimStrategy::MaxLen;
          trim.max_len = len;
        }
        XTrim::MinId(id) => {
          trim.strategy = StreamTrimStrategy::MinId;
          trim.min_id = id;
        }
        XTrim::Limit(l) => trim.limit = Some(l),
      }
    }
    trim
  }
}

/// XADD command options enumeration.
/// XADD 选项枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XAdd {
  Id(StreamId),
  Strategy(NextStreamEntryIdStrategy),
  Trim(StreamTrim),
  NoMkStream,
}

/// XADD command configuration options aligned with Apache Kvrocks StreamAddOpt.
/// XADD 配置选项（对标 Apache Kvrocks StreamAddOpt）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamAdd {
  pub trim_options: StreamTrim,
  pub next_id_strategy: NextStreamEntryIdStrategy,
  pub nomkstream: bool,
}

impl Default for StreamAdd {
  fn default() -> Self {
    Self {
      trim_options: StreamTrim::none(),
      next_id_strategy: NextStreamEntryIdStrategy::Auto,
      nomkstream: false,
    }
  }
}

impl StreamAdd {
  pub fn auto() -> Self {
    Self::default()
  }

  pub fn with_id(id: StreamId) -> Self {
    Self {
      trim_options: StreamTrim::none(),
      next_id_strategy: NextStreamEntryIdStrategy::FullySpecified(id),
      nomkstream: false,
    }
  }

  pub fn with_strategy(strategy: NextStreamEntryIdStrategy) -> Self {
    Self {
      trim_options: StreamTrim::none(),
      next_id_strategy: strategy,
      nomkstream: false,
    }
  }

  pub fn with_trim(mut self, trim_options: StreamTrim) -> Self {
    self.trim_options = trim_options;
    self
  }

  pub fn nomkstream(mut self, nomkstream: bool) -> Self {
    self.nomkstream = nomkstream;
    self
  }

  pub fn from_options(options: impl IntoIterator<Item = XAdd>) -> Self {
    let mut add = Self::default();
    for opt in options {
      match opt {
        XAdd::Id(id) => add.next_id_strategy = NextStreamEntryIdStrategy::FullySpecified(id),
        XAdd::Strategy(s) => add.next_id_strategy = s,
        XAdd::Trim(t) => add.trim_options = t,
        XAdd::NoMkStream => add.nomkstream = true,
      }
    }
    add
  }
}

impl From<Option<StreamId>> for StreamAdd {
  fn from(opt: Option<StreamId>) -> Self {
    match opt {
      Some(id) => Self::with_id(id),
      None => Self::auto(),
    }
  }
}

impl From<()> for StreamAdd {
  fn from(_: ()) -> Self {
    Self::auto()
  }
}

impl From<&StreamAdd> for StreamAdd {
  fn from(opt: &StreamAdd) -> Self {
    opt.clone()
  }
}

impl From<StreamId> for StreamAdd {
  fn from(id: StreamId) -> Self {
    Self::with_id(id)
  }
}

/// XRANGE command options enumeration.
/// XRANGE 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XRange {
  Count(usize),
  Rev,
  ExcludeStart,
  ExcludeEnd,
}

/// XRANGE / XREVRANGE configuration structure (aligned with Kvrocks StreamRangeOpt).
/// XRANGE / XREVRANGE 配置选项（对标 Apache Kvrocks StreamRangeOpt）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamRange {
  pub start: StreamId,
  pub end: StreamId,
  pub count: Option<usize>,
  pub reverse: bool,
  pub exclude_start: bool,
  pub exclude_end: bool,
}

impl Default for StreamRange {
  fn default() -> Self {
    Self {
      start: StreamId::min(),
      end: StreamId::max(),
      count: None,
      reverse: false,
      exclude_start: false,
      exclude_end: false,
    }
  }
}

impl From<(StreamId, StreamId)> for StreamRange {
  fn from((start, end): (StreamId, StreamId)) -> Self {
    Self::new(start, end)
  }
}

impl From<(StreamId, StreamId, Option<usize>)> for StreamRange {
  fn from((start, end, count): (StreamId, StreamId, Option<usize>)) -> Self {
    let mut opt = Self::new(start, end);
    opt.count = count;
    opt
  }
}

impl From<(StreamId, StreamId, usize)> for StreamRange {
  fn from((start, end, count): (StreamId, StreamId, usize)) -> Self {
    Self::new(start, end).with_count(count)
  }
}

impl From<Range<StreamId>> for StreamRange {
  #[inline]
  fn from(r: Range<StreamId>) -> Self {
    let mut s = Self::new(r.start, r.end);
    s.exclude_end = true;
    s
  }
}

impl From<RangeInclusive<StreamId>> for StreamRange {
  #[inline]
  fn from(r: RangeInclusive<StreamId>) -> Self {
    Self::new(*r.start(), *r.end())
  }
}

impl From<RangeFrom<StreamId>> for StreamRange {
  #[inline]
  fn from(r: RangeFrom<StreamId>) -> Self {
    Self::new(r.start, StreamId::max())
  }
}

impl From<RangeTo<StreamId>> for StreamRange {
  #[inline]
  fn from(r: RangeTo<StreamId>) -> Self {
    let mut s = Self::new(StreamId::min(), r.end);
    s.exclude_end = true;
    s
  }
}

impl From<RangeToInclusive<StreamId>> for StreamRange {
  #[inline]
  fn from(r: RangeToInclusive<StreamId>) -> Self {
    Self::new(StreamId::min(), r.end)
  }
}

impl From<RangeFull> for StreamRange {
  #[inline]
  fn from(_: RangeFull) -> Self {
    Self::default()
  }
}

impl From<(Bound<StreamId>, Bound<StreamId>)> for StreamRange {
  #[inline]
  fn from((start, end): (Bound<StreamId>, Bound<StreamId>)) -> Self {
    let (s_id, excl_s) = match start {
      Bound::Included(id) => (id, false),
      Bound::Excluded(id) => (id, true),
      Bound::Unbounded => (StreamId::min(), false),
    };
    let (e_id, excl_e) = match end {
      Bound::Included(id) => (id, false),
      Bound::Excluded(id) => (id, true),
      Bound::Unbounded => (StreamId::max(), false),
    };
    Self {
      start: s_id,
      end: e_id,
      count: None,
      reverse: false,
      exclude_start: excl_s,
      exclude_end: excl_e,
    }
  }
}

impl From<&StreamRange> for StreamRange {
  fn from(opt: &StreamRange) -> Self {
    *opt
  }
}

impl StreamRange {
  pub fn new(start: StreamId, end: StreamId) -> Self {
    Self {
      start,
      end,
      count: None,
      reverse: false,
      exclude_start: false,
      exclude_end: false,
    }
  }

  pub fn reverse(start: StreamId, end: StreamId) -> Self {
    Self {
      start,
      end,
      count: None,
      reverse: true,
      exclude_start: false,
      exclude_end: false,
    }
  }

  pub fn with_count(mut self, count: usize) -> Self {
    self.count = Some(count);
    self
  }

  pub fn exclude_start(mut self, exclude: bool) -> Self {
    self.exclude_start = exclude;
    self
  }

  pub fn exclude_end(mut self, exclude: bool) -> Self {
    self.exclude_end = exclude;
    self
  }

  pub fn from_options(
    start: StreamId,
    end: StreamId,
    options: impl IntoIterator<Item = XRange>,
  ) -> Self {
    let mut range = Self::new(start, end);
    for opt in options {
      match opt {
        XRange::Count(c) => range.count = Some(c),
        XRange::Rev => range.reverse = true,
        XRange::ExcludeStart => range.exclude_start = true,
        XRange::ExcludeEnd => range.exclude_end = true,
      }
    }
    range
  }
}

/// XLEN command options aligned with Apache Kvrocks StreamLenOpt.
/// XLEN 配置选项（对标 Apache Kvrocks StreamLenOpt）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamLen {
  pub entry_id: StreamId,
  pub with_entry_id: bool,
  pub to_first: bool,
}

/// XGROUP CREATE command configuration options aligned with Apache Kvrocks StreamXGroupCreate.
/// XGROUP CREATE 配置选项（对标 Apache Kvrocks StreamXGroupCreate）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamXGroupCreate {
  pub mkstream: bool,
  pub entries_read: Option<i64>,
  pub last_id: String,
}

impl Default for StreamXGroupCreate {
  fn default() -> Self {
    Self {
      mkstream: false,
      entries_read: None,
      last_id: "$".to_string(),
    }
  }
}

impl StreamXGroupCreate {
  pub fn new(last_id: impl Into<String>) -> Self {
    Self {
      mkstream: false,
      entries_read: None,
      last_id: last_id.into(),
    }
  }

  pub fn mkstream(mut self, mkstream: bool) -> Self {
    self.mkstream = mkstream;
    self
  }

  pub fn entries_read(mut self, entries_read: i64) -> Self {
    self.entries_read = Some(entries_read);
    self
  }
}

/// XCLAIM command configuration options aligned with Apache Kvrocks StreamClaimOpt.
/// XCLAIM 配置选项（对标 Apache Kvrocks StreamClaimOpt）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamClaim {
  pub idle_time_ms: u64,
  pub with_time: bool,
  pub last_delivery_time_ms: u64,
  pub with_retry_count: bool,
  pub last_delivery_count: u64,
  pub force: bool,
  pub just_id: bool,
  pub last_delivered_id: Option<StreamId>,
}

impl StreamClaim {
  pub fn new(idle_time_ms: u64) -> Self {
    Self {
      idle_time_ms,
      ..Default::default()
    }
  }

  pub fn with_time(mut self, last_delivery_time_ms: u64) -> Self {
    self.with_time = true;
    self.last_delivery_time_ms = last_delivery_time_ms;
    self
  }

  pub fn with_retry_count(mut self, last_delivery_count: u64) -> Self {
    self.with_retry_count = true;
    self.last_delivery_count = last_delivery_count;
    self
  }

  pub fn force(mut self, force: bool) -> Self {
    self.force = force;
    self
  }

  pub fn just_id(mut self, just_id: bool) -> Self {
    self.just_id = just_id;
    self
  }

  pub fn with_last_id(mut self, last_delivered_id: StreamId) -> Self {
    self.last_delivered_id = Some(last_delivered_id);
    self
  }
}

/// XAUTOCLAIM command configuration options aligned with Apache Kvrocks StreamAutoClaimOpt.
/// XAUTOCLAIM 配置选项（对标 Apache Kvrocks StreamAutoClaimOpt）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamAutoClaim {
  pub min_idle_time_ms: u64,
  pub start_id: StreamId,
  pub count: usize,
  pub attempts_factors: usize,
  pub just_id: bool,
  pub exclude_start: bool,
}

impl Default for StreamAutoClaim {
  fn default() -> Self {
    Self {
      min_idle_time_ms: 0,
      start_id: StreamId::min(),
      count: 100,
      attempts_factors: 10,
      just_id: false,
      exclude_start: false,
    }
  }
}

impl StreamAutoClaim {
  pub fn new(min_idle_time_ms: u64, start_id: StreamId) -> Self {
    Self {
      min_idle_time_ms,
      start_id,
      count: 100,
      attempts_factors: 10,
      just_id: false,
      exclude_start: false,
    }
  }

  pub fn count(mut self, count: usize) -> Self {
    self.count = count;
    self
  }

  pub fn just_id(mut self, just_id: bool) -> Self {
    self.just_id = just_id;
    self
  }

  pub fn exclude_start(mut self, exclude: bool) -> Self {
    self.exclude_start = exclude;
    self
  }
}

/// XPENDING command configuration options aligned with Apache Kvrocks StreamPendingOpt.
/// XPENDING 配置选项（对标 Apache Kvrocks StreamPendingOpt）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamPending {
  pub idle_time: u64,
  pub with_time: bool,
  pub start_id: StreamId,
  pub end_id: StreamId,
  pub exclude_start: bool,
  pub exclude_end: bool,
  pub count: Option<usize>,
  pub consumer: Option<String>,
}

impl Default for StreamPending {
  fn default() -> Self {
    Self {
      idle_time: 0,
      with_time: false,
      start_id: StreamId::min(),
      end_id: StreamId::max(),
      exclude_start: false,
      exclude_end: false,
      count: None,
      consumer: None,
    }
  }
}

impl StreamPending {
  pub fn summary() -> Self {
    Self::default()
  }

  pub fn range(start_id: StreamId, end_id: StreamId, count: usize) -> Self {
    Self {
      idle_time: 0,
      with_time: false,
      start_id,
      end_id,
      exclude_start: false,
      exclude_end: false,
      count: Some(count),
      consumer: None,
    }
  }

  pub fn idle(mut self, idle_time: u64) -> Self {
    self.with_time = true;
    self.idle_time = idle_time;
    self
  }

  pub fn consumer(mut self, consumer: impl Into<String>) -> Self {
    self.consumer = Some(consumer.into());
    self
  }

  pub fn exclude_start(mut self, exclude: bool) -> Self {
    self.exclude_start = exclude;
    self
  }

  pub fn exclude_end(mut self, exclude: bool) -> Self {
    self.exclude_end = exclude;
    self
  }
}

/// XREAD command options enumeration.
/// XREAD 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XRead {
  Count(usize),
  Block(u64),
  NoAck,
}

/// XGROUP CREATE command options enumeration.
/// XGROUP CREATE 选项枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XGroupCreate {
  MkStream,
  EntriesRead(i64),
}

/// XCLAIM command options enumeration.
/// XCLAIM 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XClaim {
  Idle(u64),
  Time(u64),
  RetryCount(u64),
  Force,
  JustId,
  LastId(StreamId),
}

/// XAUTOCLAIM command options enumeration.
/// XAUTOCLAIM 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XAutoClaim {
  Count(usize),
  JustId,
  ExcludeStart,
}

/// XPENDING command options enumeration.
/// XPENDING 选项枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XPending {
  Idle(u64),
  Consumer(String),
  ExcludeStart,
  ExcludeEnd,
  Count(usize),
}

/// XREAD command configuration options.
/// XREAD 配置选项
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamRead {
  pub count: Option<usize>,
  pub block: Option<u64>,
  pub noack: bool,
}
