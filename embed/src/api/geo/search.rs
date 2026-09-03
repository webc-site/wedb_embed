use std::str::from_utf8;

use crate::{
  api::{
    geo::{
      codec::{
        GEO_STEP_MAX, bounding_box, geohash_decode_wgs84, get_areas_by_shape_wgs84,
        haversine_distance, scores_of_geohash_box, validate_long_lat,
      },
      opt::{
        DistanceSort, DistanceUnit, GeoHashBits, GeoPoint, GeoRadius, GeoSearch, GeoSearchStore,
        GeoShape, GeoShapeType, OriginPoint,
      },
    },
    zset::opt::RangeScore,
  },
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};

#[inline]
pub(crate) fn sort_and_truncate_points(
  points: &mut Vec<GeoPoint>,
  mut sort: DistanceSort,
  count: Option<usize>,
  any: bool,
) {
  if count.is_some() && !any && sort == DistanceSort::None {
    sort = DistanceSort::Asc;
  }

  if let Some(limit) = count {
    if limit == 0 {
      points.clear();
      return;
    }
    match sort {
      DistanceSort::Asc => {
        if limit < points.len() {
          points.select_nth_unstable_by(limit, |a, b| a.dist.total_cmp(&b.dist));
          points.truncate(limit);
        }
        points.sort_unstable_by(|a, b| a.dist.total_cmp(&b.dist));
      }
      DistanceSort::Desc => {
        if limit < points.len() {
          points.select_nth_unstable_by(limit, |a, b| b.dist.total_cmp(&a.dist));
          points.truncate(limit);
        }
        points.sort_unstable_by(|a, b| b.dist.total_cmp(&a.dist));
      }
      DistanceSort::None => {
        points.truncate(limit);
      }
    }
  } else {
    match sort {
      DistanceSort::Asc => {
        points.sort_unstable_by(|a, b| a.dist.total_cmp(&b.dist));
      }
      DistanceSort::Desc => {
        points.sort_unstable_by(|a, b| b.dist.total_cmp(&a.dist));
      }
      DistanceSort::None => {}
    }
  }
}

/// Geospatial search by shape engine.
/// 地理空间多边形与圆形区域检索核心实现
pub(crate) fn search_shape_internal<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  key: K,
  geo_shape: &mut GeoShape,
  unit: DistanceUnit,
  count: Option<usize>,
  any: bool,
) -> Result<Vec<GeoPoint>>
where
  Error: From<E::Error>,
{
  if db.zcard(key.as_ref())? == 0 {
    return Ok(Vec::new());
  }

  let georadius = get_areas_by_shape_wgs84(geo_shape);
  let raw_neighbors = [
    georadius.hash,
    georadius.neighbors.north,
    georadius.neighbors.south,
    georadius.neighbors.east,
    georadius.neighbors.west,
    georadius.neighbors.north_east,
    georadius.neighbors.north_west,
    georadius.neighbors.south_east,
    georadius.neighbors.south_west,
  ];

  let mut unique_hashes = [GeoHashBits::default(); 9];
  let mut unique_count = 0usize;
  for hash in raw_neighbors {
    if hash.is_zero() {
      continue;
    }
    if !unique_hashes[..unique_count].contains(&hash) {
      unique_hashes[unique_count] = hash;
      unique_count += 1;
    }
  }

  let mut points = Vec::new();
  let center_lon = geo_shape.center_lon;
  let center_lat = geo_shape.center_lat;
  let max_radius_meters = geo_shape.radius * geo_shape.conversion;
  let bounds = geo_shape.bounds;
  let shape_type = geo_shape.shape_type;

  for &hash in &unique_hashes[..unique_count] {
    let (min_bits, max_bits) = scores_of_geohash_box(hash);
    let spec = RangeScore {
      min: min_bits as f64,
      max: max_bits as f64,
      minex: false,
      maxex: true,
      offset: 0,
      count: None,
    };

    // 零分配流式扫描：消除 zrangebyscore 的中间堆缓冲与无谓 to_vec()
    db.ziter_range_byscore(&key, &spec, |member_bytes, score| {
      let bits = GeoHashBits {
        bits: score as u64,
        step: GEO_STEP_MAX,
      };
      let (pt_lon, pt_lat) = geohash_decode_wgs84(bits);

      let (is_inside, d_meters) = match shape_type {
        GeoShapeType::Circular => {
          let d = haversine_distance(center_lon, center_lat, pt_lon, pt_lat);
          (d <= max_radius_meters, d)
        }
        GeoShapeType::Rectangular => {
          if pt_lon >= bounds[0]
            && pt_lon <= bounds[2]
            && pt_lat >= bounds[1]
            && pt_lat <= bounds[3]
          {
            let d = haversine_distance(center_lon, center_lat, pt_lon, pt_lat);
            (true, d)
          } else {
            (false, 0.0)
          }
        }
        GeoShapeType::None => (false, 0.0),
      };

      if is_inside {
        let member_str = from_utf8(member_bytes)
          .map(ToOwned::to_owned)
          .unwrap_or_else(|_| String::from_utf8_lossy(member_bytes).into_owned());
        let dist = unit.from_meters(d_meters);
        points.push(GeoPoint {
          longitude: pt_lon,
          latitude: pt_lat,
          member: member_str,
          dist,
          score,
        });

        // ANY 语义短路：一旦收集满目标条数，立即终止后续扫描，实现 O(K) 响应
        if any
          && let Some(limit) = count
          && points.len() >= limit
        {
          return false;
        }
      }
      true
    })?;

    if any
      && let Some(limit) = count
      && points.len() >= limit
    {
      break;
    }
  }

  Ok(points)
}

/// Geospatial radius and shape search interfaces (GEORADIUS, GEOSEARCH, GEOSEARCHSTORE).
/// 地理空间半径与形状搜索操作接口
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn georadius<K: AsRef<[u8]>>(
    &self,
    key: K,
    longitude: f64,
    latitude: f64,
    radius: f64,
    opt: &GeoRadius,
  ) -> Result<Vec<GeoPoint>> {
    validate_long_lat(longitude, latitude)?;
    if radius.is_nan() || radius.is_infinite() || radius < 0.0 {
      return Err(Error::invalid_data(
        "ERR radius must be greater than or equal to 0",
      ));
    }

    if self.zcard(key.as_ref())? == 0 {
      if let Some(ref store_k) = opt.store_key {
        self.del(&[store_k])?;
      }
      if let Some(ref store_dist_k) = opt.store_dist_key {
        self.del(&[store_dist_k])?;
      }
      return Ok(Vec::new());
    }

    let mut shape = GeoShape::new_circular_with_unit(longitude, latitude, radius, opt.unit);
    let mut points = search_shape_internal(self, &key, &mut shape, opt.unit, opt.count, opt.any)?;

    sort_and_truncate_points(&mut points, opt.sort, opt.count, opt.any);

    if let Some(ref store_k) = opt.store_key {
      self.del(&[store_k])?;
      if !points.is_empty() {
        let items: Vec<(f64, &str)> = points
          .iter()
          .map(|p| (p.score, p.member.as_str()))
          .collect();
        self.zadd(store_k, &items, [])?;
      }
    }

    if let Some(ref store_dist_k) = opt.store_dist_key {
      self.del(&[store_dist_k])?;
      if !points.is_empty() {
        let items: Vec<(f64, &str)> = points.iter().map(|p| (p.dist, p.member.as_str())).collect();
        self.zadd(store_dist_k, &items, [])?;
      }
    }

    Ok(points)
  }

  #[inline]
  pub fn georadiusbymember<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    member: M,
    radius: f64,
    opt: &GeoRadius,
  ) -> Result<Vec<GeoPoint>> {
    let k_ref = key.as_ref();
    let m_ref = member.as_ref();

    if self.zcard(k_ref)? == 0 {
      if let Some(ref store_k) = opt.store_key {
        self.del(&[store_k])?;
      }
      if let Some(ref store_dist_k) = opt.store_dist_key {
        self.del(&[store_dist_k])?;
      }
      return Ok(Vec::new());
    }

    let pos = self.geopos(k_ref, &[m_ref])?;
    if let Some(Some((lon, lat))) = pos.into_iter().next() {
      self.georadius(k_ref, lon, lat, radius, opt)
    } else {
      Err(Error::invalid_data(
        "ERR could not decode requested zset member",
      ))
    }
  }

  #[inline]
  pub fn geosearch<K: AsRef<[u8]>>(
    &self,
    key: K,
    origin: &OriginPoint,
    shape: &mut GeoShape,
    opt_li: impl IntoIterator<Item = GeoSearch>,
  ) -> Result<Vec<GeoPoint>> {
    let mut unit = DistanceUnit::Meters;
    let mut count = None;
    let mut any = false;
    let mut sort = DistanceSort::None;

    for o in opt_li {
      match o {
        GeoSearch::WithCoord | GeoSearch::WithDist | GeoSearch::WithHash => {}
        GeoSearch::Count(c) => count = Some(c),
        GeoSearch::Any => any = true,
        GeoSearch::Asc => sort = DistanceSort::Asc,
        GeoSearch::Desc => sort = DistanceSort::Desc,
        GeoSearch::Unit(u) => unit = u,
      }
    }

    let k_ref = key.as_ref();
    if self.zcard(k_ref)? == 0 {
      return Ok(Vec::new());
    }

    let (lon, lat) = match origin {
      OriginPoint::Coord { lon, lat } => {
        validate_long_lat(*lon, *lat)?;
        (*lon, *lat)
      }
      OriginPoint::Member(m) => {
        let pos = self.geopos(k_ref, &[m.as_bytes()])?;
        match pos.into_iter().next().flatten() {
          Some((l, t)) => (l, t),
          None => {
            return Err(Error::invalid_data(
              "ERR could not decode requested zset member",
            ));
          }
        }
      }
    };

    shape.center_lon = lon;
    shape.center_lat = lat;
    bounding_box(shape);

    let mut points = search_shape_internal(self, k_ref, shape, unit, count, any)?;
    sort_and_truncate_points(&mut points, sort, count, any);
    Ok(points)
  }

  #[inline]
  pub fn geosearchstore<K: AsRef<[u8]>, D: AsRef<[u8]>>(
    &self,
    destination: D,
    source: K,
    origin: &OriginPoint,
    shape: &mut GeoShape,
    opt_li: impl IntoIterator<Item = GeoSearchStore>,
  ) -> Result<usize> {
    let mut unit = DistanceUnit::Meters;
    let mut count = None;
    let mut any = false;
    let mut sort = DistanceSort::None;
    let mut store_dist = false;

    for o in opt_li {
      match o {
        GeoSearchStore::Count(c) => count = Some(c),
        GeoSearchStore::Any => any = true,
        GeoSearchStore::Asc => sort = DistanceSort::Asc,
        GeoSearchStore::Desc => sort = DistanceSort::Desc,
        GeoSearchStore::StoreDist => store_dist = true,
        GeoSearchStore::Unit(u) => unit = u,
      }
    }

    let dest_ref = destination.as_ref();
    let src_ref = source.as_ref();

    let (lon, lat) = match origin {
      OriginPoint::Coord { lon, lat } => {
        validate_long_lat(*lon, *lat)?;
        if self.zcard(src_ref)? == 0 {
          self.del(&[dest_ref])?;
          return Ok(0);
        }
        (*lon, *lat)
      }
      OriginPoint::Member(m) => {
        if self.zcard(src_ref)? == 0 {
          self.del(&[dest_ref])?;
          return Ok(0);
        }
        let pos = self.geopos(src_ref, &[m.as_bytes()])?;
        match pos.into_iter().next().flatten() {
          Some((l, t)) => (l, t),
          None => {
            return Err(Error::invalid_data(
              "ERR could not decode requested zset member",
            ));
          }
        }
      }
    };

    shape.center_lon = lon;
    shape.center_lat = lat;
    bounding_box(shape);

    let mut points = search_shape_internal(self, src_ref, shape, unit, count, any)?;
    sort_and_truncate_points(&mut points, sort, count, any);

    self.del(&[dest_ref])?;
    if points.is_empty() {
      return Ok(0);
    }

    let items: Vec<(f64, &str)> = points
      .iter()
      .map(|p| {
        let score = if store_dist { p.dist } else { p.score };
        (score, p.member.as_str())
      })
      .collect();

    self.zadd(dest_ref, &items, [])?;
    Ok(items.len())
  }
}
