use crate::{
  bitpack::{bitpack_fused_delta, packed_byte_size},
  encoder::{
    Exception,
    exception::{exceptions_byte_size, write_exceptions},
  },
  float::AlpFloat,
  header::{header_len, write_header},
  params::pack_params,
};

/// Encodes integer array using Delta differential Frame-of-Reference encoding.
/// 使用 Delta 一阶差分基准值对齐编码写入字节缓冲区（熔合差分打包，零多余内存读写）
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode_delta<F: AlpFloat>(
  count: usize,
  exp: u8,
  fac: u8,
  use_div: bool,
  encoded_ints: &[F::Int],
  min_delta: F::Int,
  delta_bit_width: u8,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  let Some(&first) = encoded_ints.first() else {
    return;
  };

  let is_large = count > u16::MAX as usize;
  let exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
  let deltas_len = count.saturating_sub(1);
  let total_len =
    header_len(count) + F::BASE_SIZE * 2 + packed_byte_size(deltas_len, delta_bit_width) + exc_len;
  dst.reserve(total_len);

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

  // 3. Bitpacked deltas (熔合差分直接打包，省去 1024 元素的大内存回写)
  if delta_bit_width > 0 && encoded_ints.len() > 1 {
    bitpack_fused_delta::<F>(encoded_ints, min_delta, delta_bit_width, dst);
  }

  // 4. Exceptions (仅在存在异常值时写入)
  write_exceptions::<F>(count, exceptions, dst);
}
