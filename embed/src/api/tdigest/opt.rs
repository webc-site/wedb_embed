use bitcode::{Decode, Encode};

/// TDIGEST.CREATE command options enumeration.
/// TDIGEST.CREATE 命令选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum TDigestCreate {
  Compression(u32),
}

/// TDIGEST.MERGE command options enumeration.
/// TDIGEST.MERGE 命令选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum TDigestMerge {
  Compression(u32),
  Override,
}

/// TDIGEST.INFO command response metadata (aligned with Kvrocks TDIGEST.INFO).
/// TDIGEST.INFO 命令响应信息（对标 Apache Kvrocks TDigestMetadata & TDIGEST.INFO）
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct TDigestInfo {
  pub compression: u32,
  pub capacity: usize,
  pub merged_nodes: usize,
  pub unmerged_nodes: usize,
  pub merged_weight: f64,
  pub unmerged_weight: f64,
  pub total_weight: f64,
  pub observations: u64,
  pub total_compressions: u64,
  pub minimum: Option<f64>,
  pub maximum: Option<f64>,
}
