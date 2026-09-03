use crate::{impl_simple_meta, key_composer::KeyTag, meta::RedisType};

impl_simple_meta!(
  /// 有序整型集合结构元数据（对标 Apache Kvrocks SortedintMetadata 26字节 / 紧凑25字节）
  SortedintMeta,
  RedisType::SortedInt,
  KeyTag::SortedIntMeta
);
