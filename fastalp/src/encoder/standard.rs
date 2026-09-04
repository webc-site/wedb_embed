use crate::{
  bitpack::{bitpack_encoded, packed_byte_size},
  encoder::{
    Exception,
    exception::{exceptions_byte_size, write_exceptions},
  },
  float::AlpFloat,
  header::{header_len, write_header},
  params::AlpParams,
};

/// Encodes integer array using standard Frame-of-Reference (FOR) ALP encoding.
/// 使用标准基准值对齐（FOR）ALP 编码写入字节缓冲区
#[inline(always)]
pub fn encode_standard<F: AlpFloat>(
  params: AlpParams,
  encoded_ints: &[F::Int],
  base: F::Int,
  exceptions: &[Exception<F::RawBits>],
  dst: &mut Vec<u8>,
) {
  let count = encoded_ints.len();
  let is_large = count > u16::MAX as usize;
  let exc_len = exceptions_byte_size::<F>(exceptions.len(), is_large);
  let total_len =
    header_len(count) + F::BASE_SIZE + packed_byte_size(count, params.bit_width) + exc_len;
  dst.reserve(total_len);

  // 1. Header: compact self-describing header (1B descriptor + optional count + 2B params)
  // 1. 头部：紧凑自描述头部 (1B 描述符 + 可选 count + 2B params)
  let type_byte = params.standard_type::<F>();
  write_header(type_byte, count, Some(params.pack()), dst);

  // 2. Base
  F::write_base(base, dst);

  // 3. Bitpacked data
  bitpack_encoded::<F>(encoded_ints, base, params.bit_width, dst);

  // 4. Exceptions (written only when exceptions exist)
  // 4. 异常值 (仅在存在异常值时写入)
  write_exceptions::<F>(count, exceptions, dst);
}
