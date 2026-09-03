use super::Exception;
use crate::{
  bitpack::bitpack_encoded, delta::in_place_deltas, float::AlpFloat, header::write_header,
  params::pack_params,
};

/// Encodes integer array using Delta differential Frame-of-Reference encoding.
/// 使用 Delta 一阶差分基准值对齐编码写入字节缓冲区（就地差分，零多余堆分配）
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode_delta<F: AlpFloat>(
  count: usize,
  exp: u8,
  fac: u8,
  use_div: bool,
  encoded_ints: &mut [F::Int],
  min_delta: F::Int,
  delta_bit_width: u8,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  let first = encoded_ints[0];

  // 1. Header: 紧凑自描述头部 (1B 描述符 + 可选 count + 2B params)
  let type_byte = if use_div {
    F::TYPE_DEC_DELTA_BYTE
  } else {
    F::TYPE_DELTA_BYTE
  };
  let packed_params = pack_params(exp, fac, delta_bit_width);
  write_header(type_byte, count, Some(packed_params), dst);

  // 2. Base fields: First value (BASE_SIZE) + min_delta (BASE_SIZE)
  F::write_base(first, dst);
  F::write_base(min_delta, dst);

  // 3. Bitpacked deltas (就地计算差分，零额外内存分配)
  if delta_bit_width > 0 && encoded_ints.len() > 1 {
    in_place_deltas::<F>(encoded_ints);
    bitpack_encoded::<F>(&encoded_ints[1..], min_delta, delta_bit_width, dst);
  }

  // 4. Exceptions (仅在存在异常值时写入)
  super::write_exceptions::<F>(count, exceptions, dst);
}
