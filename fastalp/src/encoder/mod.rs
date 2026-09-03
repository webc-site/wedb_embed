mod delta;
mod engine;
mod exception;
mod kernel;
mod outlier;
mod standard;
mod state;

pub use delta::encode_delta;
pub use exception::Exception;
pub use standard::encode_standard;
pub use state::Encoder;

use crate::float::AlpFloat;

/// Generic floating-point compression writing directly into `dst` buffer.
/// 通用压缩浮点数组并直接写入 `dst` 缓冲区（自适应选择 FOR 或 Delta 差分模式）
#[inline]
pub fn compress_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
  let mut encoder = Encoder::new();
  encoder.compress_into(data, dst);
}

/// Floating-point compression with enforced Delta differential encoding.
/// 强制使用 Delta 一阶差分模式压缩浮点数组并直接写入 `dst` 缓冲区
#[inline]
pub fn compress_delta_into<F: AlpFloat>(data: &[F], dst: &mut Vec<u8>) {
  let mut encoder = Encoder::new();
  encoder.compress_delta_into(data, dst);
}

/// Generic floating-point slice compression.
/// 通用压缩浮点数切片
#[inline]
pub fn compress<F: AlpFloat>(data: &[F]) -> Vec<u8> {
  let mut dst = Vec::new();
  compress_into(data, &mut dst);
  dst
}

/// Generic floating-point slice compression enforcing Delta differential mode.
/// 强制使用 Delta 差分模式压缩浮点数切片
#[inline]
pub fn compress_delta<F: AlpFloat>(data: &[F]) -> Vec<u8> {
  let mut dst = Vec::new();
  compress_delta_into(data, &mut dst);
  dst
}
