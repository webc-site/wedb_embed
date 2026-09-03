pub use crate::error::ERR_WRONG_TYPE;

pub const BITMAP_SEGMENT_BITS: usize = 1024 * 8; // 8192 位每段
pub const BITMAP_SEGMENT_BYTES: usize = 1024; // 1024 字节每段
pub const MAX_BITMAP_TO_STRING_BYTES: usize = 512 * 1024 * 1024; // 512MB
