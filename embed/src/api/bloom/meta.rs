use std::ops::{Deref, DerefMut};

use crate::{
  error::{Error, Result},
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType, generate_version},
};

/// Bloom filter chain metadata (aligned with Apache Kvrocks BloomChainMetadata 46-byte format).
/// 布隆过滤器链元数据（对标 Apache Kvrocks BloomChainMetadata 46字节）
#[derive(Debug, Clone, Copy, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct BloomChainMeta {
  pub base: KeyMeta,
  pub n_filters: u16,
  pub expansion: u16,
  pub base_capacity: u32,
  pub error_rate: f64,
  pub bloom_bytes: u32,
}

impl Deref for BloomChainMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for BloomChainMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

/// Computes cumulative capacity across multi-layer geometric filter chains (Bloom / Cuckoo).
/// 计算等比数列多层 Filter 链的总容量（支持 Bloom / Cuckoo 链）
#[inline]
pub fn calculate_geometric_capacity(base_capacity: u64, expansion: u16, n_filters: u16) -> u64 {
  if expansion == 0 || n_filters <= 1 {
    return base_capacity;
  }
  if expansion == 1 {
    return base_capacity.saturating_mul(n_filters as u64);
  }
  let r = expansion as u64;
  let n = n_filters as u32;
  if let Some(r_pow_n) = r.checked_pow(n) {
    (base_capacity as u128)
      .saturating_mul((r_pow_n.saturating_sub(1)) as u128)
      .checked_div((r - 1) as u128)
      .map(|v| v.min(u64::MAX as u128) as u64)
      .unwrap_or(u64::MAX)
  } else {
    let mut total = 0u64;
    let mut cur = base_capacity;
    for _ in 0..n_filters {
      total = total.saturating_add(cur);
      cur = cur.saturating_mul(r);
      if total == u64::MAX {
        return u64::MAX;
      }
    }
    total
  }
}

impl BloomChainMeta {
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 2 + 2 + 4 + 8 + 4; // 26 + 20 = 46

  #[inline]
  pub fn new(
    base_capacity: u32,
    error_rate: f64,
    expansion: u16,
    version: u64,
    expire_at: u64,
    bloom_bytes: u32,
  ) -> Self {
    let ver = if version == 0 {
      generate_version()
    } else {
      version
    };
    Self {
      base: KeyMeta::new(RedisType::Bloom, expire_at, ver, 0),
      n_filters: 1,
      expansion,
      base_capacity,
      error_rate,
      bloom_bytes,
    }
  }

  #[inline]
  pub const fn is_scaling(&self) -> bool {
    self.expansion != 0
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  pub fn get_capacity(&self) -> u32 {
    calculate_geometric_capacity(self.base_capacity as u64, self.expansion, self.n_filters)
      .min(u32::MAX as u64) as u32
  }

  #[inline]
  pub fn sub_filter_capacity(&self, filter_index: u16) -> u32 {
    if self.expansion <= 1 || filter_index == 0 {
      self.base_capacity
    } else {
      (self.base_capacity as u64)
        .saturating_mul((self.expansion as u64).saturating_pow(filter_index as u32))
        .min(u32::MAX as u64) as u32
    }
  }

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&self.base.encode());
    let mut offset = KeyMeta::ENCODED_SIZE;
    buf[offset..offset + 2].copy_from_slice(&self.n_filters.to_be_bytes());
    offset += 2;
    buf[offset..offset + 2].copy_from_slice(&self.expansion.to_be_bytes());
    offset += 2;
    buf[offset..offset + 4].copy_from_slice(&self.base_capacity.to_be_bytes());
    offset += 4;
    buf[offset..offset + 8].copy_from_slice(&self.error_rate.to_be_bytes());
    offset += 8;
    buf[offset..offset + 4].copy_from_slice(&self.bloom_bytes.to_be_bytes());
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < Self::ENCODED_SIZE {
      return None;
    }
    let base = KeyMeta::decode(&bytes[..KeyMeta::ENCODED_SIZE])?;
    let mut offset = KeyMeta::ENCODED_SIZE;

    let n_filters = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    offset += 2;

    let expansion = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    offset += 2;

    let base_capacity = u32::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
    ]);
    offset += 4;

    let error_rate = f64::from_be_bytes([
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

    let bloom_bytes = u32::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
    ]);

    Some(Self {
      base,
      n_filters,
      expansion,
      base_capacity,
      error_rate,
      bloom_bytes,
    })
  }
}

/// Cuckoo filter chain metadata (aligned with Apache Kvrocks CuckooChainMetadata 53-byte format).
/// 布谷鸟过滤器链元数据（对标 Apache Kvrocks CuckooChainMetadata 53字节）
#[derive(Debug, Clone, Copy, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct CuckooChainMeta {
  pub base: KeyMeta,
  pub n_filters: u16,
  pub expansion: u16,
  pub base_capacity: u64,
  pub bucket_size: u8,
  pub max_iterations: u16,
  pub num_deleted_items: u64,
  pub page_size: u32,
}

impl Deref for CuckooChainMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for CuckooChainMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl CuckooChainMeta {
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 2 + 2 + 8 + 1 + 2 + 8 + 4; // 26 + 27 = 53

  #[inline]
  pub fn new(
    base_capacity: u64,
    bucket_size: u8,
    max_iterations: u16,
    expansion: u16,
    page_size: u32,
    version: u64,
    expire_at: u64,
  ) -> Self {
    let ver = if version == 0 {
      generate_version()
    } else {
      version
    };
    let normalized_expansion = super::CuckooFilterHelper::normalize_expansion(expansion);
    Self {
      base: KeyMeta::new(RedisType::CuckooFilter, expire_at, ver, 0),
      n_filters: 1,
      expansion: normalized_expansion,
      base_capacity,
      bucket_size,
      max_iterations,
      num_deleted_items: 0,
      page_size,
    }
  }

  #[inline]
  pub const fn is_scaling(&self) -> bool {
    self.expansion > 0
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  pub fn get_total_capacity(&self) -> u64 {
    calculate_geometric_capacity(self.base_capacity, self.expansion, self.n_filters)
  }

  #[inline]
  pub fn sub_filter_capacity(&self, filter_index: u16) -> Option<u64> {
    if self.expansion == 0 {
      return if filter_index == 0 {
        Some(self.base_capacity)
      } else {
        None
      };
    }
    if self.expansion == 1 || filter_index == 0 {
      return Some(self.base_capacity);
    }
    (self.expansion as u64)
      .checked_pow(filter_index as u32)
      .and_then(|exp| self.base_capacity.checked_mul(exp))
  }

  #[inline]
  pub fn sub_filter_num_buckets(&self, filter_index: u16) -> Result<u32> {
    let cap = self
      .sub_filter_capacity(filter_index)
      .ok_or_else(|| Error::invalid_data("filter capacity overflow"))?;
    super::CuckooFilterHelper::calculate_required_buckets(cap, self.bucket_size)
  }

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&self.base.encode());
    let mut offset = KeyMeta::ENCODED_SIZE;

    buf[offset..offset + 2].copy_from_slice(&self.n_filters.to_be_bytes());
    offset += 2;
    buf[offset..offset + 2].copy_from_slice(&self.expansion.to_be_bytes());
    offset += 2;
    buf[offset..offset + 8].copy_from_slice(&self.base_capacity.to_be_bytes());
    offset += 8;
    buf[offset] = self.bucket_size;
    offset += 1;
    buf[offset..offset + 2].copy_from_slice(&self.max_iterations.to_be_bytes());
    offset += 2;
    buf[offset..offset + 8].copy_from_slice(&self.num_deleted_items.to_be_bytes());
    offset += 8;
    buf[offset..offset + 4].copy_from_slice(&self.page_size.to_be_bytes());
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < Self::ENCODED_SIZE {
      return None;
    }
    let base = KeyMeta::decode(&bytes[..KeyMeta::ENCODED_SIZE])?;
    let mut offset = KeyMeta::ENCODED_SIZE;

    let n_filters = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    offset += 2;

    let expansion = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    offset += 2;

    let base_capacity = u64::from_be_bytes([
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

    let bucket_size = bytes[offset];
    offset += 1;

    let max_iterations = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    offset += 2;

    let num_deleted_items = u64::from_be_bytes([
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

    let page_size = u32::from_be_bytes([
      bytes[offset],
      bytes[offset + 1],
      bytes[offset + 2],
      bytes[offset + 3],
    ]);

    Some(Self {
      base,
      n_filters,
      expansion,
      base_capacity,
      bucket_size,
      max_iterations,
      num_deleted_items,
      page_size,
    })
  }
}

impl MetaOps for BloomChainMeta {
  const TAG: &[u8] = KeyTag::BloomMeta.as_slice();
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

impl MetaOps for CuckooChainMeta {
  const TAG: &[u8] = KeyTag::CuckooMeta.as_slice();
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
