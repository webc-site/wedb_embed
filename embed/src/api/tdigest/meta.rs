use std::ops::{Deref, DerefMut};

pub use crate::meta::{
  decode_sortable_f64_u64 as decode_double_from_u64,
  encode_sortable_f64_u64 as encode_double_to_u64,
};
use crate::{
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType},
};

/// T-Digest structure metadata (aligned with Kvrocks TDigestMetadata 98-byte binary format).
/// T-Digest 结构元数据（对标 Apache Kvrocks TDigestMetadata，98 字节定长二进制编码）
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct TDigestMeta {
  pub base: KeyMeta,
  pub compression: u32,
  pub capacity: u32,
  pub unmerged_nodes: u64,
  pub merged_nodes: u64,
  pub total_weight: u64,
  pub merged_weight: u64,
  pub minimum: f64,
  pub maximum: f64,
  pub total_observations: u64,
  pub merge_times: u64,
}

impl Deref for TDigestMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for TDigestMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl TDigestMeta {
  /// Operation definition.
  /// 26 字节 KeyMeta + 8 字节 (compression+capacity) + 32 字节 (4*u64) + 16 字节 (2*f64) + 16 字节 (2*u64) = 98 字节
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 72; // 98
  pub const KVROCKS_ENCODED_SIZE: usize = KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE + 72; // 97

  #[inline]
  pub fn new(compression: u32, expire_at: u64, version: u64) -> Self {
    let comp = if compression == 0 {
      super::r#const::DEFAULT_COMPRESSION
    } else {
      compression
    };
    let capacity = super::r#const::calculate_capacity(comp) as u32;
    Self {
      base: KeyMeta::new(RedisType::TDigest, expire_at, version, 0),
      compression: comp,
      capacity,
      unmerged_nodes: 0,
      merged_nodes: 0,
      total_weight: 0,
      merged_weight: 0,
      minimum: f64::MAX,
      maximum: -f64::MAX,
      total_observations: 0,
      merge_times: 0,
    }
  }

  /// Resets metadata statistics (aligned with Kvrocks TDigest::Reset).
  /// 重置元数据统计（对标 Apache Kvrocks TDigest::Reset）
  #[inline]
  pub fn reset(&mut self) {
    self.unmerged_nodes = 0;
    self.merged_nodes = 0;
    self.total_weight = 0;
    self.merged_weight = 0;
    self.minimum = f64::MAX;
    self.maximum = -f64::MAX;
    self.total_observations = 0;
    self.merge_times = 0;
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  pub fn total_nodes(&self) -> u64 {
    self.merged_nodes + self.unmerged_nodes
  }

  #[inline]
  pub fn delta(&self) -> f64 {
    1.0 / (self.compression as f64)
  }

  /// Zero-copy fixed-size stack array serialization.
  /// 零拷贝栈数组定长序列化
  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&self.base.encode());
    let mut offset = KeyMeta::ENCODED_SIZE;

    buf[offset..offset + 4].copy_from_slice(&self.compression.to_be_bytes());
    offset += 4;
    buf[offset..offset + 4].copy_from_slice(&self.capacity.to_be_bytes());
    offset += 4;

    buf[offset..offset + 8].copy_from_slice(&self.unmerged_nodes.to_be_bytes());
    offset += 8;
    buf[offset..offset + 8].copy_from_slice(&self.merged_nodes.to_be_bytes());
    offset += 8;
    buf[offset..offset + 8].copy_from_slice(&self.total_weight.to_be_bytes());
    offset += 8;
    buf[offset..offset + 8].copy_from_slice(&self.merged_weight.to_be_bytes());
    offset += 8;

    buf[offset..offset + 8].copy_from_slice(&encode_double_to_u64(self.minimum).to_be_bytes());
    offset += 8;
    buf[offset..offset + 8].copy_from_slice(&encode_double_to_u64(self.maximum).to_be_bytes());
    offset += 8;

    buf[offset..offset + 8].copy_from_slice(&self.total_observations.to_be_bytes());
    offset += 8;
    buf[offset..offset + 8].copy_from_slice(&self.merge_times.to_be_bytes());

    buf
  }

  /// Compatible with Apache Kvrocks binary format.
  /// 兼容 Apache Kvrocks 二进制格式
  #[inline]
  pub fn encode_kvrocks(&self) -> Vec<u8> {
    let mut out = self.base.encode_kvrocks();
    out.extend_from_slice(&self.compression.to_be_bytes());
    out.extend_from_slice(&self.capacity.to_be_bytes());
    out.extend_from_slice(&self.unmerged_nodes.to_be_bytes());
    out.extend_from_slice(&self.merged_nodes.to_be_bytes());
    out.extend_from_slice(&self.total_weight.to_be_bytes());
    out.extend_from_slice(&self.merged_weight.to_be_bytes());
    out.extend_from_slice(&encode_double_to_u64(self.minimum).to_be_bytes());
    out.extend_from_slice(&encode_double_to_u64(self.maximum).to_be_bytes());
    out.extend_from_slice(&self.total_observations.to_be_bytes());
    out.extend_from_slice(&self.merge_times.to_be_bytes());
    out
  }

  /// Unified deserialization supporting 98-byte native and 97-byte Kvrocks formats.
  /// 统一反序列化（支持 98 字节本引擎格式与 97 字节 Kvrocks 格式）
  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < Self::KVROCKS_ENCODED_SIZE {
      return None;
    }
    let (base, mut offset) =
      if bytes.len() >= Self::ENCODED_SIZE && (bytes[1] == 0 || bytes[1] == 0x80) && bytes[0] <= 14
      {
        (
          KeyMeta::decode(&bytes[..KeyMeta::ENCODED_SIZE])?,
          KeyMeta::ENCODED_SIZE,
        )
      } else if bytes.len() >= Self::KVROCKS_ENCODED_SIZE {
        (
          KeyMeta::decode(&bytes[..KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE])?,
          KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE,
        )
      } else {
        return None;
      };

    if bytes.len() < offset + 72 {
      return None;
    }

    let compression = u32::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
    ]);
    offset += 4;

    let capacity = u32::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
    ]);
    offset += 4;

    let unmerged_nodes = u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]);
    offset += 8;

    let merged_nodes = u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]);
    offset += 8;

    let total_weight = u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]);
    offset += 8;

    let merged_weight = u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]);
    offset += 8;

    let minimum = decode_double_from_u64(u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]));
    offset += 8;

    let maximum = decode_double_from_u64(u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]));
    offset += 8;

    let total_observations = u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]);
    offset += 8;

    let merge_times = u64::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
      bytes[offset + 4],
      bytes[offset + 5],
      bytes[offset + 6],
      bytes[offset + 7],
    ]);

    Some(Self {
      base,
      compression,
      capacity,
      unmerged_nodes,
      merged_nodes,
      total_weight,
      merged_weight,
      minimum,
      maximum,
      total_observations,
      merge_times,
    })
  }
}

impl MetaOps for TDigestMeta {
  const TAG: &[u8] = KeyTag::TDigestMeta.as_slice();
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
