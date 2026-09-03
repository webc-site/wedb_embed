use bitcode::{Decode, Encode};

/// JSON.SET command conditional write options.
/// JSON.SET 命令条件选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum JsonSet {
  /// Write only if key does not exist (NX).
  /// 仅在键不存在时写入
  Nx,
  /// Write only if key already exists (XX).
  /// 仅在键已存在时覆盖写入
  Xx,
}

/// JSON.ARRINDEX command range options enumeration.
/// JSON.ARRINDEX 命令区间选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum JsonArrIndex {
  Start(isize),
  Stop(isize),
  Range(isize, isize),
}

/// JSON numeric operation type (Incr, Mul).
/// JSON 数值运算类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum JsonNumberOp {
  /// Numeric addition (Incr).
  /// 数值累加 (Incr)
  Incr,
  /// Numeric multiplication (Mul).
  /// 数值累乘 (Mul)
  Mul,
}

/// JSON.GET formatting options enumeration.
/// JSON.GET 格式化选项枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonGet {
  Indent(String),
  Newline(String),
  Space(String),
}
