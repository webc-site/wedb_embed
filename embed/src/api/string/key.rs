use crate::key_composer::{INLINE_CAP, KeyComposer, KeyTag, SmallKey};

/// Composes storage key or prefix.
/// 栈分配零堆分配构造 String 物理键（带类型前缀 KeyTag::RawString = 0x00）
#[inline(always)]
pub fn raw(kc: &KeyComposer, key: &[u8]) -> SmallKey {
  if kc.is_default() {
    let total_len = 4 + key.len();
    if total_len <= INLINE_CAP {
      let mut buf = [0u8; INLINE_CAP];
      buf[0] = 0;
      buf[1] = 0;
      buf[2] = 0;
      buf[3] = 0;
      buf[4..total_len].copy_from_slice(key);
      return SmallKey::Inline {
        buf,
        len: total_len as u8,
      };
    }
  }
  kc.compose_meta_key_stack(KeyTag::RawString.as_slice(), key)
}

/// Composes storage key or prefix.
/// 构造 String 物理键字节序列
#[inline]
pub fn raw_bytes(kc: &KeyComposer, key: &[u8]) -> Vec<u8> {
  let mut v = Vec::with_capacity(kc.scope_prefix_len() + 1 + key.len());
  kc.compose_meta_key_into(KeyTag::RawString.as_slice(), key, &mut v);
  v
}

/// Composes storage key or prefix.
/// 构造 String 物理前缀
#[inline]
pub fn prefix(kc: &KeyComposer) -> Vec<u8> {
  kc.compose_meta_prefix(KeyTag::RawString.as_slice())
}

/// Composes storage key or prefix.
/// 栈分配零堆分配构造 String 物理前缀
#[inline]
pub fn prefix_stack(kc: &KeyComposer) -> SmallKey {
  kc.compose_meta_prefix_stack(KeyTag::RawString.as_slice())
}
