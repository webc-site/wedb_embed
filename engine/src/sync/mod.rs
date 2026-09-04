pub mod seqno;
pub mod snapshot;

pub use seqno::{MAX_SEQNO, MIN_SEQNO, SeqNo};
pub use snapshot::Snapshot;

use crate::engine::Engine;

/// Extended storage engine interface supporting replication and snapshot synchronization.
/// 支持数据复制与快照同步的扩展存储引擎接口 (Sync Engine)。
pub trait SyncEngine: Engine {
  /// Associated snapshot type.
  /// 引擎生成的一致性快照关联类型。
  type Snapshot: Snapshot<Partition = Self::Partition, Error = Self::Error>;

  /// Creates a cross-partition consistent point-in-time snapshot.
  /// 创建覆盖全部分区的瞬时全局一致性快照。
  fn snapshot(&self) -> Self::Snapshot;

  /// Returns the currently visible committed sequence number (LSN watermark for incremental sync).
  /// 获取当前已提交并对快照可见的最大全局序列号（增量同步 LSN 水位线基准）。
  fn visible_seqno(&self) -> SeqNo;

  /// Returns the would-be-next sequence number to be assigned.
  /// 获取下一个待分配的序列号。
  #[inline]
  fn next_seqno(&self) -> SeqNo {
    self.visible_seqno()
  }
}
