use std::iter::FusedIterator;

use crate::{
  engine::{Engine, KvEntry, Partition},
  key_composer::{CATALOG_PREFIX, SmallKey, decode_oppv_u64},
  wedb::{Namespace, WeDb},
};

/// Namespace iterator (streams catalog entries for existing namespaces starting from begin ID)
/// 命名空间迭代器（纯流式读取 Catalog 目录，遍历所有实际存在的 Namespace，支持从指定 begin ID 开始）
pub struct Namespaces<'a, E: Engine> {
  pub(crate) wedb: WeDb<E>,
  pub(crate) iter: <E::Partition as Partition>::Iter<'a>,
  pub(crate) last_emitted_ns: Option<u64>,
}

impl<'a, E: Engine> Iterator for Namespaces<'a, E> {
  type Item = Namespace<E>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    for item in self.iter.by_ref() {
      let entry = item.ok()?;
      let k = entry.key();
      if !k.starts_with(CATALOG_PREFIX) {
        return None;
      }
      let remain = &k[CATALOG_PREFIX.len()..];
      if let Some((ns_id, _consumed)) = decode_oppv_u64(remain)
        && Some(ns_id) != self.last_emitted_ns
      {
        self.last_emitted_ns = Some(ns_id);
        return Some(Namespace {
          id: ns_id,
          inner: self.wedb.inner.clone(),
        });
      }
    }
    None
  }
}

impl<'a, E: Engine> FusedIterator for Namespaces<'a, E> {}

/// Database iterator (streams catalog entries for existing db_ids in a namespace starting from begin ID)
/// 数据库索引迭代器（纯流式读取指定 Namespace 下实际存在的 db_id，支持从指定 begin ID 开始）
pub struct Dbs<'a, E: Engine> {
  pub(crate) prefix: SmallKey,
  pub(crate) iter: <E::Partition as Partition>::Iter<'a>,
}

impl<'a, E: Engine> Iterator for Dbs<'a, E> {
  type Item = u64;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    for item in self.iter.by_ref() {
      let entry = item.ok()?;
      let k = entry.key();
      if !k.starts_with(self.prefix.as_slice()) {
        return None;
      }
      let remain = &k[self.prefix.len()..];
      if let Some((db_id, _consumed)) = decode_oppv_u64(remain) {
        return Some(db_id);
      }
    }
    None
  }
}

impl<'a, E: Engine> FusedIterator for Dbs<'a, E> {}
