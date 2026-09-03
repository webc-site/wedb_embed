use crate::key_composer::{KeyComposer, KeyTag, SmallKey};

/// Stack-allocated JSON metadata key without heap allocation.
/// 栈上定长构造 JSON 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::JsonMeta.as_slice(), key)
}

/// Composes storage key or prefix.
/// 构造 JSON 数据前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::JsonData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 JSON 数据键与前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::JsonData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 构造 JSON 元数据前缀
#[inline]
pub fn meta_prefix(kc: &KeyComposer) -> Vec<u8> {
  kc.compose_meta_prefix(KeyTag::JsonMeta.as_slice())
}

/// Composes storage key or prefix.
/// 栈上定长构造 JSON 元数据前缀（零堆分配）
#[inline]
pub fn meta_prefix_stack(kc: &KeyComposer) -> SmallKey {
  kc.compose_meta_prefix_stack(KeyTag::JsonMeta.as_slice())
}
