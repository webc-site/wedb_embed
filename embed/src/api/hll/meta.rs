use std::ops::{Deref, DerefMut};

use crate::{
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType},
};

/// Encodes data into binary format.
/// HyperLogLog 编码模式（对标 Apache Kvrocks HyperLogLogMetadata::EncodeType）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode, Default, strum::FromRepr,
)]
#[repr(u8)]
pub enum HllEncodeType {
  #[default]
  Dense = 0,
  Sparse = 1,
}

/// Encodes data into binary format.
/// HyperLogLog 结构元数据（对标 Apache Kvrocks HyperLogLogMetadata，27 字节定长编码）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct HyperLogLogMeta {
  pub base: KeyMeta,
  pub encode_type: HllEncodeType,
}

impl Deref for HyperLogLogMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for HyperLogLogMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl HyperLogLogMeta {
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 1; // 26 + 1 = 27 字节
  pub const KVROCKS_ENCODED_SIZE: usize = KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE + 1; // 25 + 1 = 26 字节

  #[inline]
  pub fn new(expire_at: u64, version: u64) -> Self {
    Self {
      base: KeyMeta::new(RedisType::HyperLogLog, expire_at, version, 0),
      encode_type: HllEncodeType::Dense,
    }
  }

  #[inline]
  pub fn new_with_version(expire_at: u64) -> Self {
    Self {
      base: KeyMeta::new_with_version(RedisType::HyperLogLog, expire_at, 0),
      encode_type: HllEncodeType::Dense,
    }
  }

  #[inline]
  pub fn new_sparse_with_version(expire_at: u64) -> Self {
    Self {
      base: KeyMeta::new_with_version(RedisType::HyperLogLog, expire_at, 0),
      encode_type: HllEncodeType::Sparse,
    }
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&self.base.encode());
    buf[KeyMeta::ENCODED_SIZE] = self.encode_type as u8;
    buf
  }

  #[inline]
  pub fn encode_kvrocks(&self) -> Vec<u8> {
    let mut out = self.base.encode_kvrocks();
    out.push(self.encode_type as u8);
    out
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < Self::KVROCKS_ENCODED_SIZE {
      return None;
    }
    let base = KeyMeta::decode(bytes)?;
    let encode_type =
      if bytes.len() >= Self::ENCODED_SIZE && (bytes[1] == 0 || bytes[1] == 0x80) && bytes[0] <= 14
      {
        match bytes[KeyMeta::ENCODED_SIZE] {
          1 => HllEncodeType::Sparse,
          _ => HllEncodeType::Dense,
        }
      } else if bytes.len() >= Self::KVROCKS_ENCODED_SIZE {
        let offset = KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE;
        if bytes.len() > offset {
          match bytes[offset] {
            1 => HllEncodeType::Sparse,
            _ => HllEncodeType::Dense,
          }
        } else {
          HllEncodeType::Dense
        }
      } else {
        HllEncodeType::Dense
      };
    Some(Self { base, encode_type })
  }
}

impl MetaOps for HyperLogLogMeta {
  const TAG: &[u8] = KeyTag::HllMeta.as_slice();
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
