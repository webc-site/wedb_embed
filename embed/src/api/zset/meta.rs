pub use crate::meta::{decode_sortable_f64, encode_sortable_f64};
use crate::{impl_simple_meta, key_composer::KeyTag, meta::RedisType};

/// 从切片快速零拷贝解码 sortable f64（保序 8 字节）
#[inline(always)]
pub fn decode_sortable_f64_slice(bytes: &[u8]) -> Option<f64> {
  if bytes.len() >= 8 {
    let arr: [u8; 8] = bytes[..8].try_into().ok()?;
    Some(decode_sortable_f64(arr))
  } else {
    None
  }
}


impl_simple_meta!(
  /// 有序集合结构元数据（对标 Apache Kvrocks ZSetMetadata 26字节 / 紧凑25字节）
  ZSetMeta,
  RedisType::ZSet,
  KeyTag::ZSetMeta
);
