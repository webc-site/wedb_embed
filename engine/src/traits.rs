//! Backward-compatible re-exports for trait abstractions.
//! 向后兼容的 Trait 抽象重导出。

#[cfg(feature = "sync")]
pub use crate::sync::*;
pub use crate::{batch::Batch, engine::Engine, entry::KvEntry, partition::Partition};
