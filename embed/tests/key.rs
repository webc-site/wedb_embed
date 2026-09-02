use std::{thread::sleep, time::Duration};

use aok::Void;
use tempfile::tempdir;
use wedb_embed::{ExpireCondition, Fjall, RedisType, WeDb};

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
