use std::{fmt, str::FromStr};

use crate::error::{Error, Result};

/// Operation definition.
/// BITCOUNT / BITPOS 位图索引单位
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitUnit {
  #[default]
  Byte,
  Bit,
}

/// BITCOUNT command options enumeration.
/// BITCOUNT 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitCount {
  Range(i64, i64),
  Start(i64),
  End(i64),
  Unit(BitUnit),
}

/// BITPOS command options enumeration.
/// BITPOS 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitPos {
  Range(i64, i64),
  Start(i64),
  End(i64),
  Unit(BitUnit),
}

/// Domain operation (aligned with Apache Kvrocks BitOpFlags).
/// BITOP 运算类型（对标 Apache Kvrocks BitOpFlags）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum BitOp {
  #[strum(serialize = "AND")]
  And,
  #[strum(serialize = "OR")]
  Or,
  #[strum(serialize = "XOR")]
  Xor,
  #[strum(serialize = "NOT")]
  Not,
}

impl BitOp {
  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }

  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::And => "AND",
      Self::Or => "OR",
      Self::Xor => "XOR",
      Self::Not => "NOT",
    }
  }
}

/// Domain operation (aligned with Apache Kvrocks BitfieldOverflowBehavior).
/// BITFIELD 溢出处理策略（对标 Apache Kvrocks BitfieldOverflowBehavior）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum BitfieldOverflow {
  #[default]
  #[strum(serialize = "WRAP")]
  Wrap,
  #[strum(serialize = "SAT")]
  Sat,
  #[strum(serialize = "FAIL")]
  Fail,
}

impl BitfieldOverflow {
  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }

  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Wrap => "WRAP",
      Self::Sat => "SAT",
      Self::Fail => "FAIL",
    }
  }
}

/// Encodes data into binary format.
/// BITFIELD 整数类型编码（对标 Apache Kvrocks BitfieldEncoding）
/// Operation definition.
/// 支持 i1~i64（有符号）与 u1~u63（无符号）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum BitfieldEncoding {
  Signed(u8),
  Unsigned(u8),
}

impl BitfieldEncoding {
  #[inline]
  pub fn signed(bits: u8) -> Result<Self> {
    if (1..=64).contains(&bits) {
      Ok(Self::Signed(bits))
    } else {
      Err(Error::invalid_data(
        "ERR Invalid bitfield signed encoding bit length (1..=64)",
      ))
    }
  }

  #[inline]
  pub fn unsigned(bits: u8) -> Result<Self> {
    if (1..=63).contains(&bits) {
      Ok(Self::Unsigned(bits))
    } else {
      Err(Error::invalid_data(
        "ERR Invalid bitfield unsigned encoding bit length (1..=63)",
      ))
    }
  }

  #[inline]
  pub const fn is_signed(&self) -> bool {
    matches!(self, Self::Signed(_))
  }

  #[inline]
  pub const fn is_unsigned(&self) -> bool {
    matches!(self, Self::Unsigned(_))
  }

  #[inline]
  pub const fn bits(&self) -> u8 {
    match self {
      Self::Signed(b) | Self::Unsigned(b) => *b,
    }
  }

  /// Returns or computes calculated value.
  /// 根据位置索引 #N 计算绝对位偏移（对标 Redis / Kvrocks #N 语法）
  #[inline]
  pub fn positional_offset(&self, index: u64) -> Result<u64> {
    let bits = self.bits() as u64;
    index
      .checked_mul(bits)
      .filter(|&off| off <= u32::MAX as u64)
      .ok_or_else(|| Error::invalid_data("ERR bit offset is not an integer or out of range"))
  }
}

impl FromStr for BitfieldEncoding {
  type Err = Error;

  #[inline]
  fn from_str(s: &str) -> Result<Self> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.is_empty() {
      return Err(Error::invalid_data(
        "ERR Invalid bitfield type: empty string",
      ));
    }

    let prefix = bytes[0].to_ascii_lowercase();
    let num_str = &s[1..];
    let bits = num_str
      .parse::<u8>()
      .map_err(|_| Error::invalid_data(format!("ERR invalid bitfield bits in '{s}'")))?;

    match prefix {
      b'i' => Self::signed(bits),
      b'u' => Self::unsigned(bits),
      _ => Err(Error::invalid_data(format!(
        "ERR Invalid bitfield type prefix in '{s}', must start with 'i' or 'u'"
      ))),
    }
  }
}

impl fmt::Display for BitfieldEncoding {
  #[inline]
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Signed(b) => write!(f, "i{b}"),
      Self::Unsigned(b) => write!(f, "u{b}"),
    }
  }
}

/// Parses parameter or binary slice.
/// 解析 BITFIELD 偏移量参数字符串（支持普通数字 "100" 以及位置索引 "#5"，对标 Kvrocks）
#[inline]
pub fn parse_bitfield_offset(offset_str: &str, encoding: BitfieldEncoding) -> Result<u64> {
  let s = offset_str.trim();
  if let Some(pos_str) = s.strip_prefix('#') {
    let idx = pos_str
      .parse::<u64>()
      .map_err(|_| Error::invalid_data("ERR bit offset is not an integer or out of range"))?;
    encoding.positional_offset(idx)
  } else {
    let off = s
      .parse::<u64>()
      .map_err(|_| Error::invalid_data("ERR bit offset is not an integer or out of range"))?;
    if off <= u32::MAX as u64 {
      Ok(off)
    } else {
      Err(Error::invalid_data(
        "ERR bit offset is not an integer or out of range",
      ))
    }
  }
}

/// Operation definition.
/// BITFIELD 单项操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum BitfieldOpType {
  Get,
  Set,
  IncrBy,
}

/// Domain operation (aligned with Apache Kvrocks BitfieldOperation).
/// BITFIELD 操作指令（对标 Apache Kvrocks BitfieldOperation）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct BitfieldOperation {
  pub op_type: BitfieldOpType,
  pub encoding: BitfieldEncoding,
  pub offset: u64,
  pub value: i64,
  pub overflow: BitfieldOverflow,
}

impl BitfieldOperation {
  #[inline]
  pub const fn get(encoding: BitfieldEncoding, offset: u64) -> Self {
    Self {
      op_type: BitfieldOpType::Get,
      encoding,
      offset,
      value: 0,
      overflow: BitfieldOverflow::Wrap,
    }
  }

  #[inline]
  pub fn get_positional(encoding: BitfieldEncoding, index: u64) -> Result<Self> {
    let offset = encoding.positional_offset(index)?;
    Ok(Self::get(encoding, offset))
  }

  #[inline]
  pub const fn set(
    encoding: BitfieldEncoding,
    offset: u64,
    value: i64,
    overflow: BitfieldOverflow,
  ) -> Self {
    Self {
      op_type: BitfieldOpType::Set,
      encoding,
      offset,
      value,
      overflow,
    }
  }

  #[inline]
  pub fn set_positional(
    encoding: BitfieldEncoding,
    index: u64,
    value: i64,
    overflow: BitfieldOverflow,
  ) -> Result<Self> {
    let offset = encoding.positional_offset(index)?;
    Ok(Self::set(encoding, offset, value, overflow))
  }

  #[inline]
  pub const fn incrby(
    encoding: BitfieldEncoding,
    offset: u64,
    increment: i64,
    overflow: BitfieldOverflow,
  ) -> Self {
    Self {
      op_type: BitfieldOpType::IncrBy,
      encoding,
      offset,
      value: increment,
      overflow,
    }
  }

  #[inline]
  pub fn incrby_positional(
    encoding: BitfieldEncoding,
    index: u64,
    increment: i64,
    overflow: BitfieldOverflow,
  ) -> Result<Self> {
    let offset = encoding.positional_offset(index)?;
    Ok(Self::incrby(encoding, offset, increment, overflow))
  }
}

/// Domain operation (aligned with Apache Kvrocks BitfieldValue).
/// BITFIELD 操作返回值（对标 Apache Kvrocks BitfieldValue）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum BitfieldValue {
  Signed(i64),
  Unsigned(u64),
}

impl BitfieldValue {
  #[inline]
  pub const fn as_i64(&self) -> i64 {
    match self {
      Self::Signed(v) => *v,
      Self::Unsigned(v) => *v as i64,
    }
  }

  #[inline]
  pub const fn as_u64(&self) -> u64 {
    match self {
      Self::Signed(v) => *v as u64,
      Self::Unsigned(v) => *v,
    }
  }
}

impl PartialEq<i64> for BitfieldValue {
  #[inline]
  fn eq(&self, other: &i64) -> bool {
    self.as_i64() == *other
  }
}

impl PartialEq<u64> for BitfieldValue {
  #[inline]
  fn eq(&self, other: &u64) -> bool {
    self.as_u64() == *other
  }
}

impl PartialEq<BitfieldValue> for i64 {
  #[inline]
  fn eq(&self, other: &BitfieldValue) -> bool {
    *self == other.as_i64()
  }
}

impl PartialEq<BitfieldValue> for u64 {
  #[inline]
  fn eq(&self, other: &BitfieldValue) -> bool {
    *self == other.as_u64()
  }
}
