pub mod r#const;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod opt;

pub use r#const::{BE_LEN, ERR_MAX_NOT_INT, ERR_MIN_GT_MAX, ERR_MIN_NOT_INT, ERR_WRONG_TYPE};
pub use key::{
  item as compose_si_item_key, key as compose_si_key, meta as compose_si_meta_key,
  prefix as compose_si_prefix, prefix_stack as compose_si_prefix_stack,
};
pub use meta::SortedintMeta;
pub use opt::{IntoSortedintRange, SortedintRange, decode_be_u64, encode_be_u64, parse_range_spec};
/// Extracts 64-bit integer ID from subkey slice (8-byte big-endian).
/// 从存储键字节切片中提取 64 位整数 ID（大端序紧凑保序 8 字节）
#[inline(always)]
pub fn extract_id(key_bytes: &[u8], prefix_len: usize) -> Option<u64> {
  let sub = key_bytes.get(prefix_len..)?;
  decode_be_u64(sub.get(..BE_LEN)?)
}
