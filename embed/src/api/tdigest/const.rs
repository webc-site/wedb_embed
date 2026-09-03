use std::f64::consts::PI;

/// Double-precision comparison epsilon threshold (aligned with Kvrocks DoubleCompare).
/// 双精度浮点数比较精度阈值（对标 Apache Kvrocks DoubleCompare）
pub const REL_EPS: f64 = 1e-12;
pub const ABS_EPS: f64 = 1e-9;
pub const SINGLETON_BOUNDARY_WEIGHT: f64 = 1.0;
pub const HALF_SINGLETON_BOUNDARY_WEIGHT: f64 = 0.5;
pub const INV_TWO_PI: f64 = 1.0 / (2.0 * PI);

/// Default compression factor.
/// 默认压缩因子
pub const DEFAULT_COMPRESSION: u32 = 100;
/// Minimum compression factor.
/// 最小压缩因子
pub const MIN_COMPRESSION: u32 = 1;
/// Maximum compression factor.
/// 最大压缩因子
pub const MAX_COMPRESSION: u32 = 1000;
/// Maximum buffer capacity.
/// 最大缓冲区容量
pub const MAX_CAPACITY: usize = 1024;

/// Calculates buffer capacity (aligned with Kvrocks std::min(compression * 6 + 10, 1024)).
/// 计算缓冲区容量 (对标 Apache Kvrocks std::min(compression * 6 + 10, 1024))
#[inline]
pub const fn calculate_capacity(compression: u32) -> usize {
  let cap = compression as usize * 6 + 10;
  if cap > MAX_CAPACITY {
    MAX_CAPACITY
  } else {
    cap
  }
}
