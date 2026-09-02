use aok::{OK, Void};
use wedb_embed::{
  Fjall, Partition, WeDb,
  api::{
    bitmap::{compose_bitmap_meta_key, compose_bitmap_segment},
    bloom::{
      compose_bloom_item, compose_bloom_meta_key, compose_cuckoo_meta_key, compose_cuckoo_page,
    },
    hash::{compose_hash_key, compose_hash_meta_key, compose_hash_prefix},
    hll::compose_hll_meta_key,
    json::compose_json_meta_key,
    list::{compose_list_item, compose_list_meta_key},
    set::{compose_set_key, compose_set_meta_key},
    sortedint::{compose_si_key, compose_si_meta_key},
    stream::{
      compose_stream_consumer_meta, compose_stream_group_meta, compose_stream_item,
      compose_stream_meta_key, compose_stream_pel_item,
    },
    string::compose_string_key,
    tdigest::compose_tdigest_meta_key,
    timeseries::{compose_ts_item, compose_ts_meta_key},
    zset::{compose_zset_key, compose_zset_meta_key, compose_zset_score_key},
  },
  key_composer::KeyComposer,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

fn to_hex(bytes: &[u8]) -> String {
  bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn test_namespace_and_select_db_scope_encoding_golden() -> Void {
  // 1. 默认命名空间默认库 (ns=0, db=0: \x00\x00\x00)
  let kc_def = KeyComposer::new(0, 0);
  assert!(kc_def.is_default());
  assert_eq!(kc_def.ns_id(), 0);
  assert_eq!(kc_def.db(), 0);
  assert_eq!(kc_def.scope_prefix_len(), 3);
  assert_eq!(kc_def.namespace_prefix(), b"\x00\x00\x00");
  assert_eq!(to_hex(&kc_def.namespace_prefix()), "000000");
  assert_eq!(
    &*compose_string_key(&kc_def, b"mykey"),
    b"\x00\x00\x00\x00mykey"
  );

  // 2. 默认命名空间多库 (ns=0, db=1: \x00\x00\x01)
  let kc_db1 = KeyComposer::new(0, 1);
  assert!(!kc_db1.is_default());
  assert_eq!(kc_db1.ns_id(), 0);
  assert_eq!(kc_db1.db(), 1);
  assert_eq!(kc_db1.namespace_prefix(), b"\x00\x00\x01");
  assert_eq!(to_hex(&kc_db1.namespace_prefix()), "000001");
  assert_eq!(
    &*compose_string_key(&kc_db1, b"mykey"),
    b"\x00\x00\x01\x00mykey"
  );

  // 3. 自定义命名空间单库 (ns=1, db=0: \x00\x01\x00)
  let kc_t1 = KeyComposer::new(1, 0);
  assert!(!kc_t1.is_default());
  assert_eq!(kc_t1.ns_id(), 1);
  assert_eq!(kc_t1.db(), 0);
  assert_eq!(kc_t1.namespace_prefix(), b"\x00\x01\x00");
  assert_eq!(to_hex(&kc_t1.namespace_prefix()), "000100");
  assert_eq!(
    &*compose_string_key(&kc_t1, b"mykey"),
    b"\x00\x01\x00\x00mykey"
  );

  // 4. 自定义命名空间多库 (ns=1, db=2: \x00\x01\x02)
  let kc_t1_db2 = KeyComposer::new(1, 2);
  assert_eq!(kc_t1_db2.namespace_prefix(), b"\x00\x01\x02");
  assert_eq!(to_hex(&kc_t1_db2.namespace_prefix()), "000102");
  assert_eq!(
    &*compose_string_key(&kc_t1_db2, b"mykey"),
    b"\x00\x01\x02\x00mykey"
  );

  // 5. 复合结构 Subkey 编码验证
  let hash_k = compose_hash_key(&kc_t1, b"user", b"email");
  assert_eq!(kc_t1.extract_user_key(&hash_k), Some(b"user".as_slice()));

  Ok(())
}

#[test]
fn test_key_composer_isolation_all_15_types() -> Void {
  let kc_def = KeyComposer::new(0, 0);
  let kc_t1 = KeyComposer::new(1, 0);
  let kc_t2 = KeyComposer::new(2, 0);

  // 1. String (Raw key)
  assert_ne!(
    compose_string_key(&kc_def, b"k"),
    compose_string_key(&kc_t1, b"k")
  );
  assert_ne!(
    compose_string_key(&kc_t1, b"k"),
    compose_string_key(&kc_t2, b"k")
  );

  // 2. Hash
  assert_ne!(
    compose_hash_meta_key(&kc_def, b"h"),
    compose_hash_meta_key(&kc_t1, b"h")
  );
  assert_ne!(
    compose_hash_key(&kc_t1, b"h", b"f"),
    compose_hash_key(&kc_t2, b"h", b"f")
  );
  assert_ne!(
    compose_hash_prefix(&kc_t1, b"h"),
    compose_hash_prefix(&kc_t2, b"h")
  );

  // 3. List
  assert_ne!(
    compose_list_meta_key(&kc_def, b"l"),
    compose_list_meta_key(&kc_t1, b"l")
  );
  assert_ne!(
    compose_list_item(&kc_t1, b"l", 1),
    compose_list_item(&kc_t2, b"l", 1)
  );

  // 4. Set
  assert_ne!(
    compose_set_meta_key(&kc_def, b"s"),
    compose_set_meta_key(&kc_t1, b"s")
  );
  assert_ne!(
    compose_set_key(&kc_t1, b"s", b"m"),
    compose_set_key(&kc_t2, b"s", b"m")
  );

  // 5. ZSet
  assert_ne!(
    compose_zset_meta_key(&kc_def, b"z"),
    compose_zset_meta_key(&kc_t1, b"z")
  );
  assert_ne!(
    compose_zset_key(&kc_t1, b"z", b"m"),
    compose_zset_key(&kc_t2, b"z", b"m")
  );
  assert_ne!(
    compose_zset_score_key(&kc_t1, b"z", 1.5, b"m"),
    compose_zset_score_key(&kc_t2, b"z", 1.5, b"m")
  );

  // 6. Bitmap
  assert_ne!(
    compose_bitmap_meta_key(&kc_def, b"b"),
    compose_bitmap_meta_key(&kc_t1, b"b")
  );
  assert_ne!(
    compose_bitmap_segment(&kc_t1, b"b", 0),
    compose_bitmap_segment(&kc_t2, b"b", 0)
  );

  // 7. Bloom Filter
  assert_ne!(
    compose_bloom_meta_key(&kc_def, b"bf"),
    compose_bloom_meta_key(&kc_t1, b"bf")
  );
  assert_ne!(
    compose_bloom_item(&kc_t1, b"bf", 1),
    compose_bloom_item(&kc_t2, b"bf", 1)
  );

  // 8. Cuckoo Filter
  assert_ne!(
    compose_cuckoo_meta_key(&kc_def, b"cf"),
    compose_cuckoo_meta_key(&kc_t1, b"cf")
  );
  assert_ne!(
    compose_cuckoo_page(&kc_t1, b"cf", 0, 1),
    compose_cuckoo_page(&kc_t2, b"cf", 0, 1)
  );

  // 9. HyperLogLog
  assert_ne!(
    compose_hll_meta_key(&kc_def, b"hll"),
    compose_hll_meta_key(&kc_t1, b"hll")
  );
  assert_ne!(
    compose_hll_meta_key(&kc_t1, b"hll"),
    compose_hll_meta_key(&kc_t2, b"hll")
  );

  // 10. JSON
  assert_ne!(
    compose_json_meta_key(&kc_def, b"j"),
    compose_json_meta_key(&kc_t1, b"j")
  );

  // 11. SortedInt
  assert_ne!(
    compose_si_meta_key(&kc_def, b"si"),
    compose_si_meta_key(&kc_t1, b"si")
  );
  assert_ne!(
    compose_si_key(&kc_t1, b"si", 42),
    compose_si_key(&kc_t2, b"si", 42)
  );

  // 13. Stream
  assert_ne!(
    compose_stream_meta_key(&kc_def, b"str"),
    compose_stream_meta_key(&kc_t1, b"str")
  );
  assert_ne!(
    compose_stream_item(&kc_t1, b"str", 100, 1),
    compose_stream_item(&kc_t2, b"str", 100, 1)
  );
  assert_ne!(
    compose_stream_group_meta(&kc_t1, b"str", b"g1"),
    compose_stream_group_meta(&kc_t2, b"str", b"g1")
  );
  assert_ne!(
    compose_stream_consumer_meta(&kc_t1, b"str", b"g1", b"c1"),
    compose_stream_consumer_meta(&kc_t2, b"str", b"g1", b"c1")
  );
  assert_ne!(
    compose_stream_pel_item(&kc_t1, b"str", b"g1", 100, 1),
    compose_stream_pel_item(&kc_t2, b"str", b"g1", 100, 1)
  );

  // 14. TDigest
  assert_ne!(
    compose_tdigest_meta_key(&kc_def, b"td"),
    compose_tdigest_meta_key(&kc_t1, b"td")
  );

  // 15. TimeSeries
  assert_ne!(
    compose_ts_meta_key(&kc_def, b"ts"),
    compose_ts_meta_key(&kc_t1, b"ts")
  );
  assert_ne!(
    compose_ts_item(&kc_t1, b"ts", 1000),
    compose_ts_item(&kc_t2, b"ts", 1000)
  );

  Ok(())
}

#[test]
fn test_user_key_extraction_and_in_ns_all_types() -> Void {
  let kc_t = KeyComposer::new(42, 0);
  let key = b"my:unique:user_key";

  // 验证所有 15 种数据结构的 meta / data / subkey 的 extract_user_key 均能正确提取出原 key
  assert_eq!(
    kc_t.extract_user_key(&compose_string_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_hash_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_list_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_set_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_zset_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_bitmap_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_bloom_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_cuckoo_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_hll_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_json_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_si_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_stream_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_tdigest_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_ts_meta_key(&kc_t, key)),
    Some(key.as_slice())
  );

  // 验证子键提取
  assert_eq!(
    kc_t.extract_user_key(&compose_bitmap_segment(&kc_t, key, 0)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_si_key(&kc_t, key, 1234)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_ts_item(&kc_t, key, 9999)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_bloom_item(&kc_t, key, 1)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_cuckoo_page(&kc_t, key, 0, 1)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_stream_item(&kc_t, key, 100, 2)),
    Some(key.as_slice())
  );
  assert_eq!(
    kc_t.extract_user_key(&compose_stream_group_meta(&kc_t, key, b"grp")),
    Some(key.as_slice())
  );

  // 验证默认命名空间下首字节为 0x70~0x7F 的常见用户键绝不会被错误拦截
  let kc_def = KeyComposer::new(0, 0);
  assert_eq!(
    kc_def.extract_user_key(&compose_string_key(&kc_def, b"password")),
    Some(b"password".as_slice())
  );
  assert_eq!(
    kc_def.extract_user_key(&compose_string_key(&kc_def, b"user")),
    Some(b"user".as_slice())
  );

  // 验证对系统管理域（\x00\x70 ~ \x00\x7F）与非法前缀被严格隔离与过滤
  assert_eq!(kc_t.extract_user_key(b"\x00\x70:ns:name:foo"), None);
  assert_eq!(kc_t.extract_user_key(b"\x00\x70:ns:token:xxx"), None);
  assert_eq!(kc_t.extract_user_key(b"\x00\x71:tenant1:db:\x01"), None);
  assert!(!kc_t.is_key_in_ns(b"\x00\x70:ns:token:xxx"));
  assert!(!kc_t.is_key_in_ns(b"\x00\x71:tenant1:db:\x01"));

  Ok(())
}

#[test]
fn test_transform_key_across_namespaces() -> Void {
  let kc_def = KeyComposer::new(0, 0);
  let kc_db1 = KeyComposer::new(0, 1);
  let kc_db2 = KeyComposer::new(0, 2);
  let kc_t1 = KeyComposer::new(1, 0);

  // 1. 默认空间 (ns 0, db 0) -> db1 (ns 0, db 1)
  let raw_def = b"\x00\x00\x00\x00my_string_key";
  let transformed_db1 = kc_def
    .transform_key_to_target_bytes(raw_def, &kc_db1)
    .unwrap();
  assert_eq!(transformed_db1, b"\x00\x00\x01\x00my_string_key");

  let hash_def = b"\x00\x00\x00\x01:my_hash";
  let transformed_h_db1 = kc_def
    .transform_key_to_target_bytes(hash_def, &kc_db1)
    .unwrap();
  assert_eq!(transformed_h_db1, b"\x00\x00\x01\x01:my_hash");

  // 2. db1 -> 默认空间
  assert_eq!(
    kc_db1.transform_key_to_target_bytes(&transformed_db1, &kc_def),
    Some(b"\x00\x00\x00\x00my_string_key".to_vec())
  );

  // 3. db1 -> db2
  assert_eq!(
    kc_db1.transform_key_to_target_bytes(&transformed_db1, &kc_db2),
    Some(b"\x00\x00\x02\x00my_string_key".to_vec())
  );

  // 4. db1 -> tenant1 (ns 1, db 0)
  let expected_t1 = b"\x00\x01\x00\x00my_string_key";
  assert_eq!(
    kc_db1.transform_key_to_target_bytes(&transformed_db1, &kc_t1),
    Some(expected_t1.to_vec())
  );

  Ok(())
}

#[test]
fn test_wedb_namespace_db_hierarchy_lifecycle() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);
  let db = wedb.ns(0)?.db(0)?;

  // 1. open 返回的是 (ns=0, db=0) 的 Db 句柄
  assert_eq!(db.ns_id(), 0);
  assert_eq!(db.id(), 0);
  assert!(db.is_default());

  // 2. 通过 wedb.ns 切换命名空间
  let ns1 = wedb.ns(1)?;
  assert_eq!(ns1.id(), 1);

  let ns2 = wedb.ns(2)?;
  assert_eq!(ns2.id(), 2);

  // 3. 从 Namespace 获取 Db 句柄（通过 db(id) 获取具体 Db）
  let ns1_db0 = ns1.db(0)?;
  let ns1_db1 = ns1.db(1)?;
  let ns2_db0 = ns2.db(0)?;

  assert_eq!(ns1_db0.ns_id(), 1);
  assert_eq!(ns1_db0.id(), 0);
  assert_eq!(ns1_db1.ns_id(), 1);
  assert_eq!(ns1_db1.id(), 1);

  // 4. 各个 Db 之间完全数据隔离
  db.set("device", "default_server", [])?;
  ns1_db0.set("device", "ns1_macbook", [])?;
  ns1_db1.set("device", "ns1_db1_ipad", [])?;
  ns2_db0.set("device", "ns2_thinkpad", [])?;

  assert_eq!(db.get("device")?, Some(b"default_server".to_vec()));
  assert_eq!(ns1_db0.get("device")?, Some(b"ns1_macbook".to_vec()));
  assert_eq!(ns1_db1.get("device")?, Some(b"ns1_db1_ipad".to_vec()));
  assert_eq!(ns2_db0.get("device")?, Some(b"ns2_thinkpad".to_vec()));

  // 5. 复合数据结构测试
  ns1_db0.hset("user:10", &[("name", "alice"), ("role", "admin")])?;
  assert_eq!(ns1_db0.hget("user:10", "name")?, Some(b"alice".to_vec()));
  assert_eq!(ns2_db0.hget("user:10", "name")?, None);

  ns1_db0.zadd("scores", &[(100.0, "alice"), (90.0, "bob")], [])?;
  assert_eq!(ns1_db0.zscore("scores", "alice")?, Some(100.0));
  assert_eq!(ns2_db0.zscore("scores", "alice")?, None);

  // 6. 清理单个 DB (rm)
  let removed = ns1_db1.rm()?;
  assert!(removed > 0);
  assert_eq!(ns1_db1.get("device")?, None);
  assert_eq!(ns1_db0.get("device")?, Some(b"ns1_macbook".to_vec()));

  // 7. 清理整个 Namespace (rm on Namespace)
  let cleared = ns1.rm()?;
  assert!(cleared > 0);
  assert_eq!(ns1_db0.get("device")?, None);
  assert_eq!(ns1_db0.hget("user:10", "name")?, None);

  // 验证不影响其他 Namespace
  assert_eq!(db.get("device")?, Some(b"default_server".to_vec()));
  assert_eq!(ns2_db0.get("device")?, Some(b"ns2_thinkpad".to_vec()));

  OK
}

#[test]
fn test_wedb_namespace_auto_increment_and_iterators() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);

  // 1. 初始至少有默认命名空间 0
  let init_ns: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
  assert_eq!(init_ns, vec![0]);

  // 2. 自动分配自增 ID (传入 None)
  let ns1 = wedb.ns(None)?;
  let ns2 = wedb.ns(None)?;
  assert_eq!(ns1.id(), 1);
  assert_eq!(ns2.id(), 2);

  // 3. 在 ns1 下新建 db 1 与 db 2
  let db1 = ns1.db(None)?;
  let db2 = ns1.db(None)?;
  assert_eq!(db1.id(), 1);
  assert_eq!(db2.id(), 2);

  // 4. 纯流式遍历实际存在的命名空间（从 0 开始）
  let all_ns: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
  assert_eq!(all_ns, vec![0, 1, 2]);

  // 从 begin = 1 开始遍历
  let ns_from_1: Vec<u64> = wedb.iter(1).map(|ns| ns.id()).collect();
  assert_eq!(ns_from_1, vec![1, 2]);

  // 5. 纯流式遍历 ns1 下实际存在的数据库索引
  let ns1_dbs: Vec<u64> = ns1.iter(0).collect();
  assert_eq!(ns1_dbs, vec![0, 1, 2]);

  // 从 begin = 2 开始遍历 db
  let ns1_dbs_from_2: Vec<u64> = ns1.iter(2).collect();
  assert_eq!(ns1_dbs_from_2, vec![2]);

  Ok(())
}

#[test]
fn test_namespace_persistence_across_restarts() -> Void {
  let dir = tempfile::tempdir()?;
  let path = dir.path();

  // 1. 第一次打开数据库，分配两个租户并写入数据
  {
    let engine = Fjall::open(path)?;
    let wedb = WeDb::new(engine);
    let ns1 = wedb.ns(None)?;
    let ns2 = wedb.ns(None)?;
    assert_eq!(ns1.id(), 1);
    assert_eq!(ns2.id(), 2);

    let ns1_db = ns1.db(0)?;
    let ns2_db = ns2.db(0)?;

    ns1_db.set("key1", "val1", [])?;
    ns2_db.set("key2", "val2", [])?;
  }

  // 2. 第二次重新打开数据库，自增 ID 继续递增分配 ID = 3
  {
    let engine = Fjall::open(path)?;
    let wedb = WeDb::new(engine);
    let ns1_db = wedb.ns(1)?.db(0)?;
    let ns2_db = wedb.ns(2)?.db(0)?;
    let ns3 = wedb.ns(None)?;
    assert_eq!(ns3.id(), 3);
    let ns3_db = ns3.db(0)?;

    assert_eq!(ns1_db.get("key1")?.unwrap(), b"val1");
    assert_eq!(ns2_db.get("key2")?.unwrap(), b"val2");

    ns3_db.set("key3", "val3", [])?;
    assert_eq!(ns3_db.get("key3")?.unwrap(), b"val3");

    let all_ns: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
    assert_eq!(all_ns, vec![0, 1, 2, 3]);
  }

  Ok(())
}

#[test]
fn test_anti_penetration_and_binary_isolation() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);
  let db0 = wedb.ns(0)?.db(0)?;
  let db1 = wedb.ns(1)?.db(0)?;

  // 恶意二进制构造测试
  let malicious_bin_key = b"\x00\x02\x01\x00user_key";
  db0.set(malicious_bin_key, b"fake_val", [])?;
  db0.hset(b"my_hash", &[(b"f1", b"v1")])?;
  db1.set(b"secret", b"real_val", [])?;

  assert_eq!(db0.get(malicious_bin_key)?.unwrap(), b"fake_val".to_vec());
  assert_eq!(db0.hget(b"my_hash", b"f1")?.unwrap(), b"v1".to_vec());
  assert_eq!(db1.get(b"secret")?.unwrap(), b"real_val".to_vec());

  db0.rm()?;
  assert_eq!(db0.get(malicious_bin_key)?, None);
  assert_eq!(db0.hget(b"my_hash", b"f1")?, None);
  assert_eq!(db1.get(b"secret")?.unwrap(), b"real_val");

  Ok(())
}

#[test]
fn test_engine_advanced_apis() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);
  let db = wedb.ns(0)?.db(0)?;

  // 1. 验证引擎元数据与内存指标接口
  assert!(wedb.is_kv_separated());
  assert_eq!(wedb.fragmented_blob_bytes(), 0);
  let _buf_size = wedb.write_buffer_size();
  let _cache_size = wedb.cache_size();
  let _cache_cap = wedb.cache_capacity();
  assert_eq!(wedb.outstanding_flushes(), 0);

  // 2. 验证 Batch 中的 rm_weak 支持
  let mut batch = db.batch();
  batch.insert_data(b"item:1", b"val:1");
  batch.insert_data(b"item:2", b"val:2");
  batch.rm_weak_data(b"item:1");
  batch.commit()?;

  assert_eq!(db.data().get(b"item:1")?, None);
  assert_eq!(db.data().get(b"item:2")?.as_deref(), Some(&b"val:2"[..]));

  // 3. 验证 dbsize 与 wedb rm
  assert!(wedb.dbsize()? >= 1);
  wedb.rm()?;
  assert!(db.data().is_empty()?);
  assert!(db.meta().is_empty()?);

  Ok(())
}

#[test]
fn test_db_new_and_auto_allocation() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);
  let ns0 = wedb.ns(0)?;

  // 1. 默认命名空间下的 db 自动分配
  let db1 = ns0.db(None)?;
  assert_eq!(db1.ns_id(), 0);
  assert_eq!(db1.id(), 1);

  let db2 = ns0.db(None)?;
  assert_eq!(db2.ns_id(), 0);
  assert_eq!(db2.id(), 2);

  let db3 = ns0.db(None)?;
  assert_eq!(db3.ns_id(), 0);
  assert_eq!(db3.id(), 3);

  // 验证 Catalog 中收录了 0, 1, 2, 3
  let default_ns = wedb.ns(0)?;
  let dbs: Vec<u64> = default_ns.iter(0).collect();
  assert_eq!(dbs, vec![0, 1, 2, 3]);

  // 2. 新建命名空间并在其中分配自增 DB
  let ns1 = wedb.ns(None)?;
  assert_eq!(ns1.id(), 1);

  let ns1_db1 = ns1.db(None)?;
  assert_eq!(ns1_db1.ns_id(), 1);
  assert_eq!(ns1_db1.id(), 1);

  let ns1_db2 = ns1.db(None)?;
  assert_eq!(ns1_db2.ns_id(), 1);
  assert_eq!(ns1_db2.id(), 2);

  // 3. 跨租户命名空间隔离验证
  let ns2 = wedb.ns(None)?;
  assert_eq!(ns2.id(), 2);
  let ns2_db1 = ns2.db(None)?;
  assert_eq!(ns2_db1.ns_id(), 2);
  assert_eq!(ns2_db1.id(), 1); // 命名空间 2 中的 DB ID 独立从 1 开始

  // 4. 数据写入与隔离
  db1.set(b"key1", b"val_db1", [])?;
  db2.set(b"key1", b"val_db2", [])?;
  ns1_db1.set(b"key1", b"val_ns1_db1", [])?;
  ns2_db1.set(b"key1", b"val_ns2_db1", [])?;

  assert_eq!(db1.get(b"key1")?.unwrap(), b"val_db1");
  assert_eq!(db2.get(b"key1")?.unwrap(), b"val_db2");
  assert_eq!(ns1_db1.get(b"key1")?.unwrap(), b"val_ns1_db1");
  assert_eq!(ns2_db1.get(b"key1")?.unwrap(), b"val_ns2_db1");

  // 5. Namespace rm 重置该命名空间的发号器
  ns1.rm()?;
  let ns1_reopened = wedb.ns(1)?;
  let new_db_after_rm = ns1_reopened.db(None)?;
  assert_eq!(new_db_after_rm.id(), 1);
  assert_eq!(ns2_db1.get(b"key1")?.unwrap(), b"val_ns2_db1"); // ns2 依然有效

  Ok(())
}

#[test]
fn test_namespace_and_db_rm() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);

  // 1. 创建多个命名空间与数据库并写入数据
  let ns1 = wedb.ns(None)?;
  let ns1_db1 = ns1.db(None)?;
  let ns1_db2 = ns1.db(None)?;

  let ns2 = wedb.ns(None)?;
  let ns2_db1 = ns2.db(None)?;

  ns1_db1.set("ns1_k1", "v1", [])?;
  ns1_db2.set("ns1_k2", "v2", [])?;
  ns2_db1.set("ns2_k1", "v3", [])?;

  // 验证当前存在的命名空间与数据库
  let all_ns: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
  assert_eq!(all_ns, vec![0, 1, 2]);

  let ns1_dbs: Vec<u64> = ns1.iter(0).collect();
  assert_eq!(ns1_dbs, vec![0, 1, 2]);

  // 2. 删除 ns1 下的 db 1 (db.rm()：清理数据并在 Catalog 中注销)
  let removed_db_entries = ns1_db1.rm()?;
  assert!(removed_db_entries > 0);
  assert_eq!(ns1_db1.get("ns1_k1")?, None);
  assert_eq!(ns1_db2.get("ns1_k2")?.unwrap(), b"v2");

  // 删除后，ns1 迭代器中不再出现 db 1
  let ns1_dbs_after: Vec<u64> = ns1.iter(0).collect();
  assert_eq!(ns1_dbs_after, vec![0, 2]);

  // 3. 删除整个 ns1 (ns.rm()：清理命名空间所有数据并在 Catalog 中注销)
  let removed_ns_entries = ns1.rm()?;
  assert!(removed_ns_entries > 0);
  assert_eq!(ns1_db2.get("ns1_k2")?, None);

  // 删除后，wedb 迭代器中不再出现 ns 1
  let all_ns_after: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
  assert_eq!(all_ns_after, vec![0, 2]);

  // ns2 及其数据完好无损
  assert_eq!(ns2_db1.get("ns2_k1")?.unwrap(), b"v3");
  let ns2_dbs: Vec<u64> = ns2.iter(0).collect();
  assert_eq!(ns2_dbs, vec![0, 1]);

  Ok(())
}

#[test]
fn test_db_select_and_with_ns_switch() -> Void {
  let dir = tempfile::tempdir()?;
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);

  // 1. 通过 wedb.db(0) 直接获取默认命名空间默认库
  let db0 = wedb.db(0)?;
  assert_eq!(db0.ns_id(), 0);
  assert_eq!(db0.id(), 0);
  db0.set("shared_key", "val_db0", [])?;

  // 2. 使用 db0.select(1) 切换到当前命名空间下的 db 1
  let db1 = db0.select(1)?;
  assert_eq!(db1.ns_id(), 0);
  assert_eq!(db1.id(), 1);
  db1.set("shared_key", "val_db1", [])?;

  // 验证隔离
  assert_eq!(db0.get("shared_key")?.unwrap(), b"val_db0");
  assert_eq!(db1.get("shared_key")?.unwrap(), b"val_db1");

  // 3. 使用 db1.with_ns(5) 跨命名空间切换
  let db_ns5 = db1.with_ns(5)?;
  assert_eq!(db_ns5.ns_id(), 5);
  assert_eq!(db_ns5.id(), 1);
  assert_eq!(db_ns5.get("shared_key")?, None);

  db_ns5.set("shared_key", "val_ns5_db1", [])?;
  assert_eq!(db_ns5.get("shared_key")?.unwrap(), b"val_ns5_db1");
  assert_eq!(db1.get("shared_key")?.unwrap(), b"val_db1");

  Ok(())
}
