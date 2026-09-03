use super::Exception;
use crate::{bitpack::bitpack_encoded, float::AlpFloat, header::write_header, params::pack_params};

/// Encodes integer array using standard Frame-of-Reference (FOR) ALP encoding.
/// 使用标准基准值对齐（FOR）ALP 编码写入字节缓冲区
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode_standard<F: AlpFloat>(
  count: usize,
  exp: u8,
  fac: u8,
  use_div: bool,
  encoded_ints: &[F::Int],
  base: F::Int,
  bit_width: u8,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  // 1. Header: 紧凑自描述头部 (1B 描述符 + 可选 count + 2B params)
  let type_byte = if use_div {
    F::TYPE_DEC_BYTE
  } else {
    F::TYPE_BYTE
  };
  let packed_params = pack_params(exp, fac, bit_width);
  write_header(type_byte, count, Some(packed_params), dst);

  // 2. Base
  F::write_base(base, dst);

  // 3. Bitpacked data
  bitpack_encoded::<F>(encoded_ints, base, bit_width, dst);

  // 4. Exceptions (仅在存在异常值时写入)
  super::write_exceptions::<F>(count, exceptions, dst);
}
