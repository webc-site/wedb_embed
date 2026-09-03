use crate::key_composer::{KeyComposer, KeyTag, SmallKey, SubkeyComposer};

/// Stack-allocated List metadata key without heap allocation.
/// 栈上定长构造 List 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::ListMeta.as_slice(), key)
}

/// Stack-allocated List data subkey prefix without heap allocation.
/// 栈上定长构造 List 数据子键前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ListData.as_slice(), key)
}

/// Composes List data subkey prefix.
/// 构造 List 数据子键前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::ListData.as_slice(), key)
}

/// Stack-allocated List element key with big-endian order-preserving encoding.
/// 栈上定长构造 List 元素存储键（零堆分配，大端序紧凑保序二进制编码）
#[inline]
pub fn item(kc: &KeyComposer, key: &[u8], idx: u64) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::ListData.as_slice(), key, &idx.to_be_bytes())
}

/// Fast List item key composer with precalculated prefix for zero-allocation lookups.
/// 列表项键快速生成器（单次分配前缀，后续寻址零堆内存分配）
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
  pub fn key_for_idx(&mut self, idx: u64) -> &[u8] {
    self.composer.compose_sub_u64_be(idx)
  }

  #[inline(always)]
  pub fn prefix(&self) -> &[u8] {
    self.composer.prefix()
  }
}
