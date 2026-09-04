use std::{error::Error as StdError, result::Result as StdResult};

use crate::partition::Partition;

/// Atomic write batch abstraction across partitions.
/// 跨分区的批量原子写入抽象 (WriteBatch)
pub trait Batch: Send {
  /// Associated error type for batch operations.
  /// 批处理操作的关联错误类型。
  type Error: StdError + Send + Sync + 'static;

  /// Associated partition type targeting operations in this batch.
  /// 当前批次操作所针对的分区关联类型。
  type Partition: Partition<Error = Self::Error>;

  /// Queues an insertion operation into the batch.
  /// 向批次中添加键值插入操作。
  fn insert(&mut self, partition: &Self::Partition, key: &[u8], value: &[u8]);

  /// Queues a removal operation into the batch.
  /// 向批次中添加键删除操作。
  fn rm(&mut self, partition: &Self::Partition, key: &[u8]);

  /// Queues a weak tombstone removal operation into the batch.
  /// 向批次中添加弱墓碑键删除操作（适用于仅写入过一次的单次键）。
  #[inline]
  fn rm_weak(&mut self, partition: &Self::Partition, key: &[u8]) {
    self.rm(partition, key);
  }

  /// Queues multiple key-value pairs into the batch for a specific partition.
  /// 向批次中批量添加属于同一分区的键值对插入操作。
  #[inline]
  fn insert_batch<I, K, V>(&mut self, partition: &Self::Partition, entries: I)
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
  {
    entries
      .into_iter()
      .for_each(|(k, v)| self.insert(partition, k.as_ref(), v.as_ref()));
  }

  /// Queues multiple keys for removal from the batch for a specific partition.
  /// 向批次中批量添加属于同一分区的键删除操作。
  #[inline]
  fn rm_batch<I, K>(&mut self, partition: &Self::Partition, keys: I)
  where
    I: IntoIterator<Item = K>,
    K: AsRef<[u8]>,
  {
    keys
      .into_iter()
      .for_each(|k| self.rm(partition, k.as_ref()));
  }

  /// Returns the number of queued operations in the write batch.
  /// 返回当前批次中已排队的写入操作数。
  #[inline]
  fn len(&self) -> usize {
    0
  }

  /// Checks if the write batch contains no operations.
  /// 检查当前写入批次是否为空。
  #[inline]
  fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Atomically commits all queued write operations to the storage engine.
  /// 原子提交批次中的全部写入操作至底层存储引擎。
  fn commit(self) -> StdResult<(), Self::Error>;
}
