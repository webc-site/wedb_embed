use std::{thread::sleep, time::Duration};

use aok::Void;
use tempfile::tempdir;
use wedb_embed::{ExpireCondition, Fjall, KeyNumStats, RedisType, SortArgs, WeDb};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_key_basic_crud_and_types() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  assert_eq!(db.type_of("nonexistent")?, "none");
  assert!(!db.exists_one("nonexistent")?);

  // 1. String
  db.set("k_str", "hello", [])?;
  assert_eq!(db.type_of("k_str")?, "string");
  assert!(db.exists_one("k_str")?);

  // 2. Hash
  db.hset("k_hash", &[("f1", "v1"), ("f2", "v2")])?;
  assert_eq!(db.type_of("k_hash")?, "hash");

  // 3. List
  db.rpush("k_list", &["e1", "e2"])?;
  assert_eq!(db.type_of("k_list")?, "list");

  // 4. Set
  db.sadd("k_set", &["m1", "m2"])?;
  assert_eq!(db.type_of("k_set")?, "set");

  // 5. ZSet
  db.zadd("k_zset", &[(10.0, "zm1".as_bytes())], [])?;
  assert_eq!(db.type_of("k_zset")?, "zset");

  assert_eq!(
    db.exists(&[
      "k_str",
      "k_hash",
      "k_list",
      "k_set",
      "k_zset",
      "nonexistent"
    ])?,
    5
  );

  assert_eq!(db.dbsize()?, 5);
  assert_eq!(db.key_count()?, 5);

  // Deletion
  assert_eq!(db.del(&["k_str", "k_hash"])?, 2);
  assert_eq!(db.dbsize()?, 3);
  assert!(!db.exists_one("k_str")?);
  assert!(!db.exists_one("k_hash")?);

  assert!(db.del_one("k_list")?);
  assert_eq!(db.dbsize()?, 2);

  Ok(())
}

#[test]
fn test_key_ttl_and_conditional_expiry() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  db.set("k1", "v1", [])?;
  assert_eq!(db.ttl("k1")?, -1);
  assert_eq!(db.pttl("k1")?, -1);

  // Condition XX should fail if no expiry
  assert!(!db.expire_with_condition("k1", 100, ExpireCondition::XX)?);
  assert_eq!(db.ttl("k1")?, -1);

  // Condition GT should fail if no expiry
  assert!(!db.expire_with_condition("k1", 100, ExpireCondition::GT)?);

  // Condition NX should succeed if no expiry
  assert!(db.expire_with_condition("k1", 100, ExpireCondition::NX)?);
  let ttl1 = db.ttl("k1")?;
  assert!((90..=100).contains(&ttl1));

  // Condition NX should now fail
  assert!(!db.expire_with_condition("k1", 200, ExpireCondition::NX)?);

  // Condition GT: 50 < 100 should fail
  assert!(!db.expire_with_condition("k1", 50, ExpireCondition::GT)?);

  // Condition GT: 150 > 100 should succeed
  assert!(db.expire_with_condition("k1", 150, ExpireCondition::GT)?);
  let ttl2 = db.ttl("k1")?;
  assert!((140..=150).contains(&ttl2));

  // Condition LT: 200 > 150 should fail
  assert!(!db.expire_with_condition("k1", 200, ExpireCondition::LT)?);

  // Condition LT: 30 < 150 should succeed
  assert!(db.expire_with_condition("k1", 30, ExpireCondition::LT)?);
  let ttl3 = db.ttl("k1")?;
  assert!((20..=30).contains(&ttl3));

  // PERSIST
  assert!(db.persist("k1")?);
  assert_eq!(db.ttl("k1")?, -1);
  assert!(!db.persist("k1")?);

  // Milliseconds precision
  assert!(db.pexpire("k1", 500)?);
  assert!(db.pttl("k1")? > 0);
  assert!(db.expiretime("k1")? > 0);
  assert!(db.pexpiretime("k1")? > 0);

  sleep(Duration::from_millis(600));
  assert_eq!(db.ttl("k1")?, -2);
  assert_eq!(db.pttl("k1")?, -2);
  assert!(!db.exists_one("k1")?);

  // Test composite type expiration
  db.hset("h1", &[("f", "v")])?;
  assert!(db.expire("h1", 60)?);
  assert!((50..=60).contains(&db.ttl("h1")?));
  assert!(db.persist("h1")?);
  assert_eq!(db.ttl("h1")?, -1);

  Ok(())
}

#[test]
fn test_key_scan_and_pattern_matching() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Populate multiple types
  for i in 0..15 {
    db.set(format!("str_{:02}", i), "val", [])?;
  }
  for i in 0..10 {
    db.hset(format!("hash_{:02}", i), &[("f", "v")])?;
  }
  for i in 0..5 {
    db.sadd(format!("set_{:02}", i), &["item"])?;
  }

  assert_eq!(db.dbsize()?, 30);

  // Test keys pattern
  let str_keys = db.keys("str_*")?;
  assert_eq!(str_keys.len(), 15);

  let hash_keys = db.keys("hash_*")?;
  assert_eq!(hash_keys.len(), 10);

  // Full scan traversal
  let mut cursor = b"0".to_vec();
  let mut scanned = Vec::new();
  loop {
    let (next_cur, batch) = db.scan(&cursor, Some(8), None, None)?;
    scanned.extend(batch);
    if next_cur == b"0" {
      break;
    }
    cursor = next_cur;
  }
  assert_eq!(scanned.len(), 30);

  // Filter scan by type: String only
  let mut str_scanned = Vec::new();
  cursor = b"0".to_vec();
  loop {
    let (next_cur, batch) = db.scan(&cursor, Some(5), None, Some(RedisType::String))?;
    str_scanned.extend(batch);
    if next_cur == b"0" {
      break;
    }
    cursor = next_cur;
  }
  assert_eq!(str_scanned.len(), 15);

  // Filter scan by type: Hash only
  let mut hash_scanned = Vec::new();
  cursor = b"0".to_vec();
  loop {
    let (next_cur, batch) = db.scan(&cursor, Some(5), None, Some(RedisType::Hash))?;
    hash_scanned.extend(batch);
    if next_cur == b"0" {
      break;
    }
    cursor = next_cur;
  }
  assert_eq!(hash_scanned.len(), 10);

  // Randomkey
  let rk = db.randomkey()?.expect("should find a random key");
  assert!(db.exists_one(&rk)?);

  Ok(())
}

#[test]
fn test_key_copy_and_rename() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // 1. String Copy & Rename
  db.set("s_src", "value123", [])?;
  assert!(db.copy("s_src", "s_dst", false)?);
  assert_eq!(db.get("s_dst")?, Some(b"value123".to_vec()));
  assert_eq!(db.get("s_src")?, Some(b"value123".to_vec()));

  // NX copy should fail if dst exists
  assert!(!db.copy("s_src", "s_dst", true)?);

  // Rename string
  db.rename("s_src", "s_renamed")?;
  assert!(!db.exists_one("s_src")?);
  assert_eq!(db.get("s_renamed")?, Some(b"value123".to_vec()));

  // Renamenx
  assert!(!db.renamenx("s_renamed", "s_dst")?); // s_dst exists
  assert!(db.renamenx("s_renamed", "s_final")?);
  assert!(!db.exists_one("s_renamed")?);
  assert!(db.exists_one("s_final")?);

  // 2. Hash Copy & Rename (verifying subkeys)
  db.hset("h_src", &[("f1", "v1"), ("f2", "v2"), ("f3", "v3")])?;
  assert!(db.copy("h_src", "h_copy", false)?);
  assert_eq!(db.hlen("h_copy")?, 3);
  assert_eq!(db.hget("h_copy", "f1")?, Some(b"v1".to_vec()));
  assert_eq!(db.hget("h_copy", "f2")?, Some(b"v2".to_vec()));
  assert_eq!(db.hget("h_copy", "f3")?, Some(b"v3".to_vec()));

  db.rename("h_src", "h_moved")?;
  assert!(!db.exists_one("h_src")?);
  assert_eq!(db.hlen("h_moved")?, 3);
  assert_eq!(db.hget("h_moved", "f1")?, Some(b"v1".to_vec()));

  // 3. List Copy & Rename
  db.rpush("l_src", &["item1", "item2", "item3"])?;
  assert!(db.copy("l_src", "l_copy", false)?);
  assert_eq!(
    db.lrange("l_copy", (0, -1))?,
    vec![b"item1".to_vec(), b"item2".to_vec(), b"item3".to_vec()]
  );

  db.rename("l_src", "l_moved")?;
  assert!(!db.exists_one("l_src")?);
  assert_eq!(
    db.lrange("l_moved", (0, -1))?,
    vec![b"item1".to_vec(), b"item2".to_vec(), b"item3".to_vec()]
  );

  // 4. Set & ZSet Copy & Rename
  db.sadd("set_src", &["m1", "m2"])?;
  db.rename("set_src", "set_moved")?;
  assert!(!db.exists_one("set_src")?);
  assert_eq!(db.scard("set_moved")?, 2);

  db.zadd(
    "z_src",
    &[(10.0, "m1".as_bytes()), (20.0, "m2".as_bytes())],
    [],
  )?;
  db.rename("z_src", "z_moved")?;
  assert!(!db.exists_one("z_src")?);
  assert_eq!(db.zcard("z_moved")?, 2);
  assert_eq!(db.zscore("z_moved", "m1")?, Some(10.0));

  // 5. Edge Cases: same key rename/renamenx, non-existent key, and TTL preservation
  assert!(db.rename("z_moved", "z_moved").is_ok());
  assert!(!db.renamenx("z_moved", "z_moved")?);
  assert!(db.rename("nonexistent_key_123", "dst").is_err());
  assert!(db.renamenx("nonexistent_key_123", "dst").is_err());

  // TTL preservation in copy
  db.set("s_exp", "val", [])?;
  db.expire("s_exp", 500)?;
  assert!(db.copy("s_exp", "s_exp_copy", false)?);
  assert!((480..=500).contains(&db.ttl("s_exp_copy")?));

  Ok(())
}

#[test]
fn test_key_flushdb() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  db.set("s", "1", [])?;
  db.hset("h", &[("f", "v")])?;
  db.lpush("l", &["v"])?;
  assert_eq!(db.dbsize()?, 3);

  let cleaned = db.flushdb()?;
  assert!(cleaned > 0);
  assert_eq!(db.dbsize()?, 0);
  assert_eq!(db.randomkey()?, None);

  // Db should still be fully functional after flushdb
  db.set("new_k", "new_v", [])?;
  assert_eq!(db.dbsize()?, 1);
  assert_eq!(db.get("new_k")?, Some(b"new_v".to_vec()));

  Ok(())
}

#[test]
fn test_key_unlink_and_copy_replace() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  db.set("s1", "v1", [])?;
  db.set("s2", "v2", [])?;
  assert!(db.unlink_one("s1")?);
  assert!(!db.exists_one("s1")?);
  assert_eq!(db.unlink(&["s2", "nonexistent"])?, 1);
  assert!(!db.exists_one("s2")?);

  // copy_replace overwriting existing destination
  db.set("src", "val1", [])?;
  db.set("dst", "val2", [])?;
  assert!(!db.copy("src", "dst", true)?); // nx fails
  assert_eq!(db.get("dst")?, Some(b"val2".to_vec()));

  assert!(db.copy_replace("src", "dst")?); // replace succeeds
  assert_eq!(db.get("dst")?, Some(b"val1".to_vec()));

  Ok(())
}

#[test]
fn test_key_all_redis_types_conformance() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // 1. String
  db.set("t_str", "hello", [])?;
  assert_eq!(db.type_of("t_str")?, "string");

  // 2. Hash
  db.hset("t_hash", &[("f", "v")])?;
  assert_eq!(db.type_of("t_hash")?, "hash");

  // 3. List
  db.rpush("t_list", &["e1"])?;
  assert_eq!(db.type_of("t_list")?, "list");

  // 4. Set
  db.sadd("t_set", &["m1"])?;
  assert_eq!(db.type_of("t_set")?, "set");

  // 5. ZSet
  db.zadd("t_zset", &[(1.0, b"zm1".as_slice())], [])?;
  assert_eq!(db.type_of("t_zset")?, "zset");

  // 6. Bitmap
  db.setbit("t_bitmap", 7, 1)?;
  assert_eq!(db.type_of("t_bitmap")?, "bitmap");

  // 7. SortedInt
  db.si_add("t_si", &[42])?;
  assert_eq!(db.type_of("t_si")?, "sortedint");

  // 8. Stream
  db.xadd("t_stream", (), &[("k", "v")])?;
  assert_eq!(db.type_of("t_stream")?, "stream");

  // 9. Bloom Filter
  db.bf_reserve("t_bf", 0.01, 100, None)?;
  assert_eq!(db.type_of("t_bf")?, "MBbloom--");

  // 10. Cuckoo Filter
  db.cf_reserve("t_cf", 100, None)?;
  assert_eq!(db.type_of("t_cf")?, "MBbloomCF");

  // 11. JSON
  db.json_set("t_json", "$", r#"{"a":1}"#, [])?;
  assert_eq!(db.type_of("t_json")?, "ReJSON-RL");

  // 12. HyperLogLog
  db.pfadd("t_hll", &["elem1"])?;
  assert_eq!(db.type_of("t_hll")?, "hyperloglog");

  // 13. TDigest
  db.tdigest_create("t_tdigest", 100.0)?;
  assert_eq!(db.type_of("t_tdigest")?, "TDIS-TYPE");

  // 14. TimeSeries
  db.ts_create("t_ts", None)?;
  assert_eq!(db.type_of("t_ts")?, "timeseries");

  // Total distinct active keys should be exactly 14, computed in O(1) space!
  assert_eq!(db.dbsize()?, 14);
  assert_eq!(db.key_count()?, 14);

  Ok(())
}

#[test]
fn test_key_sort_comprehensive_suite() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // 1. Basic Numerical SORT on List
  db.rpush("cost", &["30", "1.5", "10", "8"])?;

  let sorted_asc = db.sort("cost", &SortArgs::new())?;
  let expected_asc: Vec<Option<Vec<u8>>> = vec![
    Some(b"1.5".to_vec()),
    Some(b"8".to_vec()),
    Some(b"10".to_vec()),
    Some(b"30".to_vec()),
  ];
  assert_eq!(sorted_asc, expected_asc);

  let sorted_desc = db.sort("cost", &SortArgs::new().desc())?;
  let expected_desc: Vec<Option<Vec<u8>>> = vec![
    Some(b"30".to_vec()),
    Some(b"10".to_vec()),
    Some(b"8".to_vec()),
    Some(b"1.5".to_vec()),
  ];
  assert_eq!(sorted_desc, expected_desc);

  // SORT_RO
  let ro_res = db.sort_ro("cost", &SortArgs::new().desc())?;
  assert_eq!(ro_res, expected_desc);

  // SORT_RO rejecting STORE
  assert!(
    db.sort_ro("cost", &SortArgs::new().store("out_list"))
      .is_err()
  );

  // 2. SORT ALPHA on Strings
  db.rpush(
    "sites",
    &["www.reddit.com", "www.slashdot.com", "www.infoq.com"],
  )?;
  let sorted_alpha = db.sort("sites", &SortArgs::new().alpha())?;
  let expected_alpha: Vec<Option<Vec<u8>>> = vec![
    Some(b"www.infoq.com".to_vec()),
    Some(b"www.reddit.com".to_vec()),
    Some(b"www.slashdot.com".to_vec()),
  ];
  assert_eq!(sorted_alpha, expected_alpha);

  // Without ALPHA, strings fail float conversion
  assert!(db.sort("sites", &SortArgs::new()).is_err());

  // 3. LIMIT Pagination
  db.rpush(
    "ranks",
    &["1", "3", "5", "7", "9", "2", "4", "6", "8", "10"],
  )?;
  let limit_page1 = db.sort("ranks", &SortArgs::new().limit(0, Some(5)))?;
  assert_eq!(
    limit_page1,
    vec![
      Some(b"1".to_vec()),
      Some(b"2".to_vec()),
      Some(b"3".to_vec()),
      Some(b"4".to_vec()),
      Some(b"5".to_vec()),
    ]
  );
  let limit_page2 = db.sort("ranks", &SortArgs::new().limit(5, Some(5)))?;
  assert_eq!(
    limit_page2,
    vec![
      Some(b"6".to_vec()),
      Some(b"7".to_vec()),
      Some(b"8".to_vec()),
      Some(b"9".to_vec()),
      Some(b"10".to_vec()),
    ]
  );

  // 4. SORT BY and GET
  db.rpush("uids", &["1", "2", "3", "4"])?;
  db.set("user_name_1", "admin", [])?;
  db.set("user_name_2", "jack", [])?;
  db.set("user_name_3", "peter", [])?;
  db.set("user_name_4", "mary", [])?;

  db.set("user_level_1", "9999", [])?;
  db.set("user_level_2", "10", [])?;
  db.set("user_level_3", "25", [])?;
  db.set("user_level_4", "70", [])?;

  // Sort by external level key
  let by_level = db.sort("uids", &SortArgs::new().by("user_level_*"))?;
  assert_eq!(
    by_level,
    vec![
      Some(b"2".to_vec()),
      Some(b"3".to_vec()),
      Some(b"4".to_vec()),
      Some(b"1".to_vec()),
    ]
  );

  // Sort by level and GET username and self (#)
  let by_level_get = db.sort(
    "uids",
    &SortArgs::new()
      .by("user_level_*")
      .get("user_name_*")
      .get("#"),
  )?;
  assert_eq!(
    by_level_get,
    vec![
      Some(b"jack".to_vec()),
      Some(b"2".to_vec()),
      Some(b"peter".to_vec()),
      Some(b"3".to_vec()),
      Some(b"mary".to_vec()),
      Some(b"4".to_vec()),
      Some(b"admin".to_vec()),
      Some(b"1".to_vec()),
    ]
  );

  // 5. SORT BY with Hash Arrow (`*->field`)
  db.hset("u_meta_1", &[("score", "900")])?;
  db.hset("u_meta_2", &[("score", "100")])?;
  db.hset("u_meta_3", &[("score", "500")])?;
  db.hset("u_meta_4", &[("score", "300")])?;

  let by_hash_arrow = db.sort("uids", &SortArgs::new().by("u_meta_*->score"))?;
  assert_eq!(
    by_hash_arrow,
    vec![
      Some(b"2".to_vec()),
      Some(b"4".to_vec()),
      Some(b"3".to_vec()),
      Some(b"1".to_vec()),
    ]
  );

  // 6. SORT STORE into destination list
  let stored_count = db.sort_store("uids", "target_list", SortArgs::new().by("u_meta_*->score"))?;
  assert_eq!(stored_count, 4);
  assert_eq!(
    db.lrange("target_list", (0, -1))?,
    vec![b"2".to_vec(), b"4".to_vec(), b"3".to_vec(), b"1".to_vec()]
  );

  // 7. SORT on Set and ZSet
  db.sadd("test_set", &["30", "10", "20"])?;
  let set_sorted = db.sort("test_set", &SortArgs::new())?;
  assert_eq!(
    set_sorted,
    vec![
      Some(b"10".to_vec()),
      Some(b"20".to_vec()),
      Some(b"30".to_vec())
    ]
  );

  db.zadd(
    "test_zset",
    &[
      (5.0, b"300".as_slice()),
      (1.0, b"100".as_slice()),
      (2.0, b"200".as_slice()),
    ],
    [],
  )?;
  let zset_sorted = db.sort("test_zset", &SortArgs::new())?;
  assert_eq!(
    zset_sorted,
    vec![
      Some(b"100".to_vec()),
      Some(b"200".to_vec()),
      Some(b"300".to_vec())
    ]
  );

  // 8. Edge cases: non-existent key and wrong type
  assert_eq!(
    db.sort("non_existent", &SortArgs::new())?,
    Vec::<Option<Vec<u8>>>::new()
  );
  db.set("wrong_type_key", "just_a_string", [])?;
  assert!(db.sort("wrong_type_key", &SortArgs::new()).is_err());

  Ok(())
}

#[test]
fn test_key_dbsize_scan_and_kvrocks_stats() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Initial state before any scan
  assert_eq!(db.dbsize_cached(), 0);
  assert_eq!(db.last_dbsize_scan_time(), 0);
  assert_eq!(
    db.key_num_stats(),
    KeyNumStats {
      n_key: 0,
      n_expires: 0,
      n_expired: 0,
      avg_ttl: 0,
    }
  );

  // 1. Persistent string
  db.set("s1", "v1", [])?;

  // 2. Expiring string (3600 seconds)
  db.set("s2", "v2", [])?;
  db.expire("s2", 3600)?;

  // 3. Persistent hash
  db.hset("h1", &[("f", "v")])?;

  // 4. Expiring list (7200 seconds)
  db.rpush("l1", &["item"])?;
  db.expire("l1", 7200)?;

  // 5. Expired string (1 ms expiration, wait for it to expire)
  db.set("s_exp", "v", [])?;
  db.pexpire("s_exp", 1)?;
  sleep(Duration::from_millis(15));

  // Perform full keyspace scan (aligned with Kvrocks DBSIZE scan / GetKeyNumStats)
  let stats = db.dbsize_scan()?;
  assert_eq!(stats.n_key, 4);
  assert_eq!(stats.n_expires, 2);
  assert_eq!(stats.n_expired, 1);
  // avg_ttl should be around (3600 + 7200) / 2 = 5400 seconds
  assert!((5300..=5500).contains(&stats.avg_ttl));

  // Fast O(1) cached lookups without scanning
  assert_eq!(db.dbsize_cached(), 4);
  assert!(db.last_dbsize_scan_time() > 0);
  assert_eq!(db.key_num_stats(), stats);

  // Formatted keyspace string aligned with Kvrocks / Redis INFO keyspace
  let info_str = db.keyspace_info_string();
  assert!(info_str.starts_with("keys=4,expires=2,avg_ttl="));
  assert!(info_str.ends_with(",expired=1"));

  // db.dbsize() live count updates the cache
  assert_eq!(db.dbsize()?, 4);

  Ok(())
}
