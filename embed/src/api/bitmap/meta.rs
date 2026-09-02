use crate::{impl_simple_meta, key_composer::KeyTag, meta::RedisType};

impl_simple_meta!(
  /// 位图结构元数据（对标 Apache Kvrocks BitmapMetadata）
  BitmapMeta,
  RedisType::Bitmap,
  KeyTag::BitmapMeta
);
