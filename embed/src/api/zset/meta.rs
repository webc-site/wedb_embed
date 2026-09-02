pub use crate::meta::{decode_sortable_f64, encode_sortable_f64};
use crate::{impl_simple_meta, key_composer::KeyTag, meta::RedisType};

impl_simple_meta!(
  /// 有序集合结构元数据（对标 Apache Kvrocks ZSetMetadata 26字节 / 紧凑25字节）
  ZSetMeta,
  RedisType::ZSet,
  KeyTag::ZSetMeta
);
