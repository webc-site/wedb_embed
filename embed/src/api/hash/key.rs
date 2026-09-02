use crate::key_composer::{KeyComposer, KeyTag, SmallKey, SubkeyComposer};

/// Stack-allocated Hash metadata key without heap allocation.
/// 栈上定长构造 Hash 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::HashMeta.as_slice(), key)
}

/// Stack-allocated Hash field storage key without heap allocation.
/// 栈上定长构造 Hash 字段存储键（零堆分配）
#[inline]
pub fn field(kc: &KeyComposer, key: &[u8], field_name: &[u8]) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::HashData.as_slice(), key, field_name)
}

/// Composes Hash subkey data prefix.
/// 构造 Hash 数据子键前缀字节序列
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::HashData.as_slice(), key)
}

/// Stack-allocated Hash subkey prefix without heap allocation.
/// 栈上定长构造 Hash 数据子键前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::HashData.as_slice(), key)
}

/// Efficient hash field subkey composer with precomputed prefix and buffer reuse.
/// 哈希字段键高效构建器（预计算前缀，原地复用内存零堆分配）
#[derive(Debug, Clone)]
pub struct ItemKeyComposer {
  composer: SubkeyComposer,
}

impl ItemKeyComposer {
  #[inline]
  pub fn new(kc: &KeyComposer, key: &[u8]) -> Self {
    let p = prefix_stack(kc, key);
    Self {
      composer: SubkeyComposer::from_slice(&p),
    }
  }

  #[inline(always)]
  pub fn key_for_field<'a>(&'a mut self, field: &[u8]) -> &'a [u8] {
    self.composer.compose_sub(field)
  }

  #[inline(always)]
  pub fn prefix(&self) -> &[u8] {
    self.composer.prefix()
  }
}
