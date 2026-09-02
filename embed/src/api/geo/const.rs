pub use std::f64::consts::PI;

pub use crate::error::ERR_WRONG_TYPE;

/// Earth mean radius in meters aligned with Apache Kvrocks / Redis (6372797.560856).
/// 地球平均半径（米）（对标 Apache Kvrocks / Redis）
pub const EARTH_RADIUS_METERS: f64 = 6372797.560856;

/// Latitude and longitude bounds according to EPSG:900913 / WGS84 Mercator.
/// 经纬度限制常量（EPSG:900913 / WGS84 Mercator 规范）
pub const GEO_LAT_MIN: f64 = -85.05112878;
pub const GEO_LAT_MAX: f64 = 85.05112878;
pub const GEO_LON_MIN: f64 = -180.0;
pub const GEO_LON_MAX: f64 = 180.0;

/// Maximum Geohash precision step (26-bit lon + 26-bit lat = 52-bit integer).
/// Geohash 最大精度步长（26 位经度 + 26 位纬度 = 52 位整数）
pub const GEO_STEP_MAX: u8 = 26;
pub const MERCATOR_MAX: f64 = 20037726.37;
pub const D_R: f64 = PI / 180.0;

/// Base32 character alphabet for Geohash encoding.
/// Base32 字母表（Redis / Kvrocks 规范）
pub const BASE32_ALPHABET: &[u8; 32] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// Decodes data from binary format.
/// 编译期静态 Base32 解码表（O(1) 快速索引，支持大小写）
pub const BASE32_DECODE_TABLE: [u8; 256] = {
  let mut table = [0xFF; 256];
  let mut i = 0;
  while i < 32 {
    let byte = BASE32_ALPHABET[i];
    table[byte as usize] = i as u8;
    if byte >= b'a' && byte <= b'z' {
      table[(byte - b'a' + b'A') as usize] = i as u8;
    }
    i += 1;
  }
  table
};
