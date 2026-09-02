pub use super::r#const::*;
use super::opt::{
  DistanceUnit, GeoHashArea, GeoHashBits, GeoHashNeighbors, GeoHashRadius, GeoHashRange, GeoPoint,
  GeoShape, GeoShapeType,
};
use crate::error::{Error, Result};

/// Global longitude range for WGS84 indexing [-180, 180].
/// 经度全局范围（WGS84 索引范围）
pub const GEO_LON_RANGE: GeoHashRange = GeoHashRange {
  min: GEO_LON_MIN,
  max: GEO_LON_MAX,
};

/// Global latitude range for WGS84 indexing [-85.05112878, 85.05112878].
/// 纬度全局范围（WGS84 索引范围）
pub const GEO_LAT_RANGE: GeoHashRange = GeoHashRange {
  min: GEO_LAT_MIN,
  max: GEO_LAT_MAX,
};

/// Standard Geohash longitude range.
/// 标准 Geohash 经度范围
pub const GEO_LON_RANGE_STANDARD: GeoHashRange = GeoHashRange {
  min: -180.0,
  max: 180.0,
};

/// Standard Geohash latitude range.
/// 标准 Geohash 纬度范围
pub const GEO_LAT_RANGE_STANDARD: GeoHashRange = GeoHashRange {
  min: -90.0,
  max: 90.0,
};

/// Validates whether longitude and latitude coordinates fall within valid bounds aligned with Kvrocks ValidateLongLat.
/// 校验经纬度坐标是否在合法范围内（对标 Kvrocks ValidateLongLat）
#[inline]
pub fn validate_long_lat(lon: f64, lat: f64) -> Result<()> {
  if lon.is_nan()
    || lat.is_nan()
    || lon.is_infinite()
    || lat.is_infinite()
    || !(GEO_LON_MIN..=GEO_LON_MAX).contains(&lon)
    || !(GEO_LAT_MIN..=GEO_LAT_MAX).contains(&lat)
  {
    let mut b_lon = zmij::Buffer::new();
    let mut b_lat = zmij::Buffer::new();
    let s_lon = b_lon.format(lon);
    let s_lat = b_lat.format(lat);
    let mut msg = String::with_capacity(36 + s_lon.len() + 1 + s_lat.len());
    msg.push_str("ERR invalid longitude,latitude pair ");
    msg.push_str(s_lon);
    msg.push(',');
    msg.push_str(s_lat);
    return Err(Error::invalid_data(msg));
  }
  Ok(())
}

/// Interleaves 32-bit longitude and latitude bits into a 64-bit integer aligned with Kvrocks Interleave64.
/// 64 位整数交替编织（经纬度比特交错，对标 Kvrocks Interleave64）
#[inline(always)]
pub const fn interleave64(xlo: u32, ylo: u32) -> u64 {
  const B: [u64; 5] = [
    0x5555555555555555,
    0x3333333333333333,
    0x0F0F0F0F0F0F0F0F,
    0x00FF00FF00FF00FF,
    0x0000FFFF0000FFFF,
  ];
  const S: [u32; 5] = [1, 2, 4, 8, 16];

  let mut x = xlo as u64;
  let mut y = ylo as u64;

  x = (x | (x << S[4])) & B[4];
  y = (y | (y << S[4])) & B[4];

  x = (x | (x << S[3])) & B[3];
  y = (y | (y << S[3])) & B[3];

  x = (x | (x << S[2])) & B[2];
  y = (y | (y << S[2])) & B[2];

  x = (x | (x << S[1])) & B[1];
  y = (y | (y << S[1])) & B[1];

  x = (x | (x << S[0])) & B[0];
  y = (y | (y << S[0])) & B[0];

  x | (y << 1)
}

/// Deinterleaves a 64-bit integer into separate longitude and latitude bits aligned with Kvrocks Deinterleave64.
/// 64 位整数反交替解织（分离纬度和经度比特，对标 Kvrocks Deinterleave64）
#[inline(always)]
pub const fn deinterleave64(interleaved: u64) -> (u32, u32) {
  const B: [u64; 6] = [
    0x5555555555555555,
    0x3333333333333333,
    0x0F0F0F0F0F0F0F0F,
    0x00FF00FF00FF00FF,
    0x0000FFFF0000FFFF,
    0x00000000FFFFFFFF,
  ];
  const S: [u32; 6] = [0, 1, 2, 4, 8, 16];

  let mut x = interleaved;
  let mut y = interleaved >> 1;

  x = (x | (x >> S[0])) & B[0];
  y = (y | (y >> S[0])) & B[0];

  x = (x | (x >> S[1])) & B[1];
  y = (y | (y >> S[1])) & B[1];

  x = (x | (x >> S[2])) & B[2];
  y = (y | (y >> S[2])) & B[2];

  x = (x | (x >> S[3])) & B[3];
  y = (y | (y >> S[3])) & B[3];

  x = (x | (x >> S[4])) & B[4];
  y = (y | (y >> S[4])) & B[4];

  x = (x | (x >> S[5])) & B[5];
  y = (y | (y >> S[5])) & B[5];

  (x as u32, y as u32)
}

/// Encodes data into binary format.
/// Geohash 范围编码（对标 Kvrocks GeohashEncode）
#[inline]
pub fn geohash_encode(
  long_range: &GeoHashRange,
  lat_range: &GeoHashRange,
  longitude: f64,
  latitude: f64,
  step: u8,
) -> Option<GeoHashBits> {
  if step == 0 || step > 32 || long_range.is_zero() || lat_range.is_zero() {
    return None;
  }
  if longitude > GEO_LON_MAX
    || longitude < GEO_LON_MIN
    || latitude > GEO_LAT_MAX
    || latitude < GEO_LAT_MIN
  {
    return None;
  }
  if latitude < lat_range.min
    || latitude > lat_range.max
    || longitude < long_range.min
    || longitude > long_range.max
  {
    return None;
  }

  let lat_offset = (latitude - lat_range.min) / (lat_range.max - lat_range.min);
  let long_offset = (longitude - long_range.min) / (long_range.max - long_range.min);

  let max_val = (1u64 << step) as f64;
  let lat_val = (lat_offset * max_val) as u32;
  let long_val = (long_offset * max_val) as u32;

  let bits = interleave64(lat_val, long_val);
  Some(GeoHashBits { bits, step })
}

/// Decodes data from binary format.
/// Geohash 范围解码（对标 Kvrocks GeohashDecode）
#[inline]
pub fn geohash_decode(
  long_range: &GeoHashRange,
  lat_range: &GeoHashRange,
  hash: GeoHashBits,
) -> GeoHashArea {
  if hash.is_zero() || lat_range.is_zero() || long_range.is_zero() {
    return GeoHashArea::default();
  }

  let (ilato, ilono) = deinterleave64(hash.bits);
  let lat_scale = lat_range.max - lat_range.min;
  let long_scale = long_range.max - long_range.min;
  let max_val = (1u64 << hash.step) as f64;

  let lat_min = lat_range.min + (ilato as f64 / max_val) * lat_scale;
  let lat_max = lat_range.min + ((ilato + 1) as f64 / max_val) * lat_scale;
  let lon_min = long_range.min + (ilono as f64 / max_val) * long_scale;
  let lon_max = long_range.min + ((ilono + 1) as f64 / max_val) * long_scale;

  GeoHashArea {
    hash,
    longitude: GeoHashRange {
      min: lon_min,
      max: lon_max,
    },
    latitude: GeoHashRange {
      min: lat_min,
      max: lat_max,
    },
  }
}

/// Decodes data from binary format.
/// 将解码区域转换为中心经纬度坐标（对标 Kvrocks GeohashDecodeAreaToLongLat）
#[inline]
pub fn geohash_decode_area_to_long_lat(area: &GeoHashArea) -> (f64, f64) {
  let lon = ((area.longitude.min + area.longitude.max) * 0.5).clamp(GEO_LON_MIN, GEO_LON_MAX);
  let lat = ((area.latitude.min + area.latitude.max) * 0.5).clamp(GEO_LAT_MIN, GEO_LAT_MAX);
  (lon, lat)
}

/// Aligns a Geohash to 52 bits aligned with Kvrocks GeoHashHelper::Align52Bits.
/// 52 位对齐（对标 Kvrocks GeoHashHelper::Align52Bits）
#[inline(always)]
pub const fn align_52bits(hash: GeoHashBits) -> u64 {
  let shift = 52 - hash.step * 2;
  hash.bits << shift
}

/// Encodes data into binary format.
/// 将经纬度编码为 52 位的 Geohash 整数值（对标 Kvrocks Geo::Add 编码）
#[inline]
pub fn encode_geohash(lon: f64, lat: f64) -> u64 {
  geohash_encode(&GEO_LON_RANGE, &GEO_LAT_RANGE, lon, lat, GEO_STEP_MAX)
    .map(align_52bits)
    .unwrap_or(0)
}

/// Decodes data from binary format.
/// 将 52 位 Geohash 解码回 (lon, lat)（对标 Kvrocks Geo::decodeGeoHash）
#[inline]
pub fn decode_geohash(hash: u64) -> (f64, f64) {
  let area = geohash_decode(
    &GEO_LON_RANGE,
    &GEO_LAT_RANGE,
    GeoHashBits {
      bits: hash,
      step: GEO_STEP_MAX,
    },
  );
  geohash_decode_area_to_long_lat(&area)
}

/// Encodes data into binary format.
/// 将经纬度编码为 11 字节 Base32 字符数组（零堆分配，与 Redis/Kvrocks 规范 100% 兼容）
#[inline]
pub fn encode_geohash_bytes(lon: f64, lat: f64) -> [u8; 11] {
  let encoded = geohash_encode(
    &GEO_LON_RANGE_STANDARD,
    &GEO_LAT_RANGE_STANDARD,
    lon,
    lat,
    26,
  )
  .unwrap_or_default();

  let mut buf = [0u8; 11];
  for (i, byte) in buf.iter_mut().enumerate() {
    let idx = if i == 10 {
      0
    } else {
      ((encoded.bits >> (52 - (i + 1) * 5)) & 0x1f) as usize
    };
    *byte = BASE32_ALPHABET[idx];
  }
  buf
}

/// Encodes data into binary format.
/// 将经纬度编码为 11 位 Base32 字符串（与 Redis/Kvrocks 规范 100% 兼容）
#[inline]
pub fn encode_geohash_string(lon: f64, lat: f64) -> String {
  let buf = encode_geohash_bytes(lon, lat);
  str::from_utf8(&buf).unwrap_or("").to_string()
}

/// Encodes a 52-bit Geohash into an 11-character Base32 string aligned with Kvrocks Geo::EncodeGeoHash.
/// 将 52 位 Geohash 转换为 11 位 Base32 字符串（对标 Kvrocks Geo::EncodeGeoHash）
#[inline]
pub fn geohash_to_base32(hash: u64) -> String {
  let (lon, lat) = decode_geohash(hash);
  encode_geohash_string(lon, lat)
}

#[inline]
pub fn coords_to_base32(lon: f64, lat: f64) -> Result<String> {
  validate_long_lat(lon, lat)?;
  Ok(encode_geohash_string(lon, lat))
}

/// Decodes data from binary format.
/// 将 Base32 Geohash 字符串解码为坐标 (lon, lat)（O(1) 查找表加速）
pub fn base32_to_coords(s: &str) -> Result<(f64, f64)> {
  let s_bytes = s.as_bytes();
  if s_bytes.is_empty() || s_bytes.len() > 11 {
    return Err(Error::invalid_data("ERR invalid geohash string length"));
  }
  let mut bits: u64 = 0;
  for (i, &ch) in s_bytes.iter().enumerate() {
    let val = BASE32_DECODE_TABLE[ch as usize];
    if val == 0xFF {
      return Err(Error::invalid_data("ERR invalid character in geohash"));
    }
    if i < 10 {
      bits |= (val as u64) << (52 - (i + 1) * 5);
    } else {
      bits |= ((val & 0x18) as u64) >> 3;
    }
  }
  let area = geohash_decode(
    &GEO_LON_RANGE_STANDARD,
    &GEO_LAT_RANGE_STANDARD,
    GeoHashBits { bits, step: 26 },
  );
  Ok(geohash_decode_area_to_long_lat(&area))
}

/// Shifts Geohash block along the X axis aligned with Kvrocks GeohashMoveX.
/// X 轴移动 Geohash 块（对标 Kvrocks GeohashMoveX）
#[inline]
pub fn geohash_move_x(hash: &mut GeoHashBits, d: i8) {
  if d == 0 || hash.step == 0 || hash.step > 26 {
    return;
  }
  let mut x = hash.bits & 0xaaaaaaaaaaaaaaaa;
  let y = hash.bits & 0x5555555555555555;
  let shift = 64 - hash.step * 2;
  let zz = 0x5555555555555555 >> shift;

  if d > 0 {
    x = x.wrapping_add(zz + 1);
  } else {
    x = (x | zz).wrapping_sub(zz + 1);
  }
  x &= 0xaaaaaaaaaaaaaaaa >> shift;
  hash.bits = x | y;
}

/// Shifts Geohash block along the Y axis aligned with Kvrocks GeohashMoveY.
/// Y 轴移动 Geohash 块（对标 Kvrocks GeohashMoveY）
#[inline]
pub fn geohash_move_y(hash: &mut GeoHashBits, d: i8) {
  if d == 0 || hash.step == 0 || hash.step > 26 {
    return;
  }
  let x = hash.bits & 0xaaaaaaaaaaaaaaaa;
  let mut y = hash.bits & 0x5555555555555555;
  let shift = 64 - hash.step * 2;
  let zz = 0xaaaaaaaaaaaaaaaa >> shift;

  if d > 0 {
    y = y.wrapping_add(zz + 1);
  } else {
    y = (y | zz).wrapping_sub(zz + 1);
  }
  y &= 0x5555555555555555 >> shift;
  hash.bits = x | y;
}

/// Calculates Geohash values for all 8 neighboring grid cells aligned with Kvrocks GeohashNeighbors.
/// 获取 8 邻居区域的 Geohash（对标 Kvrocks GeohashNeighbors）
pub fn geohash_neighbors(hash: &GeoHashBits) -> GeoHashNeighbors {
  let mut n = GeoHashNeighbors {
    north: *hash,
    east: *hash,
    west: *hash,
    south: *hash,
    north_east: *hash,
    south_east: *hash,
    north_west: *hash,
    south_west: *hash,
  };

  geohash_move_x(&mut n.east, 1);
  geohash_move_x(&mut n.west, -1);
  geohash_move_y(&mut n.south, -1);
  geohash_move_y(&mut n.north, 1);

  geohash_move_x(&mut n.north_west, -1);
  geohash_move_y(&mut n.north_west, 1);

  geohash_move_x(&mut n.north_east, 1);
  geohash_move_y(&mut n.north_east, 1);

  geohash_move_x(&mut n.south_east, 1);
  geohash_move_y(&mut n.south_east, -1);

  geohash_move_x(&mut n.south_west, -1);
  geohash_move_y(&mut n.south_west, -1);

  n
}

/// Estimates Geohash precision step count based on search radius aligned with Kvrocks GeoHashHelper::EstimateStepsByRadius.
/// 根据半径估算 Geohash 搜索步长（精度比特数，对标 Kvrocks GeoHashHelper::EstimateStepsByRadius）
#[inline]
pub fn estimate_steps_by_radius(mut range_meters: f64, lat: f64) -> u8 {
  if range_meters <= 0.0 {
    return 26;
  }
  let mut step = 1i32;
  while range_meters < MERCATOR_MAX {
    range_meters *= 2.0;
    step += 1;
  }
  step -= 2;

  if !(-66.0..=66.0).contains(&lat) {
    step -= 1;
    if !(-80.0..=80.0).contains(&lat) {
      step -= 1;
    }
  }

  step.clamp(1, 26) as u8
}

/// Computes bounding box for a spatial search shape aligned with Kvrocks GeoHashHelper::BoundingBox.
/// 计算搜索形状的外接边界盒（对标 Kvrocks GeoHashHelper::BoundingBox）
pub fn bounding_box(geo_shape: &mut GeoShape) {
  let longitude = geo_shape.center_lon;
  let latitude = geo_shape.center_lat;
  let height = geo_shape.conversion
    * if geo_shape.shape_type == GeoShapeType::Circular {
      geo_shape.radius
    } else {
      geo_shape.height * 0.5
    };
  let width = geo_shape.conversion
    * if geo_shape.shape_type == GeoShapeType::Circular {
      geo_shape.radius
    } else {
      geo_shape.width * 0.5
    };

  let lat_delta = (height / EARTH_RADIUS_METERS) / D_R;
  let lat_top_rad = (latitude + lat_delta) * D_R;
  let lat_bottom_rad = (latitude - lat_delta) * D_R;

  let long_delta_top = (width / EARTH_RADIUS_METERS / lat_top_rad.cos()) / D_R;
  let long_delta_bottom = (width / EARTH_RADIUS_METERS / lat_bottom_rad.cos()) / D_R;

  let is_south = latitude < 0.0;
  let long_delta = if is_south {
    long_delta_bottom
  } else {
    long_delta_top
  };

  geo_shape.bounds[0] = longitude - long_delta;
  geo_shape.bounds[1] = latitude - lat_delta;
  geo_shape.bounds[2] = longitude + long_delta;
  geo_shape.bounds[3] = latitude + lat_delta;
}

/// Computes 9-neighbor Geohash search areas covering a shape aligned with Kvrocks GeoHashHelper::GetAreasByShapeWGS84.
/// 获取形状覆盖的 9 邻居 Geohash 检索区域（对标 Kvrocks GeoHashHelper::GetAreasByShapeWGS84）
pub fn get_areas_by_shape_wgs84(geo_shape: &mut GeoShape) -> GeoHashRadius {
  bounding_box(geo_shape);
  let min_lon = geo_shape.bounds[0];
  let min_lat = geo_shape.bounds[1];
  let max_lon = geo_shape.bounds[2];
  let max_lat = geo_shape.bounds[3];

  let longitude = geo_shape.center_lon;
  let latitude = geo_shape.center_lat;
  let radius_meters = geo_shape.conversion
    * if geo_shape.shape_type == GeoShapeType::Circular {
      geo_shape.radius
    } else {
      (geo_shape.width * 0.5).hypot(geo_shape.height * 0.5)
    };

  let mut steps = estimate_steps_by_radius(radius_meters, latitude);

  let mut hash =
    geohash_encode(&GEO_LON_RANGE, &GEO_LAT_RANGE, longitude, latitude, steps).unwrap_or_default();
  let mut neighbors = geohash_neighbors(&hash);
  let mut area = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, hash);

  let decrease_step = {
    let north = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, neighbors.north);
    haversine_distance(longitude, latitude, longitude, north.latitude.max) < radius_meters
      || {
        let south = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, neighbors.south);
        haversine_distance(longitude, latitude, longitude, south.latitude.min) < radius_meters
      }
      || {
        let east = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, neighbors.east);
        haversine_distance(longitude, latitude, east.longitude.max, latitude) < radius_meters
      }
      || {
        let west = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, neighbors.west);
        haversine_distance(longitude, latitude, west.longitude.min, latitude) < radius_meters
      }
  };

  if steps > 1 && decrease_step {
    steps -= 1;
    hash = geohash_encode(&GEO_LON_RANGE, &GEO_LAT_RANGE, longitude, latitude, steps)
      .unwrap_or_default();
    neighbors = geohash_neighbors(&hash);
    area = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, hash);
  }

  if steps >= 2 {
    if area.latitude.min < min_lat {
      neighbors.south = GeoHashBits::default();
      neighbors.south_west = GeoHashBits::default();
      neighbors.south_east = GeoHashBits::default();
    }
    if area.latitude.max > max_lat {
      neighbors.north = GeoHashBits::default();
      neighbors.north_east = GeoHashBits::default();
      neighbors.north_west = GeoHashBits::default();
    }
    if area.longitude.min < min_lon {
      neighbors.west = GeoHashBits::default();
      neighbors.south_west = GeoHashBits::default();
      neighbors.north_west = GeoHashBits::default();
    }
    if area.longitude.max > max_lon {
      neighbors.east = GeoHashBits::default();
      neighbors.south_east = GeoHashBits::default();
      neighbors.north_east = GeoHashBits::default();
    }
  }

  GeoHashRadius {
    hash,
    area,
    neighbors,
  }
}

/// Computes 52-bit ZSet score range [min, max) for a Geohash box aligned with Kvrocks Geo::scoresOfGeoHashBox.
/// 计算 Geohash 块对应的 52 位 ZSet 分值区间 [min, max)（对标 Kvrocks Geo::scoresOfGeoHashBox）
#[inline(always)]
pub const fn scores_of_geohash_box(hash: GeoHashBits) -> (u64, u64) {
  let min = align_52bits(hash);
  let next = GeoHashBits {
    bits: hash.bits.wrapping_add(1),
    step: hash.step,
  };
  let max = align_52bits(next);
  (min, max)
}

/// Calculates Haversine great-circle distance in meters between two coordinates aligned with Kvrocks GeoHashHelper::GetDistance.
/// 计算两个经纬度坐标之间的球面 Haversine 距离（米）（对标 Kvrocks GeoHashHelper::GetDistance）
#[inline]
pub fn haversine_distance(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
  if (lon1 - lon2).abs() < f64::EPSILON && (lat1 - lat2).abs() < f64::EPSILON {
    return 0.0;
  }
  let lat1r = lat1 * D_R;
  let lon1r = lon1 * D_R;
  let lat2r = lat2 * D_R;
  let lon2r = lon2 * D_R;

  let u = ((lat2r - lat1r) * 0.5).sin();
  let v = ((lon2r - lon1r) * 0.5).sin();

  let a = (u * u + lat1r.cos() * lat2r.cos() * v * v).clamp(0.0, 1.0);
  let c = 2.0 * a.sqrt().asin();

  EARTH_RADIUS_METERS * c
}

/// Converts distance in meters to target unit value.
/// 根据单位转换米数为目标单位数值
#[inline]
pub fn convert_meters_to_unit(meters: f64, unit: &str) -> f64 {
  if let Some(u) = DistanceUnit::parse(unit) {
    u.from_meters(meters)
  } else {
    meters
  }
}

/// Converts distance from source unit value to meters.
/// 根据目标单位转换距离为米数
#[inline]
pub fn convert_unit_to_meters(dist: f64, unit: &str) -> f64 {
  if let Some(u) = DistanceUnit::parse(unit) {
    u.to_meters(dist)
  } else {
    dist
  }
}

#[inline]
pub fn geohash_encode_wgs84(longitude: f64, latitude: f64, step: u8) -> Option<GeoHashBits> {
  geohash_encode(&GEO_LON_RANGE, &GEO_LAT_RANGE, longitude, latitude, step)
}

#[inline]
pub fn geohash_decode_wgs84(hash: GeoHashBits) -> (f64, f64) {
  let area = geohash_decode(&GEO_LON_RANGE, &GEO_LAT_RANGE, hash);
  geohash_decode_area_to_long_lat(&area)
}

impl GeoShape {
  pub fn new_circular(lon: f64, lat: f64, radius_meters: f64) -> Self {
    let mut shape = Self {
      shape_type: GeoShapeType::Circular,
      center_lon: lon,
      center_lat: lat,
      radius: radius_meters,
      width: 0.0,
      height: 0.0,
      conversion: 1.0,
      bounds: [0.0; 4],
    };
    bounding_box(&mut shape);
    shape
  }

  pub fn new_circular_with_unit(lon: f64, lat: f64, radius: f64, unit: DistanceUnit) -> Self {
    let mut shape = Self {
      shape_type: GeoShapeType::Circular,
      center_lon: lon,
      center_lat: lat,
      radius,
      width: 0.0,
      height: 0.0,
      conversion: unit.conversion_factor(),
      bounds: [0.0; 4],
    };
    bounding_box(&mut shape);
    shape
  }

  pub fn new_rectangular(lon: f64, lat: f64, width_meters: f64, height_meters: f64) -> Self {
    let mut shape = Self {
      shape_type: GeoShapeType::Rectangular,
      center_lon: lon,
      center_lat: lat,
      radius: 0.0,
      width: width_meters,
      height: height_meters,
      conversion: 1.0,
      bounds: [0.0; 4],
    };
    bounding_box(&mut shape);
    shape
  }

  pub fn new_rectangular_with_unit(
    lon: f64,
    lat: f64,
    width: f64,
    height: f64,
    unit: DistanceUnit,
  ) -> Self {
    let mut shape = Self {
      shape_type: GeoShapeType::Rectangular,
      center_lon: lon,
      center_lat: lat,
      radius: 0.0,
      width,
      height,
      conversion: unit.conversion_factor(),
      bounds: [0.0; 4],
    };
    bounding_box(&mut shape);
    shape
  }

  /// Checks whether coordinate point falls within current shape aligned with Kvrocks appendIfWithinShape.
  /// 检查经纬度坐标点是否在当前形状范围内（对标 Kvrocks appendIfWithinShape）
  #[inline]
  pub fn contains_point(&self, lon: f64, lat: f64) -> bool {
    match self.shape_type {
      GeoShapeType::Circular => {
        let d_meters = haversine_distance(self.center_lon, self.center_lat, lon, lat);
        d_meters <= self.radius * self.conversion
      }
      GeoShapeType::Rectangular => {
        lon >= self.bounds[0]
          && lon <= self.bounds[2]
          && lat >= self.bounds[1]
          && lat <= self.bounds[3]
      }
      GeoShapeType::None => false,
    }
  }

  /// Checks whether GeoPoint falls within current shape aligned with RediSearch/Geo.
  /// 检查 GeoPoint 是否在当前形状范围内（对标 RediSearch/Geo 规范）
  #[inline]
  pub fn point_in_radius(&self, pt: &GeoPoint) -> bool {
    self.contains_point(pt.longitude, pt.latitude)
  }
}
