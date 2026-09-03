use crate::key_composer::{KeyComposer, KeyTag, SmallKey};

/// Stack-allocated Bitmap metadata key without heap allocation.
/// 栈上定长构造 Bitmap 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::BitmapMeta.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Bitmap 分段数据键（零堆分配，大端序紧凑保序 4 字节）
#[inline]
pub fn segment(kc: &KeyComposer, key: &[u8], seg_idx: u32) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::BitmapData.as_slice(), key, &seg_idx.to_be_bytes())
}

/// Composes storage key or prefix.
/// 构造 Bitmap 数据前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::BitmapData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Bitmap 数据前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::BitmapData.as_slice(), key)
}
