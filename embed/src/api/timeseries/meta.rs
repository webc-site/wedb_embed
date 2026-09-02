use std::{
  ops::{Deref, DerefMut},
  str,
};

use crate::{
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType},
};

/// Encodes data into binary format.
/// 时序块压缩类型（Uncompressed 原始编码 / Compressed FastALP 列式压缩）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum ChunkType {
  #[default]
  #[strum(serialize = "UNCOMPRESSED", serialize = "RAW")]
  Uncompressed = 0,
  #[strum(
    serialize = "COMPRESSED",
    serialize = "ALP",
    serialize = "FASTALP",
    serialize = "GORILLA"
  )]
  Compressed = 1,
}

impl ChunkType {
  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }

  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Uncompressed => "UNCOMPRESSED",
      Self::Compressed => "COMPRESSED",
    }
  }
}

/// Domain operation (aligned with Apache Kvrocks TimeSeriesMetadata::DuplicatePolicy).
/// 样本重复策略（对标 Apache Kvrocks TimeSeriesMetadata::DuplicatePolicy）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum DuplicatePolicy {
  #[default]
  #[strum(serialize = "BLOCK")]
  Block = 0,
  #[strum(serialize = "FIRST")]
  First = 1,
  #[strum(serialize = "LAST")]
  Last = 2,
  #[strum(serialize = "MIN")]
  Min = 3,
  #[strum(serialize = "MAX")]
  Max = 4,
  #[strum(serialize = "SUM")]
  Sum = 5,
}

impl DuplicatePolicy {
  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }

  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Block => "BLOCK",
      Self::First => "FIRST",
      Self::Last => "LAST",
      Self::Min => "MIN",
      Self::Max => "MAX",
      Self::Sum => "SUM",
    }
  }

  /// Operation definition.
  /// 合并重复时间戳样本值（Block 策略返回 None）
  #[inline]
  pub fn merge_value(&self, old_val: f64, new_val: f64) -> Option<f64> {
    match self {
      Self::Block => None,
      Self::First => Some(old_val),
      Self::Last => Some(new_val),
      Self::Min => Some(old_val.min(new_val)),
      Self::Max => Some(old_val.max(new_val)),
      Self::Sum => Some(old_val + new_val),
    }
  }
}

/// Structure metadata.
/// 时序结构元数据（对标 Apache Kvrocks TimeSeriesMetadata）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct TimeSeriesMeta {
  pub base: KeyMeta,
  pub retention_time: u64,
  pub chunk_size: u64,
  pub chunk_type: ChunkType,
  pub duplicate_policy: DuplicatePolicy,
  pub source_key: String,
  pub total_samples: u64,
  pub first_time: u64,
  pub last_time: u64,
  pub labels: Vec<(String, String)>,
}

impl Deref for TimeSeriesMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for TimeSeriesMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

/// Operation definition.
/// 时序表元数据创建选项
#[derive(Debug, Clone, Default)]
pub struct TimeSeriesMetaArgs {
  pub retention_time: u64,
  pub chunk_size: u64,
  pub chunk_type: ChunkType,
  pub duplicate_policy: DuplicatePolicy,
  pub source_key: String,
  pub labels: Vec<(String, String)>,
  pub expire_at: u64,
  pub version: u64,
}

impl TimeSeriesMeta {
  pub const DEFAULT_CHUNK_SIZE: u64 = 4096;

  #[inline]
  pub fn new(
    retention_time: u64,
    chunk_size: u64,
    duplicate_policy: DuplicatePolicy,
    labels: Vec<(String, String)>,
  ) -> Self {
    Self::with_options(TimeSeriesMetaArgs {
      retention_time,
      chunk_size,
      chunk_type: ChunkType::Uncompressed,
      duplicate_policy,
      source_key: String::new(),
      labels,
      expire_at: 0,
      version: 0,
    })
  }

  #[inline]
  pub fn with_expire_and_version(
    retention_time: u64,
    chunk_size: u64,
    duplicate_policy: DuplicatePolicy,
    labels: Vec<(String, String)>,
    expire_at: u64,
    version: u64,
  ) -> Self {
    Self::with_options(TimeSeriesMetaArgs {
      retention_time,
      chunk_size,
      chunk_type: ChunkType::Uncompressed,
      duplicate_policy,
      source_key: String::new(),
      labels,
      expire_at,
      version,
    })
  }

  #[inline]
  pub fn with_options(opts: TimeSeriesMetaArgs) -> Self {
    Self {
      base: KeyMeta::new(RedisType::TimeSeries, opts.expire_at, opts.version, 0),
      retention_time: opts.retention_time,
      chunk_size: if opts.chunk_size == 0 {
        Self::DEFAULT_CHUNK_SIZE
      } else {
        opts.chunk_size
      },
      chunk_type: opts.chunk_type,
      duplicate_policy: opts.duplicate_policy,
      source_key: opts.source_key,
      total_samples: 0,
      first_time: 0,
      last_time: 0,
      labels: opts.labels,
    }
  }
}

impl Default for TimeSeriesMeta {
  fn default() -> Self {
    Self::with_options(TimeSeriesMetaArgs::default())
  }
}

impl TimeSeriesMeta {
  /// Encodes data into binary format.
  /// 编码为二进制字节（与 Kvrocks TimeSeriesMetadata 1:1 对标）
  #[inline]
  pub fn encode(&self) -> Vec<u8> {
    let labels_len: usize = self.labels.iter().map(|(k, v)| 8 + k.len() + v.len()).sum();
    let cap = KeyMeta::ENCODED_SIZE
      + 8
      + 8
      + 1
      + 1
      + 4
      + self.source_key.len()
      + 8
      + 8
      + 8
      + 4
      + labels_len;
    let mut buf = Vec::with_capacity(cap);

    buf.extend_from_slice(&self.base.encode()); // 26 bytes
    buf.extend_from_slice(&self.retention_time.to_be_bytes()); // 8 bytes
    buf.extend_from_slice(&self.chunk_size.to_be_bytes()); // 8 bytes
    buf.push(self.chunk_type as u8); // 1 byte
    buf.push(self.duplicate_policy as u8); // 1 byte

    let src_bytes = self.source_key.as_bytes();
    buf.extend_from_slice(&(src_bytes.len() as u32).to_be_bytes()); // 4 bytes
    buf.extend_from_slice(src_bytes);

    buf.extend_from_slice(&self.total_samples.to_be_bytes()); // 8 bytes
    buf.extend_from_slice(&self.first_time.to_be_bytes()); // 8 bytes
    buf.extend_from_slice(&self.last_time.to_be_bytes()); // 8 bytes

    buf.extend_from_slice(&(self.labels.len() as u32).to_be_bytes()); // 4 bytes
    for (k, v) in &self.labels {
      buf.extend_from_slice(&(k.len() as u32).to_be_bytes());
      buf.extend_from_slice(k.as_bytes());
      buf.extend_from_slice(&(v.len() as u32).to_be_bytes());
      buf.extend_from_slice(v.as_bytes());
    }
    buf
  }

  /// Decodes data from binary format.
  /// 解码二进制字节
  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < KeyMeta::ENCODED_SIZE + 8 + 8 + 1 {
      return None;
    }
    let base = KeyMeta::decode(&bytes[..KeyMeta::ENCODED_SIZE])?;
    let mut offset = KeyMeta::ENCODED_SIZE;

    let retention_time = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
    offset += 8;

    let chunk_size = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
    offset += 8;

    let (chunk_type, duplicate_policy) = if offset + 2 <= bytes.len() {
      let chunk_type = ChunkType::from_repr(bytes[offset]).unwrap_or(ChunkType::Uncompressed);
      let duplicate_policy =
        DuplicatePolicy::from_repr(bytes[offset + 1]).unwrap_or(DuplicatePolicy::Block);
      offset += 2;
      (chunk_type, duplicate_policy)
    } else {
      let duplicate_policy =
        DuplicatePolicy::from_repr(bytes[offset]).unwrap_or(DuplicatePolicy::Block);
      offset += 1;
      (ChunkType::Uncompressed, duplicate_policy)
    };

    let mut source_key = String::new();
    if offset + 4 <= bytes.len() {
      let src_len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
      offset += 4;
      if offset + src_len <= bytes.len() {
        if src_len > 0 {
          source_key = str::from_utf8(&bytes[offset..offset + src_len])
            .ok()?
            .to_owned();
          offset += src_len;
        }
      } else {
        return None;
      }
    }

    let mut total_samples = 0u64;
    let mut first_time = 0u64;
    let mut last_time = 0u64;

    if offset + 8 <= bytes.len() {
      total_samples = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
      offset += 8;
    }

    if offset + 8 <= bytes.len() {
      first_time = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
      offset += 8;
    }

    if offset + 8 <= bytes.len() {
      last_time = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
      offset += 8;
    }

    let mut labels = Vec::new();
    if offset + 4 <= bytes.len() {
      let label_count = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
      offset += 4;

      labels.reserve(label_count);
      for _ in 0..label_count {
        if offset + 4 > bytes.len() {
          break;
        }
        let klen = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;

        if offset + klen > bytes.len() {
          break;
        }
        let k = str::from_utf8(&bytes[offset..offset + klen])
          .ok()?
          .to_owned();
        offset += klen;

        if offset + 4 > bytes.len() {
          break;
        }
        let vlen = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;

        if offset + vlen > bytes.len() {
          break;
        }
        let v = str::from_utf8(&bytes[offset..offset + vlen])
          .ok()?
          .to_owned();
        offset += vlen;

        labels.push((k, v));
      }
    }

    Some(Self {
      base,
      retention_time,
      chunk_size,
      chunk_type,
      duplicate_policy,
      source_key,
      total_samples,
      first_time,
      last_time,
      labels,
    })
  }
}

impl MetaOps for TimeSeriesMeta {
  const TAG: &[u8] = KeyTag::TimeSeriesMeta.as_slice();
  type EncodedBytes = Vec<u8>;

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
