mod bitpack;
mod constants;
mod decoder;
mod encoder;
mod error;
mod float;
mod params;
mod sampler;

pub use bitpack::{bitpack_encoded, bitpack_u64, bitunpack_into, bitunpack_u64, packed_byte_size};
pub use constants::{
  BITS_PER_BYTE, BITS_U64, BYTES_U16, BYTES_U32, BYTES_U64, EARLY_EXIT_BIT_WIDTH,
  ENCODING_UPPER_LIMIT_F32, ENCODING_UPPER_LIMIT_F64, EXC_COUNT_LEN, EXC_POS_LEN, EXP_ARR_F32,
  EXP_ARR_F64, FACT_ARR_F32, FACT_ARR_F64, FRAC_ARR_F32, FRAC_ARR_F64, HDR_COUNT_END,
  HDR_COUNT_START, HDR_PARAMS_END, HDR_PARAMS_START, HDR_TYPE_IDX, HEADER_LEN, LUT_SIZE_1BIT,
  LUT_SIZE_2BIT, LUT_SIZE_4BIT, LUT_SIZE_8BIT, MAGIC_NUMBER_F32, MAGIC_NUMBER_F64,
  MAX_EXPONENT_F32, MAX_EXPONENT_F64, MAX_FAC_F32, MAX_FAC_F64, MIN_HEADER_LEN, SAMPLES_COUNT,
  TYPE_F32, TYPE_F32_RAW, TYPE_F64, TYPE_F64_RAW,
};
pub use decoder::{decompress, decompress_into};
pub use encoder::{Exception, compress, compress_into};
pub use error::{Error, Result};
pub use float::AlpFloat;
pub use params::{
  BIT_WIDTH_MASK, BIT_WIDTH_SHIFT, EXP_MASK, FAC_MASK, FAC_SHIFT, bit_mask, bits_needed,
  pack_params, unpack_params,
};
pub use sampler::{
  BestParams, find_best_params, find_identical_base, is_impossible, try_encode_fast,
  try_encode_value,
};
