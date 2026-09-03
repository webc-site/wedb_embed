use std::{
  error::Error as StdError,
  ops::{Bound, Deref},
  result::Result as StdResult,
};

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

/// Read and write interface for an individual keyspace / partition.
/// 单个分区的读写接口 (Keyspace / Partition)
pub trait Partition: Clone + Send + Sync + 'static {
  /// Associated error type for partition operations.
  /// 分区操作的关联错误类型。
  type Error: StdError + Send + Sync + 'static;

  /// Associated value type returned by point get operations.
  /// 单点读取操作返回的值关联类型。
  type Value: Deref<Target = [u8]>;

  /// Associated entry type produced by range iterators.
  /// 区间迭代器产出的条目关联类型。
  type Entry<'a>: KvEntry
  where
    Self: 'a;

  /// Associated bidirectional iterator type.
  /// 双向迭代器关联类型。
  type Iter<'a>: Iterator<Item = StdResult<Self::Entry<'a>, Self::Error>> + DoubleEndedIterator
  where
    Self: 'a;

  /// Retrieves the value associated with the given key.
  /// 获取指定键对应的值。
  fn get(&self, key: &[u8]) -> StdResult<Option<Self::Value>, Self::Error>;

  /// Returns the byte size of the value without retrieving or allocating the value payload.
  /// 获取指定键对应值的字节大小（无需读取或分配完整的 value payload）。
  #[inline]
  fn size_of(&self, key: &[u8]) -> StdResult<Option<usize>, Self::Error> {
    self.get(key).map(|opt| opt.map(|v| v.len()))
  }

  /// Checks if the partition contains the specified key.
  /// 检查分区中是否存在指定键。
  #[inline]
  fn contains_key(&self, key: &[u8]) -> StdResult<bool, Self::Error> {
    self.get(key).map(|opt| opt.is_some())
  }

  /// Checks if the partition contains no entries.
  /// 检查分区是否为空（默认通过首条目探测，复杂度逼近 O(1)/O(log N)，避免全表扫描）。
  #[inline]
  fn is_empty(&self) -> StdResult<bool, Self::Error> {
    self.first_entry().map(|opt| opt.is_none())
  }

  /// Returns the number of entries in the partition.
  /// 返回分区中的条目总数。
  #[inline]
  fn len(&self) -> StdResult<usize, Self::Error> {
    let mut count = 0;
    for item in self.iter() {
      let _ = item?;
      count += 1;
    }
    Ok(count)
  }

  /// Returns an approximate number of entries in the partition in O(1) time.
  /// 以 O(1) 时间复杂度获取分区的近似条目数。
  #[inline]
  fn approximate_len(&self) -> StdResult<usize, Self::Error> {
    self.len()
  }

  /// Inserts a key-value pair into the partition.
  /// 向分区中插入键值对。
  fn insert(&self, key: &[u8], value: &[u8]) -> StdResult<(), Self::Error>;

  /// Removes a key from the partition.
  /// 从分区中删除指定键。
  fn rm(&self, key: &[u8]) -> StdResult<(), Self::Error>;

  /// Removes an item leaving a weak tombstone (safe for keys created once).
  /// 从分区中删除指定键并留下弱墓碑标记（适用于仅写入一次的单次键，如队列/流/单次索引）。
  #[inline]
  fn rm_weak(&self, key: &[u8]) -> StdResult<(), Self::Error> {
    self.rm(key)
  }

  /// Inserts multiple key-value pairs into the partition.
  /// 向分区中批量插入键值对。
  #[inline]
  fn insert_batch<I, K, V>(&self, entries: I) -> StdResult<(), Self::Error>
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
  {
    for (k, v) in entries {
      self.insert(k.as_ref(), v.as_ref())?;
    }
    Ok(())
  }

  /// Removes multiple keys from the partition.
  /// 从分区中批量删除指定键。
  #[inline]
  fn rm_batch<I, K>(&self, keys: I) -> StdResult<(), Self::Error>
  where
    I: IntoIterator<Item = K>,
    K: AsRef<[u8]>,
  {
    for k in keys {
      self.rm(k.as_ref())?;
    }
    Ok(())
  }

  /// Truncates / clears all entries in the partition.
  /// 清空当前分区中的全部物理条目。
  #[inline]
  fn clear(&self) -> StdResult<(), Self::Error> {
    for item in self.iter() {
      let entry = item?;
      self.rm(entry.key().deref())?;
    }
    Ok(())
  }

  /// Returns a bidirectional iterator over all entries in the partition.
  /// 返回遍历分区中所有条目的双向迭代器。
  fn iter(&self) -> Self::Iter<'_>;

  /// Returns a bidirectional iterator over entries matching the given prefix.
  /// 返回遍历指定前缀匹配条目的双向迭代器。
  fn prefix(&self, prefix: &[u8]) -> Self::Iter<'_>;

  /// Returns a bidirectional iterator over entries within the specified range.
  /// 返回遍历指定键区间内条目的双向迭代器。
  fn range(&self, range: (Bound<&[u8]>, Bound<&[u8]>)) -> Self::Iter<'_>;

  /// Returns the first key-value entry in the partition (minimum key).
  /// 获取分区中的第一个键值对条目（键最小的条目）。
  #[inline]
  fn first_entry(&self) -> StdResult<Option<Self::Entry<'_>>, Self::Error> {
    self.iter().next().transpose()
  }

  /// Returns the last key-value entry in the partition (maximum key).
  /// 获取分区中的最后一个键值对条目（键最大的条目）。
  #[inline]
  fn last_entry(&self) -> StdResult<Option<Self::Entry<'_>>, Self::Error> {
    self.iter().next_back().transpose()
  }

  /// Returns true if the partition enables key-value separation for large blobs.
  /// 返回当前分区是否启用了大 Value 键值分离存储 (KV separation / Blob)。
  #[inline]
  fn is_kv_separated(&self) -> bool {
    false
  }

  /// Returns the unreferenced blob bytes on disk for this partition.
  /// 返回当前分区未引用的陈旧 Blob 磁盘占用字节数。
  #[inline]
  fn fragmented_blob_bytes(&self) -> u64 {
    0
  }

  /// Returns the approximate physical disk usage in bytes for this partition.
  /// 获取当前分区的近似物理磁盘占用字节数。
  #[inline]
  fn disk_space(&self) -> StdResult<u64, Self::Error> {
    Ok(0)
  }

  /// Returns the number of SST tables in the partition.
  /// 返回当前分区的 SST 表文件总数。
  #[inline]
  fn table_count(&self) -> usize {
    0
  }

  /// Returns the number of blob files in the partition.
  /// 返回当前分区的 Blob 文件总数。
  #[inline]
  fn blob_file_count(&self) -> usize {
    0
  }

  /// Triggers a manual compaction / garbage collection for this partition.
  /// 触发当前分区的全量压缩与空间整理 (Major Compaction / GC)。
  #[inline]
  fn compact(&self) -> StdResult<(), Self::Error> {
    Ok(())
  }
}

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
    for (k, v) in entries {
      self.insert(partition, k.as_ref(), v.as_ref());
    }
  }

  /// Queues multiple keys for removal from the batch for a specific partition.
  /// 向批次中批量添加属于同一分区的键删除操作。
  #[inline]
  fn rm_batch<I, K>(&mut self, partition: &Self::Partition, keys: I)
  where
    I: IntoIterator<Item = K>,
    K: AsRef<[u8]>,
  {
    for k in keys {
      self.rm(partition, k.as_ref());
    }
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
