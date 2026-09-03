use std::str::from_utf8;

use crate::{
  api::{
    geo::{
      codec::{
        GEO_STEP_MAX, align_52bits, encode_geohash_string, geohash_encode_wgs84,
        haversine_distance, score_to_coord, validate_long_lat,
      },
      opt::{DistanceUnit, GeoPoint},
    },
    zset::opt::ZAdd,
  },
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};
/// Geospatial indexing operations interface (GEO).
/// 地理空间索引接口 (GEO)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn geoadd_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    lon: f64,
    lat: f64,
    member: M,
  ) -> Result<usize> {
    self.geoadd(key, &[(lon, lat, member)], [])
  }

  #[inline]
  pub fn geoadd<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[(f64, f64, M)],
    opt_li: impl IntoIterator<Item = ZAdd>,
  ) -> Result<usize> {
    let mut score_members = Vec::with_capacity(items.len());
    for (lon, lat, member) in items {
      validate_long_lat(*lon, *lat)?;
      let bits = geohash_encode_wgs84(*lon, *lat, GEO_STEP_MAX)
        .ok_or_else(|| Error::invalid_data("ERR invalid longitude/latitude coordinates"))?;
      let score = align_52bits(bits) as f64;
      score_members.push((score, member));
    }
    self.zadd(key, &score_members, opt_li)
  }

  #[inline]
  pub fn geodist<K: AsRef<[u8]>, M1: AsRef<[u8]>, M2: AsRef<[u8]>>(
    &self,
    key: K,
    member1: M1,
    member2: M2,
    unit: Option<&str>,
  ) -> Result<Option<f64>> {
    let u = match unit {
      Some(s) => DistanceUnit::parse(s).ok_or_else(|| {
        Error::invalid_data("ERR unsupported unit provided. please use M, KM, FT, MI")
      })?,
      None => DistanceUnit::Meters,
    };

    let scores = self.zmscore(&key, &[member1.as_ref(), member2.as_ref()])?;
    if let (Some(Some(s1)), Some(Some(s2))) = (scores.first().copied(), scores.get(1).copied()) {
      let (lon1, lat1) = score_to_coord(s1);
      let (lon2, lat2) = score_to_coord(s2);
      let d_meters = haversine_distance(lon1, lat1, lon2, lat2);
      Ok(Some(u.from_meters(d_meters)))
    } else {
      Ok(None)
    }
  }

  #[inline]
  pub fn geopos_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    member: M,
  ) -> Result<Option<(f64, f64)>> {
    Ok(self.zscore(key, member)?.map(score_to_coord))
  }

  #[inline]
  pub fn geopos<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<Vec<Option<(f64, f64)>>> {
    let scores = self.zmscore(&key, members)?;
    let results = scores
      .into_iter()
      .map(|score| score.map(score_to_coord))
      .collect();
    Ok(results)
  }

  /// Retrieves complete GeoPoint for a member aligned with Kvrocks Geo::Get.
  /// 获取成员完整的 GeoPoint 信息（对标 Kvrocks Geo::Get）
  #[inline]
  pub fn geoget<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    member: M,
  ) -> Result<Option<GeoPoint>> {
    let mut res = self.geomget(key, &[member])?;
    Ok(res.pop().flatten())
  }

  /// Retrieves complete GeoPoints for multiple members aligned with Kvrocks Geo::MGet.
  /// 批量获取多个成员完整的 GeoPoint 信息（对标 Kvrocks Geo::MGet）
  #[inline]
  pub fn geomget<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<Vec<Option<GeoPoint>>> {
    let scores = self.zmscore(&key, members)?;
    let mut results = Vec::with_capacity(members.len());
    for (m, score_opt) in members.iter().zip(scores) {
      match score_opt {
        Some(s) => {
          let (lon, lat) = score_to_coord(s);
          let member_str = from_utf8(m.as_ref())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|_| String::from_utf8_lossy(m.as_ref()).into_owned());
          results.push(Some(GeoPoint {
            longitude: lon,
            latitude: lat,
            member: member_str,
            dist: 0.0,
            score: s,
          }));
        }
        None => results.push(None),
      }
    }
    Ok(results)
  }

  #[inline]
  pub fn geohash_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    member: M,
  ) -> Result<Option<String>> {
    Ok(self.zscore(key, member)?.map(|s| {
      let (lon, lat) = score_to_coord(s);
      encode_geohash_string(lon, lat)
    }))
  }

  #[inline]
  pub fn geohash<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<Vec<Option<String>>> {
    let scores = self.zmscore(&key, members)?;
    let results = scores
      .into_iter()
      .map(|score| {
        score.map(|s| {
          let (lon, lat) = score_to_coord(s);
          encode_geohash_string(lon, lat)
        })
      })
      .collect();
    Ok(results)
  }
}
