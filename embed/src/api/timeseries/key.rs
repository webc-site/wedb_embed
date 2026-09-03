use crate::key_composer::{KeyComposer, KeyTag, SmallKey};

/// Stack-allocated TimeSeries metadata key without heap allocation.
/// 栈上定长构造 TimeSeries 元数据键（零堆分配）
#[inline]
pub fn meta(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_meta_key_stack(KeyTag::TimeSeriesMeta.as_slice(), key)
}

/// Composes TimeSeries metadata prefix.
/// 构造 TimeSeries 元数据前缀
#[inline]
pub fn meta_prefix(kc: &KeyComposer) -> Vec<u8> {
  kc.compose_meta_prefix(KeyTag::TimeSeriesMeta.as_slice())
}

/// Composes storage key or prefix.
/// 栈上定长构造 TimeSeries 数据分块键（零堆分配，大端序紧凑保序 8 字节）
#[inline]
pub fn chunk(kc: &KeyComposer, key: &[u8], chunk_time: u64) -> SmallKey {
  kc.compose_subkey_stack(
    KeyTag::TimeSeriesData.as_slice(),
    key,
    &chunk_time.to_be_bytes(),
  )
}

/// Composes storage key or prefix.
/// 构造 TimeSeries 数据前缀
#[inline]
pub fn prefix(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::TimeSeriesData.as_slice(), key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 TimeSeries 数据前缀（零堆分配）
#[inline]
pub fn prefix_stack(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::TimeSeriesData.as_slice(), key)
}

/// Composes TimeSeries metadata prefix.
/// 栈上定长构造 TimeSeries 元数据前缀（零堆分配）
#[inline]
pub fn meta_prefix_stack(kc: &KeyComposer) -> SmallKey {
  kc.compose_meta_prefix_stack(KeyTag::TimeSeriesMeta.as_slice())
}

/// Composes storage key or prefix.
/// 栈上定长构造 TimeSeries 下游规则元数据键（零堆分配，编码：`[prefix][dst_key]`）
#[inline]
pub fn downstream_meta(kc: &KeyComposer, src_key: &[u8], dst_key: &[u8]) -> SmallKey {
  kc.compose_subkey_stack(KeyTag::TimeSeriesData.as_slice(), src_key, dst_key)
}

/// Composes storage key or prefix.
/// 构造 TimeSeries 下游规则前缀
#[inline]
pub fn downstream_prefix(kc: &KeyComposer, src_key: &[u8]) -> Vec<u8> {
  kc.compose_prefix(KeyTag::TimeSeriesData.as_slice(), src_key)
}

/// Composes storage key or prefix.
/// 栈上定长构造 TimeSeries 下游规则前缀（零堆分配）
#[inline]
pub fn downstream_prefix_stack(kc: &KeyComposer, src_key: &[u8]) -> SmallKey {
  kc.compose_prefix_stack(KeyTag::TimeSeriesData.as_slice(), src_key)
}
