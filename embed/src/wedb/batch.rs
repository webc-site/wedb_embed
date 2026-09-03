use crate::{
  engine::{Batch, Engine},
  error::{Error, Result},
};

/// High-level atomic batch write wrapper bound to a specific Db / WeDb instance.
/// 上层针对特定 Db/WeDb 实例的批量原子写入封装
pub struct DbBatch<E: Engine> {
  pub(crate) data: E::Partition,
  pub(crate) meta: E::Partition,
  pub(crate) inner: E::Batch,
}

impl<E: Engine> DbBatch<E> {
  /// Creates a new `DbBatch` wrapper.
  /// 创建新的 `DbBatch` 批次包装实例。
  #[inline(always)]
  pub fn new(data: E::Partition, meta: E::Partition, inner: E::Batch) -> Self {
    Self { data, meta, inner }
  }

  /// Queues an insertion operation into the data partition.
  /// 向数据分区中排队插入键值对。
  #[inline(always)]
  pub fn insert_data(&mut self, key: &[u8], value: &[u8]) {
    self.inner.insert(&self.data, key, value);
  }

  /// Queues multiple key-value pairs into the data partition.
  /// 向数据分区中批量排队插入键值对。
  #[inline(always)]
  pub fn insert_data_batch<I, K, V>(&mut self, entries: I)
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
  {
    self.inner.insert_batch(&self.data, entries);
  }

  /// Queues an insertion operation into the metadata partition.
  /// 向元数据分区中排队插入键值对。
  #[inline(always)]
  pub fn insert_meta(&mut self, key: &[u8], value: &[u8]) {
    self.inner.insert(&self.meta, key, value);
  }

  /// Queues multiple key-value pairs into the metadata partition.
  /// 向元数据分区中批量排队插入键值对。
  #[inline(always)]
  pub fn insert_meta_batch<I, K, V>(&mut self, entries: I)
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
  {
    self.inner.insert_batch(&self.meta, entries);
  }

  /// Queues a removal operation into the data partition.
  /// 从数据分区中排队删除指定键。
  #[inline(always)]
  pub fn rm_data(&mut self, key: &[u8]) {
    self.inner.rm(&self.data, key);
  }

  /// Queues multiple keys for removal from the data partition.
  /// 从数据分区中批量排队删除指定键。
  #[inline(always)]
  pub fn rm_data_batch<I, K>(&mut self, keys: I)
  where
    I: IntoIterator<Item = K>,
    K: AsRef<[u8]>,
  {
    self.inner.rm_batch(&self.data, keys);
  }

  /// Queues a weak tombstone removal operation into the data partition.
  /// 从数据分区中排队弱墓碑删除指定键（适用于单次写入键）。
  #[inline(always)]
  pub fn rm_weak_data(&mut self, key: &[u8]) {
    self.inner.rm_weak(&self.data, key);
  }

  /// Queues a removal operation into the metadata partition.
  /// 从元数据分区中排队删除指定键。
  #[inline(always)]
  pub fn rm_meta(&mut self, key: &[u8]) {
    self.inner.rm(&self.meta, key);
  }

  /// Queues a weak tombstone removal operation into the metadata partition.
  /// 从元数据分区中排队弱墓碑删除指定键。
  #[inline(always)]
  pub fn rm_weak_meta(&mut self, key: &[u8]) {
    self.inner.rm_weak(&self.meta, key);
  }

  /// Queues an insertion operation into a specified custom partition.
  /// 向指定自定义分区中排队插入键值对。
  #[inline(always)]
  pub fn insert(&mut self, partition: &E::Partition, key: &[u8], value: &[u8]) {
    self.inner.insert(partition, key, value);
  }

  /// Queues a removal operation into a specified custom partition.
  /// 从指定自定义分区中排队删除指定键。
  #[inline(always)]
  pub fn rm(&mut self, partition: &E::Partition, key: &[u8]) {
    self.inner.rm(partition, key);
  }

  /// Queues a weak tombstone removal operation into a specified custom partition.
  /// 从指定自定义分区中排队弱墓碑删除指定键。
  #[inline(always)]
  pub fn rm_weak(&mut self, partition: &E::Partition, key: &[u8]) {
    self.inner.rm_weak(partition, key);
  }

  /// Returns the number of queued operations in the write batch.
  /// 返回当前批次中排队的操作数。
  #[inline(always)]
  pub fn len(&self) -> usize {
    self.inner.len()
  }

  /// Checks if the write batch contains no operations.
  /// 检查当前写入批次是否为空。
  #[inline(always)]
  pub fn is_empty(&self) -> bool {
    self.inner.is_empty()
  }

  /// Returns an immutable reference to the underlying engine batch.
  /// 获取底层引擎 Batch 的不可变引用。
  #[inline(always)]
  pub fn inner(&self) -> &E::Batch {
    &self.inner
  }

  /// Returns a mutable reference to the underlying engine batch.
  /// 获取底层引擎 Batch 的可变引用。
  #[inline(always)]
  pub fn inner_mut(&mut self) -> &mut E::Batch {
    &mut self.inner
  }

  /// Consumes the wrapper and returns the underlying engine batch.
  /// 消耗当前包装并解构成底层引擎 Batch。
  #[inline(always)]
  pub fn into_inner(self) -> E::Batch {
    self.inner
  }

  /// Atomically commits all queued write operations across partitions.
  /// 原子提交当前批次中的全部操作至底层引擎。
  #[inline(always)]
  pub fn commit(self) -> Result<()>
  where
    Error: From<E::Error>,
  {
    Ok(self.inner.commit()?)
  }
}
