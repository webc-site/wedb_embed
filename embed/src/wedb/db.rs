use std::sync::Arc;

use crate::{
  engine::Engine,
  error::{Error, Result},
  key_composer::KeyComposer,
  wedb::{
    DbBatch, IntoOptId, Namespace, WeDb,
    core::{WeDbInner, db_rm_impl},
  },
};

/// Specific database operations handle (representing specific namespace_id and db_id)
/// 具体数据库操作句柄（代表特定的 namespace_id 与 db_id）
///
/// Full Redis API capabilities implemented as inherent methods on Db
/// 具备完整的 Redis API 能力（所有命令直接作为固有方法实现于 Db 上）
pub struct Db<E: Engine> {
  pub kc: KeyComposer,
  pub(crate) inner: Arc<WeDbInner<E>>,
}

impl<E: Engine> Clone for Db<E> {
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      kc: self.kc,
      inner: self.inner.clone(),
    }
  }
}

impl<E: Engine> Db<E> {
  /// Underlying data partition reference
  /// 底层业务数据 Partition 引用
  #[inline(always)]
  pub fn data(&self) -> &E::Partition {
    &self.inner.data
  }

  /// Underlying metadata partition reference
  /// 底层元数据 Partition 引用
  #[inline(always)]
  pub fn meta(&self) -> &E::Partition {
    &self.inner.meta
  }

  /// Key composer instance
  /// 键编排器
  #[inline(always)]
  pub const fn kc(&self) -> KeyComposer {
    self.kc
  }

  /// Underlying storage engine reference
  /// 底层存储引擎引用
  #[inline(always)]
  pub fn engine(&self) -> &E {
    &self.inner.engine
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

  /// Numerical namespace ID
  /// 获取当前命名空间 ID
  #[inline(always)]
  pub const fn ns_id(&self) -> u64 {
    self.kc.ns_id()
  }

  /// Numerical database ID
  /// 获取当前数据库编号
  #[inline(always)]
  pub const fn id(&self) -> u64 {
    self.kc.db()
  }

  /// Whether this is the default database in default namespace (ns == 0 && db == 0)
  /// 是否为默认命名空间的默认数据库 (ns == 0 && db == 0)
  #[inline(always)]
  pub const fn is_default(&self) -> bool {
    self.kc.is_default()
  }

  /// Get handle of the namespace this database belongs to
  /// 获取当前 Db 所属的命名空间句柄
  #[inline(always)]
  pub fn ns(&self) -> Namespace<E> {
    Namespace {
      id: self.ns_id(),
      inner: self.inner.clone(),
    }
  }

  /// Get underlying WeDb instance
  /// 获取所属 WeDb 实例句柄
  #[inline(always)]
  pub fn wedb(&self) -> WeDb<E> {
    WeDb {
      inner: self.inner.clone(),
    }
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  /// Select/switch to another database within the same namespace (Redis SELECT db_id)
  /// 切换至当前命名空间下的指定数据库（对标 Redis SELECT 命令）
  #[inline]
  pub fn select(&self, db_id: impl IntoOptId) -> Result<Db<E>> {
    self.ns().db(db_id)
  }

  /// Switch to specified namespace while preserving current database ID
  /// 切换至指定命名空间并保持当前数据库编号
  #[inline]
  pub fn with_ns(&self, ns_id: impl IntoOptId) -> Result<Db<E>> {
    self.wedb().ns(ns_id)?.db(self.id())
  }

  /// Delete current database cascadingly (clears all business data and metadata for this db, and deregisters it from the catalog so it will no longer appear when iterating)
  /// 级联删除当前数据库（清除当前 db_id 的所有业务数据与元数据，并从 Catalog 目录中注销，迭代时不复现）
  #[inline]
  pub fn rm(&self) -> Result<u64> {
    let count = db_rm_impl::<E>(
      &self.inner.data,
      &self.inner.meta,
      &self.inner.engine,
      self.ns_id(),
      self.id(),
    )?;
    Ok(count)
  }
}
