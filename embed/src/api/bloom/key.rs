use crate::key_composer::{KeyComposer, KeyTag, SmallKey};

/// Composes storage key or prefix.
/// 栈上定长构造 Bloom Filter 元数据键（零堆分配）
#[inline]
pub fn bloom_meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::BloomMeta.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Bloom Filter 分块数据键（零堆分配，filter_idx 为 2 字节大端序）
#[inline]
pub fn bloom_item(kc: &KeyComposer, key: &[u8], filter_idx: u16) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::BloomData.as_slice(), key, &filter_idx.to_be_bytes())
}

/// Composes storage key or prefix.
/// 构造 Bloom Filter 数据前缀
#[inline]
pub fn bloom_prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::BloomData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Bloom Filter 数据前缀（零堆分配）
#[inline]
pub fn bloom_prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::BloomData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Cuckoo Filter 元数据键（零堆分配）
#[inline]
pub fn cuckoo_meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::CuckooMeta.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Cuckoo Filter 分页数据键（零堆分配，filter_idx 2 字节 + page_idx 4 字节）
#[inline]
pub fn cuckoo_page(kc: &KeyComposer, key: &[u8], filter_idx: u16, page_idx: u32) -> SmallKey {
  kc.compose_subkey2_stack(
    KeyTag::CuckooData.as_slice(),
    key,
    &filter_idx.to_be_bytes(),
    &page_idx.to_be_bytes(),
  )
}

/// Composes storage key or prefix.
/// 构造 Cuckoo Filter 数据前缀
#[inline]
pub fn cuckoo_prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::CuckooData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Cuckoo Filter 数据前缀（零堆分配）
#[inline]
pub fn cuckoo_prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::CuckooData.as_slice(), key)
}
