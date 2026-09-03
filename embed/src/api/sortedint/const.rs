/// Fixed 8-byte length for 64-bit unsigned integer in big-endian order.
/// 64 位无符号整数原生大端序定长字节数（8 字节）
pub const BE_LEN: usize = 8;

/// Error message constants (aligned with Apache Kvrocks error strings without runtime heap allocation).
/// 错误常量定义（对标 Apache Kvrocks 错误字符串，避免运行时动态堆分配）
pub const ERR_MIN_NOT_INT: &str = "ERR the min isn't integer";
pub const ERR_MAX_NOT_INT: &str = "ERR the max isn't integer";
pub const ERR_MIN_GT_MAX: &str = "ERR min > max";
pub use crate::error::ERR_WRONG_TYPE;
