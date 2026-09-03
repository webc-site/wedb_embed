use std::result::Result as StdResult;

use thiserror::Error;

pub type Result<T> = StdResult<T, Error>;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum Error {
  #[error("Invalid ALP header or type byte")]
  InvalidHeader,
  #[error("Unexpected end of buffer (needed {needed} bytes, had {available})")]
  UnexpectedEof { needed: usize, available: usize },
  #[error("Corrupted ALP bitstream or exception out of bounds: index {index} >= count {count}")]
  CorruptedData { index: usize, count: usize },
  #[error("Unsupported exponent or bit width: exp={exp}, fac={fac}, bit_width={bit_width}")]
  UnsupportedParams { exp: u8, fac: u8, bit_width: u8 },
}
