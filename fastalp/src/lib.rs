#![cfg_attr(docsrs, feature(doc_cfg))]

use core::mem::size_of;

#[macro_use]
mod macros;

pub mod bitpack;
pub mod header;

#[cfg(feature = "capi")]
pub mod capi;

mod constants;
mod decoder;
mod delta;
mod encoder;
mod error;
mod float;
mod params;
mod sampler;

#[cfg(feature = "capi")]
pub use capi::*;
pub use constants::{CHUNK_SIZE, CHUNK_SIZE_1024};
pub use decoder::{decompress, decompress_into, decompress_into_raw, decompress_into_slice};
pub use encoder::{
  Encoder, compress, compress_delta, compress_delta_into, compress_into, profile_compress_breakdown,
};
pub use error::{Error, Result};
pub use float::AlpFloat;
pub use header::{ChunkType, MAX_HEADER_LEN, ParsedHeader, read_count, read_header};
pub use params::AlpParams;
pub use sampler::BestParams;

/// Reads the element count from the compressed data header in O(1) time without decompressing the payload.
///
/// 从压缩数据头部快速读取元素总数（O(1) 复杂度，零内存分配，无需解压任何有效载荷数据）。
#[inline(always)]
pub fn count(src: &[u8]) -> Result<usize> {
  header::read_count(src)
}

/// Computes the maximum possible compressed buffer size in bytes for `count` values of type `F`.
/// Guaranteed to never overflow even in the worst-case uncompressible fallback.
///
/// 计算 `count` 个 `F` 类型浮点数值在最差情况下所需的最大压缩缓冲区字节大小。
/// 确保预分配该大小的目标缓冲区绝不发生溢出。
#[inline(always)]
pub const fn max_compressed_size<F: AlpFloat>(count: usize) -> usize {
  MAX_HEADER_LEN.saturating_add(count.saturating_mul(size_of::<F>()))
}
