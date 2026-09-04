use std::ops::Deref;

/// Key-value entry abstraction for storage items.
/// 存储条目键值对抽象 (UserKey, UserValue)
pub trait KvEntry {
  /// Associated key type implementing `Deref<Target = [u8]>`.
  /// 实现 `Deref<Target = [u8]>` 的键关联类型。
  type Key: Deref<Target = [u8]>;

  /// Associated value type implementing `Deref<Target = [u8]>`.
  /// 实现 `Deref<Target = [u8]>` 的值关联类型。
  type Value: Deref<Target = [u8]>;

  /// Returns a reference to the entry key.
  /// 获取条目的键引用。
  fn key(&self) -> &Self::Key;

  /// Returns a reference to the entry value.
  /// 获取条目的值引用。
  fn value(&self) -> &Self::Value;

  /// Returns the entry key as a byte slice.
  /// 获取条目键的字节切片引用。
  #[inline(always)]
  fn key_ref(&self) -> &[u8] {
    self.key().deref()
  }

  /// Returns the entry value as a byte slice.
  /// 获取条目值的字节切片引用。
  #[inline(always)]
  fn value_ref(&self) -> &[u8] {
    self.value().deref()
  }
}

impl<K, V> KvEntry for (K, V)
where
  K: Deref<Target = [u8]>,
  V: Deref<Target = [u8]>,
{
  type Key = K;
  type Value = V;

  #[inline(always)]
  fn key(&self) -> &Self::Key {
    &self.0
  }

  #[inline(always)]
  fn value(&self) -> &Self::Value {
    &self.1
  }
}

impl<E: KvEntry> KvEntry for &E {
  type Key = E::Key;
  type Value = E::Value;

  #[inline(always)]
  fn key(&self) -> &Self::Key {
    (**self).key()
  }

  #[inline(always)]
  fn value(&self) -> &Self::Value {
    (**self).value()
  }
}
