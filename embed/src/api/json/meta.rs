use std::ops::{Deref, DerefMut};

use crate::{
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType},
};

/// JSON storage encoding format aligned with Apache Kvrocks JsonStorageFormat.
/// JSON 存储格式（对标 Apache Kvrocks JsonStorageFormat）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, bitcode::Encode, bitcode::Decode, strum::FromRepr,
)]
#[repr(u8)]
pub enum JsonStorageFormat {
  #[default]
  Json = 0,
  Cbor = 1,
}

/// Structure metadata.
/// JSON 结构元数据（对标 Apache Kvrocks JsonMetadata 27 字节/26 字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct JsonMeta {
  pub base: KeyMeta,
  pub format: JsonStorageFormat,
}

impl Deref for JsonMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for JsonMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl JsonMeta {
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 1; // 26 + 1 = 27
  pub const KVROCKS_ENCODED_SIZE: usize = KeyMeta::KVROCKS_SINGLE_KV_ENCODED_SIZE + 1; // 9 + 1 = 10

  #[inline]
  pub fn new(expire_at: u64, version: u64, size: u64) -> Self {
    Self {
      base: KeyMeta::new(RedisType::Json, expire_at, version, size),
      format: JsonStorageFormat::Json,
    }
  }

  #[inline]
  pub fn new_with_version(expire_at: u64, size: u64) -> Self {
    Self {
      base: KeyMeta::new_with_version(RedisType::Json, expire_at, size),
      format: JsonStorageFormat::Json,
    }
  }

  #[inline]
  pub fn with_format(format: JsonStorageFormat, expire_at: u64, version: u64, size: u64) -> Self {
    Self {
      base: KeyMeta::new(RedisType::Json, expire_at, version, size),
      format,
    }
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  /// Encodes data into binary format.
  /// 编码为标准 27 字节 wedb 元数据头（栈上定长数组，零堆分配）
  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    let base_enc = self.base.encode();
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&base_enc);
    buf[KeyMeta::ENCODED_SIZE] = self.format as u8;
    buf
  }

  /// Encodes data into binary format.
  /// 编码为 Kvrocks 10 字节紧凑元数据头（栈上定长数组，零堆分配，对标 Kvrocks JsonMetadata）
  #[inline]
  pub fn encode_kvrocks(&self) -> [u8; Self::KVROCKS_ENCODED_SIZE] {
    let mut buf = [0u8; Self::KVROCKS_ENCODED_SIZE];
    let flags =
      KeyMeta::META_64BIT_ENCODING_MASK | (self.base.rtype as u8 & KeyMeta::META_TYPE_MASK);
    buf[0] = flags;
    buf[1..9].copy_from_slice(&self.base.expire_at.to_be_bytes());
    buf[9] = self.format as u8;
    buf
  }

  /// Decodes data from binary format.
  /// 解码元数据头与载荷切片（支持 27 字节 wedb 标准头与 10 字节 Kvrocks 头）
  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<(Self, &[u8])> {
    if bytes.is_empty() {
      return None;
    }

    // 1. 标准 27 字节 wedb JsonMeta 头部 (rtype == RedisType::Json)
    if bytes.len() >= Self::ENCODED_SIZE
      && bytes[0] == RedisType::Json as u8
      && let Some(base) = KeyMeta::decode(&bytes[..KeyMeta::ENCODED_SIZE])
    {
      let format = match bytes[KeyMeta::ENCODED_SIZE] {
        1 => JsonStorageFormat::Cbor,
        _ => JsonStorageFormat::Json,
      };
      let payload = &bytes[Self::ENCODED_SIZE..];
      return Some((Self { base, format }, payload));
    }

    // 2. Kvrocks 10 字节 JsonMetadata 头部 (flags & 0x0F == 10, flags & 0x80 != 0)
    if bytes.len() >= Self::KVROCKS_ENCODED_SIZE
      && (bytes[0] & KeyMeta::META_TYPE_MASK == RedisType::Json as u8)
      && (bytes[0] & KeyMeta::META_64BIT_ENCODING_MASK != 0)
      && let Some(base) = KeyMeta::decode(&bytes[..KeyMeta::KVROCKS_SINGLE_KV_ENCODED_SIZE])
    {
      let format = match bytes[KeyMeta::KVROCKS_SINGLE_KV_ENCODED_SIZE] {
        1 => JsonStorageFormat::Cbor,
        _ => JsonStorageFormat::Json,
      };
      let payload = &bytes[Self::KVROCKS_ENCODED_SIZE..];
      return Some((Self { base, format }, payload));
    }

    None
  }
}

pub fn encode_json_value(meta: &JsonMeta, payload: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(JsonMeta::ENCODED_SIZE + payload.len());
  out.extend_from_slice(&meta.encode());
  out.extend_from_slice(payload);
  out
}

impl MetaOps for JsonMeta {
  const TAG: &[u8] = KeyTag::JsonMeta.as_slice();
  type EncodedBytes = [u8; Self::ENCODED_SIZE];

  #[inline]
  fn decode(bytes: &[u8]) -> Option<Self> {
    Self::decode(bytes).map(|(m, _)| m)
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
