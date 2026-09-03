pub use crate::error::ERR_WRONG_TYPE;

/// Error message constants (1:1 aligned with Apache Kvrocks and Redis specifications).
/// 错误提示常量定义（1:1 对标 Apache Kvrocks 与 Redis 规范）
pub const ERR_VALUE_NOT_INTEGER: &str = "ERR value is not an integer or out of range";
pub const ERR_VALUE_NOT_FLOAT: &str = "ERR value is not a valid float";
pub const ERR_INCREMENT_OVERFLOW: &str = "ERR increment or decrement would overflow";
pub const ERR_INCREMENT_NAN_OR_INFINITY: &str = "ERR increment would produce NaN or Infinity";
pub const ERR_DIGEST_INVALID_LEN: &str = "ERR digest must be exactly 16 hexadecimal characters";
pub const ERR_OFFSET_OUT_OF_RANGE: &str = "ERR offset is out of range";
pub const ERR_STRING_EXCEEDS_MAX_SIZE: &str = "ERR string exceeds maximum allowed size (512MB)";
pub const ERR_LCS_TOO_LONG: &str = "ERR String too long for LCS";
pub const ERR_LCS_INSUFFICIENT_MEMORY: &str =
  "ERR Insufficient memory, transient memory for LCS exceeds proto-max-bulk-len";
/// Maximum string length (512MB, aligned with Kvrocks and Redis proto-max-bulk-len).
/// 字符串最大长度（512MB，对标 Kvrocks 与 Redis proto-max-bulk-len）
pub const MAX_STRING_SIZE: usize = 512 * 1024 * 1024;
