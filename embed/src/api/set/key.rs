use crate::key_composer::{KeyComposer, KeyTag, SmallKey, SubkeyComposer};

/// Stack-allocated Set metadata key without heap allocation.
/// 栈上定长构造 Set 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::SetMeta.as_slice(), key)
}

/// Stack-allocated Set data subkey prefix without heap allocation.
/// 栈上定长构造 Set 数据子键前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::SetData.as_slice(), key)
}

/// Composes Set data subkey prefix.
/// 构造 Set 数据子键前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::SetData.as_slice(), key)
}

/// Stack-allocated Set member storage key without heap allocation.
/// 栈上定长构造 Set 成员存储键（零堆分配）
#[inline]
pub fn member(kc: &KeyComposer, key: &[u8], item: &[u8]) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::SetData.as_slice(), key, item)
}

/// Fast Set member key composer with precalculated prefix for zero-allocation lookups.
/// Set 成员键快速构建器（预计算前缀，后续寻址零堆内存分配）
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
  pub fn key_for_member<'a>(&'a mut self, member: &[u8]) -> &'a [u8] {
    self.composer.compose_sub(member)
  }

  #[inline(always)]
  pub fn prefix(&self) -> &[u8] {
    self.composer.prefix()
  }
}
