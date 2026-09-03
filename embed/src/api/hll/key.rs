use crate::key_composer::{KeyComposer, KeyTag, SmallKey};

/// Composes storage key or prefix.
/// 栈上定长构造 HyperLogLog 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::HllMeta.as_slice(), key)
}

/// Composes storage key or prefix.
/// 构造 HyperLogLog 元数据前缀
#[inline]
pub fn meta_prefix(kc: &KeyComposer) -> Vec<u8> {
  kc.compose_meta_prefix(KeyTag::HllMeta.as_slice())
}

/// Composes storage key or prefix.
/// 栈上定长构造 HyperLogLog 稠密原始寄存器数据键（零堆分配）
#[inline]
pub fn raw(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::HllRaw.as_slice(), key)
}
