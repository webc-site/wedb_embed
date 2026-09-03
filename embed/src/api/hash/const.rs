pub use crate::error::ERR_WRONG_TYPE;

/// Hash field status codes and response constants aligned with Redis 7.4 / Apache Kvrocks.
/// 哈希字段状态码与返回值常量
pub const HASH_FIELD_NOT_FOUND: i64 = -2;
pub const HASH_FIELD_PERSISTENT: i64 = -1;
pub const HASH_EXPIRE_COND_FAILED: i64 = 0;
pub const HASH_EXPIRE_SET_OK: i64 = 1;
pub const HASH_EXPIRE_DELETED: i64 = 2;

/// Error message constants aligned with Redis 7.4 / Apache Kvrocks.
/// 错误提示常量
pub const ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING: &str =
  "ERR hash field expiration is not supported by legacy hash encoding";
pub const ERR_HASH_VALUE_NOT_INTEGER: &str = "ERR hash value is not an integer";
pub const ERR_HASH_VALUE_NOT_FLOAT: &str = "ERR hash value is not a valid float";
pub const ERR_INCREMENT_OVERFLOW: &str = "ERR increment or decrement would overflow";
pub const ERR_INCREMENT_NAN_OR_INFINITY: &str = "ERR increment would produce NaN or Infinity";
