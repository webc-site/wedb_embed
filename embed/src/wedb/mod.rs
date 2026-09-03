//! Core WeDb database handles, multi-tenant namespaces, and write batches.
//! WeDb 核心数据库操作句柄、多租户命名空间与批量写入。

pub mod batch;
pub mod core;
pub mod db;
pub mod iter;
pub mod namespace;

pub use core::{DATA, ExpireCursors, META, WeDb, clear_ks_prefix};

pub use batch::DbBatch;
pub use db::Db;
pub use iter::{Dbs, Namespaces};
pub use namespace::Namespace;

/// Trait for converting types into an optional numerical ID (`Some(id)` to open, `None` to auto-allocate new ID).
/// 用于将参数转换为可选数字 ID 的 Trait（传入数字表示打开指定 ID，传入 `None` 或 `()` 表示自动新建分配）。
pub trait IntoOptId {
  fn into_opt_id(self) -> Option<u64>;
}

impl IntoOptId for u64 {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self)
  }
}

impl IntoOptId for usize {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self as u64)
  }
}

impl IntoOptId for u32 {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self as u64)
  }
}

impl IntoOptId for u16 {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self as u64)
  }
}

impl IntoOptId for u8 {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self as u64)
  }
}

impl IntoOptId for i64 {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self as u64)
  }
}

impl IntoOptId for i32 {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    Some(self as u64)
  }
}

impl IntoOptId for Option<u64> {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    self
  }
}

impl IntoOptId for () {
  #[inline(always)]
  fn into_opt_id(self) -> Option<u64> {
    None
  }
}
