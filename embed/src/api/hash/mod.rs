pub mod r#const;
pub mod hfe;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod num;
pub mod opt;
pub mod query;
pub mod scan;

pub use r#const::{
  ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING, ERR_HASH_VALUE_NOT_FLOAT, ERR_HASH_VALUE_NOT_INTEGER,
  ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW, ERR_WRONG_TYPE, HASH_EXPIRE_COND_FAILED,
  HASH_EXPIRE_DELETED, HASH_EXPIRE_SET_OK, HASH_FIELD_NOT_FOUND, HASH_FIELD_PERSISTENT,
};
pub use r#impl::prepare_hash_meta_for_write;
pub use meta::{
  FIELD_EXPIRE_PREFIX_LEN, HashFieldState, HashFieldStateKind, HashItemKeyComposer, HashMeta,
  HashSubkeyEncodingMode, compose_hash_key, compose_hash_meta_key, compose_hash_prefix,
  compose_hash_prefix_stack, decode_field_state, decode_hash_value, decode_live_hash_value,
  encode_hash_value, encode_hash_value_into, hexpire_condition_passes, is_field_expired,
  is_immediate_expire,
};
pub use opt::{
  FieldValue, HExpire, HGetEx, HSet, HashFieldSetCondition, HashGetEx, HashLengthMode, HashSetEx,
  RangeLex, TTLAction,
};
pub type HashFieldPair = (Vec<u8>, Vec<u8>);
pub type HashRandField = (Vec<u8>, Option<Vec<u8>>);
pub type HashScanResult = (usize, Vec<HashFieldPair>);
pub type HashScanByFieldResult = (Option<Vec<u8>>, Vec<HashFieldPair>);

use crate::{
  error::Result,
  meta::{parse_redis_float, parse_redis_integer},
};

/// Divides by 1000 with ceiling rounding aligned with Kvrocks CeilDiv1000.
/// 向上取整除以 1000
#[inline(always)]
pub(crate) const fn ceil_div_1000(val: u64) -> u64 {
  val.div_ceil(1000)
}

/// Parses a Redis integer from byte slice with strict whitespace validation.
/// 解析 Redis 整数（严格校验空白符与合法性）
#[inline]
pub(crate) fn parse_hash_integer(v: &[u8]) -> Result<i64> {
  parse_redis_integer(v, ERR_HASH_VALUE_NOT_INTEGER)
}

/// Parses a Redis float from byte slice with strict whitespace validation.
/// 解析 Redis 浮点数（严格校验空白符与浮点合法性）
#[inline]
pub(crate) fn parse_hash_float(v: &[u8]) -> Result<f64> {
  parse_redis_float(v, ERR_HASH_VALUE_NOT_FLOAT)
}

/// Cached field state and underlying raw byte buffer.
/// 缓存的字段状态与原始物理切片
#[derive(Clone)]
pub(crate) struct CachedFieldState {
  pub kind: HashFieldStateKind,
  pub expire: u64,
  pub raw: Option<Box<[u8]>>,
}
