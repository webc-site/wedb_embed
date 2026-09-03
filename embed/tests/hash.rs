use std::{thread::sleep, time::Duration};

use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  hash::{
    ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING, ERR_HASH_VALUE_NOT_FLOAT,
    ERR_HASH_VALUE_NOT_INTEGER, ERR_INCREMENT_NAN_OR_INFINITY, ERR_INCREMENT_OVERFLOW,
    ERR_WRONG_TYPE, HASH_EXPIRE_COND_FAILED, HASH_EXPIRE_DELETED, HASH_EXPIRE_SET_OK,
    HASH_FIELD_NOT_FOUND, HASH_FIELD_PERSISTENT, HExpire, HGetEx, HSet, HashItemKeyComposer,
    HashLengthMode, HashMeta, HashSubkeyEncodingMode, RangeLex, compose_hash_meta_key,
  },
  key_composer::KeyComposer,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_hash_basic_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let fields = ["test-hash-key-1", "test-hash-key-2", "test-hash-key-3"];
  let values = [
    "hash-test-value-1",
    "hash-test-value-2",
    "hash-test-value-3",
  ];

  let fvs: Vec<(&str, &str)> = fields.iter().copied().zip(values.iter().copied()).collect();

  assert_eq!(db.hset("test_hash->key", &fvs)?, 3);
  assert_eq!(db.hlen("test_hash->key")?, 3);
  assert_eq!(
    db.hlen_with_mode("test_hash->key", HashLengthMode::Approximate)?,
    3
  );

  for (f, v) in &fvs {
    assert_eq!(db.hget("test_hash->key", f)?, Some(v.as_bytes().to_vec()));
    assert!(db.hexists("test_hash->key", f)?);
  }
  assert!(!db.hexists("test_hash->key", "nonexistent_field")?);

  // 重复字段去重覆盖
  assert_eq!(
    db.hset("test_hash->key", &[("test-hash-key-1", "new_val")])?,
    0
  );
  assert_eq!(
    db.hget("test_hash->key", "test-hash-key-1")?,
    Some(b"new_val".to_vec())
  );

  // HSETNX
  assert!(!db.hsetnx("test_hash->key", "test-hash-key-1", "ignored")?);
  assert!(db.hsetnx("test_hash->key", "new_field", "new_val")?);
  assert_eq!(db.hlen("test_hash->key")?, 4);

  // HMGET
  let m_res = db.hmget("test_hash->key", &["test-hash-key-1", "nonexistent"])?;
  assert_eq!(m_res[0], Some(b"new_val".to_vec()));
  assert_eq!(m_res[1], None);

  // HKEYS & HVALS
  let keys = db.hkeys("test_hash->key")?;
  assert_eq!(keys.len(), 4);
  let vals = db.hvals("test_hash->key")?;
  assert_eq!(vals.len(), 4);

  // HDEL
  assert_eq!(
    db.hdel("test_hash->key", &["test-hash-key-1", "nonexistent"])?,
    1
  );
  assert_eq!(db.hlen("test_hash->key")?, 3);

  // HINCRBY & HINCRBYFLOAT
  db.hset("test_hash->key", &[("counter", "10")])?;
  assert_eq!(db.hincrby("test_hash->key", "counter", 5)?, 15);
  assert_eq!(db.hincrby("test_hash->key", "counter", -20)?, -5);

  db.hset("test_hash->key", &[("float", "10.5")])?;
  let f_res = db.hincrbyfloat("test_hash->key", "float", 2.25)?;
  assert!((f_res - 12.75).abs() < 1e-9);

  // HSTRLEN
  assert_eq!(db.hstrlen("test_hash->key", "test-hash-key-2")?, 17);
  assert_eq!(db.hstrlen("test_hash->key", "nonexistent")?, 0);

  // HRANGEBYLEX
  let range_res = db.hrangebylex(
    "test_hash->key",
    RangeLex {
      min: b"hash-".to_vec(),
      max: b"test-hash-key-3".to_vec(),
      maxex: false,
      ..Default::default()
    },
  )?;
  assert!(!range_res.is_empty());

  // HSCAN
  let (cursor, items) = db.hscan("test_hash->key", 0, 10, None)?;
  assert_eq!(cursor, 0);
  assert!(!items.is_empty());

  // HRANDFIELD
  let rand_fields = db.hrandfield("test_hash->key", 2, false)?;
  assert_eq!(rand_fields.len(), 2);

  let rand_fvs = db.hrandfield("test_hash->key", -3, true)?;
  assert_eq!(rand_fvs.len(), 3);

  Ok(())
}

#[test]
fn test_hash_incrby_and_incrbyfloat_errors() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 HINCRBY 溢出错误 (i64::MAX + 1 / i64::MIN - 1)
  db.hset("err_hash", &[("max_int", i64::MAX.to_string().as_str())])?;
  let err_over = db.hincrby("err_hash", "max_int", 1).unwrap_err();
  assert!(err_over.to_string().contains(ERR_INCREMENT_OVERFLOW));

  db.hset("err_hash", &[("min_int", i64::MIN.to_string().as_str())])?;
  let err_under = db.hincrby("err_hash", "min_int", -1).unwrap_err();
  assert!(err_under.to_string().contains(ERR_INCREMENT_OVERFLOW));

  // 2. 测试 HINCRBY 非整数解析错误
  db.hset("err_hash", &[("not_int", "12.34")])?;
  let err_not_int = db.hincrby("err_hash", "not_int", 1).unwrap_err();
  assert!(err_not_int.to_string().contains(ERR_HASH_VALUE_NOT_INTEGER));

  db.hset("err_hash", &[("str_val", "abc")])?;
  let err_str = db.hincrby("err_hash", "str_val", 1).unwrap_err();
  assert!(err_str.to_string().contains(ERR_HASH_VALUE_NOT_INTEGER));

  // 3. 测试 HINCRBYFLOAT NaN / Infinity 校验
  db.hset("err_hash", &[("f_val", "1.0")])?;
  let err_nan = db.hincrbyfloat("err_hash", "f_val", f64::NAN).unwrap_err();
  assert!(err_nan.to_string().contains(ERR_INCREMENT_NAN_OR_INFINITY));

  let err_inf = db
    .hincrbyfloat("err_hash", "f_val", f64::INFINITY)
    .unwrap_err();
  assert!(err_inf.to_string().contains(ERR_INCREMENT_NAN_OR_INFINITY));

  // 4. 测试 HINCRBYFLOAT 非浮点值解析错误
  let err_f_str = db.hincrbyfloat("err_hash", "str_val", 1.0).unwrap_err();
  assert!(err_f_str.to_string().contains(ERR_HASH_VALUE_NOT_FLOAT));

  Ok(())
}

#[test]
fn test_hash_field_expiration_comprehensive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.hset("hexp", &[("f1", "v1"), ("f2", "v2"), ("f3", "v3")])?;

  // 1. HEXPIRE
  let res = db.hexpire("hexp", &["f1", "f2"], 100, [])?;
  assert_eq!(res, vec![HASH_EXPIRE_SET_OK, HASH_EXPIRE_SET_OK]);

  // 2. HTTL / HPTTL
  let ttls = db.httl("hexp", &["f1", "f2", "f3", "f_none"])?;
  assert!(ttls[0] > 0 && ttls[0] <= 100);
  assert!(ttls[1] > 0 && ttls[1] <= 100);
  assert_eq!(ttls[2], HASH_FIELD_PERSISTENT);
  assert_eq!(ttls[3], HASH_FIELD_NOT_FOUND);

  let pttls = db.hpttl("hexp", &["f1", "f3"])?;
  assert!(pttls[0] > 0);
  assert_eq!(pttls[1], HASH_FIELD_PERSISTENT);

  // 3. HEXPIRETIME / HPEXPIRETIME
  let extimes = db.hexpiretime("hexp", &["f1", "f3"])?;
  assert!(extimes[0] > 0);
  assert_eq!(extimes[1], HASH_FIELD_PERSISTENT);

  let pextimes = db.hpexpiretime("hexp", &["f1", "f3"])?;
  assert!(pextimes[0] > 0);
  assert_eq!(pextimes[1], HASH_FIELD_PERSISTENT);

  // 4. 条件检查 (NX / XX / GT / LT)
  let nx_res = db.hexpire("hexp", &["f1", "f3"], 200, [HExpire::Nx])?;
  assert_eq!(nx_res, vec![HASH_EXPIRE_COND_FAILED, HASH_EXPIRE_SET_OK]); // f1 已有 TTL 不满足 NX，f3 持久满足 NX

  let xx_res = db.hexpire("hexp", &["f1", "f2"], 300, [HExpire::Xx])?;
  assert_eq!(xx_res, vec![HASH_EXPIRE_SET_OK, HASH_EXPIRE_SET_OK]); // f1, f2 均有 TTL 满足 XX

  // GT 条件测试
  let gt_fail = db.hexpire("hexp", &["f1"], 50, [HExpire::Gt])?;
  assert_eq!(gt_fail, vec![HASH_EXPIRE_COND_FAILED]); // 50 < 300, 失败
  let gt_succ = db.hexpire("hexp", &["f1"], 500, [HExpire::Gt])?;
  assert_eq!(gt_succ, vec![HASH_EXPIRE_SET_OK]); // 500 > 300, 成功

  // LT 条件测试
  let lt_fail = db.hexpire("hexp", &["f1"], 600, [HExpire::Lt])?;
  assert_eq!(lt_fail, vec![HASH_EXPIRE_COND_FAILED]); // 600 > 500, 失败
  let lt_succ = db.hexpire("hexp", &["f1"], 400, [HExpire::Lt])?;
  assert_eq!(lt_succ, vec![HASH_EXPIRE_SET_OK]); // 400 < 500, 成功

  // 5. HPERSIST
  let persist_res = db.hpersist("hexp", &["f1", "f2", "f3", "f_none"])?;
  assert_eq!(
    persist_res,
    vec![
      HASH_EXPIRE_SET_OK,
      HASH_EXPIRE_SET_OK,
      HASH_EXPIRE_SET_OK,
      HASH_FIELD_NOT_FOUND
    ]
  );
  let persist_again = db.hpersist("hexp", &["f1", "f2", "f3"])?;
  assert_eq!(
    persist_again,
    vec![
      HASH_FIELD_PERSISTENT,
      HASH_FIELD_PERSISTENT,
      HASH_FIELD_PERSISTENT
    ]
  );

  // 6. 立即过期删除 (seconds <= 0)
  let imm_res = db.hexpire("hexp", &["f1"], 0, [])?;
  assert_eq!(imm_res, vec![HASH_EXPIRE_DELETED]);
  assert_eq!(db.hget("hexp", "f1")?, None);
  assert_eq!(db.hlen("hexp")?, 2);

  Ok(())
}

#[test]
fn test_hash_legacy_encoding_compatibility() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 模拟写入 Legacy 编码哈希
  let legacy_meta = HashMeta::new_with_mode(HashSubkeyEncodingMode::Legacy, 0, 100, 1);
  let kc = KeyComposer::default();
  let meta_k = compose_hash_meta_key(&kc, b"legacy_key");
  let mut composer = HashItemKeyComposer::new(&kc, b"legacy_key");
  let item_k = composer.key_for_field(b"f1");

  let mut batch = db.batch();
  batch.insert_meta(&meta_k, &legacy_meta.encode());
  batch.insert_data(item_k, b"val1");
  batch.commit()?;

  // 1. 常规读取支持 Legacy 编码
  assert_eq!(db.hget("legacy_key", "f1")?, Some(b"val1".to_vec()));
  assert_eq!(db.hlen("legacy_key")?, 1);
  assert!(db.hexists("legacy_key", "f1")?);
  assert_eq!(db.hstrlen("legacy_key", "f1")?, 4);

  // 2. 过期相关命令应拒绝 Legacy 编码并返回错误
  let err_exp = db.hexpire("legacy_key", &["f1"], 10, []).unwrap_err();
  assert!(
    err_exp
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_ttl = db.httl("legacy_key", &["f1"]).unwrap_err();
  assert!(
    err_ttl
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_pttl = db.hpttl("legacy_key", &["f1"]).unwrap_err();
  assert!(
    err_pttl
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_ex_time = db.hexpiretime("legacy_key", &["f1"]).unwrap_err();
  assert!(
    err_ex_time
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_pex_time = db.hpexpiretime("legacy_key", &["f1"]).unwrap_err();
  assert!(
    err_pex_time
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_persist = db.hpersist("legacy_key", &["f1"]).unwrap_err();
  assert!(
    err_persist
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_setex = db
    .hsetex("legacy_key", &[("f1", "val2")], [HSet::Ex(100)])
    .unwrap_err();
  assert!(
    err_setex
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  let err_getex = db
    .hgetex("legacy_key", "f1", [HGetEx::Persist])
    .unwrap_err();
  assert!(
    err_getex
      .to_string()
      .contains(ERR_HASH_FIELD_EXPIRATION_LEGACY_ENCODING)
  );

  // 3. 常规写操作（HSETNX, HINCRBY, HINCRBYFLOAT, HSET）对 Legacy 编码无缝兼容
  assert!(!db.hsetnx("legacy_key", "f1", "val_nx")?);
  assert!(db.hsetnx("legacy_key", "f2", "val2")?);
  assert_eq!(db.hget("legacy_key", "f2")?, Some(b"val2".to_vec()));
  assert_eq!(db.hincrby("legacy_key", "num_field", 42)?, 42);
  assert_eq!(db.hincrbyfloat("legacy_key", "float_field", 2.5)?, 2.5);

  // 4. with_hget 零拷贝读取
  let f1_len = db.with_hget("legacy_key", "f1", |v| v.len())?;
  assert_eq!(f1_len, Some(4));
  let nonexist_len = db.with_hget("legacy_key", "nonexist", |v| v.len())?;
  assert_eq!(nonexist_len, None);

  Ok(())
}

#[test]
fn test_hash_hgetdel_and_hsetex_hgetex() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. HSETEX
  assert!(db.hsetex(
    "h_ex_key",
    &[("a", "1"), ("b", "2")],
    [HSet::Ex(10), HSet::Fnx]
  )?);
  assert_eq!(db.hlen("h_ex_key")?, 2);

  // Fnx 遇到已有字段失败
  assert!(!db.hsetex("h_ex_key", &[("a", "1_new")], [HSet::Ex(10), HSet::Fnx])?);

  // 便捷 hsetex 接口
  assert!(db.hsetex("h_ex_key2", &[("c", "100")], [HSet::Ex(30), HSet::Fnx])?);
  assert_eq!(db.hlen("h_ex_key2")?, 1);

  // 2. HGETEX (持久化)
  let g_val = db.hgetex("h_ex_key", "a", [HGetEx::Persist])?;
  assert_eq!(g_val, Some(b"1".to_vec()));
  let g_none = db.hgetex("h_ex_key", "nonexistent", [HGetEx::Persist])?;
  assert_eq!(g_none, None);
  assert_eq!(db.httl("h_ex_key", &["a"])?, vec![HASH_FIELD_PERSISTENT]);

  // 便捷 hgetex 接口
  let g_val = db.hgetex("h_ex_key2", "c", [HGetEx::Persist])?;
  assert_eq!(g_val, Some(b"100".to_vec()));
  assert_eq!(db.httl("h_ex_key2", &["c"])?, vec![HASH_FIELD_PERSISTENT]);

  // 3. HGETDEL
  let del_res = db.hgetdel("h_ex_key", &["a", "b", "c"])?;
  assert_eq!(
    del_res,
    vec![Some(b"1".to_vec()), Some(b"2".to_vec()), None]
  );
  assert_eq!(db.hlen("h_ex_key")?, 0);

  Ok(())
}

#[test]
fn test_hash_expired_key_cleanup_and_ttl_preservation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 HINCRBY 保持现有 LiveTTL 字段过期时间
  db.hset("h_inc_ttl", &[("counter", "10")])?;
  db.hexpire("h_inc_ttl", &["counter"], 300, [])?;
  let ttl_before = db.httl("h_inc_ttl", &["counter"])?[0];
  assert!(ttl_before > 0 && ttl_before <= 300);

  let new_val = db.hincrby("h_inc_ttl", "counter", 5)?;
  assert_eq!(new_val, 15);
  let ttl_after = db.httl("h_inc_ttl", &["counter"])?[0];
  assert!(ttl_after > 0 && ttl_after <= 300);

  // 2. 测试 HINCRBYFLOAT 保持现有 LiveTTL 字段过期时间
  db.hset("h_inc_ttl", &[("float_val", "1.5")])?;
  db.hexpire("h_inc_ttl", &["float_val"], 200, [])?;
  let f_val = db.hincrbyfloat("h_inc_ttl", "float_val", 2.25)?;
  assert!((f_val - 3.75).abs() < 1e-6);
  let f_ttl = db.httl("h_inc_ttl", &["float_val"])?[0];
  assert!(f_ttl > 0 && f_ttl <= 200);

  // 3. 测试 HSTRLEN 对不存在或已过期字段返回 0
  assert_eq!(db.hstrlen("h_inc_ttl", "nonexistent")?, 0);
  assert_eq!(db.hstrlen("h_inc_ttl", "counter")?, 2);

  // 立即过期后 HSTRLEN 查不到
  db.hexpire("h_inc_ttl", &["counter"], 0, [])?;
  assert_eq!(db.hstrlen("h_inc_ttl", "counter")?, 0);

  Ok(())
}

#[test]
fn test_hash_scan_and_repair_and_edge_cases() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 设置字段并设置短期过期时间 (毫秒级)
  db.hset("h_repair", &[("f1", "v1"), ("f2", "v2"), ("f3", "v3")])?;
  db.hpexpire("h_repair", &["f1", "f2"], 15, [])?; // 15 毫秒过期

  // 极速休眠 25ms 让其物理过期
  sleep(Duration::from_millis(25));

  // 此时 f1, f2 已过期，f3 为持久字段
  // 调用 hlen(Accurate) 触发 scanAndRepair
  let len = db.hlen("h_repair")?;
  assert_eq!(len, 1);
  assert_eq!(db.hget("h_repair", "f1")?, None);
  assert_eq!(db.hget("h_repair", "f2")?, None);
  assert_eq!(db.hget("h_repair", "f3")?, Some(b"v3".to_vec()));

  // 2. 将剩余唯一的持久字段 f3 设置为已过期 (立即过期)
  let imm = db.hexpire("h_repair", &["f3"], 0, [])?;
  assert_eq!(imm, vec![HASH_EXPIRE_DELETED]);
  assert_eq!(db.hlen("h_repair")?, 0);
  assert_eq!(db.hgetall("h_repair")?, Vec::<(Vec<u8>, Vec<u8>)>::new());

  // 3. 空键与单键边界测试
  assert_eq!(db.hdel("nonexistent_hash", &["f1", "f2"])?, 0);
  assert_eq!(db.hget("nonexistent_hash", "f1")?, None);
  assert_eq!(
    db.hmget("nonexistent_hash", &["f1", "f2"])?,
    vec![None, None]
  );
  assert_eq!(
    db.httl("nonexistent_hash", &["f1"])?,
    vec![HASH_FIELD_NOT_FOUND]
  );

  Ok(())
}

#[test]
fn test_hash_binary_safe_keys_and_fields() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 非 UTF-8 二进制键和字段
  let bin_key = b"\xff\xfe\x00\x01binary_hash";
  let bin_field1 = b"\x00\x01\x02field";
  let bin_field2 = b"\xaa\xbb\xcc\xdd";
  let bin_val1 = b"\xde\xad\xbe\xef";
  let bin_val2 = b"\x01\x02\x03\x04\x05";

  assert_eq!(
    db.hset(
      bin_key,
      &[
        (bin_field1.as_slice(), bin_val1.as_slice()),
        (bin_field2.as_slice(), bin_val2.as_slice())
      ]
    )?,
    2
  );
  assert_eq!(db.hlen(bin_key)?, 2);
  assert_eq!(db.hget(bin_key, bin_field1)?, Some(bin_val1.to_vec()));
  assert_eq!(db.hget(bin_key, bin_field2)?, Some(bin_val2.to_vec()));

  // 二进制安全过期
  let exp_res = db.hexpire(bin_key, &[bin_field1.as_slice()], 60, [])?;
  assert_eq!(exp_res, vec![HASH_EXPIRE_SET_OK]);
  let ttl_res = db.httl(bin_key, &[bin_field1.as_slice(), bin_field2.as_slice()])?;
  assert!(ttl_res[0] > 0);
  assert_eq!(ttl_res[1], HASH_FIELD_PERSISTENT);

  // 二进制安全删除
  assert_eq!(db.hdel(bin_key, &[bin_field1.as_slice()])?, 1);
  assert_eq!(db.hlen(bin_key)?, 1);

  Ok(())
}

#[test]
fn test_hash_wrongtype_handling() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 先将键设置为 String 类型
  db.set("str_conflict_key", "string_value", [])?;

  // 2. Hash 命令操作此 String 键，必须返回 WRONGTYPE 错误
  let err_hset = db.hset("str_conflict_key", &[("f1", "v1")]).unwrap_err();
  assert!(err_hset.to_string().contains(ERR_WRONG_TYPE));

  let err_hget = db.hget("str_conflict_key", "f1").unwrap_err();
  assert!(err_hget.to_string().contains(ERR_WRONG_TYPE));

  let err_hlen = db.hlen("str_conflict_key").unwrap_err();
  assert!(err_hlen.to_string().contains(ERR_WRONG_TYPE));

  let err_hexists = db.hexists("str_conflict_key", "f1").unwrap_err();
  assert!(err_hexists.to_string().contains(ERR_WRONG_TYPE));

  let err_hdel = db.hdel("str_conflict_key", &["f1"]).unwrap_err();
  assert!(err_hdel.to_string().contains(ERR_WRONG_TYPE));

  let err_hincrby = db.hincrby("str_conflict_key", "f1", 1).unwrap_err();
  assert!(err_hincrby.to_string().contains(ERR_WRONG_TYPE));

  let err_hincrbyfloat = db.hincrbyfloat("str_conflict_key", "f1", 1.0).unwrap_err();
  assert!(err_hincrbyfloat.to_string().contains(ERR_WRONG_TYPE));

  let err_hkeys = db.hkeys("str_conflict_key").unwrap_err();
  assert!(err_hkeys.to_string().contains(ERR_WRONG_TYPE));

  let err_hvals = db.hvals("str_conflict_key").unwrap_err();
  assert!(err_hvals.to_string().contains(ERR_WRONG_TYPE));

  let err_hgetall = db.hgetall("str_conflict_key").unwrap_err();
  assert!(err_hgetall.to_string().contains(ERR_WRONG_TYPE));

  let err_hstrlen = db.hstrlen("str_conflict_key", "f1").unwrap_err();
  assert!(err_hstrlen.to_string().contains(ERR_WRONG_TYPE));

  let err_hexpire = db.hexpire("str_conflict_key", &["f1"], 10, []).unwrap_err();
  assert!(err_hexpire.to_string().contains(ERR_WRONG_TYPE));

  let err_httl = db.httl("str_conflict_key", &["f1"]).unwrap_err();
  assert!(err_httl.to_string().contains(ERR_WRONG_TYPE));

  let err_hpersist = db.hpersist("str_conflict_key", &["f1"]).unwrap_err();
  assert!(err_hpersist.to_string().contains(ERR_WRONG_TYPE));

  let err_hrange = db
    .hrangebylex("str_conflict_key", RangeLex::default())
    .unwrap_err();
  assert!(err_hrange.to_string().contains(ERR_WRONG_TYPE));

  let err_hscan = db.hscan("str_conflict_key", 0, 10, None).unwrap_err();
  assert!(err_hscan.to_string().contains(ERR_WRONG_TYPE));

  let err_hiter = db.hiter("str_conflict_key", |_, _| true).unwrap_err();
  assert!(err_hiter.to_string().contains(ERR_WRONG_TYPE));

  Ok(())
}

#[test]
fn test_hash_iter() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 未初始化的 Key 遍历返回 Ok(())
  let mut count = 0;
  db.hiter("nonexistent_hash", |_, _| {
    count += 1;
    true
  })?;
  assert_eq!(count, 0);

  // 2. 正常遍历
  let pairs = [
    ("field1", "val1"),
    ("field2", "val2"),
    ("field3", "val3"),
    ("field4", "val4"),
  ];
  db.hset("h_iter_test", &pairs)?;

  let mut collected = Vec::new();
  db.hiter("h_iter_test", |f, v| {
    collected.push((f.to_vec(), v.to_vec()));
    true
  })?;
  assert_eq!(collected.len(), 4);
  assert_eq!(collected[0], (b"field1".to_vec(), b"val1".to_vec()));
  assert_eq!(collected[1], (b"field2".to_vec(), b"val2".to_vec()));
  assert_eq!(collected[2], (b"field3".to_vec(), b"val3".to_vec()));
  assert_eq!(collected[3], (b"field4".to_vec(), b"val4".to_vec()));

  // 3. 提前终止遍历（返回 false）
  let mut early_stopped = Vec::new();
  db.hiter("h_iter_test", |f, v| {
    early_stopped.push((f.to_vec(), v.to_vec()));
    early_stopped.len() < 2
  })?;
  assert_eq!(early_stopped.len(), 2);
  assert_eq!(early_stopped[0].0, b"field1");
  assert_eq!(early_stopped[1].0, b"field2");

  // 4. 字段过期自动过滤
  db.hset("h_iter_expire", &[("f1", "v1"), ("f2", "v2")])?;
  db.hexpire("h_iter_expire", &["f1"], 1, [])?;
  sleep(Duration::from_millis(1100));

  let mut valid_fields = Vec::new();
  db.hiter("h_iter_expire", |f, v| {
    valid_fields.push((f.to_vec(), v.to_vec()));
    true
  })?;
  assert_eq!(valid_fields.len(), 1);
  assert_eq!(valid_fields[0].0, b"f2");
  assert_eq!(valid_fields[0].1, b"v2");

  // 5. 二进制安全遍历
  let bin_pairs = [
    (vec![0u8, 1, 2], vec![255u8, 254]),
    (vec![3u8, 4, 5], vec![100u8, 200]),
  ];
  db.hset("h_bin_iter", &bin_pairs)?;

  let mut bin_collected = Vec::new();
  db.hiter("h_bin_iter", |f, v| {
    bin_collected.push((f.to_vec(), v.to_vec()));
    true
  })?;
  assert_eq!(bin_collected.len(), 2);
  assert_eq!(bin_collected[0].0, vec![0u8, 1, 2]);
  assert_eq!(bin_collected[0].1, vec![255u8, 254]);
  assert_eq!(bin_collected[1].0, vec![3u8, 4, 5]);
  assert_eq!(bin_collected[1].1, vec![100u8, 200]);

  Ok(())
}

#[test]
fn test_hash_hrandfield_comprehensive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 0. 空键返回空
  assert!(db.hrandfield("non_exist", 5, false)?.is_empty());
  assert!(db.hrandfield("non_exist", 5, true)?.is_empty());

  let fields = [
    ("f0", "v0"),
    ("f1", "v1"),
    ("f2", "v2"),
    ("f3", "v3"),
    ("f4", "v4"),
  ];
  db.hset("rand_hash", &fields)?;

  // 1. count == 0 返回空
  assert!(db.hrandfield("rand_hash", 0, false)?.is_empty());
  assert!(db.hrandfield("rand_hash", 0, true)?.is_empty());

  // 2. 正数 count < len (无重复采样，且只取键零值读取)
  let rf_no_val = db.hrandfield("rand_hash", 3, false)?;
  assert_eq!(rf_no_val.len(), 3);
  for (f, v) in &rf_no_val {
    assert!(v.is_none());
    assert!(fields.iter().any(|(exp_f, _)| exp_f.as_bytes() == f));
  }

  // 3. 正数 count >= len (全量返回)
  let rf_all = db.hrandfield("rand_hash", 10, true)?;
  assert_eq!(rf_all.len(), 5);
  for (f, v) in &rf_all {
    let exp = fields
      .iter()
      .find(|(exp_f, _)| exp_f.as_bytes() == f)
      .unwrap();
    assert_eq!(v.as_deref(), Some(exp.1.as_bytes()));
  }

  // 4. 负数 count (允许重复采样，总数严格等于 abs(count))
  let rf_repeat_no_val = db.hrandfield("rand_hash", -8, false)?;
  assert_eq!(rf_repeat_no_val.len(), 8);
  for (f, v) in &rf_repeat_no_val {
    assert!(v.is_none());
    assert!(fields.iter().any(|(exp_f, _)| exp_f.as_bytes() == f));
  }

  let rf_repeat_with_val = db.hrandfield("rand_hash", -8, true)?;
  assert_eq!(rf_repeat_with_val.len(), 8);
  for (f, v) in &rf_repeat_with_val {
    let exp = fields
      .iter()
      .find(|(exp_f, _)| exp_f.as_bytes() == f)
      .unwrap();
    assert_eq!(v.as_deref(), Some(exp.1.as_bytes()));
  }

  Ok(())
}

#[test]
fn test_hash_kvrocks_rangebylex_and_hscan_by_field() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 HRANGEBYLEX 全场景（对标 Kvrocks HRangeByLex 测试用例）
  let mut fvs = Vec::new();
  for i in 0..4 {
    fvs.push((format!("key{i}"), format!("value{i}")));
  }
  for i in 0..26 {
    let ch = (b'a' + i) as char;
    fvs.push((ch.to_string(), ch.to_string()));
  }
  let fv_refs: Vec<(&str, &str)> = fvs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
  db.hset("lex_hash", &fv_refs)?;

  // 1.1 全量覆盖 [key0, key3]
  let res_all = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      ..Default::default()
    },
  )?;
  assert_eq!(res_all.len(), 4);
  assert_eq!(res_all[0].0, b"key0");
  assert_eq!(res_all[1].0, b"key1");
  assert_eq!(res_all[2].0, b"key2");
  assert_eq!(res_all[3].0, b"key3");

  // 1.2 offset = 1, count = None (全部剩余)
  let res_offset1 = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      offset: 1,
      ..Default::default()
    },
  )?;
  assert_eq!(res_offset1.len(), 3);
  assert_eq!(res_offset1[0].0, b"key1");
  assert_eq!(res_offset1[1].0, b"key2");
  assert_eq!(res_offset1[2].0, b"key3");

  // 1.3 offset = 1, count = Some(1)
  let res_cnt1 = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      offset: 1,
      count: Some(1),
      ..Default::default()
    },
  )?;
  assert_eq!(res_cnt1.len(), 1);
  assert_eq!(res_cnt1[0].0, b"key1");

  // 1.4 count = Some(0) 或 超大 offset
  let res_zero = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      count: Some(0),
      ..Default::default()
    },
  )?;
  assert!(res_zero.is_empty());

  let res_big_offset = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      offset: 1000,
      count: Some(1000),
      ..Default::default()
    },
  )?;
  assert!(res_big_offset.is_empty());

  // 1.5 开闭区间 (minex, maxex)
  let res_minex = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      minex: true,
      ..Default::default()
    },
  )?;
  assert_eq!(res_minex.len(), 3);
  assert_eq!(res_minex[0].0, b"key1");

  let res_maxex = db.hrange_by_lex(
    "lex_hash",
    RangeLex {
      min: b"key0".to_vec(),
      max: b"key3".to_vec(),
      maxex: true,
      ..Default::default()
    },
  )?;
  assert_eq!(res_maxex.len(), 3);
  assert_eq!(res_maxex[2].0, b"key2");

  // 1.6 不存在的键返回空
  let res_nonexist = db.hrange_by_lex("nonexist_lex", RangeLex::default())?;
  assert!(res_nonexist.is_empty());

  // 2. 测试 HSCAN_BY_FIELD（对标 Kvrocks Scan 游标直接 Seek）
  let (cursor1, batch1) = db.hscan_by_field("lex_hash", "", 10, None)?;
  assert_eq!(batch1.len(), 10);
  assert!(cursor1.is_some());

  let cursor1_str = cursor1.unwrap();
  let (cursor2, batch2) = db.hscan_by_field("lex_hash", &cursor1_str, 10, None)?;
  assert_eq!(batch2.len(), 10);
  assert_ne!(batch1[0].0, batch2[0].0);
  assert!(cursor2.is_some());

  let cursor2_str = cursor2.unwrap();
  let (cursor3, batch3) = db.hscan_by_field("lex_hash", &cursor2_str, 20, None)?;
  assert_eq!(batch3.len(), 10); // 剩余 10 个全部扫完
  assert!(cursor3.is_none()); // 遍历结束返回 None

  // 2.4 limit == 0 边界拦截
  let (cursor_zero, batch_zero) = db.hscan_by_field("lex_hash", "", 0, None)?;
  assert!(batch_zero.is_empty());
  assert!(cursor_zero.is_none());

  // 3. 过期字段被 HSETNX 覆盖后的 hlen 准确性验证（防止 size 漂移）
  db.hset("exp_drift", &[("f_ttl", "v1")])?;
  db.hpexpire("exp_drift", &["f_ttl"], 1, [])?;
  sleep(Duration::from_millis(15));
  assert_eq!(db.hlen("exp_drift")?, 0);

  // 用 hsetnx 覆盖已过期的物理字段，size 不应重复累加
  assert!(db.hsetnx("exp_drift", "f_ttl", "v2")?);
  assert_eq!(db.hlen("exp_drift")?, 1);
  assert_eq!(db.hget("exp_drift", "f_ttl")?, Some(b"v2".to_vec()));

  // 4. HDEL 删除物理过期字段正常提交与清理
  db.hpexpire("exp_drift", &["f_ttl"], 1, [])?;
  sleep(Duration::from_millis(15));
  assert_eq!(db.hdel_one("exp_drift", "f_ttl")?, 0); // 对客户端返回 0
  assert_eq!(db.hlen("exp_drift")?, 0); // 元数据被正确清理

  Ok(())
}

#[test]
fn test_hash_hmset_hrandfield_one_and_multi_field_optimizations() -> Void {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // 1. HRANDFIELD_ONE on non-existent key
  assert_eq!(db.hrandfield_one("non_existent")?, None);

  // 2. HMSET (alias for HSET, aligned with Redis / Kvrocks)
  let count = db.hmset("hm_key", &[("f1", "v1"), ("f2", "v2"), ("f3", "v3")])?;
  assert_eq!(count, 3);
  assert_eq!(db.hlen("hm_key")?, 3);

  let mvals = db.hmget("hm_key", &["f1", "f2", "f3", "f4"])?;
  assert_eq!(
    mvals,
    vec![
      Some(b"v1".to_vec()),
      Some(b"v2".to_vec()),
      Some(b"v3".to_vec()),
      None,
    ]
  );

  // 3. HRANDFIELD_ONE on populated key
  let rand_f = db.hrandfield_one("hm_key")?.expect("should find field");
  assert!(
    rand_f == b"f1" || rand_f == b"f2" || rand_f == b"f3",
    "sampled field must be in hash"
  );

  // 4. HSET multi-field with duplicates within batch and existing keys (zero-allocation path)
  // f1 is updated (not new), f4 is new with duplicate in same batch -> net new fields = 1
  let updated_new = db.hset(
    "hm_key",
    &[("f1", "v1_updated"), ("f4", "v4_draft"), ("f4", "v4_final")],
  )?;
  assert_eq!(updated_new, 1);
  assert_eq!(db.hlen("hm_key")?, 4);
  assert_eq!(db.hget("hm_key", "f1")?, Some(b"v1_updated".to_vec()));
  assert_eq!(db.hget("hm_key", "f4")?, Some(b"v4_final".to_vec()));

  // 5. Stack-allocated SmallKey hiter and hrange_by_lex verification
  let mut keys_found = Vec::new();
  db.hiter("hm_key", |f, _| {
    keys_found.push(f.to_vec());
    true
  })?;
  assert_eq!(keys_found.len(), 4);

  Ok(())
}
