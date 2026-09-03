use crate::key_composer::{KeyComposer, KeyTag, SmallKey, SubkeyComposer};

/// Stack-allocated SortedInt metadata key without heap allocation.
/// 栈上定长构造 SortedInt 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::SortedIntMeta.as_slice(), key)
}

/// Stack-allocated SortedInt element key based on prefix and id.
/// 栈上定长构造 SortedInt 元素键（零堆分配，基于 prefix 和 id）
#[inline]
pub fn item(prefix: &[u8], id: u64) -> SmallKey {
  let mut sk = SmallKey::from_slice(prefix);
  sk.extend_from_slice(&id.to_be_bytes());
  sk
}

/// Stack-allocated SortedInt element key based on kc, key, and id.
/// 栈上定长构造 SortedInt 元素键（零堆分配，基于 kc, key, id）
#[inline]
pub fn key(kc: &KeyComposer, key: &[u8], id: u64) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::SortedIntData.as_slice(), key, &id.to_be_bytes())
}

/// Composes SortedInt data prefix.
/// 构造 SortedInt 数据前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::SortedIntData.as_slice(), key)
}

/// Stack-allocated SortedInt data prefix without heap allocation.
/// 栈上定长构造 SortedInt 数据前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::SortedIntData.as_slice(), key)
}

/// Fast SortedInt key composer with precalculated prefix for zero-allocation lookups.
/// 有序整数键快速生成器（单次分配前缀，后续构建 id 键零堆内存分配）
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
  pub fn key_for_id(&mut self, id: u64) -> &[u8] {
    self.composer.compose_sub_u64_be(id)
  }

  #[inline(always)]
  pub fn prefix(&self) -> &[u8] {
    self.composer.prefix()
  }
}
