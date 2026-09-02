use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, Partition, WeDb, api::set::compose_set_meta_key, key_composer::KeyComposer, set::SetMeta,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_set_metadata_codec() -> Void {
  let meta = SetMeta::new(1700000000000, 101, 55);

  // 标准 26 字节
  let enc = meta.encode();
  assert_eq!(enc.len(), SetMeta::ENCODED_SIZE);
  let dec = SetMeta::decode(&enc).expect("decode failed");
  assert_eq!(dec.base.size, 55);
  assert_eq!(dec.base.version, 101);
  assert_eq!(dec.size(), 55);
  assert_eq!(dec.version(), 101);
  assert_eq!(dec.expire_at(), 1700000000000);
  assert!(!dec.is_empty());

  // Kvrocks 紧凑 25 字节
  let kv_enc = meta.encode_kvrocks();
  assert_eq!(kv_enc.len(), SetMeta::KVROCKS_ENCODED_SIZE);
  let kv_dec = SetMeta::decode(&kv_enc).expect("decode kvrocks failed");
  assert_eq!(kv_dec.base.size, 55);
  assert_eq!(kv_dec.base.version, 101);

  // new_with_version
  let v_meta = SetMeta::new_with_version(0, 10);
  assert_eq!(v_meta.size(), 10);
  assert!(v_meta.version() > 0);

  Ok(())
}

#[test]
fn test_set_basic_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 空操作测试
  assert_eq!(db.sadd("skey", &[] as &[&str])?, 0);
  assert_eq!(db.srem("skey", &[] as &[&str])?, 0);
  assert_eq!(db.scard("non_existent")?, 0);
  assert!(!db.sismember("non_existent", "m1")?);
  assert_eq!(
    db.smismember("non_existent", &["m1", "m2"])?,
    vec![false, false]
  );
  assert!(db.smembers("non_existent")?.is_empty());

  // 基础添加与基数
  assert_eq!(db.sadd("skey", &["m1", "m2", "m3"])?, 3);
  assert_eq!(db.sadd("skey", &["m2", "m4", "m4"])?, 1);
  assert_eq!(db.scard("skey")?, 4);

  assert!(db.sismember("skey", "m1")?);
  assert!(db.sismember("skey", "m2")?);
  assert!(db.sismember("skey", "m3")?);
  assert!(db.sismember("skey", "m4")?);
  assert!(!db.sismember("skey", "m5")?);

  let mism = db.smismember("skey", &["m1", "m5", "m3", "m99"])?;
  assert_eq!(mism, vec![true, false, true, false]);

  assert_eq!(db.srem("skey", &["m1", "m5"])?, 1);
  assert_eq!(db.scard("skey")?, 3);

  let members = db.smembers("skey")?;
  assert_eq!(members.len(), 3);

  // siter 流式遍历
  let mut collected = Vec::new();
  db.siter("skey", |m| {
    collected.push(m.to_vec());
    true
  })?;
  assert_eq!(collected.len(), 3);

  // SMOVE 测试
  assert!(db.smove("skey", "skey2", "m2")?);
  assert_eq!(db.scard("skey")?, 2);
  assert_eq!(db.scard("skey2")?, 1);
  assert!(!db.smove("skey", "skey2", "nonexistent")?);
  // 自身移动
  assert!(db.smove("skey2", "skey2", "m2")?);
  assert_eq!(db.scard("skey2")?, 1);

  // 清空集合并校验元数据删除
  assert_eq!(db.srem("skey", &["m3", "m4"])?, 2);
  assert_eq!(db.scard("skey")?, 0);
  assert_eq!(db.smembers("skey")?.len(), 0);

  Ok(())
}

#[test]
fn test_set_binary_safety() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let b1 = b"\x00\x01\x02\xff";
  let b2 = b"prefix:\x00:suffix";
  let b3 = b"\xfe\xed\xfa\xce";

  assert_eq!(
    db.sadd("bin_key", &[b1.as_slice(), b2.as_slice(), b3.as_slice()])?,
    3
  );
  assert_eq!(db.scard("bin_key")?, 3);
  assert!(db.sismember("bin_key", b1)?);
  assert!(db.sismember("bin_key", b2)?);
  assert!(db.sismember("bin_key", b3)?);

  let members = db.smembers("bin_key")?;
  assert_eq!(members.len(), 3);
  assert!(members.contains(&b1.to_vec()));
  assert!(members.contains(&b2.to_vec()));
  assert!(members.contains(&b3.to_vec()));

  assert_eq!(db.srem("bin_key", &[b2.as_slice()])?, 1);
  assert_eq!(db.scard("bin_key")?, 2);
  assert!(!db.sismember("bin_key", b2)?);

  // 2. 二进制 Key 自身测试（非 UTF-8 字节作为集合 Key）
  let raw_key = b"\x00\xff\xfe_set_key";
  assert_eq!(db.sadd(raw_key, &[b"v1", b"v2"])?, 2);
  assert_eq!(db.scard(raw_key)?, 2);
  assert!(db.sismember(raw_key, b"v1")?);
  assert!(db.sismember(raw_key, b"v2")?);
  let raw_members = db.smembers(raw_key)?;
  assert_eq!(raw_members.len(), 2);
  assert_eq!(db.srem(raw_key, &[b"v1"])?, 1);
  assert_eq!(db.scard(raw_key)?, 1);

  Ok(())
}

#[test]
fn test_set_pop_and_randmember() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.sadd("pop_set", &["a", "b", "c", "d", "e"])?;
  assert_eq!(db.scard("pop_set")?, 5);

  // SPOP 0
  assert!(db.spop("pop_set", 0)?.is_empty());
  assert_eq!(db.scard("pop_set")?, 5);

  // SRANDMEMBER 正数
  let r1 = db.srandmember("pop_set", 2)?;
  assert_eq!(r1.len(), 2);
  assert_ne!(r1[0], r1[1]); // 无重复

  let r_all = db.srandmember("pop_set", 10)?;
  assert_eq!(r_all.len(), 5);

  // SRANDMEMBER 负数（允许重复）
  let r_neg = db.srandmember("pop_set", -10)?;
  assert_eq!(r_neg.len(), 10);

  // SPOP 随机抽取 2 个
  let popped = db.spop("pop_set", 2)?;
  assert_eq!(popped.len(), 2);
  assert_eq!(db.scard("pop_set")?, 3);
  for p in &popped {
    assert!(!db.sismember("pop_set", p)?);
  }

  // SPOP 全部剩余
  let popped_rest = db.spop("pop_set", 10)?;
  assert_eq!(popped_rest.len(), 3);
  assert_eq!(db.scard("pop_set")?, 0);
  assert!(db.smembers("pop_set")?.is_empty());

  // 对空集合 pop
  assert!(db.spop("pop_set", 1)?.is_empty());

  Ok(())
}

#[test]
fn test_set_algebra_ops_and_scan() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.sadd("set_a", &["1", "2", "3", "4"])?;
  db.sadd("set_b", &["3", "4", "5", "6"])?;
  db.sadd("set_c", &["4", "6", "7"])?;

  // SDIFF & SDIFFSTORE & SDIFFCARD
  let diff = db.sdiff(&["set_a", "set_b"])?;
  assert_eq!(diff.len(), 2);
  let diff_self = db.sdiff(&["set_a", "set_a"])?;
  assert!(diff_self.is_empty());
  assert_eq!(db.sdiffcard(&["set_a", "set_b"], 0)?, 2);
  assert_eq!(db.sdiffcard(&["set_a", "set_b"], 1)?, 1);

  assert_eq!(db.sdiffstore("diff_dst", &["set_a", "set_b"])?, 2);
  assert_eq!(db.scard("diff_dst")?, 2);

  // SUNION & SUNIONSTORE & SUNIONCARD
  let union_res = db.sunion(&["set_a", "set_b", "set_c"])?;
  assert_eq!(union_res.len(), 7);
  assert_eq!(db.sunioncard(&["set_a", "set_b", "set_c"], 0)?, 7);
  assert_eq!(db.sunioncard(&["set_a", "set_b", "set_c"], 4)?, 4);

  assert_eq!(
    db.sunionstore("union_dst", &["set_a", "set_b", "set_c"])?,
    7
  );
  assert_eq!(db.scard("union_dst")?, 7);

  // SINTER & SINTERSTORE & SINTERCARD
  let inter_res = db.sinter(&["set_a", "set_b", "set_c"])?;
  assert_eq!(inter_res, vec![b"4".to_vec()]);
  assert_eq!(
    db.sinterstore("inter_dst", &["set_a", "set_b", "set_c"])?,
    1
  );
  assert_eq!(db.sintercard(&["set_a", "set_b", "set_c"], 0)?, 1);
  assert_eq!(db.sintercard(&["set_a", "set_b", "set_c"], 5)?, 1);

  // SINTER 短路测试（与不存在的空 key 交集必为空）
  assert!(db.sinter(&["set_a", "empty_key"])?.is_empty());
  assert_eq!(db.sintercard(&["set_a", "empty_key"], 0)?, 0);

  // SSCAN
  let (cur, page) = db.sscan("union_dst", 0, None, Some(4))?;
  assert_eq!(page.len(), 4);
  assert_eq!(cur, 4);

  let (cur2, page2) = db.sscan("union_dst", cur, None, Some(10))?;
  assert_eq!(page2.len(), 3);
  assert_eq!(cur2, 0);

  // SSCAN Pattern 过滤
  let (_, pattern_res) = db.sscan("union_dst", 0, Some(b"[1-3]"), Some(10))?;
  assert_eq!(pattern_res.len(), 3);

  Ok(())
}

#[test]
fn test_set_expiration_behavior() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 手动写入一个已过期的元数据
  let kc = KeyComposer::default();
  let meta_k = compose_set_meta_key(&kc, b"expired_set");
  let expired_meta = SetMeta::new(1000, 1, 5); // expire_at = 1000ms
  db.meta().insert(&meta_k, &expired_meta.encode())?;

  // 所有只读与删除操作对过期 key 应视为不存在
  assert_eq!(db.scard("expired_set")?, 0);
  assert!(!db.sismember("expired_set", "item")?);
  assert_eq!(
    db.smismember("expired_set", &["i1", "i2"])?,
    vec![false, false]
  );
  assert!(db.smembers("expired_set")?.is_empty());
  assert_eq!(db.srem("expired_set", &["item"])?, 0);
  assert!(db.spop("expired_set", 1)?.is_empty());
  assert!(db.srandmember("expired_set", 1)?.is_empty());
  let (cur, scan_items) = db.sscan("expired_set", 0, None, None)?;
  assert_eq!(cur, 0);
  assert!(scan_items.is_empty());

  // SADD 会重新创建并覆盖过期 key，并清理旧残留子键
  assert_eq!(db.sadd("expired_set", &["new_item"])?, 1);
  assert_eq!(db.scard("expired_set")?, 1);
  assert!(db.sismember("expired_set", "new_item")?);
  assert!(!db.sismember("expired_set", "item")?); // 确保旧键被彻底清理
  let sm = db.smembers("expired_set")?;
  assert_eq!(sm, vec![b"new_item".to_vec()]);

  Ok(())
}

#[test]
fn test_set_ttl_and_persist_helpers() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.sadd("ttl_set", &["a", "b", "c"])?;
  assert_eq!(db.scard("ttl_set")?, 3);

  // 未设置过期时间时 TTL 为 -1
  assert_eq!(db.ttl("ttl_set")?, -1);

  // 设置绝对过期时间
  let future_ts = ts_::sec() + 300;
  assert!(db.expireat("ttl_set", future_ts)?);
  let ttl_val = db.ttl("ttl_set")?;
  assert!(ttl_val > 0 && ttl_val <= 300);

  // key_persist 移除过期时间
  assert!(db.persist("ttl_set")?);
  assert_eq!(db.ttl("ttl_set")?, -1);

  // 不存在的 key
  assert!(!db.expireat("non_existent", future_ts)?);
  assert_eq!(db.ttl("non_existent")?, -2);
  assert!(!db.persist("non_existent")?);

  // 测试 expire 与 pttl
  assert!(db.expire("ttl_set", 60)?);
  assert!(db.ttl("ttl_set")? > 0);
  assert!(db.pttl("ttl_set")? > 0);
  assert!(db.persist("ttl_set")?);
  assert_eq!(db.ttl("ttl_set")?, -1);

  assert_eq!(db.ttl("non_existent")?, -2);
  assert_eq!(db.pttl("non_existent")?, -2);

  assert!(db.expire("ttl_set", 60)?);
  assert!(db.pttl("ttl_set")? > 0);
  assert!(db.persist("ttl_set")?);

  Ok(())
}

#[test]
fn test_set_sinter_and_sdiff_adaptive_point_lookups() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 创建一个小集合和一个大集合以触发自适应点查分支 (len * 4 < next_card)
  let mut small_items = Vec::new();
  for i in 0..5 {
    small_items.push(format!("item_{i}"));
  }
  let small_slices: Vec<&str> = small_items.iter().map(|s| s.as_str()).collect();
  db.sadd("small_set", &small_slices)?;

  let mut large_items = Vec::new();
  for i in 0..100 {
    large_items.push(format!("item_{i}"));
  }
  let large_slices: Vec<&str> = large_items.iter().map(|s| s.as_str()).collect();
  db.sadd("large_set", &large_slices)?;

  // SINTER：交集应为 small_set 的全部 5 个元素
  let inter = db.sinter(&["small_set", "large_set"])?;
  assert_eq!(inter.len(), 5);

  // SINTERCARD
  assert_eq!(db.sintercard(&["small_set", "large_set"], 0)?, 5);
  assert_eq!(db.sintercard(&["small_set", "large_set"], 3)?, 3);
  assert_eq!(db.sintercard(&["small_set", "large_set"], 10)?, 5);

  // 空集交集短路
  assert_eq!(db.sintercard(&["small_set", "non_existent"], 0)?, 0);
  assert!(db.sinter(&["small_set", "non_existent"])?.is_empty());

  // SDIFF 自适应点查：large_set - small_set (large_set 大，small_set 小 -> standard scan)
  let diff1 = db.sdiff(&["large_set", "small_set"])?;
  assert_eq!(diff1.len(), 95);

  // SDIFF 自适应点查：small_set - large_set (small_set 5 个，large_set 100 个 -> 点查)
  let diff2 = db.sdiff(&["small_set", "large_set"])?;
  assert_eq!(diff2.len(), 0);

  // SDIFFCARD
  assert_eq!(db.sdiffcard(&["large_set", "small_set"], 0)?, 95);
  assert_eq!(db.sdiffcard(&["large_set", "small_set"], 10)?, 10);

  Ok(())
}

#[test]
fn test_set_overwrite_and_smove_scenarios() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. overwrite_set
  assert_eq!(db.overwrite_set("ow_key", &["x", "y", "z", "z"])?, 3);
  assert_eq!(db.scard("ow_key")?, 3);
  assert!(db.sismember("ow_key", "x")?);
  assert!(db.sismember("ow_key", "y")?);
  assert!(db.sismember("ow_key", "z")?);

  // 再次覆盖为空
  assert_eq!(db.overwrite_set("ow_key", &[] as &[&str])?, 0);
  assert_eq!(db.scard("ow_key")?, 0);
  assert!(db.smembers("ow_key")?.is_empty());

  // 2. SMOVE 各种场景
  db.sadd("src_set", &["alpha", "beta"])?;
  db.sadd("dst_set", &["gamma"])?;

  // 移动存在的元素
  assert!(db.smove("src_set", "dst_set", "alpha")?);
  assert_eq!(db.scard("src_set")?, 1);
  assert_eq!(db.scard("dst_set")?, 2);
  assert!(!db.sismember("src_set", "alpha")?);
  assert!(db.sismember("dst_set", "alpha")?);

  // 移动到已经包含该元素的目标集合
  assert!(db.smove("src_set", "dst_set", "beta")?);
  assert_eq!(db.scard("src_set")?, 0); // src 变为空并清除元数据
  assert_eq!(db.scard("dst_set")?, 3);

  // 对已空的源集合进行 move
  assert!(!db.smove("src_set", "dst_set", "beta")?);

  // 移动不存在的元素
  assert!(!db.smove("dst_set", "new_dst", "not_exist")?);

  // 移动到全新集合
  assert!(db.smove("dst_set", "fresh_dst", "gamma")?);
  assert_eq!(db.scard("fresh_dst")?, 1);
  assert_eq!(db.scard("dst_set")?, 2);

  Ok(())
}

#[test]
fn test_set_wrong_type_validation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 设置一个普通的 String 类型键
  db.set("str_key", "str_value", [])?;

  // 2. SADD 到已存在的 String 键上应报错 WRONGTYPE
  let sadd_err = db.sadd("str_key", &["m1"]).unwrap_err();
  assert!(sadd_err.to_string().contains("WRONGTYPE"));

  // 3. SMOVE 目标键为 String 类型时应报错 WRONGTYPE
  db.sadd("valid_set", &["item1"])?;
  let smove_err = db.smove("valid_set", "str_key", "item1").unwrap_err();
  assert!(smove_err.to_string().contains("WRONGTYPE"));

  // 4. 源 key 未被误删除
  assert_eq!(db.scard("valid_set")?, 1);
  assert!(db.sismember("valid_set", "item1")?);

  // 5. 设置一个 Hash 类型键
  db.hset("hash_key", &[("field1", "val1")])?;
  let sadd_hash_err = db.sadd("hash_key", &["m1"]).unwrap_err();
  assert!(sadd_hash_err.to_string().contains("WRONGTYPE"));

  let smove_hash_err = db.smove("valid_set", "hash_key", "item1").unwrap_err();
  assert!(smove_hash_err.to_string().contains("WRONGTYPE"));

  // 6. 对 String 键执行各种 Set 只读与变更操作均应报错 WRONGTYPE
  assert!(
    db.scard("str_key")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sismember("str_key", "m1")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.smismember("str_key", &["m1"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.smembers("str_key")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.srem("str_key", &["m1"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.spop("str_key", 1)
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.srandmember("str_key", 1)
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sscan("str_key", 0, None, None)
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sdiff(&["str_key"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sunion(&["str_key"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sinter(&["str_key"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.overwrite_set("str_key", &["m1"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.smove("str_key", "valid_set", "m1")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );

  // 7. 对 Hash 键执行 Set 操作同样报错 WRONGTYPE
  assert!(
    db.scard("hash_key")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sismember("hash_key", "field1")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.smembers("hash_key")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.srem("hash_key", &["field1"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.overwrite_set("hash_key", &["m1"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );

  Ok(())
}

#[test]
fn test_set_expired_key_overwritten_by_other_type() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 创建 Set 并设置过期（已过期状态）
  let kc = KeyComposer::default();
  let meta_k = compose_set_meta_key(&kc, b"exp_set");
  let expired_meta = SetMeta::new(100, 1, 3);
  db.meta().insert(&meta_k, &expired_meta.encode())?;

  // 2. 写入原生 String 键覆盖同名 key
  db.set("exp_set", "now_a_string", [])?;

  // 3. 对该 key 执行 Set 操作应精准拦截 WRONGTYPE
  assert!(
    db.scard("exp_set")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sismember("exp_set", "item")
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.sadd("exp_set", &["item"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );
  assert!(
    db.srem("exp_set", &["item"])
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE")
  );

  Ok(())
}

#[test]
fn test_set_algebra_advanced_scenarios() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 多集合交集、并集、差集综合运算
  db.sadd("set1", &["a", "b", "c", "d", "e"])?;
  db.sadd("set2", &["b", "c", "f", "g"])?;
  db.sadd("set3", &["c", "d", "h"])?;

  // SDIFF: set1 - set2 - set3 => {a, e}
  let diff = db.sdiff(&["set1", "set2", "set3"])?;
  assert_eq!(diff.len(), 2);
  assert!(diff.contains(&b"a".to_vec()));
  assert!(diff.contains(&b"e".to_vec()));
  assert_eq!(db.sdiffcard(&["set1", "set2", "set3"], 0)?, 2);
  assert_eq!(db.sdiffcard(&["set1", "set2", "set3"], 1)?, 1);

  // SUNION: set1 | set2 | set3 => {a, b, c, d, e, f, g, h} (8 items)
  let union_res = db.sunion(&["set1", "set2", "set3"])?;
  assert_eq!(union_res.len(), 8);
  assert_eq!(db.sunioncard(&["set1", "set2", "set3"], 0)?, 8);
  assert_eq!(db.sunioncard(&["set1", "set2", "set3"], 5)?, 5);

  // SINTER: set1 & set2 & set3 => {c}
  let inter_res = db.sinter(&["set1", "set2", "set3"])?;
  assert_eq!(inter_res, vec![b"c".to_vec()]);
  assert_eq!(db.sintercard(&["set1", "set2", "set3"], 0)?, 1);
  assert_eq!(db.sintercard(&["set1", "set2", "set3"], 10)?, 1);

  // SINTERCARD limit = 0 returns full cardinality
  assert_eq!(db.sintercard(&["set1", "set2"], 0)?, 2); // {b, c}
  assert_eq!(db.sintercard(&["set1", "set2"], 1)?, 1);

  // SSCAN 边界测试
  let (cur, page) = db.sscan("set1", 0, None, Some(0))?;
  assert_eq!(page.len(), 1); // min step is 1
  assert_eq!(cur, 1);

  let (cur_end, page_end) = db.sscan("set1", 0, None, Some(100))?;
  assert_eq!(page_end.len(), 5);
  assert_eq!(cur_end, 0);

  // SPOP 边界测试：count > n
  let popped_all = db.spop("set1", 100)?;
  assert_eq!(popped_all.len(), 5);
  assert_eq!(db.scard("set1")?, 0);

  Ok(())
}

#[test]
fn test_kvrocks_set_add_and_remove_repeated() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 对标 kvrocks AddAndRemoveRepeated
  let all_members = vec!["m1", "m1", "m2", "m3"];
  let added = db.sadd("key", &all_members)?;
  assert_eq!(added, 3);
  assert_eq!(db.scard("key")?, 3);

  let re_members = vec!["m1", "m2", "m2"];
  let removed = db.srem("key", &re_members)?;
  assert_eq!(removed, 2);
  assert_eq!(db.scard("key")?, 1);
  assert!(db.sismember("key", "m3")?);
  assert!(!db.sismember("key", "m1")?);
  assert!(!db.sismember("key", "m2")?);

  Ok(())
}

#[test]
fn test_kvrocks_set_inter_and_intercard_exhaustive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 对标 kvrocks Inter & InterCard
  let k1 = "key1";
  let k2 = "key2";
  let k3 = "key3";
  let k4 = "key4"; // non-existent
  let k5 = "key5";

  db.sadd(k1, &["a", "b", "c", "d"])?;
  db.sadd(k2, &["c", "d", "e"])?;
  db.sadd(k3, &["e", "f"])?;
  db.sadd(k5, &["a"])?;

  // SINTER key1 key2 => {c, d}
  let inter12 = db.sinter(&[k1, k2])?;
  assert_eq!(inter12.len(), 2);
  assert!(inter12.contains(&b"c".to_vec()));
  assert!(inter12.contains(&b"d".to_vec()));

  // SINTER key1 key2 key3 => empty
  let inter123 = db.sinter(&[k1, k2, k3])?;
  assert!(inter123.is_empty());

  // SINTER with non-existent key4 => empty
  assert!(db.sinter(&[k1, k2, k4])?.is_empty());
  assert!(db.sinter(&[k1, k4, k5])?.is_empty());

  // SINTER single key => returns all members
  let inter1 = db.sinter(&[k1])?;
  assert_eq!(inter1.len(), 4);

  // SINTERCARD
  assert_eq!(db.sintercard(&[k1, k2], 0)?, 2);
  assert_eq!(db.sintercard(&[k1, k2], 1)?, 1);
  assert_eq!(db.sintercard(&[k1, k2], 3)?, 2);
  assert_eq!(db.sintercard(&[k2, k3], 1)?, 1);
  assert_eq!(db.sintercard(&[k1, k3], 5)?, 0);
  assert_eq!(db.sintercard(&[k1, k4], 5)?, 0);
  assert_eq!(db.sintercard(&[k1], 0)?, 4);

  for i in 1..20 {
    let expected = if i >= 4 { 4 } else { i };
    assert_eq!(db.sintercard(&[k1], i)?, expected);
  }

  Ok(())
}

#[test]
fn test_kvrocks_diff_union_store_comprehensive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let k1 = "key1";
  let k2 = "key2";
  let k3 = "key3";

  db.sadd(k1, &["a", "b", "c", "d"])?;
  db.sadd(k2, &["c"])?;
  db.sadd(k3, &["a", "c", "e"])?;

  // SDIFF key1 key2 key3 => {b, d}
  let diff = db.sdiff(&[k1, k2, k3])?;
  assert_eq!(diff.len(), 2);
  assert!(diff.contains(&b"b".to_vec()));
  assert!(diff.contains(&b"d".to_vec()));

  // SDIFFSTORE dst key1 key2 key3
  let saved_diff = db.sdiffstore("diff_dst", &[k1, k2, k3])?;
  assert_eq!(saved_diff, 2);
  assert_eq!(db.scard("diff_dst")?, 2);

  // SUNION key1 key2 key3 => {a, b, c, d, e}
  let union_res = db.sunion(&[k1, k2, k3])?;
  assert_eq!(union_res.len(), 5);

  // SUNIONSTORE dst key1 key2 key3
  let saved_union = db.sunionstore("union_dst", &[k1, k2, k3])?;
  assert_eq!(saved_union, 5);
  assert_eq!(db.scard("union_dst")?, 5);

  // SINTERSTORE dst key1 key2 key3 => {c}
  let saved_inter = db.sinterstore("inter_dst", &[k1, k2, k3])?;
  assert_eq!(saved_inter, 1);
  assert_eq!(db.scard("inter_dst")?, 1);
  assert!(db.sismember("inter_dst", "c")?);

  // Store into existing set overwrites it
  let saved_inter_overwrite = db.sinterstore("diff_dst", &[k1, k2, k3])?;
  assert_eq!(saved_inter_overwrite, 1);
  assert_eq!(db.scard("diff_dst")?, 1);

  Ok(())
}

#[test]
fn test_single_key_fast_paths_and_empty_inputs() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 空输入
  assert_eq!(db.sdiff(&[] as &[&str])?.len(), 0);
  assert_eq!(db.sunion(&[] as &[&str])?.len(), 0);
  assert_eq!(db.sinter(&[] as &[&str])?.len(), 0);
  assert_eq!(db.sdiffcard(&[] as &[&str], 0)?, 0);
  assert_eq!(db.sunioncard(&[] as &[&str], 0)?, 0);
  assert_eq!(db.sintercard(&[] as &[&str], 0)?, 0);

  db.sadd("single_key", &["x", "y", "z"])?;

  // 单 key 快速路径
  assert_eq!(db.sdiff(&["single_key"])?.len(), 3);
  assert_eq!(db.sunion(&["single_key"])?.len(), 3);
  assert_eq!(db.sinter(&["single_key"])?.len(), 3);
  assert_eq!(db.sdiffcard(&["single_key"], 0)?, 3);
  assert_eq!(db.sdiffcard(&["single_key"], 2)?, 2);
  assert_eq!(db.sunioncard(&["single_key"], 0)?, 3);
  assert_eq!(db.sunioncard(&["single_key"], 2)?, 2);
  assert_eq!(db.sintercard(&["single_key"], 0)?, 3);
  assert_eq!(db.sintercard(&["single_key"], 2)?, 2);

  Ok(())
}

#[test]
fn test_set_kvrocks_comprehensive_suite_and_scan_by_member() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. SSCAN_BY_MEMBER 游标范围扫描
  let members = [
    "user_100", "user_101", "user_102", "user_103", "user_104", "user_105",
  ];
  db.sadd("scan_set", &members)?;

  let (next_cursor, batch1) = db.sscan_by_member("scan_set", None, None, Some(3))?;
  assert_eq!(batch1.len(), 3);
  assert!(next_cursor.is_some());
  let c1 = next_cursor.unwrap();

  let (next_cursor2, batch2) = db.sscan_by_member("scan_set", Some(&c1), None, Some(3))?;
  assert_eq!(batch2.len(), 3);
  assert!(next_cursor2.is_some());

  let mut combined = batch1;
  combined.extend(batch2);
  assert_eq!(combined.len(), 6);

  // 模式过滤扫描
  let (_, pat_batch) = db.sscan_by_member("scan_set", None, Some(b"*_10[1-3]"), Some(10))?;
  assert_eq!(pat_batch.len(), 3);

  // 2. SPOP 边界：全量清空、单项弹出与空弹出
  db.sadd("pop_set", &["p1", "p2", "p3", "p4", "p5"])?;
  let single = db.spop_one("pop_set")?;
  assert!(single.is_some());
  assert_eq!(db.scard("pop_set")?, 4);

  // 弹出超过剩余总数（触发全量 clear_prefix_in_batch 快速路径）
  let all_popped = db.spop("pop_set", 100)?;
  assert_eq!(all_popped.len(), 4);
  assert_eq!(db.scard("pop_set")?, 0);
  assert!(db.spop("pop_set", 1)?.is_empty());
  assert_eq!(db.spop_one("pop_set")?, None);

  // 3. SRANDMEMBER 快速路径与负数采样
  db.sadd("rand_set", &["r1", "r2", "r3", "r4"])?;
  let r_one = db.srandmember_one("rand_set")?;
  assert!(r_one.is_some());
  assert!(["r1", "r2", "r3", "r4"].contains(&std::str::from_utf8(&r_one.unwrap()).unwrap()));

  // 负数允许重复采样
  let dup_samples = db.srandmember("rand_set", -10)?;
  assert_eq!(dup_samples.len(), 10);

  // 4. SMOVE 边界
  assert!(!db.smove("nonexistent_src", "rand_set", "r1")?);
  assert!(!db.smove("rand_set", "rand_set_dst", "nonexistent_m")?);
  assert!(db.smove("rand_set", "rand_set", "r1")?); // src == dst
  assert!(db.smove("rand_set", "rand_set_dst", "r1")?);
  assert_eq!(db.scard("rand_set_dst")?, 1);
  assert!(!db.sismember("rand_set", "r1")?);

  // 5. SINTERSTORE / SDIFFSTORE 清空目标键
  db.sadd("diff_src1", &["a", "b"])?;
  db.sadd("diff_src2", &["a", "b"])?;
  db.sadd("diff_target", &["old1", "old2"])?;
  // diff 为空，必须彻底删除 diff_target
  let diff_empty_cnt = db.sdiffstore("diff_target", &["diff_src1", "diff_src2"])?;
  assert_eq!(diff_empty_cnt, 0);
  assert_eq!(db.scard("diff_target")?, 0);
  assert!(db.smembers("diff_target")?.is_empty());

  // 6. OVERWRITE_SET 空列表彻底删除
  db.sadd("over_k", &["v1", "v2"])?;
  assert_eq!(db.overwrite_set("over_k", &[] as &[&str])?, 0);
  assert_eq!(db.scard("over_k")?, 0);

  Ok(())
}
