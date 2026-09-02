use crate::{impl_simple_meta, key_composer::KeyTag, meta::RedisType};

impl_simple_meta!(
  /// 集合结构元数据（对标 Apache Kvrocks SetMetadata 26字节 / 紧凑25字节）
  SetMeta,
  RedisType::Set,
  KeyTag::SetMeta
);
