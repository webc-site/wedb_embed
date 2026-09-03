use std::{error::Error as StdError, io, result::Result as StdResult};

use thiserror::Error;

pub type Result<T, E = Error> = StdResult<T, E>;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Engine error: {0}")]
  Engine(String),

  #[error("Storage error: {0}")]
  Storage(String),

  #[error("Config error: {0}")]
  Config(String),

  #[error("Internal error: {0}")]
  Internal(String),

  #[error(transparent)]
  Io(#[from] io::Error),

  #[error(transparent)]
  Strum(#[from] strum::ParseError),

  #[error("Serialization error: {0}")]
  Serialization(String),

  #[error("Redis error: {0}")]
  Redis(String),

  #[error("{0}")]
  WrongType(String),

  #[error("Invalid data: {0}")]
  InvalidData(String),

  #[error("Not found: {0}")]
  NotFound(String),

  #[cfg(feature = "fjall")]
  #[error(transparent)]
  Fjall(#[from] fjall::Error),
}

impl Error {
  pub fn engine(msg: impl Into<String>) -> Self {
    Self::Engine(msg.into())
  }

  pub fn io(e: io::Error) -> Self {
    Self::Io(e)
  }

  pub fn conf(msg: impl Into<String>) -> Self {
    Self::Config(msg.into())
  }

  pub fn internal(msg: impl Into<String>) -> Self {
    Self::Internal(msg.into())
  }

  pub fn internal_with_source<E: StdError + Send + Sync + 'static>(
    msg: impl Into<String>,
    source: E,
  ) -> Self {
    let msg_str: String = msg.into();
    Self::Internal(format!("{msg_str}: {source}"))
  }

  pub fn invalid_data(msg: impl Into<String>) -> Self {
    Self::InvalidData(msg.into())
  }

  pub fn not_found(msg: impl Into<String>) -> Self {
    Self::NotFound(msg.into())
  }

  pub fn redis(msg: impl Into<String>) -> Self {
    Self::Redis(msg.into())
  }

  pub fn storage(msg: impl Into<String>) -> Self {
    Self::Storage(msg.into())
  }

  pub fn wrong_type(msg: impl Into<String>) -> Self {
    Self::WrongType(msg.into())
  }

  /// Convenient constructor for WRONGTYPE error.
  /// 便捷构造 WRONGTYPE 错误（使用统一常量）
  #[inline]
  pub fn wrong_type_default() -> Self {
    Self::WrongType(ERR_WRONG_TYPE.to_string())
  }

  #[inline]
  pub fn is_wrong_type(&self) -> bool {
    matches!(self, Self::WrongType(_))
  }
}

/// Cross-type operation error constant.
/// 跨类型操作错误（统一定义，各模块共用）
pub const ERR_WRONG_TYPE: &str =
  "WRONGTYPE Operation against a key holding the wrong kind of value";

/// Key not found error constant.
/// 键不存在错误常量
pub const ERR_NO_SUCH_KEY: &str = "ERR no such key";
