use std::{ops::Bound, sync::Arc};

use crate::{
  engine::{Engine, Partition},
  error::{Error, Result},
  key_composer::{
    KeyComposer, SmallKey, encode_catalog_db_key_fixed, encode_catalog_ns_prefix_fixed,
  },
  wedb::{
    Db, DbBatch, Dbs, IntoOptId, WeDb,
    core::{WeDbInner, activate_db_impl, namespace_rm_impl, next_db_id_impl},
  },
};

/// Namespace handle for tenant data isolation (u64 numerical identifier)
/// 命名空间句柄对象（纯数字编号 u64 隔离）
pub struct Namespace<E: Engine> {
  pub id: u64,
  pub(crate) inner: Arc<WeDbInner<E>>,
}

impl<E: Engine> Clone for Namespace<E> {
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      id: self.id,
      inner: self.inner.clone(),
    }
  }
}

impl<E: Engine> Namespace<E> {
  /// Numerical namespace ID
  /// 命名空间数字 ID
  #[inline(always)]
  pub const fn id(&self) -> u64 {
    self.id
  }

  /// Get underlying WeDb reference
  /// 获取所属 WeDb 实例句柄
  #[inline(always)]
  pub fn wedb(&self) -> WeDb<E> {
    WeDb {
      inner: self.inner.clone(),
    }
  }

  /// Create a batch write handle
  /// 创建批量写入句柄
  #[inline(always)]
  pub fn batch(&self) -> DbBatch<E> {
    DbBatch::new(
      self.inner.data.clone(),
      self.inner.meta.clone(),
      self.inner.engine.batch(),
    )
  }

  /// Create a batch write handle with pre-allocated capacity
  /// 创建具有预分配容量槽位的批量写入句柄
  #[inline(always)]
  pub fn batch_with_capacity(&self, capacity: usize) -> DbBatch<E> {
    DbBatch::new(
      self.inner.data.clone(),
      self.inner.meta.clone(),
      self.inner.engine.batch_with_capacity(capacity),
    )
  }

  /// Stream iterate all existing databases in this namespace starting from begin ID
  /// 纯流式迭代当前命名空间下实际存在的数据库索引（支持从指定起始 begin ID 开始遍历）
  #[inline]
  pub fn iter(&self, begin: u64) -> Dbs<'_, E> {
    let mut cat_prefix_buf = [0u8; 11];
    let cat_len = encode_catalog_ns_prefix_fixed(self.id, &mut cat_prefix_buf);
    let cat_prefix = &cat_prefix_buf[..cat_len];

    let mut db_key_buf = [0u8; 20];
    let start_key: &[u8] = if begin == 0 {
      cat_prefix
    } else {
      let db_len = encode_catalog_db_key_fixed(self.id, begin, &mut db_key_buf);
      &db_key_buf[..db_len]
    };

    let iter = self
      .inner
      .meta
      .range((Bound::Included(start_key), Bound::Unbounded));
    Dbs {
      prefix: SmallKey::from_slice(cat_prefix),
      iter,
    }
  }
}

impl<E: Engine> Namespace<E>
where
  Error: From<E::Error>,
{
  /// Open existing database in current namespace by numerical ID, or allocate a new auto-increment database if `None` is passed.
  /// 打开当前命名空间下指定编号的数据库；若传入 `None`，则自动分配下一个递增自增 ID 并持久化至 Catalog 目录。
  #[inline]
  pub fn db(&self, id: impl IntoOptId) -> Result<Db<E>> {
    let db_id = match id.into_opt_id() {
      Some(id) => id,
      None => next_db_id_impl::<E>(&self.inner.meta, &self.inner.ns_lock, self.id)?,
    };
    activate_db_impl::<E>(&self.inner.meta, self.id, db_id)?;
    Ok(Db {
      kc: KeyComposer::new(self.id, db_id),
      inner: self.inner.clone(),
    })
  }

  /// Delete current namespace cascadingly (clears all databases, data, metadata, and auto-increment ID counter in this namespace; deregisters from the catalog so it will no longer appear when iterating)
  /// 级联删除当前命名空间（清除该命名空间下的所有数据库、业务数据、元数据与发号器，并从 Catalog 目录中注销，迭代时不复现）
  #[inline]
  pub fn rm(&self) -> Result<u64> {
    let count = namespace_rm_impl::<E>(
      &self.inner.data,
      &self.inner.meta,
      &self.inner.engine,
      self.id,
    )?;
    Ok(count)
  }
}
