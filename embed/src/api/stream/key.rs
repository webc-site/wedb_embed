use crate::key_composer::{KeyComposer, KeyTag, SmallKey, encode_oppv_u64};

/// Stack-allocated Stream metadata key without heap allocation.
/// 栈上定长构造 Stream 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::StreamMeta.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 消息项数据键（零堆分配，编码格式：`[prefix][ms_u64_be][seq_u64_be]`）
#[inline]
pub fn item(kc: &KeyComposer, key: &[u8], ms: u64, seq: u64) -> SmallKey {
  kc.compose_subkey2_stack(
    KeyTag::StreamData.as_slice(),
    key,
    &ms.to_be_bytes(),
    &seq.to_be_bytes(),
  )
}

/// Composes storage key or prefix.
/// 构造 Stream 消息项数据前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::StreamData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 消息项数据前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::StreamData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 消费组元数据键（零堆分配，编码格式：`[prefix][group]`）
#[inline]
pub fn group_meta(kc: &KeyComposer, key: &[u8], group: &[u8]) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::StreamGroup.as_slice(), key, group)
}

/// Composes storage key or prefix.
/// 构造 Stream 消费组数据前缀
#[inline]
pub fn group_prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::StreamGroup.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 消费组数据前缀（零堆分配）
#[inline]
pub fn group_prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::StreamGroup.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 消费者元数据键（零堆分配，前缀无关多层编码：`[prefix][oppv(len(group))][group][consumer]`）
#[inline]
pub fn consumer_meta(kc: &KeyComposer, key: &[u8], group: &[u8], consumer: &[u8]) -> SmallKey {
  kc.compose_oppv_subkey_stack(KeyTag::StreamConsumer.as_slice(), key, group, consumer)
}

/// Composes storage key or prefix.
/// 构造指定消费组下的消费者数据前缀
#[inline]
pub fn consumer_prefix(kc: &KeyComposer, key: &[u8], group: &[u8]) -> Vec<u8> {
  let mut v = Vec::with_capacity(kc.scope_prefix_len() + 1 + 9 + key.len() + 9 + group.len());
  kc.encode_scope_prefix(&mut v);
  v.extend_from_slice(KeyTag::StreamConsumer.as_slice());
  encode_oppv_u64(key.len() as u64, &mut v);
  v.extend_from_slice(key);
  encode_oppv_u64(group.len() as u64, &mut v);
  v.extend_from_slice(group);
  v
}

/// Composes storage key or prefix.
/// 构造 Stream 所有消费者数据前缀
#[inline]
pub fn consumer_prefix_all(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::StreamConsumer.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 所有消费者数据前缀（零堆分配）
#[inline]
pub fn consumer_prefix_all_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::StreamConsumer.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream PEL 项数据键（零堆分配，编码格式：`[prefix][oppv(len(group))][group][ms_u64_be][seq_u64_be]`）
#[inline]
pub fn pel_item(kc: &KeyComposer, key: &[u8], group: &[u8], ms: u64, seq: u64) -> SmallKey {
  let mut sub = [0u8; 16];
  sub[0..8].copy_from_slice(&ms.to_be_bytes());
  sub[8..16].copy_from_slice(&seq.to_be_bytes());
  kc.compose_oppv_subkey_stack(KeyTag::StreamPel.as_slice(), key, group, &sub)
}

/// Composes storage key or prefix.
/// 构造指定消费组下的 PEL 数据前缀
#[inline]
pub fn pel_prefix(kc: &KeyComposer, key: &[u8], group: &[u8]) -> Vec<u8> {
  let mut v = Vec::with_capacity(kc.scope_prefix_len() + 1 + 9 + key.len() + 9 + group.len());
  kc.encode_scope_prefix(&mut v);
  v.extend_from_slice(KeyTag::StreamPel.as_slice());
  encode_oppv_u64(key.len() as u64, &mut v);
  v.extend_from_slice(key);
  encode_oppv_u64(group.len() as u64, &mut v);
  v.extend_from_slice(group);
  v
}

/// Composes storage key or prefix.
/// 构造 Stream 所有 PEL 数据前缀
#[inline]
pub fn pel_prefix_all(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::StreamPel.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 Stream 所有 PEL 数据前缀（零堆分配）
#[inline]
pub fn pel_prefix_all_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::StreamPel.as_slice(), key)
}
