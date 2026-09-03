//! Storage engine abstractions and concrete engine implementations.
//! 存储引擎抽象与具体底层实现。

#[cfg(feature = "fjall")]
pub mod fjall;

#[cfg(feature = "fjall")]
pub use fjall::Fjall;
pub use wedb_embed_engine::{Batch, Engine, KvEntry, Partition};
