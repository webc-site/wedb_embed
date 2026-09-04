use std::{
  error::Error as StdError,
  ops::{Bound, Deref},
  result::Result as StdResult,
};

use crate::{entry::KvEntry, partition::Partition, sync::seqno::SeqNo};

/// Cross-partition consistent point-in-time snapshot abstraction.
/// 跨分区瞬时全局一致性快照抽象。
pub trait Snapshot: Clone + Send + Sync + 'static {
  /// Associated error type for snapshot operations.
  /// 快照操作关联错误类型。
  type Error: StdError + Send + Sync + 'static;

  /// Associated partition type targeting this snapshot.
  /// 该快照针对的分区关联类型。
  type Partition: Partition<Error = Self::Error>;

  /// Associated value type returned by snapshot point get operations.
  /// 快照单点读取操作返回的值关联类型。
  type Value: Deref<Target = [u8]>;

  /// Associated entry type produced by snapshot range iterators.
  /// 快照区间迭代器产出的条目关联类型。
  type Entry<'a>: KvEntry
  where
    Self: 'a;

  /// Associated bidirectional iterator type for snapshot reads.
  /// 快照双向迭代器关联类型。
  type Iter<'a>: Iterator<Item = StdResult<Self::Entry<'a>, Self::Error>> + DoubleEndedIterator
  where
    Self: 'a;

  /// Returns the sequence number (LSN) at which this snapshot was frozen.
  /// 返回该一致性快照生成时刻冻结的全局序列号 / LSN。
  fn seqno(&self) -> SeqNo;

  /// Retrieves the value associated with the given key in the partition at this snapshot instant.
  /// 在该快照时刻获取指定分区中对应键的值。
  fn get(
    &self,
    partition: &Self::Partition,
    key: &[u8],
  ) -> StdResult<Option<Self::Value>, Self::Error>;

  /// Returns the byte size of the value in the partition at this snapshot instant without allocating the payload.
  /// 在该快照时刻获取指定分区中对应键值的字节大小（无需分配完整 payload）。
  #[inline]
  fn size_of(
    &self,
    partition: &Self::Partition,
    key: &[u8],
  ) -> StdResult<Option<usize>, Self::Error> {
    self.get(partition, key).map(|opt| opt.map(|v| v.len()))
  }

  /// Checks if the partition contains the specified key at this snapshot instant.
  /// 在该快照时刻检查指定分区中是否存在指定键。
  #[inline]
  fn contains_key(&self, partition: &Self::Partition, key: &[u8]) -> StdResult<bool, Self::Error> {
    self.get(partition, key).map(|opt| opt.is_some())
  }

  /// Checks if the partition contains no entries at this snapshot instant.
  /// 检查指定分区在该快照时刻是否为空。
  #[inline]
  fn is_empty(&self, partition: &Self::Partition) -> StdResult<bool, Self::Error> {
    self.first_entry(partition).map(|opt| opt.is_none())
  }

  /// Returns the number of entries in the partition at this snapshot instant.
  /// 获取指定分区在该快照时刻的条目总数。
  #[inline]
  fn len(&self, partition: &Self::Partition) -> StdResult<usize, Self::Error> {
    self
      .iter(partition)
      .try_fold(0usize, |count, item| item.map(|_| count + 1))
  }

  /// Returns a bidirectional iterator over all entries in the partition at this snapshot instant.
  /// 返回遍历指定分区在快照时刻所有条目的双向迭代器。
  fn iter<'a>(&'a self, partition: &'a Self::Partition) -> Self::Iter<'a>;

  /// Returns a bidirectional iterator over entries matching the given prefix at this snapshot instant.
  /// 返回遍历指定分区在快照时刻匹配前缀条目的双向迭代器。
  fn prefix<'a>(&'a self, partition: &'a Self::Partition, prefix: &[u8]) -> Self::Iter<'a>;

  /// Returns a bidirectional iterator over entries within the specified range at this snapshot instant.
  /// 返回遍历指定分区在快照时刻指定键区间内条目的双向迭代器。
  fn range<'a>(
    &'a self,
    partition: &'a Self::Partition,
    range: (Bound<&[u8]>, Bound<&[u8]>),
  ) -> Self::Iter<'a>;

  /// Returns the first key-value entry in the partition at this snapshot instant.
  /// 获取指定分区在该快照时刻的第一个键值对条目（键最小的条目）。
  #[inline]
  fn first_entry<'a>(
    &'a self,
    partition: &'a Self::Partition,
  ) -> StdResult<Option<Self::Entry<'a>>, Self::Error> {
    self.iter(partition).next().transpose()
  }

  /// Returns the last key-value entry in the partition at this snapshot instant.
  /// 获取指定分区在该快照时刻的最后一个键值对条目（键最大的条目）。
  #[inline]
  fn last_entry<'a>(
    &'a self,
    partition: &'a Self::Partition,
  ) -> StdResult<Option<Self::Entry<'a>>, Self::Error> {
    self.iter(partition).next_back().transpose()
  }
}
