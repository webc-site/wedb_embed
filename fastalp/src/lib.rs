mod bitpack;
mod constants;
mod decoder;
mod delta;
mod encoder;
mod error;
mod float;
mod header;
mod params;
mod sampler;

pub use bitpack::{
  bitpack_encoded, bitpack_u64, bitunpack_into, bitunpack_into_div, bitunpack_u64,
  bitunpack_u64_slice, packed_byte_size,
};
pub use constants::{
  BITS_PER_BYTE, BITS_U64, BYTES_U16, BYTES_U32, BYTES_U64, CHUNK_SIZE_1024, EARLY_EXIT_BIT_WIDTH,
  ENCODING_UPPER_LIMIT_F32, ENCODING_UPPER_LIMIT_F64, EXC_COUNT_LEN, EXC_COUNT_LEN_U32,
  EXC_POS_LEN, EXC_POS_LEN_U32, EXP_ARR_F32, EXP_ARR_F64, FACT_ARR_F32, FACT_ARR_F64, FRAC_ARR_F32,
  FRAC_ARR_F64, LEN_TAG_1024, LEN_TAG_MASK, LEN_TAG_SHIFT, LEN_TAG_U8, LEN_TAG_U16, LEN_TAG_U32,
  LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT, LUT_SIZE_8BIT, MAGIC_NUMBER_F32, MAGIC_NUMBER_F64,
  MAX_EXPONENT_F32, MAX_EXPONENT_F64, MAX_FAC_F32, MAX_FAC_F64, SAMPLES_COUNT, TYPE_F32,
  TYPE_F32_DEC, TYPE_F32_DEC_DELTA, TYPE_F32_DELTA, TYPE_F32_RAW, TYPE_F64, TYPE_F64_DEC,
  TYPE_F64_DEC_DELTA, TYPE_F64_DELTA, TYPE_F64_RAW, TYPE_MASK,
};
pub use decoder::{decode_delta, decode_standard, decompress, decompress_into};
pub use delta::{eval_delta_benefit, in_place_deltas, reconstruct_ramp_into_floats};
pub use encoder::{
  Exception, compress, compress_delta, compress_delta_into, compress_into, encode_delta,
  encode_standard,
};
pub use error::{Error, Result};
pub use float::AlpFloat;
pub use header::{
  MAX_HEADER_LEN, ParsedHeader, count_bytes, header_len, raw_header_len, read_header, write_header,
};
pub use params::{
  BIT_WIDTH_MASK, BIT_WIDTH_SHIFT, EXP_MASK, FAC_MASK, FAC_SHIFT, bit_mask, bits_needed,
  pack_params, unpack_params,
};
pub use sampler::{
  BestParams, find_best_params, find_identical_base, is_impossible, try_encode_fast,
  try_encode_value,
};
