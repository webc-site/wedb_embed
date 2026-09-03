pub mod algo;
pub mod r#const;
pub mod core;
pub mod dense;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod sparse;
pub use core::HyperLogLog;

pub use algo::{
  HLL_ALPHA_INF, HLL_DENSE_SIZE, HLL_HASH_BIT_COUNT, HLL_HASH_SEED, HLL_REGISTER_BITS,
  HLL_REGISTER_COUNT_MASK, HLL_REGISTER_COUNT_POW, HLL_REGISTER_MAX, HLL_REGISTERS,
  HLL_SEGMENT_BYTES, HLL_SEGMENT_COUNT, HLL_SEGMENT_REGISTERS, extract_dense_hll_result,
  hll_estimate_from_histo, hll_murmur_hash_64a, hll_sigma, hll_tau, murmur_hash_64a, rapid_hash,
};
pub use r#const::*;
pub use dense::{
  dense_estimate, get_register, hll_dense_estimate, hll_dense_estimate_segments,
  hll_dense_get_register, hll_dense_reg_histo, hll_dense_set_register, hll_merge_bytes,
  hll_merge_segments, set_register,
};
pub use key::{
  meta as compose_hll_meta_key, meta_prefix as compose_hll_meta_prefix, raw as compose_hll_data_key,
};
pub use meta::{HllEncodeType, HyperLogLogMeta};
pub use sparse::{
  HLL_SPARSE_MAX_BYTES, HLL_SPARSE_VAL_MAX_LEN, HLL_SPARSE_VAL_MAX_VALUE, HLL_SPARSE_XZERO_MAX_LEN,
  HLL_SPARSE_ZERO_MAX_LEN, HllSparseOp, decode_sparse_op, encode_sparse_val, encode_sparse_zero,
  hll_dense_to_sparse, hll_merge_sparse_into_dense, hll_sparse_estimate, hll_sparse_get_register,
  hll_sparse_is_valid, hll_sparse_new, hll_sparse_reg_histo, hll_sparse_set_register,
  hll_sparse_to_dense,
};
