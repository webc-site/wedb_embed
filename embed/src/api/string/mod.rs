pub mod r#const;
pub mod r#impl;
pub mod key;
pub mod lcs;
pub mod meta;
pub mod opt;

use std::str;

pub use r#const::{
  ERR_DIGEST_INVALID_LEN, ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW,
  ERR_LCS_INSUFFICIENT_MEMORY, ERR_LCS_TOO_LONG, ERR_OFFSET_OUT_OF_RANGE,
  ERR_STRING_EXCEEDS_MAX_SIZE, ERR_VALUE_NOT_FLOAT, ERR_VALUE_NOT_INTEGER, ERR_WRONG_TYPE,
};
pub use meta::{
  STRING_HDR_SIZE, StringMeta, decode_live_string_value, decode_string_value, encode_string_header,
  encode_string_value, encode_string_value_into, is_string_expired,
};
pub use opt::{
  DelEx, GetEx, Lcs, Set, StringLCS, StringLCSIdxResult, StringLCSMatchedRange, StringLCSRange,
  StringLCSResult, StringLCSType, StringMSet, StringPair, StringSet, StringSetType,
};
use rapidhash::v3::rapidhash_v3;
pub use wedb_resp::parse_i64_fast;

use crate::{
  engine::{Engine, Partition},
  error::{Error, Result},
  key::check_composite_meta_not_other_type,
  meta::{
    bytes_to_hex_16, current_now_ms, parse_redis_float as meta_parse_redis_float,
    parse_redis_integer as meta_parse_redis_integer, u64_to_hex_16,
  },
  wedb::Db,
};

/// Maximum supported string value size (512MB).
/// 字符串最大支持 512MB
pub const MAX_STRING_SIZE: usize = 512 * 1024 * 1024;

pub use key::{
  prefix as compose_string_prefix, prefix_stack as compose_string_prefix_stack,
  raw as compose_string_key, raw_bytes as compose_string_key_bytes,
};
pub use lcs::{compute_lcs, compute_lcs_with};

pub use crate::meta::normalize_range;

/// Parses a Redis integer with zero-copy byte inspection and strict whitespace validation.
/// 解析 Redis 整数（单次遍历零拷贝字节解析，严格校验空白符与数值合法性，对标 Kvrocks ParseInt）
#[inline]
pub fn parse_redis_integer(v: &[u8]) -> Result<i64> {
  meta_parse_redis_integer(v, ERR_VALUE_NOT_INTEGER)
}

/// Parses Redis float from byte slice with strict validation.
/// 解析 Redis 浮点数（严格校验空白符与浮点合法性，对标 Kvrocks ParseFloat）
#[inline]
pub fn parse_redis_float(v: &[u8]) -> Result<f64> {
  meta_parse_redis_float(v, ERR_VALUE_NOT_FLOAT)
}

/// Computes 64-bit hexadecimal digest string for comparison operations.
/// 计算字符串 64 位十六进制摘要（对标 Kvrocks util::StringDigest，单次分配）
#[inline]
pub fn string_digest(val: &[u8]) -> String {
  let hash = rapidhash_v3(val);
  let bytes = u64_to_hex_16(hash);
  // SAFETY: u64_to_hex_16 仅生成 '0'..='9' 与 'a'..='f' 的 ASCII 字符，必为有效 UTF-8。
  unsafe { str::from_utf8_unchecked(&bytes) }.to_string()
}

/// Computes 16-byte hexadecimal digest array with zero heap allocation.
/// 计算字符串 16 字节十六进制摘要数组（零堆分配，对标 Kvrocks util::StringDigest）
#[inline]
pub fn string_digest_bytes(val: &[u8]) -> [u8; 16] {
  let hash = rapidhash_v3(val);
  bytes_to_hex_16(hash.to_be_bytes())
}

/// Formats a float into compact byte slice without heap allocation.
/// 紧凑浮点数字节序列化（基于 zmij 实现零堆分配切片生成，对标 Kvrocks util::Float2String）
#[inline]
pub fn format_float_bytes(val: f64, buf: &mut zmij::Buffer) -> &[u8] {
  if val.is_infinite() {
    if val.is_sign_positive() {
      b"inf"
    } else {
      b"-inf"
    }
  } else if val.is_nan() {
    b"nan"
  } else if val == 0.0 {
    b"0"
  } else {
    let s = buf.format_finite(val);
    if let Some(stripped) = s.strip_suffix(".0") {
      stripped.as_bytes()
    } else {
      s.as_bytes()
    }
  }
}

/// Serializes float to compact string format using zmij with zero heap allocation.
/// 紧凑浮点数字符串序列化（基于 zmij 实现高性能零堆分配格式化，对标 Kvrocks util::Float2String）
#[inline]
pub fn format_float(val: f64) -> String {
  let mut buf = zmij::Buffer::new();
  let bytes = format_float_bytes(val, &mut buf);
  str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Raw String payload tuple: (raw_value_guard, expire_at_ms, payload_offset).
/// String 原始数据三元组：(raw_value_guard, expire_at_ms, payload_offset)
pub type StringRawPayload<V> = (V, u64, usize);

/// Zero-copy reads underlying SingleKV raw slice and expiration timestamp.
/// 零拷贝读取底层 SingleKV 原始切片与过期信息（内部核心辅助方法，支持严格 WRONGTYPE 校验）
#[inline]
pub fn get_string_raw<E: Engine>(
  db: &Db<E>,
  key_bytes: &[u8],
) -> Result<Option<StringRawPayload<<E::Partition as Partition>::Value>>>
where
  Error: From<E::Error>,
{
  let kc = db.kc();
  let raw_k = compose_string_key(&kc, key_bytes);
  let now_ms = current_now_ms();

  let data_ks = db.data();

  if let Some(raw) = data_ks.get(&raw_k)? {
    let (expire_at, payload) = decode_string_value(&raw);
    if !is_string_expired(expire_at, now_ms) {
      let offset = raw.len() - payload.len();
      return Ok(Some((raw, expire_at, offset)));
    }
  }

  check_composite_meta_not_other_type(db, key_bytes, b"", now_ms)?;
  Ok(None)
}
