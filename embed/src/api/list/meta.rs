use std::ops::{Deref, DerefMut};

use crate::{
  key_composer::KeyTag,
  meta::{KeyMeta, MetaOps, RedisType},
};

/// List structure metadata (aligned with Apache Kvrocks ListMetadata 42-byte / compact 41-byte format).
/// 列表结构元数据（对标 Apache Kvrocks ListMetadata 42字节 / 紧凑41字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ListMeta {
  pub base: KeyMeta,
  pub head: u64,
  pub tail: u64,
}

impl Deref for ListMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for ListMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl ListMeta {
  pub const INITIAL_INDEX: u64 = u64::MAX / 2; // 0x7fff_ffff_ffff_ffff
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 16; // 26 + 16 = 42
  pub const KVROCKS_ENCODED_SIZE: usize = KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE + 16; // 25 + 16 = 41

  #[inline]
  pub const fn new(expire_at: u64, version: u64) -> Self {
    Self {
      base: KeyMeta::new(RedisType::List, expire_at, version, 0),
      head: Self::INITIAL_INDEX,
      tail: Self::INITIAL_INDEX,
    }
  }

  #[inline]
  pub fn new_with_version(expire_at: u64) -> Self {
    Self {
      base: KeyMeta::new_with_version(RedisType::List, expire_at, 0),
      head: Self::INITIAL_INDEX,
      tail: Self::INITIAL_INDEX,
    }
  }

  #[inline]
  pub const fn size(&self) -> u64 {
    self.base.size
  }

  #[inline]
  pub const fn version(&self) -> u64 {
    self.base.version
  }

  #[inline]
  pub const fn expire_at(&self) -> u64 {
    self.base.expire_at
  }

  #[inline]
  pub const fn ttl(&self, now_ms: u64) -> i64 {
    self.base.ttl(now_ms)
  }

  #[inline]
  pub const fn is_empty(&self) -> bool {
    self.base.size == 0
  }

  #[inline]
  pub const fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline(always)]
  pub fn push_head(&mut self) -> u64 {
    self.head = self.head.wrapping_sub(1);
    self.head
  }

  #[inline(always)]
  pub fn push_tail(&mut self) -> u64 {
    let t = self.tail;
    self.tail = self.tail.wrapping_add(1);
    t
  }

  #[inline(always)]
  pub fn pop_head(&mut self) -> u64 {
    let h = self.head;
    self.head = self.head.wrapping_add(1);
    h
  }

  #[inline(always)]
  pub fn pop_tail(&mut self) -> u64 {
    self.tail = self.tail.wrapping_sub(1);
    self.tail
  }

  #[inline(always)]
  pub fn push_index(&mut self, left: bool) -> u64 {
    if left {
      self.push_head()
    } else {
      self.push_tail()
    }
  }

  #[inline(always)]
  pub fn pop_index(&mut self, left: bool) -> u64 {
    if left {
      self.pop_head()
    } else {
      self.pop_tail()
    }
  }

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&self.base.encode());
    buf[KeyMeta::ENCODED_SIZE..KeyMeta::ENCODED_SIZE + 8].copy_from_slice(&self.head.to_be_bytes());
    buf[KeyMeta::ENCODED_SIZE + 8..Self::ENCODED_SIZE].copy_from_slice(&self.tail.to_be_bytes());
    buf
  }

  /// Encodes into compact 41-byte Kvrocks binary format without heap allocation.
  /// 编码为 Kvrocks 1:1 紧凑 41 字节格式（零堆内存分配）
  #[inline]
  pub fn encode_kvrocks(&self) -> [u8; Self::KVROCKS_ENCODED_SIZE] {
    let mut buf = [0u8; Self::KVROCKS_ENCODED_SIZE];
    let flags =
      KeyMeta::META_64BIT_ENCODING_MASK | (RedisType::List as u8 & KeyMeta::META_TYPE_MASK);
    buf[0] = flags;
    buf[1..9].copy_from_slice(&self.base.expire_at.to_be_bytes());
    buf[9..17].copy_from_slice(&self.base.version.to_be_bytes());
    buf[17..25].copy_from_slice(&self.base.size.to_be_bytes());
    buf[25..33].copy_from_slice(&self.head.to_be_bytes());
    buf[33..41].copy_from_slice(&self.tail.to_be_bytes());
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() >= Self::ENCODED_SIZE && bytes[0] <= 14 && (bytes[1] == 0 || bytes[1] == 0x80) {
      let base = KeyMeta::decode(&bytes[..KeyMeta::ENCODED_SIZE])?;
      let head = read_u64_be(bytes, KeyMeta::ENCODED_SIZE)?;
      let tail = read_u64_be(bytes, KeyMeta::ENCODED_SIZE + 8)?;
      Some(Self { base, head, tail })
    } else if bytes.len() >= Self::KVROCKS_ENCODED_SIZE
      && (bytes[0] & KeyMeta::META_64BIT_ENCODING_MASK != 0)
    {
      let base = KeyMeta::decode(&bytes[..KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE])?;
      let head = read_u64_be(bytes, KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE)?;
      let tail = read_u64_be(bytes, KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE + 8)?;
      Some(Self { base, head, tail })
    } else if bytes.len() >= 33 && (bytes[0] & KeyMeta::META_64BIT_ENCODING_MASK == 0) {
      // Kvrocks 32-bit 紧凑格式 (17字节 base + 16字节 head/tail)
      let base = KeyMeta::decode(&bytes[..17])?;
      let head = read_u64_be(bytes, 17)?;
      let tail = read_u64_be(bytes, 25)?;
      Some(Self { base, head, tail })
    } else if bytes.len() == 16 {
      let head = read_u64_be(bytes, 0)?;
      let tail = read_u64_be(bytes, 8)?;
      let size = if tail >= head {
        tail.wrapping_sub(head)
      } else {
        0
      };
      Some(Self {
        base: KeyMeta::new(RedisType::List, 0, 0, size),
        head,
        tail,
      })
    } else {
      None
    }
  }
}

#[inline(always)]
fn read_u64_be(bytes: &[u8], offset: usize) -> Option<u64> {
  let s: &[u8; 8] = bytes.get(offset..offset + 8)?.try_into().ok()?;
  Some(u64::from_be_bytes(*s))
}

impl MetaOps for ListMeta {
  const TAG: &[u8] = KeyTag::ListMeta.as_slice();
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
