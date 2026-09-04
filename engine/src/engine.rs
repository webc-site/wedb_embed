use std::{error::Error as StdError, result::Result as StdResult};

use crate::{batch::Batch, partition::Partition};

/// Storage engine abstraction providing partitioned storage and atomic batch writes.
/// 通用底层存储引擎 Trait (Database / Storage Engine)
pub trait Engine: Send + Sync + 'static {
  /// Associated error type for engine operations.
  /// 存储引擎操作的关联错误类型。
  type Error: StdError + Send + Sync + 'static;

  /// Associated partition type managed by the engine.
  /// 引擎所管理的分区关联类型。
  type Partition: Partition<Error = Self::Error>;

  /// Associated atomic write batch type created by the engine.
  /// 引擎所创建的原子写批次关联类型。
  type Batch: Batch<Partition = Self::Partition, Error = Self::Error>;

  /// Opens or creates a named partition (Keyspace).
  /// 打开或创建指定名称的分区 (Keyspace)。
  fn partition(&self, name: &str) -> StdResult<Self::Partition, Self::Error>;

  /// Checks if a named partition exists in the storage engine.
  /// 检查指定名称的分区是否存在。
  #[inline]
  fn partition_exists(&self, name: &str) -> bool {
    self.partition(name).is_ok()
  }

  /// Lists all partition names in the storage engine.
  /// 列出存储引擎中的所有分区名称。
  #[inline]
  fn list_partitions(&self) -> StdResult<Vec<String>, Self::Error> {
    Ok(Vec::new())
  }

  /// Destroys the partition, removing all data associated with it.
  /// 删除指定分区并回收其物理存储资源。
  #[inline]
  fn rm_partition(&self, _partition: &Self::Partition) -> StdResult<(), Self::Error> {
    Ok(())
  }

  /// Returns the total write buffer memory size (active + sealed memtables) across all partitions in bytes.
  /// 返回当前存储引擎所有分区的总写入缓冲区内存占用字节数 (Active + Sealed Memtables)。
  #[inline]
  fn write_buffer_size(&self) -> u64 {
    0
  }

  /// Returns the current block cache memory usage in bytes.
  /// 获取当前块缓存占用的内存字节数。
  #[inline]
  fn cache_size(&self) -> u64 {
    0
  }

  /// Returns the configured block cache capacity in bytes.
  /// 获取配置的块缓存容量字节数。
  #[inline]
  fn cache_capacity(&self) -> u64 {
    0
  }

  /// Returns the number of pending memtable flush tasks.
  /// 获取排队等待落盘的 Memtable 刷盘任务数。
  #[inline]
  fn outstanding_flushes(&self) -> usize {
    0
  }

  /// Returns the number of active compactions currently running.
  /// 获取当前正在运行的压缩任务数。
  #[inline]
  fn active_compactions(&self) -> usize {
    0
  }

  /// Returns the number of completed compactions.
  /// 获取已完成的压缩任务总数。
  #[inline]
  fn compactions_completed(&self) -> usize {
    0
  }

  /// Returns the number of journal files on disk.
  /// 获取磁盘上的 WAL 日志文件数量。
  #[inline]
  fn journal_count(&self) -> usize {
    0
  }

  /// Returns the disk space usage of the write-ahead journal in bytes.
  /// 获取 WAL 日志占用的磁盘字节数。
  #[inline]
  fn journal_disk_space(&self) -> StdResult<u64, Self::Error> {
    Ok(0)
  }

  /// Creates a new atomic write batch.
  /// 创建新的原子批量写入批次。
  fn batch(&self) -> Self::Batch;

  /// Creates a new atomic write batch with pre-allocated capacity.
  /// 创建具有预分配容量槽位的新原子批量写入批次。
  #[inline]
  fn batch_with_capacity(&self, _capacity: usize) -> Self::Batch {
    self.batch()
  }

  /// Persists in-memory and write-ahead logs to durable storage.
  /// 将内存与 WAL 日志刷盘持久化到持久存储介质。
  fn persist(&self) -> StdResult<(), Self::Error>;

  /// Returns the approximate physical disk usage in bytes for the entire storage engine.
  /// 获取整个存储引擎的近似物理磁盘占用字节数。
  #[inline]
  fn disk_space(&self) -> StdResult<u64, Self::Error> {
    Ok(0)
  }

  /// Triggers a manual compaction / garbage collection across all partitions.
  /// 触发所有分区的全量压缩与空间整理 (Major Compaction / GC)。
  #[inline]
  fn compact(&self) -> StdResult<(), Self::Error> {
    Ok(())
  }
}
