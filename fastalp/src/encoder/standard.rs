use super::Exception;
use crate::{bitpack::bitpack_encoded, float::AlpFloat, params::pack_params};

/// Encodes integer array using standard Frame-of-Reference (FOR) ALP encoding.
/// 使用标准基准值对齐（FOR）ALP 编码写入字节缓冲区
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode_standard<F: AlpFloat>(
  count: u16,
  exp: u8,
  fac: u8,
  use_div: bool,
  encoded_ints: &[F::Int],
  base: F::Int,
  bit_width: u8,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  // 1. Header (5B): 1B 类型 + 2B 数量 + 2B 参数 (exp, fac, bit_width)
  let count_bytes = count.to_le_bytes();
  let params_bytes = pack_params(exp, fac, bit_width).to_le_bytes();
  let type_byte = if use_div {
    F::TYPE_DEC_BYTE
  } else {
    F::TYPE_BYTE
  };
  let header = [
    type_byte,
    count_bytes[0],
    count_bytes[1],
    params_bytes[0],
    params_bytes[1],
  ];
  dst.extend_from_slice(&header);

  // 2. Base
  F::write_base(base, dst);

  // 3. Bitpacked data
  bitpack_encoded::<F>(encoded_ints, base, bit_width, dst);

  // 4. Exceptions (仅在存在异常值时写入)
  if !exceptions.is_empty() {
    let exc_count = exceptions.len() as u16;
    dst.extend_from_slice(&exc_count.to_le_bytes());
    for exc in exceptions {
      F::write_exception(exc.pos, exc.bits, dst);
    }
  }
}
