use std::{
  error::Error as StdError,
  ops::{Bound, Deref},
  result::Result as StdResult,
};

use crate::entry::KvEntry;

/// Read and write interface for an individual keyspace / partition.
/// 单个分区的读写及运维接口 (Keyspace / Partition)
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
  /// 检查分区是否为空（默认通过首条目探测，避免全表扫描）。
  #[inline]
  fn is_empty(&self) -> StdResult<bool, Self::Error> {
    self.first_entry().map(|opt| opt.is_none())
  }

  /// Returns the number of entries in the partition.
  /// 返回分区中的条目总数。
  #[inline]
  fn len(&self) -> StdResult<usize, Self::Error> {
    self
      .iter()
      .try_fold(0usize, |count, item| item.map(|_| count + 1))
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
    entries
      .into_iter()
      .try_for_each(|(k, v)| self.insert(k.as_ref(), v.as_ref()))
  }

  /// Removes multiple keys from the partition.
  /// 从分区中批量删除指定键。
  #[inline]
  fn rm_batch<I, K>(&self, keys: I) -> StdResult<(), Self::Error>
  where
    I: IntoIterator<Item = K>,
    K: AsRef<[u8]>,
  {
    keys.into_iter().try_for_each(|k| self.rm(k.as_ref()))
  }

  /// Truncates / clears all entries in the partition.
  /// 清空当前分区中的全部物理条目。
  #[inline]
  fn clear(&self) -> StdResult<(), Self::Error> {
    self.iter().try_for_each(|item| self.rm(item?.key_ref()))
  }

  /// Flushes active in-memory buffer (MemTable) of this partition to durable storage.
  /// 将该分区的活跃内存写入缓冲区 (MemTable) 刷盘持久化。
  #[inline]
  fn flush(&self) -> StdResult<(), Self::Error> {
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
