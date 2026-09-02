use crate::key_composer::{KeyComposer, KeyTag, SmallKey, SubkeyComposer};

/// Stack-allocated ZSet metadata key without heap allocation.
/// 栈上定长构造 ZSet 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::ZSetMeta.as_slice(), key)
}

/// Stack-allocated ZSet member-to-score data key without heap allocation.
/// 栈上定长构造 ZSet 成员->分数数据键（零堆分配）
#[inline]
pub fn member(kc: &KeyComposer, key: &[u8], member_bytes: &[u8]) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::ZSetData.as_slice(), key, member_bytes)
}

/// Composes ZSet data subkey prefix.
/// 构造 ZSet 数据子键前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::ZSetData.as_slice(), key)
}

/// Stack-allocated ZSet data subkey prefix without heap allocation.
/// 栈上定长构造 ZSet 数据子键前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ZSetData.as_slice(), key)
}

/// Stack-allocated ZSet score index key (format: `[prefix][score_u64_be][member]`).
/// 栈上定长构造 ZSet 分数索引键（零堆分配，编码格式：`[prefix][score_u64_be][member]`）
#[inline]
pub fn score(kc: &KeyComposer, key: &[u8], score_val: f64, member_bytes: &[u8]) -> SmallKey {
  let score_bits = encode_sortable_f64(score_val);
  kc.compose_subkey2_stack(
    KeyTag::ZSetScore.as_slice(),
    key,
    &score_bits.to_be_bytes(),
    member_bytes,
  )
}

/// Composes ZSet score index subkey prefix.
/// 构造 ZSet 分数索引子键前缀
#[inline]
pub fn score_prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::ZSetScore.as_slice(), key)
}

/// Stack-allocated ZSet score index subkey prefix without heap allocation.
/// 栈上定长构造 ZSet 分数索引子键前缀（零堆分配）
#[inline]
pub fn score_prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::ZSetScore.as_slice(), key)
}

pub use member as compose_zset_key;
pub use score as compose_zset_score_key;

/// Encodes f64 score into order-preserving u64 integer.
/// 将 f64 浮点分数编码为保序 u64 整数
pub use crate::meta::{
  decode_sortable_f64_u64 as decode_sortable_f64, encode_sortable_f64_u64 as encode_sortable_f64,
};

/// Fast ZSet key composer with dual precalculated prefixes for zero-allocation lookups.
/// 有序集合键快速生成器（单次分配双前缀，后续构建 member 与 score 键零堆内存分配）
#[derive(Debug, Clone)]
pub struct ItemKeyComposer {
  member_composer: SubkeyComposer,
  score_buf: Vec<u8>,
  score_prefix_len: usize,
}

impl ItemKeyComposer {
  #[inline]
  pub fn new(kc: &KeyComposer, key: &[u8]) -> Self {
    let mp = prefix_stack(kc, key);
    let sp = score_prefix_stack(kc, key);
    let score_prefix_len = sp.len();
    Self {
      member_composer: SubkeyComposer::from_slice(&mp),
      score_buf: sp.to_vec(),
      score_prefix_len,
    }
  }

  #[inline(always)]
  pub fn key_for_member<'a>(&'a mut self, member: &[u8]) -> &'a [u8] {
    self.member_composer.compose_sub(member)
  }

  #[inline(always)]
  pub fn key_for_score<'a>(&'a mut self, score_val: f64, member: &[u8]) -> &'a [u8] {
    let score_bits = encode_sortable_f64(score_val);
    self.score_buf.truncate(self.score_prefix_len);
    self.score_buf.extend_from_slice(&score_bits.to_be_bytes());
    self.score_buf.extend_from_slice(member);
    &self.score_buf
  }

  #[inline(always)]
  pub fn member_prefix(&self) -> &[u8] {
    self.member_composer.prefix()
  }

  #[inline(always)]
  pub fn score_prefix(&self) -> &[u8] {
    &self.score_buf[..self.score_prefix_len]
  }
}
