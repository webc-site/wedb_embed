use bitcode::{Decode, Encode};

/// Domain operation (aligned with Kvrocks DistanceUnit).
/// 距离单位（对标 Kvrocks DistanceUnit）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Encode,
  Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum DistanceUnit {
  #[default]
  #[strum(serialize = "m", serialize = "meters")]
  Meters,
  #[strum(serialize = "km", serialize = "kilometers")]
  Kilometers,
  #[strum(serialize = "mi", serialize = "miles")]
  Miles,
  #[strum(serialize = "ft", serialize = "feet")]
  Feet,
}

impl DistanceUnit {
  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }

  /// Returns or computes calculated value.
  /// 获取转换为米的乘数因子
  #[inline]
  pub const fn conversion_factor(&self) -> f64 {
    match self {
      Self::Meters => 1.0,
      Self::Kilometers => 1000.0,
      Self::Miles => 1609.34,
      Self::Feet => 0.3048,
    }
  }

  /// Operation definition.
  /// 转换为米
  #[inline]
  pub const fn to_meters(&self, dist: f64) -> f64 {
    dist * self.conversion_factor()
  }

  /// Operation definition.
  /// 从米转换为当前单位数值
  #[inline]
  pub const fn from_meters(&self, meters: f64) -> f64 {
    meters / self.conversion_factor()
  }
}

/// Domain operation (aligned with Kvrocks DistanceSort).
/// 空间结果排序方式（对标 Kvrocks DistanceSort）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Encode,
  Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum DistanceSort {
  #[default]
  None,
  #[strum(serialize = "asc", serialize = "ASC")]
  Asc,
  #[strum(serialize = "desc", serialize = "DESC")]
  Desc,
}

impl DistanceSort {
  /// Parses parameter or binary slice.
  /// 解析排序方式字符串（零内存分配）
  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }
}

/// Domain operation (aligned with Kvrocks OriginPointType).
/// 空间查询原点类型（对标 Kvrocks OriginPointType）
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum OriginPoint {
  Coord { lon: f64, lat: f64 },
  Member(String),
}

impl OriginPoint {
  #[inline]
  pub const fn coord(lon: f64, lat: f64) -> Self {
    Self::Coord { lon, lat }
  }

  #[inline]
  pub fn member(m: impl Into<String>) -> Self {
    Self::Member(m.into())
  }
}

/// Domain operation (aligned with Kvrocks GeoPoint).
/// 地理点详细信息（对标 Kvrocks GeoPoint）
#[derive(Debug, Clone, PartialEq, Default, Encode, Decode)]
pub struct GeoPoint {
  pub longitude: f64,
  pub latitude: f64,
  pub member: String,
  pub dist: f64,
  pub score: f64,
}

/// Domain operation (aligned with Kvrocks GeoHashBits).
/// Geohash 比特结构（对标 Kvrocks GeoHashBits）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GeoHashBits {
  pub bits: u64,
  pub step: u8,
}

impl GeoHashBits {
  #[inline]
  pub const fn is_zero(&self) -> bool {
    self.bits == 0 && self.step == 0
  }
}

/// Domain operation (aligned with Kvrocks GeoHashRange).
/// 经纬度范围（对标 Kvrocks GeoHashRange）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GeoHashRange {
  pub min: f64,
  pub max: f64,
}

impl GeoHashRange {
  #[inline]
  pub const fn is_zero(&self) -> bool {
    self.min == 0.0 && self.max == 0.0
  }
}

/// Domain operation (aligned with Kvrocks GeoHashArea).
/// Geohash 区域范围（对标 Kvrocks GeoHashArea）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GeoHashArea {
  pub hash: GeoHashBits,
  pub latitude: GeoHashRange,
  pub longitude: GeoHashRange,
}

/// Domain operation (aligned with Kvrocks GeoHashNeighbors).
/// Geohash 8 邻域结构（对标 Kvrocks GeoHashNeighbors）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GeoHashNeighbors {
  pub north: GeoHashBits,
  pub east: GeoHashBits,
  pub west: GeoHashBits,
  pub south: GeoHashBits,
  pub north_east: GeoHashBits,
  pub south_east: GeoHashBits,
  pub north_west: GeoHashBits,
  pub south_west: GeoHashBits,
}

/// Domain operation (aligned with Kvrocks GeoHashRadius).
/// Geohash 范围查询多步长邻域集合（对标 Kvrocks GeoHashRadius）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GeoHashRadius {
  pub hash: GeoHashBits,
  pub area: GeoHashArea,
  pub neighbors: GeoHashNeighbors,
}

/// Operation definition.
/// 空间查询形状类型
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  Encode,
  Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum GeoShapeType {
  #[default]
  None,
  Circular,
  Rectangular,
}

/// Domain operation (aligned with Kvrocks GeoShape).
/// 空间查询几何形状（对标 Kvrocks GeoShape）
#[derive(Debug, Clone, PartialEq, Default, Encode, Decode)]
pub struct GeoShape {
  pub shape_type: GeoShapeType,
  pub center_lon: f64,
  pub center_lat: f64,
  pub radius: f64,
  pub width: f64,
  pub height: f64,
  pub conversion: f64,
  pub bounds: [f64; 4],
}

/// Operation definition.
/// 空间地理范围检索选项
#[derive(Debug, Clone, PartialEq, Default, Encode, Decode)]
pub struct GeoRadius {
  pub with_coord: bool,
  pub with_dist: bool,
  pub with_hash: bool,
  pub count: Option<usize>,
  pub any: bool,
  pub sort: DistanceSort,
  pub store_key: Option<String>,
  pub store_dist_key: Option<String>,
  pub unit: DistanceUnit,
}

/// Command options enumeration.
/// GEOSEARCH 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoSearch {
  WithCoord,
  WithDist,
  WithHash,
  Count(usize),
  Any,
  Asc,
  Desc,
  Unit(DistanceUnit),
}

/// Command options enumeration.
/// GEOSEARCHSTORE 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoSearchStore {
  Count(usize),
  Any,
  Asc,
  Desc,
  StoreDist,
  Unit(DistanceUnit),
}
