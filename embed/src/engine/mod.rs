//! Storage engine abstractions and concrete engine implementations.
//! 存储引擎抽象与具体底层实现。

#[cfg(feature = "fjall")]
pub mod fjall;

#[cfg(feature = "fjall")]
pub use fjall::Fjall;
#[cfg(all(feature = "fjall", feature = "sync"))]
pub use fjall::FjallSnapshot;
pub use wedb_embed_engine::{Batch, Engine, KvEntry, Partition};
#[cfg(feature = "sync")]
pub use wedb_embed_engine::{MAX_SEQNO, MIN_SEQNO, SeqNo, Snapshot, SyncEngine};
