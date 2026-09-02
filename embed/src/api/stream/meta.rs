use std::{
  fmt,
  ops::{Deref, DerefMut},
  str,
};

use crate::{
  error::{Error, Result},
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType, generate_version},
};

pub const ERR_INVALID_STREAM_ID: &str = "Invalid stream ID specified as stream command argument";
pub const ERR_LAST_ENTRY_ID_REACHED: &str = "last possible entry id reached";
pub const ERR_ADD_ENTRY_ID_SMALLER: &str =
  "The ID specified in XADD is equal or smaller than the target stream top item";
pub const ERR_SEQ_OVERFLOW: &str = "Elements are too large to be stored";
pub const ERR_ENTRY_ID_OUT_OF_RANGE: &str = "The ID specified in XADD must be greater than 0-0";
pub const ERR_STREAM_EXHAUSTED_ID: &str =
  "The stream has exhausted the last possible ID, unable to add more items";

/// Encodes data into binary format.
/// 将 StreamId 大端编码至 16 字节切片
#[inline]
pub fn encode_id(buf: &mut [u8], id: StreamId) {
  if let Some(chunk) = buf.first_chunk_mut::<16>() {
    chunk[..8].copy_from_slice(&id.ms.to_be_bytes());
    chunk[8..16].copy_from_slice(&id.seq.to_be_bytes());
  }
}

#[inline]
pub fn decode_id(buf: &[u8]) -> StreamId {
  if let Some(chunk) = buf.first_chunk::<16>() {
    let ms = u64::from_be_bytes([
      chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
    ]);
    let seq = u64::from_be_bytes([
      chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14], chunk[15],
    ]);
    StreamId::new(ms, seq)
  } else {
    StreamId::min()
  }
}

/// Parses parameter or binary slice.
/// 快速解析 ms[-seq] 格式字符串
#[inline]
fn parse_id_parts(s: &str, default_seq: u64) -> Result<StreamId> {
  if let Some((ms_str, seq_str)) = s.split_once('-') {
    let ms = ms_str
      .parse::<u64>()
      .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
    let seq = seq_str
      .parse::<u64>()
      .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
    Ok(StreamId { ms, seq })
  } else {
    let ms = s
      .parse::<u64>()
      .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
    Ok(StreamId {
      ms,
      seq: default_seq,
    })
  }
}

/// Operation definition.
/// Stream 消息唯一标识 (ms-seq)
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Hash,
  bitcode::Encode,
  bitcode::Decode,
  Default,
)]
pub struct StreamId {
  pub ms: u64,
  pub seq: u64,
}

impl StreamId {
  #[inline]
  pub const fn new(ms: u64, seq: u64) -> Self {
    Self { ms, seq }
  }

  #[inline]
  pub const fn min() -> Self {
    Self { ms: 0, seq: 0 }
  }

  #[inline]
  pub const fn max() -> Self {
    Self {
      ms: u64::MAX - 1,
      seq: u64::MAX,
    }
  }

  #[inline]
  pub const fn is_min(&self) -> bool {
    self.ms == 0 && self.seq == 0
  }

  #[inline]
  pub const fn is_max(&self) -> bool {
    self.ms == u64::MAX - 1 && self.seq == u64::MAX
  }

  #[inline]
  pub fn clear(&mut self) {
    self.ms = 0;
    self.seq = 0;
  }
}

impl fmt::Display for StreamId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let ms = self.ms;
    let seq = self.seq;
    write!(f, "{ms}-{seq}")
  }
}

impl StreamId {
  /// Domain operation (aligned with Kvrocks IncrementStreamEntryID).
  /// 对标 Kvrocks IncrementStreamEntryID
  pub fn increment(&mut self) -> Result<()> {
    if self.seq == u64::MAX {
      if self.ms == u64::MAX - 1 {
        self.ms = 0;
        self.seq = 0;
        return Err(Error::invalid_data(ERR_LAST_ENTRY_ID_REACHED));
      } else {
        self.ms += 1;
        self.seq = 0;
      }
    } else {
      self.seq += 1;
    }
    Ok(())
  }

  /// Domain operation (aligned with Kvrocks ParseStreamEntryID).
  /// 对标 Kvrocks ParseStreamEntryID
  pub fn parse(s: &str) -> Result<Self> {
    let s = s.trim();
    if s == "+" {
      return Ok(Self::max());
    }
    if s == "-" {
      return Ok(Self::min());
    }
    let s = s.strip_prefix('[').unwrap_or(s);
    let s = s.strip_prefix('(').unwrap_or(s);
    parse_id_parts(s, 0)
  }

  /// Domain operation (aligned with Kvrocks ParseRangeStart).
  /// 对标 Kvrocks ParseRangeStart
  pub fn parse_range_start(s: &str) -> Result<(Self, bool)> {
    let s = s.trim();
    let (s, exclude) = if let Some(stripped) = s.strip_prefix('(') {
      (stripped, true)
    } else if let Some(stripped) = s.strip_prefix('[') {
      (stripped, false)
    } else {
      (s, false)
    };
    if s == "-" {
      return Ok((Self::min(), exclude));
    }
    if s == "+" {
      return Ok((Self::max(), exclude));
    }
    let id = parse_id_parts(s, 0)?;
    Ok((id, exclude))
  }

  /// Domain operation (aligned with Kvrocks ParseRangeEnd).
  /// 对标 Kvrocks ParseRangeEnd
  pub fn parse_range_end(s: &str) -> Result<(Self, bool)> {
    let s = s.trim();
    let (s, exclude) = if let Some(stripped) = s.strip_prefix('(') {
      (stripped, true)
    } else if let Some(stripped) = s.strip_prefix('[') {
      (stripped, false)
    } else {
      (s, false)
    };
    if s == "+" {
      return Ok((Self::max(), exclude));
    }
    if s == "-" {
      return Ok((Self::min(), exclude));
    }
    let id = parse_id_parts(s, u64::MAX)?;
    Ok((id, exclude))
  }

  #[inline]
  pub fn to_string_id(&self) -> String {
    let mut buf_ms = itoa::Buffer::new();
    let s_ms = buf_ms.format(self.ms);
    let mut buf_seq = itoa::Buffer::new();
    let s_seq = buf_seq.format(self.seq);
    let mut res = String::with_capacity(s_ms.len() + 1 + s_seq.len());
    res.push_str(s_ms);
    res.push('-');
    res.push_str(s_seq);
    res
  }
}

/// Domain operation (aligned with Apache Kvrocks NextStreamEntryIDGenerationStrategy).
/// 下一个 StreamEntryID 生成策略（对标 Apache Kvrocks NextStreamEntryIDGenerationStrategy）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum NextStreamEntryIdStrategy {
  Auto,
  CurrentTimestampSpecificSeq(u64),
  SpecificTimestampAnySeq(u64),
  FullySpecified(StreamId),
}

impl NextStreamEntryIdStrategy {
  /// Domain operation (aligned with Kvrocks ParseNextStreamEntryIDStrategy).
  /// 对标 Kvrocks ParseNextStreamEntryIDStrategy
  pub fn parse(input: &str) -> Result<Self> {
    let input = input.trim();
    if input == "*" {
      return Ok(Self::Auto);
    }
    if let Some((ms_str, seq_str)) = input.split_once('-') {
      if ms_str == "*" {
        let seq = seq_str
          .parse::<u64>()
          .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
        return Ok(Self::CurrentTimestampSpecificSeq(seq));
      }
      let ms = ms_str
        .parse::<u64>()
        .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
      if seq_str == "*" {
        return Ok(Self::SpecificTimestampAnySeq(ms));
      }
      let seq = seq_str
        .parse::<u64>()
        .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
      return Ok(Self::FullySpecified(StreamId::new(ms, seq)));
    }
    let ms = input
      .parse::<u64>()
      .map_err(|_| Error::invalid_data(ERR_INVALID_STREAM_ID))?;
    Ok(Self::FullySpecified(StreamId::new(ms, 0)))
  }

  /// Domain operation (aligned with Kvrocks GenerateID).
  /// 对标 Kvrocks GenerateID
  pub fn generate_id(&self, last_id: StreamId, now_ms: u64) -> Result<StreamId> {
    match *self {
      Self::Auto => {
        if now_ms > last_id.ms {
          Ok(StreamId::new(now_ms, 0))
        } else {
          let mut next = last_id;
          next.increment()?;
          Ok(next)
        }
      }
      Self::CurrentTimestampSpecificSeq(seq) => {
        let next = StreamId::new(now_ms, seq);
        if next <= last_id {
          return Err(Error::invalid_data(ERR_ADD_ENTRY_ID_SMALLER));
        }
        Ok(next)
      }
      Self::SpecificTimestampAnySeq(ms) => {
        if ms < last_id.ms {
          return Err(Error::invalid_data(ERR_ADD_ENTRY_ID_SMALLER));
        }
        if ms == last_id.ms {
          if last_id.seq == u64::MAX {
            return Err(Error::invalid_data(ERR_SEQ_OVERFLOW));
          }
          Ok(StreamId::new(ms, last_id.seq + 1))
        } else {
          Ok(StreamId::new(ms, 0))
        }
      }
      Self::FullySpecified(id) => {
        if last_id.ms == u64::MAX - 1 && last_id.seq == u64::MAX {
          return Err(Error::invalid_data(ERR_STREAM_EXHAUSTED_ID));
        }
        if id.ms == 0 && id.seq == 0 {
          return Err(Error::invalid_data(ERR_ENTRY_ID_OUT_OF_RANGE));
        }
        if id <= last_id {
          return Err(Error::invalid_data(ERR_ADD_ENTRY_ID_SMALLER));
        }
        Ok(id)
      }
    }
  }
}

/// Domain operation (aligned with Apache Kvrocks StreamSubkeyType).
/// 流子键类型枚举（对标 Apache Kvrocks StreamSubkeyType）
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::FromRepr)]
#[repr(u8)]
pub enum StreamSubkeyType {
  StreamEntry = 0,
  StreamConsumerGroupMetadata = 1,
  StreamConsumerMetadata = 2,
  StreamPelEntry = 3,
}

/// Structure metadata.
/// 流结构元数据（对标 Apache Kvrocks StreamMetadata 122字节全量元数据）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamMeta {
  pub base: KeyMeta,
  pub last_generated_id: StreamId,
  pub recorded_first_entry_id: StreamId,
  pub max_deleted_entry_id: StreamId,
  pub first_entry_id: StreamId,
  pub last_entry_id: StreamId,
  pub entries_added: u64,
  pub group_number: u64,
}

impl Deref for StreamMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for StreamMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl StreamMeta {
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 16 * 5 + 8 + 8; // 26 + 80 + 16 = 122

  #[inline]
  pub fn new(expire_at: u64, version: u64) -> Self {
    let ver = if version == 0 {
      generate_version()
    } else {
      version
    };
    Self {
      base: KeyMeta::new(RedisType::Stream, expire_at, ver, 0),
      last_generated_id: StreamId::min(),
      recorded_first_entry_id: StreamId::min(),
      max_deleted_entry_id: StreamId::min(),
      first_entry_id: StreamId::min(),
      last_entry_id: StreamId::min(),
      entries_added: 0,
      group_number: 0,
    }
  }

  #[inline]
  pub fn size(&self) -> u64 {
    self.base.size
  }

  #[inline]
  pub fn is_empty(&self) -> bool {
    self.base.size == 0
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&self.base.encode());
    let mut offset = KeyMeta::ENCODED_SIZE;

    encode_id(&mut buf[offset..offset + 16], self.last_generated_id);
    offset += 16;
    encode_id(&mut buf[offset..offset + 16], self.recorded_first_entry_id);
    offset += 16;
    encode_id(&mut buf[offset..offset + 16], self.max_deleted_entry_id);
    offset += 16;
    encode_id(&mut buf[offset..offset + 16], self.first_entry_id);
    offset += 16;
    encode_id(&mut buf[offset..offset + 16], self.last_entry_id);
    offset += 16;

    buf[offset..offset + 8].copy_from_slice(&self.entries_added.to_be_bytes());
    offset += 8;
    buf[offset..offset + 8].copy_from_slice(&self.group_number.to_be_bytes());

    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < KeyMeta::ENCODED_SIZE {
      return None;
    }
    let base = KeyMeta::decode(bytes)?;
    if base.rtype != RedisType::Stream {
      return None;
    }
    if bytes.len() < Self::ENCODED_SIZE {
      return Some(Self {
        base,
        last_generated_id: StreamId::min(),
        recorded_first_entry_id: StreamId::min(),
        max_deleted_entry_id: StreamId::min(),
        first_entry_id: StreamId::min(),
        last_entry_id: StreamId::min(),
        entries_added: base.size,
        group_number: 0,
      });
    }

    let mut offset = KeyMeta::ENCODED_SIZE;

    let last_generated_id = decode_id(&bytes[offset..offset + 16]);
    offset += 16;
    let recorded_first_entry_id = decode_id(&bytes[offset..offset + 16]);
    offset += 16;
    let max_deleted_entry_id = decode_id(&bytes[offset..offset + 16]);
    offset += 16;
    let first_entry_id = decode_id(&bytes[offset..offset + 16]);
    offset += 16;
    let last_entry_id = decode_id(&bytes[offset..offset + 16]);
    offset += 16;

    let entries_added = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
    offset += 8;
    let group_number = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);

    Some(Self {
      base,
      last_generated_id,
      recorded_first_entry_id,
      max_deleted_entry_id,
      first_entry_id,
      last_entry_id,
      entries_added,
      group_number,
    })
  }
}

impl MetaOps for StreamMeta {
  const TAG: &[u8] = KeyTag::StreamMeta.as_slice();
  type EncodedBytes = [u8; Self::ENCODED_SIZE];

  #[inline]
  fn decode(bytes: &[u8]) -> Option<Self> {
    Self::decode(bytes)
  }

  #[inline]
  fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  fn encode_bytes(&self) -> Self::EncodedBytes {
    self.encode()
  }

  #[inline]
  fn base(&self) -> &KeyMeta {
    &self.base
  }

  #[inline]
  fn base_mut(&mut self) -> &mut KeyMeta {
    &mut self.base
  }
}

/// Consumer group metadata (aligned with Apache Kvrocks StreamConsumerGroupMetadata 48-byte format).
/// 消费者组元数据（对标 Apache Kvrocks StreamConsumerGroupMetadata 48字节）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamConsumerGroupMeta {
  pub consumer_number: u64,
  pub pending_number: u64,
  pub last_delivered_id: StreamId,
  pub entries_read: i64,
  pub lag: u64,
}

impl StreamConsumerGroupMeta {
  pub const ENCODED_SIZE: usize = 8 + 8 + 16 + 8 + 8; // 48 bytes

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[0..8].copy_from_slice(&self.consumer_number.to_be_bytes());
    buf[8..16].copy_from_slice(&self.pending_number.to_be_bytes());
    encode_id(&mut buf[16..32], self.last_delivered_id);
    buf[32..40].copy_from_slice(&self.entries_read.to_be_bytes());
    buf[40..48].copy_from_slice(&self.lag.to_be_bytes());
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < Self::ENCODED_SIZE {
      return None;
    }
    let consumer_number = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
    let pending_number = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
    let last_delivered_id = decode_id(&bytes[16..32]);
    let entries_read = i64::from_be_bytes(bytes[32..40].try_into().ok()?);
    let lag = u64::from_be_bytes(bytes[40..48].try_into().ok()?);

    Some(Self {
      consumer_number,
      pending_number,
      last_delivered_id,
      entries_read,
      lag,
    })
  }
}

/// Consumer metadata (aligned with Apache Kvrocks StreamConsumerMetadata 24-byte format).
/// 消费者元数据（对标 Apache Kvrocks StreamConsumerMetadata 24字节）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamConsumerMeta {
  pub pending_number: u64,
  pub last_attempted_interaction_ms: u64,
  pub last_successful_interaction_ms: u64,
}

impl StreamConsumerMeta {
  pub const ENCODED_SIZE: usize = 8 + 8 + 8; // 24 bytes

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[0..8].copy_from_slice(&self.pending_number.to_be_bytes());
    buf[8..16].copy_from_slice(&self.last_attempted_interaction_ms.to_be_bytes());
    buf[16..24].copy_from_slice(&self.last_successful_interaction_ms.to_be_bytes());
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < Self::ENCODED_SIZE {
      return None;
    }
    let pending_number = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
    let last_attempted_interaction_ms = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
    let last_successful_interaction_ms = u64::from_be_bytes(bytes[16..24].try_into().ok()?);

    Some(Self {
      pending_number,
      last_attempted_interaction_ms,
      last_successful_interaction_ms,
    })
  }
}

/// Domain operation (aligned with Apache Kvrocks StreamPelEntry).
/// 消费者组 Pending 列表条目（PEL Entry，对标 Apache Kvrocks StreamPelEntry）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamPelEntry {
  pub last_delivery_time_ms: u64,
  pub last_delivery_count: u64,
  pub consumer_name: String,
}

impl StreamPelEntry {
  #[inline]
  pub fn encode(&self) -> Vec<u8> {
    let c_bytes = self.consumer_name.as_bytes();
    let mut buf = Vec::with_capacity(8 + 8 + 8 + c_bytes.len());
    buf.extend_from_slice(&self.last_delivery_time_ms.to_be_bytes());
    buf.extend_from_slice(&self.last_delivery_count.to_be_bytes());
    buf.extend_from_slice(&(c_bytes.len() as u64).to_be_bytes());
    buf.extend_from_slice(c_bytes);
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() >= 24 {
      let last_delivery_time_ms = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
      let last_delivery_count = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
      let name_len = u64::from_be_bytes(bytes[16..24].try_into().ok()?) as usize;

      if bytes.len() >= 24 + name_len {
        let consumer_name = str::from_utf8(&bytes[24..24 + name_len]).ok()?.to_string();
        return Some(Self {
          last_delivery_time_ms,
          last_delivery_count,
          consumer_name,
        });
      }
    }
    if bytes.len() >= 20 {
      let last_delivery_time_ms = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
      let last_delivery_count = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
      let name_len = u32::from_be_bytes(bytes[16..20].try_into().ok()?) as usize;

      if bytes.len() >= 20 + name_len {
        let consumer_name = str::from_utf8(&bytes[20..20 + name_len]).ok()?.to_string();
        return Some(Self {
          last_delivery_time_ms,
          last_delivery_count,
          consumer_name,
        });
      }
    }
    None
  }
}

/// Domain operation (aligned with Apache Kvrocks StreamNACK).
/// Stream NACK 待确认详情（对标 Apache Kvrocks StreamNACK）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamNack {
  pub id: StreamId,
  pub pel_entry: StreamPelEntry,
}

/// Domain operation (aligned with Apache Kvrocks StreamInfo).
/// 流信息结构体（对标 Apache Kvrocks StreamInfo）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamInfo {
  pub size: u64,
  pub entries_added: u64,
  pub last_generated_id: StreamId,
  pub max_deleted_entry_id: StreamId,
  pub recorded_first_entry_id: StreamId,
  pub first_entry: Option<(StreamId, Vec<(String, String)>)>,
  pub last_entry: Option<(StreamId, Vec<(String, String)>)>,
  pub groups: u64,
  pub entries: Vec<(StreamId, Vec<(String, String)>)>,
}

/// Returns or computes calculated value.
/// 获取 Pending 摘要统计结果（对标 Apache Kvrocks StreamGetPendingEntryResult）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamGetPendingEntryResult {
  pub pending_number: u64,
  pub first_entry_id: StreamId,
  pub last_entry_id: StreamId,
  pub consumer_infos: Vec<(String, u64)>,
}

/// Domain operation (aligned with Apache Kvrocks StreamClaimResult).
/// XCLAIM 结果（对标 Apache Kvrocks StreamClaimResult）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamClaimResult {
  pub ids: Vec<StreamId>,
  pub entries: Vec<(StreamId, Vec<(String, String)>)>,
}

/// Domain operation (aligned with Apache Kvrocks StreamAutoClaimResult).
/// XAUTOCLAIM 结果（对标 Apache Kvrocks StreamAutoClaimResult）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default)]
pub struct StreamAutoClaimResult {
  pub next_claim_id: StreamId,
  pub entries: Vec<(StreamId, Vec<(String, String)>)>,
  pub deleted_ids: Vec<StreamId>,
}

/// Domain operation (aligned with Apache Kvrocks StreamReadResult).
/// Stream 读取结果（对标 Apache Kvrocks StreamReadResult）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct StreamReadResult {
  pub name: String,
  pub entries: Vec<(StreamId, Vec<(String, String)>)>,
}
