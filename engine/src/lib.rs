//! Generic trait abstractions for embedded key-value storage engines.
//! 嵌入式键值存储引擎的通用 Trait 抽象定义。

pub mod batch;
pub mod engine;
pub mod entry;
pub mod partition;
pub mod traits;

#[cfg(feature = "sync")]
pub mod sync;

pub use batch::Batch;
pub use engine::Engine;
pub use entry::KvEntry;
pub use partition::Partition;
#[cfg(feature = "sync")]
pub use sync::*;
