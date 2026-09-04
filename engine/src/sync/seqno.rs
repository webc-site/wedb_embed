/// Sequence number / Logical Sequence Number (LSN) type for version tracking.
/// 用于数据同步与版本水位追踪的逻辑序列号 (LSN / SeqNo)。
pub type SeqNo = u64;

/// Minimum sequence number (initial baseline).
/// 最小序列号（初始基准值）。
pub const MIN_SEQNO: SeqNo = 0;

/// Maximum possible sequence number.
/// 最大可能序列号。
pub const MAX_SEQNO: SeqNo = u64::MAX;
