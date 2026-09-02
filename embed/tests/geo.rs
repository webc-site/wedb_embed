use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  geo::{
    DistanceSort, DistanceUnit, GeoRadius, GeoSearch, GeoSearchStore, GeoShape, OriginPoint,
    encode_geohash_string,
  },
  zset::opt::ZAdd,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_geo_basic_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert_eq!(
    db.geoadd(
      "Sicily",
      &[
        (13.361389, 38.115556, "Palermo"),
        (15.087269, 37.502669, "Catania"),
      ],
      []
    )?,
    2
  );

  // Duplicate add should return 0 new elements
  assert_eq!(
    db.geoadd(
      "Sicily",
      &[
        (13.361389, 38.115556, "Palermo"),
        (15.087269, 37.502669, "Catania"),
      ],
      []
    )?,
    0
  );

  // GEODIST
  let dist_m = db.geodist("Sicily", "Palermo", "Catania", None)?;
  assert!(dist_m.is_some());
  let m = dist_m.unwrap();
  assert!((m - 166274.0).abs() < 5000.0);

  let dist_km = db.geodist("Sicily", "Palermo", "Catania", Some("km"))?;
  assert!(dist_km.is_some());
  let km = dist_km.unwrap();
  assert!((km - 166.27).abs() < 5.0);

  let dist_ft = db.geodist("Sicily", "Palermo", "Catania", Some("ft"))?;
  assert!(dist_ft.is_some());
  let ft = dist_ft.unwrap();
  assert!((ft - (m / 0.3048)).abs() < 1e-3);

  let dist_mi = db.geodist("Sicily", "Palermo", "Catania", Some("mi"))?;
  assert!(dist_mi.is_some());
  let mi = dist_mi.unwrap();
  assert!((mi - (m / 1609.34)).abs() < 1e-3);

  // Same member distance should be 0.0
  let dist_same = db.geodist("Sicily", "Palermo", "Palermo", None)?;
  assert_eq!(dist_same, Some(0.0));

  // GEOPOS
  let pos = db.geopos("Sicily", &["Palermo", "Catania", "Nonexistent"])?;
  assert_eq!(pos.len(), 3);
  assert!(pos[0].is_some());
  let (lon, lat) = pos[0].unwrap();
  assert!((lon - 13.361389).abs() < 0.001);
  assert!((lat - 38.115556).abs() < 0.001);
  assert!(pos[1].is_some());
  assert_eq!(pos[2], None);

  // GEOHASH
  let hashes = db.geohash("Sicily", &["Palermo", "Catania", "Nonexistent"])?;
  assert_eq!(hashes.len(), 3);
  assert!(hashes[0].is_some());
  assert_eq!(hashes[0].as_ref().unwrap().len(), 11);
  assert!(hashes[1].is_some());
  assert_eq!(hashes[2], None);

  Ok(())
}

#[test]
fn test_kvrocks_geo_compatibility() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let key = "test_geo_key";
  let fields = [
    "geo_test_key-1",
    "geo_test_key-2",
    "geo_test_key-3",
    "geo_test_key-4",
    "geo_test_key-5",
    "geo_test_key-6",
    "geo_test_key-7",
  ];
  let longitudes = [-180.0, -1.23402, -1.23402, 0.0, 1.23402, 1.23402, 179.12345];
  let latitudes = [
    -85.05112878,
    -1.23402,
    -1.23402,
    0.0,
    1.23402,
    1.23402,
    85.0511,
  ];
  let expected_hashes = [
    "00bh0hbj200",
    "7zz0gzm7m10",
    "7zz0gzm7m10",
    "s0000000000",
    "s00zh0dsdy0",
    "s00zh0dsdy0",
    "zzp7u51dwf0",
  ];

  let items: Vec<(f64, f64, &str)> = fields
    .iter()
    .enumerate()
    .map(|(i, &m)| (longitudes[i], latitudes[i], m))
    .collect();

  let added = db.geoadd(key, &items, [])?;
  // Note: fields 1 and 2, 4 and 5 have distinct member names but same coordinates.
  // ZSet has 7 distinct members!
  assert_eq!(added, 7);

  // GEOHASH check against Kvrocks test vectors
  let hashes = db.geohash(key, &fields)?;
  for i in 0..fields.len() {
    assert_eq!(
      hashes[i].as_deref(),
      Some(expected_hashes[i]),
      "Hash mismatch at index {i}"
    );
  }

  // GEODIST between fields[2] and fields[3]
  let dist = db.geodist(key, fields[2], fields[3], None)?;
  assert!(dist.is_some());
  assert_eq!(dist.unwrap().ceil(), 194102.0);

  // GEOPOS check
  let pos = db.geopos(key, &fields)?;
  for i in 0..fields.len() {
    assert!(pos[i].is_some());
    let (lon, lat) = pos[i].unwrap();
    let encoded_h = encode_geohash_string(lon, lat);
    assert_eq!(encoded_h, expected_hashes[i]);
  }

  // GEORADIUS with huge radius should return all points
  let radius_opt = GeoRadius {
    with_coord: true,
    with_dist: true,
    with_hash: true,
    count: Some(100),
    any: false,
    sort: DistanceSort::Asc,
    store_key: None,
    store_dist_key: None,
    unit: DistanceUnit::Meters,
  };
  let radius_res = db.georadius(key, longitudes[0], latitudes[0], 100_000_000.0, &radius_opt)?;
  assert_eq!(radius_res.len(), fields.len());

  // GEORADIUSBYMEMBER
  let by_member_res = db.georadiusbymember(key, fields[0], 100_000_000.0, &radius_opt)?;
  assert_eq!(by_member_res.len(), fields.len());

  Ok(())
}

#[test]
fn test_georadius_and_geosearch() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.geoadd(
    "Sicily",
    &[
      (13.361389, 38.115556, "Palermo"),
      (15.087269, 37.502669, "Catania"),
      (13.583333, 37.316667, "Agrigento"),
    ],
    [],
  )?;

  // 1. GEORADIUS from coordinates with STORE and STOREDIST
  let radius_opt = GeoRadius {
    with_coord: true,
    with_dist: true,
    with_hash: false,
    count: None,
    any: false,
    sort: DistanceSort::Asc,
    store_key: Some("stored_radius".into()),
    store_dist_key: Some("stored_dist".into()),
    unit: DistanceUnit::Kilometers,
  };

  let pts = db.georadius("Sicily", 15.0, 37.5, 200.0, &radius_opt)?;
  assert!(!pts.is_empty());
  assert_eq!(pts[0].member, "Catania");

  // Verify stored keys
  let stored_cnt = db.zcard("stored_radius")?;
  assert_eq!(stored_cnt as usize, pts.len());
  let stored_dist_cnt = db.zcard("stored_dist")?;
  assert_eq!(stored_dist_cnt as usize, pts.len());

  let catania_dist_score = db.zscore("stored_dist", "Catania")?;
  assert!(catania_dist_score.is_some());
  assert!((catania_dist_score.unwrap() - pts[0].dist).abs() < 1e-4);

  // 2. GEORADIUSBYMEMBER
  let by_member_pts = db.georadiusbymember("Sicily", "Agrigento", 100.0, &radius_opt)?;
  assert!(!by_member_pts.is_empty());
  assert_eq!(by_member_pts[0].member, "Agrigento");

  // 3. GEOSEARCH with Box
  let search_opt = [
    GeoSearch::WithCoord,
    GeoSearch::WithDist,
    GeoSearch::Count(2),
    GeoSearch::Asc,
    GeoSearch::Unit(DistanceUnit::Kilometers),
  ];

  let mut shape =
    GeoShape::new_rectangular_with_unit(15.0, 37.5, 200.0, 200.0, DistanceUnit::Kilometers);
  let search_res = db.geosearch(
    "Sicily",
    &OriginPoint::Coord {
      lon: 15.0,
      lat: 37.5,
    },
    &mut shape,
    search_opt,
  )?;

  assert!(!search_res.is_empty());
  assert!(search_res.len() <= 2);

  // 4. GEOSEARCHSTORE with BYBOX and STOREDIST
  let store_opt = [
    GeoSearchStore::Count(10),
    GeoSearchStore::Asc,
    GeoSearchStore::StoreDist,
    GeoSearchStore::Unit(DistanceUnit::Kilometers),
  ];
  let stored_count = db.geosearchstore(
    "search_dest",
    "Sicily",
    &OriginPoint::Member("Catania".into()),
    &mut shape,
    store_opt,
  )?;
  assert!(stored_count > 0);
  assert_eq!(db.zcard("search_dest")?, stored_count as u64);

  Ok(())
}

#[test]
fn test_geo_edge_cases_and_validation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Out of bounds longitude
  assert!(db.geoadd("k", &[(180.1, 10.0, "m1")], []).is_err());
  assert!(db.geoadd("k", &[(-180.1, 10.0, "m1")], []).is_err());

  // Out of bounds latitude
  assert!(db.geoadd("k", &[(0.0, 85.051129, "m1")], []).is_err());
  assert!(db.geoadd("k", &[(0.0, -85.051129, "m1")], []).is_err());

  // NaN / Inf coordinates
  assert!(db.geoadd("k", &[(f64::NAN, 10.0, "m1")], []).is_err());
  assert!(db.geoadd("k", &[(10.0, f64::INFINITY, "m1")], []).is_err());

  // Valid exact boundary coordinates
  assert_eq!(
    db.geoadd(
      "k",
      &[
        (-180.0, -85.05112878, "min_bound"),
        (180.0, 85.05112878, "max_bound")
      ],
      []
    )?,
    2
  );

  // Unsupported distance unit
  assert!(
    db.geodist("k", "min_bound", "max_bound", Some("invalid_unit"))
      .is_err()
  );

  // Nonexistent member in geodist returns None
  assert_eq!(db.geodist("k", "min_bound", "ghost", None)?, None);
  assert_eq!(
    db.geodist("ghost_key", "min_bound", "max_bound", None)?,
    None
  );

  // Nonexistent member in georadiusbymember returns error
  let opt = GeoRadius::default();
  assert!(db.georadiusbymember("k", "ghost", 1000.0, &opt).is_err());

  // Nonexistent member in geosearch returns error
  let mut shape = GeoShape::new_circular(0.0, 0.0, 1000.0);
  assert!(
    db.geosearch("k", &OriginPoint::Member("ghost".into()), &mut shape, [])
      .is_err()
  );

  // Negative radius in georadius
  assert!(db.georadius("k", 0.0, 0.0, -10.0, &opt).is_err());

  // GEOADD with NX and XX options
  assert_eq!(db.geoadd("k", &[(0.0, 0.0, "min_bound")], [ZAdd::Nx])?, 0);
  assert_eq!(db.geoadd("k", &[(0.0, 0.0, "min_bound")], [ZAdd::Xx])?, 0);

  Ok(())
}

#[test]
fn test_geohash_and_distance_units() -> Void {
  use wedb_embed::geo::{
    GeoHashBits, align_52bits, decode_geohash, encode_geohash, estimate_steps_by_radius,
    geohash_neighbors, geohash_to_base32, haversine_distance, scores_of_geohash_box,
  };

  let lon = 116.4074;
  let lat = 39.9042;

  // 52-bit Geohash encoding / decoding roundtrip
  let hash = encode_geohash(lon, lat);
  let (dec_lon, dec_lat) = decode_geohash(hash);
  assert!((lon - dec_lon).abs() < 1e-4);
  assert!((lat - dec_lat).abs() < 1e-4);

  // Base32 representation
  let b32 = geohash_to_base32(hash);
  assert_eq!(b32.len(), 11);

  // Neighbors & score box
  let gh_bits = GeoHashBits {
    bits: hash,
    step: 26,
  };
  let neighbors = geohash_neighbors(&gh_bits);
  assert_ne!(neighbors.north.bits, 0);
  assert_ne!(neighbors.south.bits, 0);
  assert_ne!(neighbors.east.bits, 0);
  assert_ne!(neighbors.west.bits, 0);

  let (min_score, max_score) = scores_of_geohash_box(gh_bits);
  assert_eq!(min_score, align_52bits(gh_bits));
  assert!(min_score <= hash);
  assert!(hash < max_score);

  // Unit conversion
  assert_eq!(DistanceUnit::Kilometers.to_meters(1.5), 1500.0);
  assert_eq!(DistanceUnit::Miles.to_meters(1.0), 1609.34);
  assert_eq!(DistanceUnit::Feet.to_meters(100.0), 30.48);

  // Distance calculation
  let dist = haversine_distance(116.4074, 39.9042, 121.4737, 31.2304); // Beijing to Shanghai ~1068km
  assert!((dist - 1_068_000.0).abs() < 50_000.0);

  let step = estimate_steps_by_radius(5000.0, 39.9);
  assert!((10..=26).contains(&step));

  Ok(())
}

#[test]
fn test_kvrocks_gocase_comprehensive_edge_cases() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. GEORADIUSBYMEMBER against non-existing src key -> returns empty vec
  let radius_opt = GeoRadius {
    unit: DistanceUnit::Kilometers,
    ..Default::default()
  };
  let res = db.georadiusbymember("non_existing_points", "member", 1.0, &radius_opt)?;
  assert!(res.is_empty());

  // 2. GEORADIUSBYMEMBER against non-existing member -> error, destination is not touched
  db.geoadd("src", &[(10.0, 10.0, "Shenzhen")], [])?;
  db.geoadd("dst", &[(20.0, 20.0, "Guangzhou")], [])?;
  let opt_store = GeoRadius {
    store_key: Some("dst".into()),
    unit: DistanceUnit::Meters,
    ..Default::default()
  };
  assert!(
    db.georadiusbymember("src", "Shenzhen_2", 20.0, &opt_store)
      .is_err()
  );
  assert_eq!(db.zcard("dst")?, 1);

  // 3. GEORADIUS / GEORADIUSBYMEMBER store: remove destination key when there is no result set
  db.del(&["dst"])?;
  db.geoadd("dst", &[(10.0, 10.0, "Shenzhen")], [])?;
  let res_empty = db.georadius("empty_src", 15.0, 37.0, 88.0, &opt_store)?;
  assert!(res_empty.is_empty());
  assert_eq!(db.zcard("dst")?, 0);

  // 4. GEOSEARCH against non-existing src key -> returns empty vec
  let s_opt = [GeoSearch::Unit(DistanceUnit::Meters)];
  let mut shape = GeoShape::new_rectangular_with_unit(0.0, 0.0, 88.0, 88.0, DistanceUnit::Meters);
  let search_res = db.geosearch(
    "empty_src",
    &OriginPoint::Member("Shenzhen".into()),
    &mut shape,
    s_opt,
  )?;
  assert!(search_res.is_empty());

  // 5. GEOSEARCH / GEOSEARCHSTORE FROMMEMBER against non-existing member -> error, dst unchanged
  db.geoadd("dst", &[(20.0, 20.0, "Guangzhou")], [])?;
  let store_opt = [GeoSearchStore::Unit(DistanceUnit::Meters)];
  assert!(
    db.geosearchstore(
      "dst",
      "src",
      &OriginPoint::Member("Shenzhen_2".into()),
      &mut shape,
      store_opt,
    )
    .is_err()
  );
  assert_eq!(db.zcard("dst")?, 1);

  // 6. GEOSEARCHSTORE with empty results -> removes destination key, keeps source key
  let mut shape_tiny = GeoShape::new_circular_with_unit(1.0, 1.0, 1.0, DistanceUnit::Meters);
  let stored = db.geosearchstore(
    "dst",
    "src",
    &OriginPoint::Coord { lon: 1.0, lat: 1.0 },
    &mut shape_tiny,
    store_opt,
  )?;
  assert_eq!(stored, 0);
  assert_eq!(db.zcard("dst")?, 0);
  assert_eq!(db.zcard("src")?, 1);

  // 7. GEORADIUS DESC with equal distances should not crash or produce duplicates
  db.geoadd(
    "geokey",
    &[
      (13.361389, 38.115556, "A"),
      (13.361389, 38.115556, "B"),
      (13.361389, 38.115556, "C"),
      (15.087269, 37.502669, "D"),
    ],
    [],
  )?;
  let desc_opt = GeoRadius {
    sort: DistanceSort::Desc,
    unit: DistanceUnit::Kilometers,
    ..Default::default()
  };
  let desc_res = db.georadius("geokey", 13.361389, 38.115556, 500.0, &desc_opt)?;
  assert_eq!(desc_res.len(), 4);
  assert_eq!(desc_res[0].member, "D"); // farthest first

  // 8. GEODIST with missing elements
  assert_eq!(db.geodist("geokey", "A", "Nonexistent", None)?, None);
  assert_eq!(db.geodist("nonexistent_key", "A", "B", None)?, None);

  Ok(())
}

#[test]
fn test_kvrocks_regression_vectors() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Kvrocks gocase regression vectors (seed, km, lon, lat)
  let regression_vectors: &[(i64, f64, f64)] = &[
    (7083, 81.634_948_934_258_38, 30.561_509_253_718_67),
    (5416, -70.863_281_847_379_77, -46.347_003_465_679_95),
    (6064, -89.818_768_962_202_01, -40.463_868_561_416_8),
    (156, 149.29737817929004, 15.95807862745508),
    (143, 59.235461856813856, 66.269_555_127_373_68),
    (187, -101.88575239939883, 49.061_997_951_502_92),
    (154, -90.187_939_661_642_52, 66.615_930_412_251_49),
    (145, 163.03472387745728, 64.012_747_720_821_18),
    (143, 137.866_635_172_565_8, 63.986745399416776),
    (151, 59.149_620_271_823_18, 65.204_186_651_485_14),
    (149, 84.062_063_109_158_54, -65.685_403_922_426_23),
    (16751, -1.8175081637769495, 20.665668878082954),
  ];

  for (idx, &(radius_km, search_lon, search_lat)) in regression_vectors.iter().enumerate() {
    let key = format!("regression_points_{idx}");
    // Add center point and peripheral test points
    let p_center = (search_lon, search_lat, "center");
    let p_inside = (search_lon + 0.001, search_lat + 0.001, "inside");
    let p_outside = (
      if search_lon > 0.0 {
        search_lon - 50.0
      } else {
        search_lon + 50.0
      },
      if search_lat > 0.0 {
        search_lat - 50.0
      } else {
        search_lat + 50.0
      },
      "outside",
    );

    db.geoadd(&key, &[p_center, p_inside, p_outside], [])?;

    let opt = GeoRadius {
      unit: DistanceUnit::Kilometers,
      sort: DistanceSort::Asc,
      ..Default::default()
    };

    let res = db.georadius(&key, search_lon, search_lat, radius_km as f64, &opt)?;
    assert!(!res.is_empty());
    assert_eq!(res[0].member, "center");
    assert!(
      res[0].dist < 0.001,
      "distance {dist} exceeds precision threshold",
      dist = res[0].dist
    );
  }

  Ok(())
}

#[test]
fn test_geosearch_bybox_frommember_and_storedist() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // NYC test points from Kvrocks gocase test
  db.geoadd(
    "nyc",
    &[
      (-73.9733487, 40.7648057, "central park n/q/r"),
      (-73.9903085, 40.7362513, "union square"),
      (-74.0131604, 40.7126674, "wtc one"),
      (-73.7858139, 40.6428986, "jfk"),
      (-73.9375699, 40.7498929, "q4"),
      (-73.9564142, 40.7480973, "4545"),
      (-73.9454966, 40.747533, "lic market"),
    ],
    [],
  )?;

  // 1. GEOSEARCH BYBOX
  let mut shape =
    GeoShape::new_rectangular_with_unit(0.0, 0.0, 10.0, 10.0, DistanceUnit::Kilometers);
  let s_opt = [
    GeoSearch::WithCoord,
    GeoSearch::WithDist,
    GeoSearch::Asc,
    GeoSearch::Unit(DistanceUnit::Kilometers),
  ];
  let res = db.geosearch(
    "nyc",
    &OriginPoint::Member("wtc one".into()),
    &mut shape,
    s_opt,
  )?;
  assert!(!res.is_empty());
  assert_eq!(res[0].member, "wtc one");

  // 2. GEOSEARCHSTORE with BYBOX and STOREDIST
  let store_opt = [
    GeoSearchStore::Count(3),
    GeoSearchStore::Asc,
    GeoSearchStore::StoreDist,
    GeoSearchStore::Unit(DistanceUnit::Kilometers),
  ];
  let stored_cnt = db.geosearchstore(
    "nyc_dest",
    "nyc",
    &OriginPoint::Member("wtc one".into()),
    &mut shape,
    store_opt,
  )?;
  assert_eq!(stored_cnt, 3);
  assert_eq!(db.zcard("nyc_dest")?, 3);

  // Verify distance stored as score
  let wtc_score = db.zscore("nyc_dest", "wtc one")?;
  assert_eq!(wtc_score, Some(0.0));

  Ok(())
}

#[test]
fn test_base32_decoding_and_shape_methods() -> Void {
  use wedb_embed::geo::{
    GeoPoint, GeoShape, base32_to_coords, coords_to_base32, encode_geohash_string,
  };

  // 1. Base32 decoding and coordinate roundtrip
  let lon = -5.6;
  let lat = 42.6;
  let gh_str = encode_geohash_string(lon, lat);
  assert_eq!(gh_str, "ezs42e44yx0");

  let (dec_lon, dec_lat) = base32_to_coords(&gh_str)?;
  assert!((dec_lon - lon).abs() < 0.001);
  assert!((dec_lat - lat).abs() < 0.001);

  // Case-insensitive base32 decoding
  let (upper_lon, upper_lat) = base32_to_coords("EZS42E44YX0")?;
  assert!((upper_lon - lon).abs() < 0.001);
  assert!((upper_lat - lat).abs() < 0.001);

  // Invalid base32 strings
  assert!(base32_to_coords("").is_err());
  assert!(base32_to_coords("ezs42e44yx01234").is_err()); // > 11 chars
  assert!(base32_to_coords("ezs42e44yx!").is_err()); // invalid char

  // coords_to_base32 with validation
  assert!(coords_to_base32(200.0, 0.0).is_err());
  assert!(coords_to_base32(0.0, 95.0).is_err());
  assert!(coords_to_base32(0.0, 0.0).is_ok());

  // 2. GeoShape contains_point and point_in_radius
  let circular = GeoShape::new_circular(0.0, 0.0, 1000.0);
  assert!(circular.contains_point(0.0, 0.0));
  assert!(circular.contains_point(0.001, 0.001));
  assert!(!circular.contains_point(1.0, 1.0));

  let pt_inside = GeoPoint {
    longitude: 0.001,
    latitude: 0.001,
    member: "inside".into(),
    dist: 100.0,
    score: 0.0,
  };
  assert!(circular.point_in_radius(&pt_inside));

  let rect = GeoShape::new_rectangular_with_unit(0.0, 0.0, 10.0, 10.0, DistanceUnit::Kilometers);
  assert!(rect.contains_point(0.0, 0.0));
  assert!(rect.contains_point(0.01, 0.01));
  assert!(!rect.contains_point(10.0, 10.0));

  Ok(())
}

#[test]
fn test_geo_count_and_any_topk_semantics() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Populate 50 points along a line
  let mut items = Vec::new();
  for i in 1..=50 {
    let name = format!("point_{i}");
    items.push((i as f64 * 0.01, i as f64 * 0.01, name));
  }
  let refs: Vec<(f64, f64, &str)> = items
    .iter()
    .map(|(lon, lat, name)| (*lon, *lat, name.as_str()))
    .collect();
  db.geoadd("linear", &refs, [])?;

  // 1. COUNT 0 returns empty
  let opt_count0 = GeoRadius {
    count: Some(0),
    unit: DistanceUnit::Kilometers,
    sort: DistanceSort::Asc,
    ..Default::default()
  };
  let res0 = db.georadius("linear", 0.0, 0.0, 1000.0, &opt_count0)?;
  assert!(res0.is_empty());

  // 2. COUNT 5 ASC (Top-K quickselect verification)
  let opt_count5 = GeoRadius {
    count: Some(5),
    unit: DistanceUnit::Kilometers,
    sort: DistanceSort::Asc,
    ..Default::default()
  };
  let res5 = db.georadius("linear", 0.0, 0.0, 1000.0, &opt_count5)?;
  assert_eq!(res5.len(), 5);
  assert_eq!(res5[0].member, "point_1");
  assert_eq!(res5[1].member, "point_2");
  assert_eq!(res5[4].member, "point_5");

  // 3. COUNT 5 DESC (Top-K descending quickselect verification)
  let opt_desc5 = GeoRadius {
    count: Some(5),
    unit: DistanceUnit::Kilometers,
    sort: DistanceSort::Desc,
    ..Default::default()
  };
  let res_desc5 = db.georadius("linear", 0.0, 0.0, 1000.0, &opt_desc5)?;
  assert_eq!(res_desc5.len(), 5);
  assert_eq!(res_desc5[0].member, "point_50");
  assert_eq!(res_desc5[1].member, "point_49");
  assert_eq!(res_desc5[4].member, "point_46");

  // 4. COUNT with ANY true (no strict sorting requirement)
  let opt_any = GeoRadius {
    count: Some(3),
    any: true,
    unit: DistanceUnit::Kilometers,
    sort: DistanceSort::None,
    ..Default::default()
  };
  let res_any = db.georadius("linear", 0.0, 0.0, 1000.0, &opt_any)?;
  assert_eq!(res_any.len(), 3);

  // 5. Dual STORE and STOREDIST in single georadius call
  let opt_dual_store = GeoRadius {
    count: Some(5),
    unit: DistanceUnit::Kilometers,
    sort: DistanceSort::Asc,
    store_key: Some("stored_scores".into()),
    store_dist_key: Some("stored_dists".into()),
    ..Default::default()
  };
  let res_dual = db.georadius("linear", 0.0, 0.0, 1000.0, &opt_dual_store)?;
  assert_eq!(res_dual.len(), 5);
  assert_eq!(db.zcard("stored_scores")?, 5);
  assert_eq!(db.zcard("stored_dists")?, 5);

  let p1_dist_score = db.zscore("stored_dists", "point_1")?.unwrap();
  assert!((p1_dist_score - res_dual[0].dist).abs() < 1e-6);

  Ok(())
}

#[test]
fn test_geo_kvrocks_get_and_mget() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.geoadd(
    "sicily",
    &[
      (13.361389, 38.115556, "Palermo"),
      (15.087269, 37.502669, "Catania"),
    ],
    [],
  )?;

  // 1. geoget
  let p1 = db.geoget("sicily", "Palermo")?.unwrap();
  assert_eq!(p1.member, "Palermo");
  assert!((p1.longitude - 13.361389).abs() < 1e-4);
  assert!((p1.latitude - 38.115556).abs() < 1e-4);

  assert!(db.geoget("sicily", "Rome")?.is_none());

  // 2. geomget
  let mg = db.geomget("sicily", &["Palermo", "Rome", "Catania"])?;
  assert_eq!(mg.len(), 3);
  assert!(mg[0].is_some());
  assert!(mg[1].is_none());
  assert!(mg[2].is_some());
  assert_eq!(mg[0].as_ref().unwrap().member, "Palermo");
  assert_eq!(mg[2].as_ref().unwrap().member, "Catania");

  Ok(())
}
