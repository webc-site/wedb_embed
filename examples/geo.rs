//! # Geospatial (Geo)
//!
//! ## Overview
//! Encodes longitude and latitude coordinates into 52-bit Geohash integers on top of sorted sets.
//! Supports point-to-point distance calculations, radius queries, and box/polygon spatial search.
//!
//! ## Use Cases
//! - Location-based services (LBS) and nearby point-of-interest (POI) discovery
//! - Ride-hailing and food delivery driver dispatching
//! - Asset and fleet geographical tracking
//! - Spatial polygon fencing and regional clustering
//!
//! ---
//!
//! # 地理空间位置
//!
//! ## 概述
//! 基于有序集合与 52 位经纬度编码存储二维地理坐标。
//! 支持两点间球面大圆距离计算、圆形半径搜索与空间多边形检索。
//!
//! ## 使用场景
//! - 附近兴趣点搜索与周边生活服务推荐
//! - 网约车与外卖配送骑手就近调度
//! - 物流车辆与移动资产实时定位追踪
//! - 地理围栏判定与区域数据归集

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  geo::{GeoRadius, GeoShape, OriginPoint},
  zset::ZAdd,
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Add geospatial coordinates
  // 添加地理坐标与带选项写入
  db.geoadd(
    b"cities",
    &[
      (116.4074, 39.9042, b"Beijing".as_slice()),
      (121.4737, 31.2304, b"Shanghai".as_slice()),
      (113.2644, 23.1291, b"Guangzhou".as_slice()),
    ],
    [],
  )?;
  db.geoadd(
    b"cities",
    &[(114.0579, 22.5431, b"Shenzhen".as_slice())],
    [ZAdd::Nx],
  )?;

  // Distance, positions, and geohash strings
  // 距离计算、坐标反查与编码字符串获取
  assert!(
    db.geodist(b"cities", b"Beijing", b"Shanghai", Some("km"))?
      .is_some()
  );
  assert_eq!(
    db.geopos(b"cities", &[b"Beijing".as_slice(), b"Shanghai".as_slice()])?
      .len(),
    2
  );
  assert_eq!(
    db.geohash(b"cities", &[b"Beijing".as_slice(), b"Shanghai".as_slice()])?
      .len(),
    2
  );

  // Radius search from coordinates and members
  // 按坐标与按成员半径范围搜索
  let _ = db.georadius(b"cities", 116.4, 39.9, 200.0, &GeoRadius::default())?;
  assert_eq!(
    db.georadiusbymember(b"cities", b"Beijing", 200.0, &GeoRadius::default())?
      .len(),
    1
  );

  // Polygon/box spatial search and storage
  // 空间多边形检索与结果持久化存储
  let origin = OriginPoint::coord(116.4074, 39.9042);
  let mut shape = GeoShape::new_circular(116.4074, 39.9042, 500_000.0);
  assert_eq!(db.geosearch(b"cities", &origin, &mut shape, [])?.len(), 1);
  assert_eq!(
    db.geosearchstore(b"stored_cities", b"cities", &origin, &mut shape, [])?,
    1
  );

  db.geoadd_one(b"cities", 13.361389, 38.115556, b"Palermo_one")?;
  let _ = db.geopos_one(b"cities", b"Palermo_one")?;
  let _ = db.geohash_one(b"cities", b"Palermo_one")?;

  println!("Geo 示例全部接口执行成功");
  Ok(())
}
