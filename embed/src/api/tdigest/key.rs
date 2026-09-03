use crate::key_composer::{KeyComposer, KeyTag, SmallKey};

/// Stack-allocated TDigest metadata key without heap allocation.
/// 栈上定长构造 TDigest 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::TDigestMeta.as_slice(), key)
}

/// Composes TDigest metadata prefix.
/// 构造 TDigest 元数据前缀
#[inline]
pub fn meta_prefix(kc: &KeyComposer) -> Vec<u8> {
  kc.compose_meta_prefix(KeyTag::TDigestMeta.as_slice())
}

/// Stack-allocated TDigest unmerged buffer chunk key without heap allocation.
/// 栈上定长构造 TDigest 未合并数据缓冲块键（零堆分配，大端序紧凑保序 4 字节）
#[inline]
pub fn buffer_chunk(kc: &KeyComposer, key: &[u8], chunk_idx: u32) -> SmallKey {
  kc.compose_subkey_stack(
    KeyTag::TDigestData.as_slice(),
    key,
    &chunk_idx.to_be_bytes(),
  )
}

/// Composes TDigest data prefix.
/// 构造 TDigest 数据前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::TDigestData.as_slice(), key)
}

/// Stack-allocated TDigest data prefix without heap allocation.
/// 栈上定长构造 TDigest 数据前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::TDigestData.as_slice(), key)
}
