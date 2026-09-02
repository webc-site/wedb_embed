use std::{
  str,
  sync::atomic::{AtomicU64, Ordering},
};

use rapidhash::v3::rapidhash_v3;
use wedb_resp::parse_i64_fast;

use crate::error::{Error, Result};

/// Redis data type enumeration (aligned with Apache Kvrocks RedisType).
/// Redis 数据类型枚举（对标 Apache Kvrocks RedisType）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode, strum::FromRepr)]
#[repr(u8)]
pub enum RedisType {
  None = 0,
  String = 1,
  Hash = 2,
  List = 3,
  Set = 4,
  ZSet = 5,
  Bitmap = 6,
  SortedInt = 7,
  Stream = 8,
  Bloom = 9,
  Json = 10,
  HyperLogLog = 11,
  TDigest = 12,
  TimeSeries = 13,
  CuckooFilter = 14,
}

impl RedisType {
  #[inline]
  pub const fn name(&self) -> &'static str {
    match self {
      Self::None => "none",
      Self::String => "string",
      Self::Hash => "hash",
      Self::List => "list",
      Self::Set => "set",
      Self::ZSet => "zset",
      Self::Bitmap => "bitmap",
      Self::SortedInt => "sortedint",
      Self::Stream => "stream",
      Self::Bloom => "MBbloom--",
      Self::Json => "ReJSON-RL",
      Self::HyperLogLog => "hyperloglog",
      Self::TDigest => "TDIS-TYPE",
      Self::TimeSeries => "timeseries",
      Self::CuckooFilter => "MBbloomCF",
    }
  }

  #[inline]
  pub const fn from_u8(val: u8) -> Self {
    match Self::from_repr(val) {
      Some(t) => t,
      None => Self::None,
    }
  }

  #[inline]
  pub const fn is_single_kv_type(&self) -> bool {
    matches!(self, Self::String | Self::Json)
  }

  #[inline]
  pub const fn is_emptyable_type(&self) -> bool {
    matches!(
      self,
      Self::String
        | Self::Json
        | Self::Stream
        | Self::Bloom
        | Self::HyperLogLog
        | Self::TDigest
        | Self::TimeSeries
        | Self::CuckooFilter
    )
  }
}

// ================= Version 生成机制（对标 Apache Kvrocks 53-bit 时间戳 + 11-bit 计数器） =================

pub const VERSION_COUNTER_BITS: u32 = 11;
pub const VERSION_COUNTER_MASK: u64 = (1 << VERSION_COUNTER_BITS) - 1;

static VERSION_COUNTER: AtomicU64 = AtomicU64::new(0);
static LAST_VERSION: AtomicU64 = AtomicU64::new(0);

/// Initializes version counter with microsecond timestamp and rapidhash seed.
/// 初始化版本计数器（基于微秒与 rapidhash 生成随机初始偏移，避免主从切换时时钟回退冲突）
pub fn init_version_counter() {
  let now_nanos = coarsetime::Clock::now_since_epoch().as_nanos();
  let seed = rapidhash_v3(&now_nanos.to_be_bytes());
  VERSION_COUNTER.store(seed & VERSION_COUNTER_MASK, Ordering::Relaxed);
}

/// Generates strictly monotonic version number: high 53-bit microseconds + low 11-bit atomic counter.
/// 生成唯一严格递增版本号：高 53 位微秒时间戳 + 低 11 位原子计数器（基于 coarsetime 与 CAS 保证绝对单调递增）
#[inline]
pub fn generate_version() -> u64 {
  let ts_us = coarsetime::Clock::now_since_epoch().as_micros();
  let counter = VERSION_COUNTER.fetch_add(1, Ordering::Relaxed);
  let mut candidate = (ts_us << VERSION_COUNTER_BITS) | (counter & VERSION_COUNTER_MASK);

  let mut last = LAST_VERSION.load(Ordering::Relaxed);
  loop {
    if candidate <= last {
      candidate = last + 1;
    }
    match LAST_VERSION.compare_exchange_weak(last, candidate, Ordering::AcqRel, Ordering::Relaxed) {
      Ok(_) => break candidate,
      Err(actual) => last = actual,
    }
  }
}

/// Returns current timestamp in milliseconds (aligned with Kvrocks GetTimeStampMS).
/// 获取当前时间戳毫秒数（基于 coarsetime::Clock，极速无昂贵系统调用，对标 Kvrocks GetTimeStampMS）
#[inline(always)]
pub fn current_now_ms() -> u64 {
  coarsetime::Clock::now_since_epoch().as_millis()
}

/// Returns current timestamp in seconds.
/// 获取当前时间戳秒数（基于 ts_::sec() 极速时间戳）
#[inline(always)]
pub fn current_now_sec() -> u64 {
  ts_::sec()
}

/// Decodes creation time in seconds and microseconds from version number.
/// 从版本号中解析出创建时间（秒与微秒，对标 Kvrocks Metadata::Time）
#[inline]
pub fn version_to_time(version: u64) -> (u64, u32) {
  let ts_us = version >> VERSION_COUNTER_BITS;
  let sec = ts_us / 1_000_000;
  let usec = (ts_us % 1_000_000) as u32;
  (sec, usec)
}

/// Unified trait for composite metadata types.
/// 复合数据类型 meta 统一操作 trait（泛型函数核心约束）。
/// Operation definition.
/// 所有复合 meta（Set/List/Hash/HLL/SortedInt）实现此 trait，使 `WeDb` 可通过泛型函数消除跨模块重复逻辑。
pub trait MetaOps: Sized {
  /// Metadata key tag slice (e.g. `b"sm"`, `b"hm"`).
  /// meta key 标签（如 `b"sm"`, `b"hm"`），用于 `check_key_not_other_type`
  const TAG: &[u8];

  type EncodedBytes: AsRef<[u8]>;

  fn decode(bytes: &[u8]) -> Option<Self>;
  fn is_expired(&self, now_ms: u64) -> bool;

  /// Encoded bytes slice for storage write operations without heap allocation.
  /// 编码后的字节，用于写入存储（零堆分配）
  fn encode_bytes(&self) -> Self::EncodedBytes;

  /// Returns immutable reference to base KeyMeta.
  /// 获取 base 引用（访问 expire_at / ttl 等通用字段）
  fn base(&self) -> &KeyMeta;

  /// Returns mutable reference to base KeyMeta.
  /// 获取 base 可变引用（修改 expire_at）
  fn base_mut(&mut self) -> &mut KeyMeta;
}

/// Macro for implementing standard KeyMeta wrapper types with MetaOps and Deref.
/// 宏：一键实现基于基础 KeyMeta 的复合数据结构元数据及其 MetaOps / Deref 特征
#[macro_export]
macro_rules! impl_simple_meta {
  ($(#[$meta:meta])* $struct_name:ident, $redis_type:expr, $key_tag:expr) => {
    $(#[$meta])*
    #[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
    pub struct $struct_name {
      pub base: $crate::meta::KeyMeta,
    }

    impl $crate::meta::MetaOps for $struct_name {
      const TAG: &'static [u8] = $key_tag.as_slice();
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
      fn base(&self) -> &$crate::meta::KeyMeta {
        &self.base
      }

      #[inline]
      fn base_mut(&mut self) -> &mut $crate::meta::KeyMeta {
        &mut self.base
      }
    }

    impl ::core::ops::Deref for $struct_name {
      type Target = $crate::meta::KeyMeta;
      #[inline(always)]
      fn deref(&self) -> &Self::Target {
        &self.base
      }
    }

    impl ::core::ops::DerefMut for $struct_name {
      #[inline(always)]
      fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
      }
    }

    impl Default for $struct_name {
      #[inline]
      fn default() -> Self {
        Self::new_with_version(0, 0)
      }
    }

    impl $struct_name {
      pub const ENCODED_SIZE: usize = $crate::meta::KeyMeta::ENCODED_SIZE;
      pub const KVROCKS_ENCODED_SIZE: usize = $crate::meta::KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE;

      #[inline]
      pub const fn new(expire_at: u64, version: u64, size: u64) -> Self {
        Self {
          base: $crate::meta::KeyMeta::new($redis_type, expire_at, version, size),
        }
      }

      #[inline]
      pub fn new_with_version(expire_at: u64, size: u64) -> Self {
        Self {
          base: $crate::meta::KeyMeta::new_with_version($redis_type, expire_at, size),
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

      #[inline]
      pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
        self.base.encode()
      }

      #[inline]
      pub fn encode_kvrocks(&self) -> Vec<u8> {
        self.base.encode_kvrocks()
      }

      #[inline]
      pub fn decode(bytes: &[u8]) -> Option<Self> {
        let base = $crate::meta::KeyMeta::decode(bytes)?;
        if base.rtype == $redis_type {
          Some(Self { base })
        } else {
          None
        }
      }
    }
  };
}

/// Fundamental 26-byte metadata structure (aligned with Apache Kvrocks KeyMetadata).
/// 基础通用 26 字节元数据结构（对标 Apache Kvrocks KeyMetadata）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct KeyMeta {
  pub rtype: RedisType,
  pub flags: u8,
  pub expire_at: u64,
  pub version: u64,
  pub size: u64,
}

impl KeyMeta {
  pub const META_64BIT_ENCODING_MASK: u8 = 0x80;
  pub const META_TYPE_MASK: u8 = 0x0F;
  pub const ENCODED_SIZE: usize = 26; // 1B rtype + 1B flags + 8B expire + 8B version + 8B size
  pub const KVROCKS_COMPLEX_ENCODED_SIZE: usize = 25; // 1B flags(type+64bit) + 8B expire + 8B version + 8B size
  pub const KVROCKS_SINGLE_KV_ENCODED_SIZE: usize = 9; // 1B flags + 8B expire

  #[inline]
  pub const fn new(rtype: RedisType, expire_at: u64, version: u64, size: u64) -> Self {
    Self {
      rtype,
      flags: 0,
      expire_at,
      version,
      size,
    }
  }

  #[inline]
  pub fn new_with_version(rtype: RedisType, expire_at: u64, size: u64) -> Self {
    Self {
      rtype,
      flags: 0,
      expire_at,
      version: generate_version(),
      size,
    }
  }

  #[inline]
  pub const fn is_expired(&self, now_ms: u64) -> bool {
    if !self.is_emptyable_type() && self.size == 0 {
      return true;
    }
    self.expire_at > 0 && self.expire_at <= now_ms
  }

  #[inline]
  pub const fn is_single_kv_type(&self) -> bool {
    self.rtype.is_single_kv_type()
  }

  #[inline]
  pub const fn is_emptyable_type(&self) -> bool {
    self.rtype.is_emptyable_type()
  }

  #[inline]
  pub const fn ttl(&self, now_ms: u64) -> i64 {
    self.ttl_ms(now_ms)
  }

  #[inline]
  pub const fn ttl_ms(&self, now_ms: u64) -> i64 {
    if self.expire_at == 0 {
      -1
    } else if self.expire_at <= now_ms {
      -2
    } else {
      (self.expire_at - now_ms) as i64
    }
  }

  #[inline]
  pub const fn ttl_sec(&self, now_ms: u64) -> i64 {
    let ms = self.ttl_ms(now_ms);
    if ms < 0 { ms } else { (ms + 999) / 1000 }
  }

  #[inline]
  pub fn expire_at_ms_to_sec(ms: u64) -> u64 {
    if ms == 0 {
      0
    } else if ms < 1000 {
      1
    } else {
      (ms + 499) / 1000
    }
  }

  #[inline]
  pub const fn is_64bit_encoded_flags(flags: u8) -> bool {
    flags & Self::META_64BIT_ENCODING_MASK != 0
  }

  #[inline]
  pub const fn is_64bit_encoded(&self) -> bool {
    Self::is_64bit_encoded_flags(self.flags)
  }

  #[inline]
  pub const fn common_encoded_size(&self) -> usize {
    if self.is_64bit_encoded() { 8 } else { 4 }
  }

  /// Returns byte offset following expiration timestamp in encoded metadata.
  /// 获取元数据过期时间之后的字节偏移量（对标 Kvrocks Metadata::GetOffsetAfterExpire）
  #[inline]
  pub const fn get_offset_after_expire(flags: u8) -> usize {
    if Self::is_64bit_encoded_flags(flags) {
      1 + 8 // 1B flags + 8B expire
    } else {
      1 + 4 // 1B flags + 4B expire
    }
  }

  /// Returns byte offset following size field in encoded metadata.
  /// 获取复合元数据大小之后的字节偏移量（对标 Kvrocks Metadata::GetOffsetAfterSize）
  #[inline]
  pub const fn get_offset_after_size(flags: u8) -> usize {
    if Self::is_64bit_encoded_flags(flags) {
      1 + 8 + 8 + 8 // 1B flags + 8B expire + 8B version + 8B size
    } else {
      1 + 4 + 8 + 4 // 1B flags + 4B expire + 8B version + 4B size
    }
  }

  /// Encodes into standard 26-byte metadata header.
  /// 编码为标准 26 字节元数据头
  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    buf[0] = self.rtype as u8;
    buf[1] = self.flags;
    buf[2..10].copy_from_slice(&self.expire_at.to_be_bytes());
    buf[10..18].copy_from_slice(&self.version.to_be_bytes());
    buf[18..26].copy_from_slice(&self.size.to_be_bytes());
    buf
  }

  /// Encodes into compact Kvrocks binary format (25-byte composite / 9-byte SingleKV).
  /// 编码为 Kvrocks 1:1 紧凑二进制格式（25字节复合类型 / 9字节SingleKV）
  #[inline]
  pub fn encode_kvrocks(&self) -> Vec<u8> {
    let flags = Self::META_64BIT_ENCODING_MASK | (self.rtype as u8 & Self::META_TYPE_MASK);
    if self.is_single_kv_type() {
      let mut out = Vec::with_capacity(Self::KVROCKS_SINGLE_KV_ENCODED_SIZE);
      out.push(flags);
      out.extend_from_slice(&self.expire_at.to_be_bytes());
      out
    } else {
      let mut out = Vec::with_capacity(Self::KVROCKS_COMPLEX_ENCODED_SIZE);
      out.push(flags);
      out.extend_from_slice(&self.expire_at.to_be_bytes());
      out.extend_from_slice(&self.version.to_be_bytes());
      out.extend_from_slice(&self.size.to_be_bytes());
      out
    }
  }

  /// Decodes metadata header from binary slice.
  /// 解码元数据头（支持标准 26 字节头、25 字节复合结构头与 9 字节 SingleKV 头）
  #[inline(always)]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    let len = bytes.len();
    if len == 0 {
      return None;
    }
    let first = bytes[0];

    // 1. 标准 26 字节头格式
    if len >= Self::ENCODED_SIZE && first <= 14 {
      let rtype = RedisType::from_u8(first);
      let flags = bytes[1];
      let expire_at = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
      ]);
      let version = u64::from_be_bytes([
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17],
      ]);
      let size = u64::from_be_bytes([
        bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25],
      ]);

      return Some(Self {
        rtype,
        flags,
        expire_at,
        version,
        size,
      });
    }

    // 2. 紧凑 25 字节复合结构头或 9 字节 SingleKV 头
    if first & Self::META_64BIT_ENCODING_MASK != 0 {
      let flags = first;
      let rtype = RedisType::from_u8(flags & Self::META_TYPE_MASK);
      if rtype.is_single_kv_type() {
        if len < Self::KVROCKS_SINGLE_KV_ENCODED_SIZE {
          return None;
        }
        let expire_at = u64::from_be_bytes([
          bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
        ]);
        return Some(Self {
          rtype,
          flags,
          expire_at,
          version: 0,
          size: 0,
        });
      } else if len >= Self::KVROCKS_COMPLEX_ENCODED_SIZE {
        let expire_at = u64::from_be_bytes([
          bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
        ]);
        let version = u64::from_be_bytes([
          bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
        ]);
        let size = u64::from_be_bytes([
          bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23], bytes[24],
        ]);

        return Some(Self {
          rtype,
          flags,
          expire_at,
          version,
          size,
        });
      }
    }

    None
  }
}

/// Normalizes Redis index range supporting negative indices and wrapping.
/// 归一化 Redis 索引范围（支持负数索引回环，未命中时 start > end）
#[inline]
pub const fn normalize_range(start: i64, stop: i64, len: i64) -> (i64, i64) {
  if len <= 0 {
    return (0, -1);
  }
  let mut s = if start < 0 { len + start } else { start };
  let mut e = if stop < 0 { len + stop } else { stop };
  if s < 0 {
    s = 0;
  }
  if e >= len {
    e = len - 1;
  }
  (s, e)
}

/// Normalizes Bitmap index range with lower bound clamped to 0.
/// 归一化 Bitmap 位图索引范围（对标 Apache Kvrocks BitmapString::NormalizeRange，负数下界对齐到 0）
#[inline]
pub const fn normalize_bitmap_range(origin_start: i64, origin_end: i64, length: i64) -> (i64, i64) {
  if length <= 0 {
    return (0, -1);
  }
  let mut start = if origin_start < 0 {
    origin_start + length
  } else {
    origin_start
  };
  let mut end = if origin_end < 0 {
    origin_end + length
  } else {
    origin_end
  };
  if start < 0 {
    start = 0;
  }
  if end < 0 {
    end = 0;
  }
  if end >= length {
    end = length - 1;
  }
  (start, end)
}

const SIGN_MASK: u64 = 1 << 63;

/// Order-preserving IEEE 754 double conversion to u64.
/// IEEE 754 浮点数保序转 u64（对标 Kvrocks EncodeDoubleToUInt64）
#[inline(always)]
pub const fn encode_sortable_f64_u64(val: f64) -> u64 {
  let bits = val.to_bits();
  if bits & SIGN_MASK != 0 {
    !bits
  } else {
    bits ^ SIGN_MASK
  }
}

/// Decodes IEEE 754 double from order-preserving u64.
/// 从保序 u64 解码出 IEEE 754 浮点数（对标 Kvrocks DecodeDoubleFromUInt64）
#[inline(always)]
pub const fn decode_sortable_f64_u64(sortable: u64) -> f64 {
  let orig = if sortable & SIGN_MASK != 0 {
    sortable ^ SIGN_MASK
  } else {
    !sortable
  };
  f64::from_bits(orig)
}

/// Big-endian order-preserving 8-byte encoding for IEEE 754 double score.
/// IEEE 754 浮点数大端序保序编码为 8 字节（对标 Kvrocks/RocksDB Score Encoding）
#[inline(always)]
pub const fn encode_sortable_f64(val: f64) -> [u8; 8] {
  encode_sortable_f64_u64(val).to_be_bytes()
}

/// Decodes f64 score from big-endian order-preserving 8-byte slice.
/// IEEE 754 浮点数大端序保序解码为 f64
#[inline(always)]
pub const fn decode_sortable_f64(bytes: [u8; 8]) -> f64 {
  decode_sortable_f64_u64(u64::from_be_bytes(bytes))
}

// ================= 十六进制编解码工具（全库统一静态 LUT 驱动，零堆分配） =================

/// Hexadecimal ASCII character table.
/// 十六进制字符表
pub const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Static hexadecimal decode lookup table with 0xFF representing invalid byte.
/// 编译期静态十六进制解码表（0xFF 表示非法字符，消除运行时多重分支与跳转）
pub const HEX_DECODE_LUT: [u8; 256] = {
  let mut table = [0xFFu8; 256];
  let mut i = 0u8;
  while i < 10 {
    table[(b'0' + i) as usize] = i;
    i += 1;
  }
  let mut i = 0u8;
  while i < 6 {
    table[(b'a' + i) as usize] = 10 + i;
    table[(b'A' + i) as usize] = 10 + i;
    i += 1;
  }
  table
};

/// Converts 8-byte array into 16-byte hexadecimal ASCII array.
/// 字节数组（8字节）转 16 字节十六进制字符数组
#[inline(always)]
pub const fn bytes_to_hex_16(bytes: [u8; 8]) -> [u8; 16] {
  let mut out = [0u8; 16];
  let mut i = 0;
  while i < 8 {
    let b = bytes[i];
    out[i * 2] = HEX_CHARS[(b >> 4) as usize];
    out[i * 2 + 1] = HEX_CHARS[(b & 0x0f) as usize];
    i += 1;
  }
  out
}

/// Converts u64 big-endian value into 16-byte hexadecimal ASCII array.
/// u64 大端序转 16 字节十六进制字符数组
#[inline(always)]
pub const fn u64_to_hex_16(val: u64) -> [u8; 16] {
  bytes_to_hex_16(val.to_be_bytes())
}

/// Fast decodes 16-character hexadecimal slice into u64.
/// 快速解析 16 位十六进制为 u64
#[inline(always)]
pub const fn decode_hex_u64(hex: &[u8]) -> Option<u64> {
  if hex.len() != 16 {
    return None;
  }
  let mut val = 0u64;
  let mut i = 0;
  while i < 16 {
    let digit = HEX_DECODE_LUT[hex[i] as usize];
    if digit == 0xFF {
      return None;
    }
    val = (val << 4) | (digit as u64);
    i += 1;
  }
  Some(val)
}

/// Validates that number byte slice is non-empty and has no leading/trailing whitespace.
/// 校验数值字节切片非空且无前导/尾随空白符
#[inline(always)]
pub fn validate_redis_number_slice<'a>(v: &'a [u8], err_msg: &'static str) -> Result<&'a [u8]> {
  if v.is_empty() || v[0].is_ascii_whitespace() || v.last().is_some_and(|b| b.is_ascii_whitespace())
  {
    return Err(Error::invalid_data(err_msg));
  }
  Ok(v)
}

/// Parses Redis integer from byte slice with strict validation.
/// 解析 Redis 整数（严格校验空白符与数值合法性，对标 Kvrocks ParseInt）
#[inline]
pub fn parse_redis_integer(v: &[u8], err_msg: &'static str) -> Result<i64> {
  let v = validate_redis_number_slice(v, err_msg)?;
  parse_i64_fast(v).ok_or_else(|| Error::invalid_data(err_msg))
}

/// Parses Redis float from byte slice with strict validation.
/// 解析 Redis 浮点数（严格校验空白符与浮点合法性，对标 Kvrocks ParseFloat）
#[inline]
pub fn parse_redis_float(v: &[u8], err_msg: &'static str) -> Result<f64> {
  let v = validate_redis_number_slice(v, err_msg)?;
  let val = str::from_utf8(v)
    .map_err(|_| Error::invalid_data(err_msg))?
    .parse::<f64>()
    .map_err(|_| Error::invalid_data(err_msg))?;
  if val.is_nan() || val.is_infinite() {
    return Err(Error::invalid_data(err_msg));
  }
  Ok(val)
}

use std::ops::{Bound, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

/// Trait for converting Rust native ranges into index ranges `(start, stop)`.
/// Helper trait for converting integer types into i64 index bounds without macros.
/// 索引整数泛型转换 trait
pub trait ToIndexBound: Copy {
  fn to_i64(self) -> i64;
}

impl ToIndexBound for i64 {
  #[inline(always)]
  fn to_i64(self) -> i64 {
    self
  }
}

impl ToIndexBound for usize {
  #[inline(always)]
  fn to_i64(self) -> i64 {
    self as i64
  }
}

impl ToIndexBound for isize {
  #[inline(always)]
  fn to_i64(self) -> i64 {
    self as i64
  }
}

impl ToIndexBound for i32 {
  #[inline(always)]
  fn to_i64(self) -> i64 {
    self as i64
  }
}

impl ToIndexBound for u32 {
  #[inline(always)]
  fn to_i64(self) -> i64 {
    self as i64
  }
}

/// Trait for converting Rust native ranges into index ranges `(start, stop)`.
/// 支持从 Rust 原生 Range (如 `0..=5`, `0..-1`, `..`) 或元组 `(start, stop)` 统一转换索引范围
pub trait IntoIndexRange {
  fn into_index_range(self) -> (i64, i64);
}

impl IntoIndexRange for RangeFull {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (0, -1)
  }
}

impl<T: ToIndexBound> IntoIndexRange for (T, T) {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (self.0.to_i64(), self.1.to_i64())
  }
}

impl<T: ToIndexBound> IntoIndexRange for &(T, T) {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (self.0.to_i64(), self.1.to_i64())
  }
}

impl<T: ToIndexBound> IntoIndexRange for Range<T> {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (self.start.to_i64(), self.end.to_i64().saturating_sub(1))
  }
}

impl<T: ToIndexBound> IntoIndexRange for RangeInclusive<T> {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (self.start().to_i64(), self.end().to_i64())
  }
}

impl<T: ToIndexBound> IntoIndexRange for RangeFrom<T> {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (self.start.to_i64(), -1)
  }
}

impl<T: ToIndexBound> IntoIndexRange for RangeTo<T> {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (0, self.end.to_i64().saturating_sub(1))
  }
}

impl<T: ToIndexBound> IntoIndexRange for RangeToInclusive<T> {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    (0, self.end.to_i64())
  }
}

impl<T: ToIndexBound> IntoIndexRange for (Bound<T>, Bound<T>) {
  #[inline]
  fn into_index_range(self) -> (i64, i64) {
    let start = match self.0 {
      Bound::Included(v) => v.to_i64(),
      Bound::Excluded(v) => v.to_i64().saturating_add(1),
      Bound::Unbounded => 0,
    };
    let stop = match self.1 {
      Bound::Included(v) => v.to_i64(),
      Bound::Excluded(v) => v.to_i64().saturating_sub(1),
      Bound::Unbounded => -1,
    };
    (start, stop)
  }
}
