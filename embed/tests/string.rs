use std::{thread, time::Duration};

use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  string::{
    DelEx, GetEx, Lcs, Set, StringLCSMatchedRange, StringLCSResult, StringMSet, StringSet,
    StringSetType, compute_lcs, normalize_range, string_digest, string_digest_bytes,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_string_get_and_set() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let pairs = [
    ("test-string-key1", "test-strings-value1"),
    ("test-string-key2", "test-strings-value2"),
    ("test-string-key3", "test-strings-value3"),
    ("test-string-key4", "test-strings-value4"),
    ("test-string-key5", "test-strings-value5"),
    ("test-string-key6", "test-strings-value6"),
  ];

  for (k, v) in pairs {
    db.set(k, v, [])?;
  }
  for (k, v) in pairs {
    let val = db.get(k)?;
    assert_eq!(val, Some(v.as_bytes().to_vec()));
  }
  for (k, _) in pairs {
    assert_eq!(db.del(&[k])?, 1);
    assert_eq!(db.get(k)?, None);
  }

  Ok(())
}

#[test]
fn test_string_set_options() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // NX 选项
  let res = db.set("k1", "v1", [Set::Nx])?;
  assert_eq!(res, Some(Vec::new()));
  assert_eq!(db.get("k1")?, Some(b"v1".to_vec()));

  let res_nx_fail = db.set("k1", "v2", [Set::Nx])?;
  assert_eq!(res_nx_fail, None);
  assert_eq!(db.get("k1")?, Some(b"v1".to_vec()));

  // XX 选项
  let res_xx_fail = db.set("k_nonexist", "v", [Set::Xx])?;
  assert_eq!(res_xx_fail, None);
  assert_eq!(db.get("k_nonexist")?, None);

  let res_xx_ok = db.set("k1", "v_updated", [Set::Xx])?;
  assert_eq!(res_xx_ok, Some(Vec::new()));
  assert_eq!(db.get("k1")?, Some(b"v_updated".to_vec()));

  // GET 选项
  let old = db.set("k1", "v_new", [Set::Get])?;
  assert_eq!(old, Some(b"v_updated".to_vec()));
  assert_eq!(db.get("k1")?, Some(b"v_new".to_vec()));

  // IfEq 选项
  let if_eq_fail = db.set("k1", "v_fail", [Set::IfEq(b"wrong")])?;
  assert_eq!(if_eq_fail, None);
  assert_eq!(db.get("k1")?, Some(b"v_new".to_vec()));

  let if_eq_ok = db.set("k1", "v_pass", [Set::IfEq(b"v_new")])?;
  assert_eq!(if_eq_ok, Some(Vec::new()));
  assert_eq!(db.get("k1")?, Some(b"v_pass".to_vec()));

  // IfNe 选项
  let if_ne_fail = db.set("k1", "v_ne_fail", [Set::IfNe(b"v_pass")])?;
  assert_eq!(if_ne_fail, None);

  let if_ne_ok = db.set("k1", "v_ne_ok", [Set::IfNe(b"other")])?;
  assert_eq!(if_ne_ok, Some(Vec::new()));
  assert_eq!(db.get("k1")?, Some(b"v_ne_ok".to_vec()));

  Ok(())
}

#[test]
fn test_string_append_and_strlen() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 0..32 {
    let ret = db.append("test_append_k", "a")?;
    assert_eq!(ret, i + 1);
  }
  assert_eq!(db.strlen("test_append_k")?, 32);
  assert_eq!(db.del(&["test_append_k"])?, 1);
  assert_eq!(db.strlen("test_append_k")?, 0);

  Ok(())
}

#[test]
fn test_string_mget_and_mset() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let pairs = [
    ("m_k1", "m_v1"),
    ("m_k2", "m_v2"),
    ("m_k3", "m_v3"),
    ("m_k4", "m_v4"),
  ];

  db.mset(&pairs)?;

  let keys: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
  let values = db.mget(&keys)?;
  for (i, (_, v)) in pairs.iter().enumerate() {
    assert_eq!(values[i], Some(v.as_bytes().to_vec()));
  }

  let mixed = db.mget(&["m_k1", "nonexistent_k", "m_k3"])?;
  assert_eq!(
    mixed,
    vec![Some(b"m_v1".to_vec()), None, Some(b"m_v3".to_vec())]
  );

  Ok(())
}

#[test]
fn test_string_incr_and_decr() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert_eq!(db.incr("counter")?, 1);
  assert_eq!(db.incr("counter")?, 2);
  assert_eq!(db.incrby("counter", 10)?, 12);
  assert_eq!(db.decr("counter")?, 11);
  assert_eq!(db.decrby("counter", 5)?, 6);
  assert_eq!(db.incrby("counter", -6)?, 0);

  // 溢出与边界条件测试（对标 Kvrocks IncrBy）
  assert_eq!(db.incrby("counter", i64::MAX)?, i64::MAX);
  assert!(db.incrby("counter", 1).is_err());
  assert_eq!(db.incrby("counter", i64::MIN + 1)?, 0);
  assert_eq!(db.incrby("counter", i64::MIN)?, i64::MIN);
  assert!(db.incrby("counter", -1).is_err());

  // 非数值测试
  db.set("str_k", "abc", [])?;
  assert!(db.incr("str_k").is_err());
  assert!(db.incrby("str_k", 5).is_err());
  assert!(db.decr("str_k").is_err());

  // 前导空白符测试
  db.set("ws_k", " 123", [])?;
  assert!(db.incr("ws_k").is_err());
  db.set("ws_k2", "123 ", [])?;
  assert!(db.incr("ws_k2").is_err());

  Ok(())
}

#[test]
fn test_string_incrbyfloat_stored_format() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 紧凑格式，无末尾多余零
  let f1 = db.incrbyfloat("float_k", 10.5)?;
  assert!((f1 - 10.5).abs() < 1e-9);
  assert_eq!(db.get("float_k")?, Some(b"10.5".to_vec()));

  db.del(&["float_k"])?;

  // 整数形式的浮点不带小数点
  let f2 = db.incrbyfloat("float_k", 3.0)?;
  assert!((f2 - 3.0).abs() < 1e-9);
  assert_eq!(db.get("float_k")?, Some(b"3".to_vec()));

  // 累加
  let f3 = db.incrbyfloat("float_k", 1.5)?;
  assert!((f3 - 4.5).abs() < 1e-9);
  assert_eq!(db.get("float_k")?, Some(b"4.5".to_vec()));

  // 负数与零
  let f4 = db.incrbyfloat("float_k", -4.5)?;
  assert!((f4 - 0.0).abs() < 1e-9);
  assert_eq!(db.get("float_k")?, Some(b"0".to_vec()));

  // NaN / Inf 校验
  assert!(db.incrbyfloat("float_k", f64::NAN).is_err());
  assert!(db.incrbyfloat("float_k", f64::INFINITY).is_err());

  Ok(())
}

#[test]
fn test_string_getrange_and_setrange() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.set("range_k", "Hello World", [])?;

  assert_eq!(db.getrange("range_k", (0, 4))?, b"Hello");
  assert_eq!(db.getrange("range_k", (-5, -1))?, b"World");
  assert_eq!(db.getrange("range_k", (0, -1))?, b"Hello World");
  assert_eq!(db.getrange("range_k", (100, 200))?, b"");
  assert_eq!(db.getrange("range_k", (-100, 2))?, b"Hel");
  // 边界条件：负数区间超出字符串头部（如 -20 到 -15），返回空字节
  assert_eq!(db.getrange("range_k", (-20, -15))?, b"");
  // 边界条件：start > end，返回空字节
  assert_eq!(db.getrange("range_k", (5, 2))?, b"");

  let new_len = db.setrange("range_k", 6, "Redis")?;
  assert_eq!(new_len, 11);
  assert_eq!(db.get("range_k")?, Some(b"Hello Redis".to_vec()));

  let ext_len = db.setrange("range_k", 12, "Extension")?;
  assert_eq!(ext_len, 21);
  assert_eq!(db.strlen("range_k")?, 21);

  // 不存在 key 时 setrange 空值应直接返回 0 且不写入
  let zero_len = db.setrange("nonexist_range", 0, "")?;
  assert_eq!(zero_len, 0);
  assert_eq!(db.get("nonexist_range")?, None);

  // setrange 偏移量超过 512MB 应当报错
  assert!(
    db.setrange("range_k", 600 * 1024 * 1024, "overflow")
      .is_err()
  );

  Ok(())
}

#[test]
fn test_string_getdel() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.set("gd_k", "gd_v", [])?;
  assert_eq!(db.getdel("gd_k")?, Some(b"gd_v".to_vec()));
  assert_eq!(db.get("gd_k")?, None);
  assert_eq!(db.getdel("gd_k")?, None);

  Ok(())
}

#[test]
fn test_string_lcs_comprehensive() {
  let (s, e) = normalize_range(0, 5, 10);
  assert_eq!((s, e), (0, 5));
  let (s, e) = normalize_range(-5, -1, 10);
  assert_eq!((s, e), (5, 9));
  let (s, e) = normalize_range(0, -1, 0);
  assert_eq!((s, e), (0, -1));

  // Kvrocks 对标 LCS 测试集：abcdef vs acdf
  let res_str = compute_lcs(b"abcdef", b"acdf", []).expect("lcs str failed");
  assert_eq!(res_str, StringLCSResult::Str("acdf".to_string()));

  let res_len = compute_lcs(b"abcdef", b"acdf", [Lcs::Len]).expect("lcs len failed");
  assert_eq!(res_len, StringLCSResult::Len(4));

  let res_idx = compute_lcs(b"abcdef", b"acdf", [Lcs::Idx]).expect("lcs idx failed");
  if let StringLCSResult::Idx(idx_res) = res_idx {
    assert_eq!(idx_res.len, 4);
    assert_eq!(idx_res.matches.len(), 3);
    assert_eq!(
      idx_res.matches[0],
      StringLCSMatchedRange::new(5, 5, 3, 3, 1)
    );
    assert_eq!(
      idx_res.matches[1],
      StringLCSMatchedRange::new(2, 3, 1, 2, 2)
    );
    assert_eq!(
      idx_res.matches[2],
      StringLCSMatchedRange::new(0, 0, 0, 0, 1)
    );
  } else {
    panic!("expected Idx result");
  }

  // MinMatchLen 过滤
  let res_idx_min =
    compute_lcs(b"abcdef", b"acdf", [Lcs::Idx, Lcs::MinMatchLen(2)]).expect("lcs idx min failed");
  if let StringLCSResult::Idx(idx_res) = res_idx_min {
    assert_eq!(idx_res.len, 4);
    assert_eq!(idx_res.matches.len(), 1);
    assert_eq!(
      idx_res.matches[0],
      StringLCSMatchedRange::new(2, 3, 1, 2, 2)
    );
  } else {
    panic!("expected Idx result with min_match_len");
  }
}

#[test]
fn test_string_advanced_kvrocks_features() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // GETEX
  db.set("gex", "val", [])?;
  assert_eq!(db.getex("gex", [GetEx::Ex(10)])?, Some(b"val".to_vec()));
  let (_, exp) = db.get_with_expire("gex")?;
  assert!(exp > 0);

  assert_eq!(db.getex("gex", [GetEx::Persist])?, Some(b"val".to_vec()));
  let (_, exp_p) = db.get_with_expire("gex")?;
  assert_eq!(exp_p, 0);

  // CAS & CAD
  db.set("cas_k", "v1", [])?;
  assert_eq!(db.cas("cas_k", "v_wrong", "v2", 0)?, 0);
  assert_eq!(db.cas("cas_k", "v1", "v2", 0)?, 1);
  assert_eq!(db.get("cas_k")?, Some(b"v2".to_vec()));
  assert_eq!(db.cas("cas_missing", "v1", "v2", 0)?, -1);

  assert_eq!(db.cad("cas_k", "v_wrong")?, 0);
  assert_eq!(db.cad("cas_k", "v2")?, 1);
  assert_eq!(db.get("cas_k")?, None);
  assert_eq!(db.cad("cas_missing", "v1")?, -1);

  // DELEX
  db.set("del_k", "secret", [])?;
  let dig = db.digest("del_k")?.expect("digest failed");
  assert!(!db.delex("del_k", [DelEx::IfEq(b"wrong")])?);
  assert!(db.delex("del_k", [DelEx::IfDeq(dig.as_bytes())])?);
  assert_eq!(db.get("del_k")?, None);

  // SET IFDEQ / IFDNE
  let d_hello = string_digest(b"hello");
  let set_ifdeq_fail = db.set_with(
    "ifdeq_k",
    "new",
    &StringSet {
      expire: 0,
      set_type: StringSetType::IfDeq,
      get: false,
      keep_ttl: false,
      cmp_value: Some(d_hello.as_bytes()),
    },
  )?;
  assert_eq!(set_ifdeq_fail, None);
  assert_eq!(db.get("ifdeq_k")?, None);

  db.set("ifdeq_k", "hello", [])?;
  let set_ifdeq_ok = db.set_with(
    "ifdeq_k",
    "new",
    &StringSet {
      expire: 0,
      set_type: StringSetType::IfDeq,
      get: false,
      keep_ttl: false,
      cmp_value: Some(d_hello.as_bytes()),
    },
  )?;
  assert_eq!(set_ifdeq_ok, Some(Vec::new()));
  assert_eq!(db.get("ifdeq_k")?, Some(b"new".to_vec()));

  // MSETNX / MSETEX
  let pairs = [("k_m1", "v_m1"), ("k_m2", "v_m2")];
  assert!(db.msetnx(&pairs)?);
  assert_eq!(db.get("k_m1")?, Some(b"v_m1".to_vec()));

  // MSETNX 存在已有 key 应返回 false 并不执行写入
  let pairs_conflict = [("k_m1", "v_m1_new"), ("k_m3", "v_m3")];
  assert!(!db.msetnx(&pairs_conflict)?);
  assert_eq!(db.get("k_m3")?, None);

  // DELEX 各分支全覆盖
  db.set("del_opt_k", "abc", [])?;
  assert!(!db.delex("del_opt_k", [DelEx::IfEq(b"def")])?);
  assert!(!db.delex("del_opt_k", [DelEx::IfNe(b"abc")])?);
  assert!(db.delex("del_opt_k", [DelEx::IfNe(b"xyz")])?);
  assert_eq!(db.get("del_opt_k")?, None);

  // MSET_ARGS with keep_ttl
  let now_ms = coarsetime::Clock::now_since_epoch().as_millis();
  db.setex("keep_ttl_1", "v1", now_ms + 60_000)?;
  db.setex("keep_ttl_2", "v2", now_ms + 120_000)?;
  let (_, exp1_before) = db.get_with_expire("keep_ttl_1")?;
  let (_, exp2_before) = db.get_with_expire("keep_ttl_2")?;
  assert!(exp1_before > 0 && exp2_before > 0);

  db.mset_with(
    &[("keep_ttl_1", "v1_updated"), ("keep_ttl_2", "v2_updated")],
    StringMSet {
      expire: 0,
      set_type: StringSetType::None,
      keep_ttl: true,
    },
  )?;
  let (v1_new, exp1_after) = db.get_with_expire("keep_ttl_1")?;
  let (v2_new, exp2_after) = db.get_with_expire("keep_ttl_2")?;
  assert_eq!(v1_new, Some(b"v1_updated".to_vec()));
  assert_eq!(v2_new, Some(b"v2_updated".to_vec()));
  assert_eq!(exp1_after, exp1_before);
  assert_eq!(exp2_after, exp2_before);

  // 二进制安全性测试
  let bin_key = b"\x00\x01\xfe\xff_bin_key";
  let bin_val = b"\xde\xad\xbe\xef";
  db.set(bin_key, bin_val, [])?;
  assert_eq!(db.get(bin_key)?, Some(bin_val.to_vec()));
  assert_eq!(db.del(&[bin_key])?, 1);
  assert_eq!(db.get(bin_key)?, None);

  // INCRBY_EX / INCRBYFLOAT_EX 测试
  db.setex("incr_ex_key", "100", now_ms + 100_000)?;
  let val_after_incr = db.incrby_ex("incr_ex_key", 25, 0, true)?;
  assert_eq!(val_after_incr, 125);
  let (_, ttl_preserved) = db.get_with_expire("incr_ex_key")?;
  assert!(ttl_preserved > 0);

  let val_with_new_ttl = db.incrby_ex("incr_ex_key", 25, now_ms + 50_000, false)?;
  assert_eq!(val_with_new_ttl, 150);
  let (_, ttl_updated) = db.get_with_expire("incr_ex_key")?;
  assert_eq!(ttl_updated, now_ms + 50_000);

  let f_after_incr = db.incrbyfloat_ex("float_ex_key", 3.5, now_ms + 80_000, false)?;
  assert!((f_after_incr - 3.5).abs() < 1e-9);
  let (f_val, f_exp) = db.get_with_expire("float_ex_key")?;
  assert_eq!(f_val, Some(b"3.5".to_vec()));
  assert_eq!(f_exp, now_ms + 80_000);

  // 校验 IFDEQ / IFDNE 长度不合法错误
  let invalid_digest_err = db.set_with(
    "ifdeq_k",
    "val",
    &StringSet {
      expire: 0,
      set_type: StringSetType::IfDeq,
      get: false,
      keep_ttl: false,
      cmp_value: Some(b"invalid_len"),
    },
  );
  assert!(invalid_digest_err.is_err());

  let invalid_delex_err = db.delex("ifdeq_k", [DelEx::IfDeq(b"short")]);
  assert!(invalid_delex_err.is_err());

  Ok(())
}

#[test]
fn test_string_mget_mixed_types_and_wrongtype_handling() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 设置普通 String
  db.set("str_1", "val_1", [])?;

  // 2. 设置 Hash 复合类型
  db.hset("hash_1", &[("field_1", "hval_1")])?;

  // 3. 设置 List 复合类型
  db.lpush("list_1", &["item_1"])?;

  // 4. 对单个 key 进行 GET 测试：复合类型返回 WRONGTYPE
  assert_eq!(db.get("str_1")?, Some(b"val_1".to_vec()));
  assert!(db.get("hash_1").is_err());
  assert!(db.get("list_1").is_err());

  // 5. 对混合 key 执行 MGET：对标 Redis / Kvrocks 规范，非 String 类型或不存在的 key 均返回 None (nil)，不中断整体
  let mget_res = db.mget(&["str_1", "hash_1", "nonexistent_k", "list_1"])?;
  assert_eq!(mget_res, vec![Some(b"val_1".to_vec()), None, None, None]);

  // 6. MSETNX 遇到复合类型键应判定 key 已存在并返回 false
  let msetnx_res = db.msetnx(&[("hash_1", "new_val"), ("str_new", "val_new")])?;
  assert!(!msetnx_res);
  assert_eq!(db.get("str_new")?, None);

  // 7. MSET 直接覆盖复合类型键
  db.mset(&[("hash_1", "overwritten_as_str")])?;
  assert_eq!(db.get("hash_1")?, Some(b"overwritten_as_str".to_vec()));

  Ok(())
}

#[test]
fn test_string_edge_cases_and_conformance() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. GETSET
  let gs_none = db.getset("gs_k", "val1")?;
  assert_eq!(gs_none, None);
  assert_eq!(db.get("gs_k")?, Some(b"val1".to_vec()));
  let gs_old = db.getset("gs_k", "val2")?;
  assert_eq!(gs_old, Some(b"val1".to_vec()));
  assert_eq!(db.get("gs_k")?, Some(b"val2".to_vec()));

  // 2. SET option precedence: EX then KEEPTTL vs KEEPTTL then EX
  let now_ms = coarsetime::Clock::now_since_epoch().as_millis();
  db.set("opt_prec_1", "v1", [Set::Ex(60), Set::KeepTtl])?;
  let (_, exp_ttl) = db.get_with_expire("opt_prec_1")?;
  assert_eq!(exp_ttl, 0);

  db.set("opt_prec_2", "v2", [Set::KeepTtl, Set::Ex(60)])?;
  let (_, exp_ex) = db.get_with_expire("opt_prec_2")?;
  assert!(exp_ex >= now_ms + 59_000);

  // 3. SET PX / EXAT / PXAT
  db.set("opt_px", "v_px", [Set::Px(50_000)])?;
  let (_, exp_px) = db.get_with_expire("opt_px")?;
  assert!(exp_px >= now_ms + 49_000);

  db.set("opt_exat", "v_exat", [Set::ExAt(2_000_000_000)])?;
  let (_, exp_exat) = db.get_with_expire("opt_exat")?;
  assert_eq!(exp_exat, 2_000_000_000 * 1000);

  db.set("opt_pxat", "v_pxat", [Set::PxAt(2_000_000_000_123)])?;
  let (_, exp_pxat) = db.get_with_expire("opt_pxat")?;
  assert_eq!(exp_pxat, 2_000_000_000_123);

  // 4. LCS equal string fast-path
  let lcs_eq = compute_lcs(b"helloworld", b"helloworld", [])?;
  assert_eq!(lcs_eq, StringLCSResult::Str("helloworld".to_string()));

  let lcs_eq_len = compute_lcs(b"helloworld", b"helloworld", [Lcs::Len])?;
  assert_eq!(lcs_eq_len, StringLCSResult::Len(10));

  let lcs_empty = compute_lcs(b"", b"abc", [Lcs::Len])?;
  assert_eq!(lcs_empty, StringLCSResult::Len(0));

  // 5. LCS on WeDb keys
  db.set("lcs_k1", "ohmygod", [])?;
  db.set("lcs_k2", "ohgod", [])?;
  let lcs_res = db.lcs("lcs_k1", "lcs_k2", [])?;
  assert_eq!(lcs_res, StringLCSResult::Str("ohgod".to_string()));

  // 6. WRONGTYPE checks on various commands
  db.hset("wrong_type_hash", &[("f", "v")])?;
  assert!(db.append("wrong_type_hash", "suffix").is_err());
  assert!(db.setrange("wrong_type_hash", 0, "val").is_err());
  assert!(db.incr("wrong_type_hash").is_err());
  assert!(db.incrbyfloat("wrong_type_hash", 1.0).is_err());
  assert!(db.cas("wrong_type_hash", "a", "b", 0).is_err());
  assert!(db.cad("wrong_type_hash", "a").is_err());
  assert!(db.lcs("wrong_type_hash", "lcs_k1", []).is_err());

  Ok(())
}

#[test]
fn test_string_digest_zero_allocation() -> Void {
  let data = b"hello rapidhash zero copy";
  let dig_str = string_digest(data);
  let dig_bytes = string_digest_bytes(data);

  assert_eq!(dig_str.as_bytes(), &dig_bytes);
  assert_eq!(dig_bytes.len(), 16);

  Ok(())
}

#[test]
fn test_set_fast_path_and_concurrency() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 基础极速直写通道功能与不同大小 Value 测试
  let sizes = [0, 1, 16, 128, 4096, 65536];
  for &sz in &sizes {
    let val = vec![b'x'; sz];
    let key = format!("fp_key_{sz}");
    db.set(&key, &val, [])?;
    assert_eq!(db.get(&key)?, Some(val));
  }

  // 2. 多线程高并发极速直写与覆盖写入测试
  let mut handles = Vec::new();
  let thread_count = 8;
  let ops_per_thread = 500;

  for t in 0..thread_count {
    let db_cloned = db.clone();
    handles.push(thread::spawn(move || -> Result<(), wedb_embed::Error> {
      for i in 0..ops_per_thread {
        let idx = i % 50;
        let key = format!("concurrent_k_{idx}");
        let val = format!("val_t_{t}_i_{i}");
        // 极速直写
        db_cloned.set(key.as_bytes(), val.as_bytes(), [])?;
        // 立即读取验证
        let read = db_cloned.get(key.as_bytes())?;
        assert!(read.is_some());
      }
      Ok(())
    }));
  }

  for h in handles {
    h.join().unwrap()?;
  }

  // 验证所有并发 key 均存在且有效
  for i in 0..50 {
    let key = format!("concurrent_k_{i}");
    assert!(db.get(key.as_bytes())?.is_some());
  }

  Ok(())
}

#[test]
fn test_set_composite_type_overwrite() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 创建复合数据结构：Hash、List、Set、ZSet
  db.hset("conflict_hash", &[("f1", "v1"), ("f2", "v2")])?;
  db.lpush("conflict_list", &["item1", "item2"])?;
  db.sadd("conflict_set", &["m1", "m2"])?;
  db.zadd("conflict_zset", &[(10.0, "z1"), (20.0, "z2")], [])?;

  // 验证复合数据结构存在且读取正常
  assert_eq!(db.hget("conflict_hash", "f1")?, Some(b"v1".to_vec()));
  assert_eq!(db.llen("conflict_list")?, 2);
  assert_eq!(db.scard("conflict_set")?, 2);
  assert_eq!(db.zcard("conflict_zset")?, 2);

  // 2. 使用常规 SET 极速直写通道覆盖写入 Hash 键
  db.set("conflict_hash", "overwritten_hash_value", [])?;
  assert_eq!(
    db.get("conflict_hash")?,
    Some(b"overwritten_hash_value".to_vec())
  );
  // 覆盖后对原 Hash 类型的操作应返回 WRONGTYPE
  assert!(db.hget("conflict_hash", "f1").is_err());

  // 3. 使用 SETXX 覆盖写入 List 键（触发显式冲突检测与元数据清理路径）
  let setxx_res = db.setxx("conflict_list", "overwritten_list_value", 0)?;
  assert!(setxx_res);
  assert_eq!(
    db.get("conflict_list")?,
    Some(b"overwritten_list_value".to_vec())
  );
  assert!(db.lpop("conflict_list", 1).is_err());

  // 4. 使用 MSET 覆盖写入 Set 与 ZSet 键
  db.mset(&[
    ("conflict_set", "overwritten_set_value"),
    ("conflict_zset", "overwritten_zset_value"),
  ])?;
  assert_eq!(
    db.get("conflict_set")?,
    Some(b"overwritten_set_value".to_vec())
  );
  assert_eq!(
    db.get("conflict_zset")?,
    Some(b"overwritten_zset_value".to_vec())
  );

  // 5. 验证覆盖写入后的键可正常使用常规字符串命令
  assert_eq!(db.strlen("conflict_hash")?, 22);
  assert_eq!(db.append("conflict_hash", "_suffix")?, 22 + "_suffix".len());
  assert_eq!(
    db.get("conflict_hash")?,
    Some(b"overwritten_hash_value_suffix".to_vec())
  );

  Ok(())
}

#[test]
fn test_mset_fast_path_and_performance() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 当 meta 表为空时的批量极速写入
  let mut pairs = Vec::new();
  for i in 0..100 {
    pairs.push((format!("batch_k_{i}"), format!("batch_v_{i}")));
  }
  let p_refs: Vec<(&[u8], &[u8])> = pairs
    .iter()
    .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
    .collect();

  db.mset(&p_refs)?;

  for (k, v) in &pairs {
    assert_eq!(db.get(k.as_bytes())?, Some(v.as_bytes().to_vec()));
  }

  // 2. MSETNX 行为验证
  let msetnx_false = db.msetnx(&[
    (b"batch_k_0".as_slice(), b"new".as_slice()),
    (b"batch_k_new".as_slice(), b"new".as_slice()),
  ])?;
  assert!(!msetnx_false);
  assert_eq!(db.get("batch_k_new")?, None);

  let msetnx_true = db.msetnx(&[
    (b"brand_new_1".as_slice(), b"v1".as_slice()),
    (b"brand_new_2".as_slice(), b"v2".as_slice()),
  ])?;
  assert!(msetnx_true);
  assert_eq!(db.get("brand_new_1")?, Some(b"v1".to_vec()));
  assert_eq!(db.get("brand_new_2")?, Some(b"v2".to_vec()));

  // 3. MSETEX 行为验证
  let expire_at = wedb_embed::current_now_ms().saturating_add(60_000);
  db.msetex(&[(b"exp_k_1".as_slice(), b"exp_v_1".as_slice())], expire_at)?;
  let (val, exp) = db.get_with_expire("exp_k_1")?;
  assert_eq!(val, Some(b"exp_v_1".to_vec()));
  assert!(exp >= expire_at);

  Ok(())
}

#[test]
fn test_string_kvrocks_msetxx_and_mset_keep_ttl() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. MSETXX: 当全部键都不存在时，操作失败返回 false
  let flag0 = db.msetxx(&[("xx_k1", "v1"), ("xx_k2", "v2")])?;
  assert!(!flag0);
  assert_eq!(db.get("xx_k1")?, None);
  assert_eq!(db.get("xx_k2")?, None);

  // 写入部分键
  db.set_one("xx_k1", "orig_v1")?;
  // 部分键存在、部分键缺失：依然整体失败
  let flag1 = db.msetxx(&[("xx_k1", "new_v1"), ("xx_k2", "new_v2")])?;
  assert!(!flag1);
  assert_eq!(db.get("xx_k1")?, Some(b"orig_v1".to_vec()));
  assert_eq!(db.get("xx_k2")?, None);

  // 全部键均存在：MSETXX 成功
  db.set_one("xx_k2", "orig_v2")?;
  let flag2 = db.msetxx(&[("xx_k1", "new_v1"), ("xx_k2", "new_v2")])?;
  assert!(flag2);
  assert_eq!(db.get("xx_k1")?, Some(b"new_v1".to_vec()));
  assert_eq!(db.get("xx_k2")?, Some(b"new_v2".to_vec()));

  // 2. SETNX_EX 与 SETXX 带绝对时间戳测试（对标 Kvrocks SetNX / SetXX）
  let now = wedb_embed::current_now_ms();
  let expire_10s = now + 10_000;
  let flag_nx = db.setnx_ex("nx_ttl_key", "nx_val", expire_10s)?;
  assert!(flag_nx);
  let (val, exp) = db.get_with_expire("nx_ttl_key")?;
  assert_eq!(val, Some(b"nx_val".to_vec()));
  assert_eq!(exp, expire_10s);

  // 再次对已有键调用 setnx_ex 应失败
  let flag_nx_fail = db.setnx_ex("nx_ttl_key", "other_val", expire_10s)?;
  assert!(!flag_nx_fail);

  // 3. MSET 带 KEEPTTL 继承原 TTL
  let pairs = [("nx_ttl_key", "updated_nx_val")];
  let mset_keep_res = db.mset_with(
    &pairs,
    StringMSet {
      expire: 0,
      set_type: StringSetType::None,
      keep_ttl: true,
    },
  )?;
  assert!(mset_keep_res);
  let (val2, exp2) = db.get_with_expire("nx_ttl_key")?;
  assert_eq!(val2, Some(b"updated_nx_val".to_vec()));
  assert_eq!(exp2, expire_10s);

  // 4. LCS 同串快速通道当 min_match_len 大于串长时返回空匹配
  let lcs_idx = compute_lcs(b"hello", b"hello", [Lcs::Idx, Lcs::MinMatchLen(10)])?;
  if let StringLCSResult::Idx(idx_res) = lcs_idx {
    assert_eq!(idx_res.len, 5);
    assert!(idx_res.matches.is_empty());
  } else {
    panic!("expected Idx result");
  }

  // min_match_len 小于等于串长时返回有效区间
  let lcs_idx2 = compute_lcs(b"hello", b"hello", [Lcs::Idx, Lcs::MinMatchLen(3)])?;
  if let StringLCSResult::Idx(idx_res) = lcs_idx2 {
    assert_eq!(idx_res.len, 5);
    assert_eq!(idx_res.matches.len(), 1);
    assert_eq!(idx_res.matches[0].match_len, 5);
  } else {
    panic!("expected Idx result");
  }

  Ok(())
}

#[test]
fn test_string_kvrocks_extensions_and_empty_value() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 空字符串读写（Kvrocks GetEmptyValue 测试）
  db.set("empty_k", "", [])?;
  assert_eq!(db.get("empty_k")?, Some(Vec::new()));
  assert_eq!(db.strlen("empty_k")?, 0);

  // 2. psetex 与 setex_ttl
  db.psetex("psetex_k", "v_ms", 10_000)?;
  assert_eq!(db.get("psetex_k")?, Some(b"v_ms".to_vec()));
  let pttl = db.pttl("psetex_k")?;
  assert!(pttl > 8000 && pttl <= 10_000);

  db.setex_ttl("setex_sec_k", "v_sec", 100)?;
  assert_eq!(db.get("setex_sec_k")?, Some(b"v_sec".to_vec()));
  let ttl = db.ttl("setex_sec_k")?;
  assert!(ttl > 80 && ttl <= 100);

  // 3. SETRANGE 空串与非空填充
  assert_eq!(db.setrange("nonexist_empty", 10, "")?, 0);
  assert_eq!(db.get("nonexist_empty")?, None);

  assert_eq!(db.setrange("nonexist_pad", 5, "hello")?, 10);
  let padded = db.get("nonexist_pad")?.unwrap();
  assert_eq!(&padded[..5], &[0, 0, 0, 0, 0]);
  assert_eq!(&padded[5..], b"hello");

  // 4. 过期 Hash 覆盖创建字符串自动清理残留元数据
  db.hset("h_exp", &[("f1", "v1")])?;
  db.pexpire("h_exp", 1)?;
  thread::sleep(Duration::from_millis(10));
  assert!(!db.exists_one("h_exp")?);

  // 通过 append 建立字符串并清理旧 Hash 残留
  assert_eq!(db.append("h_exp", "new_str")?, 7);
  assert_eq!(db.get("h_exp")?, Some(b"new_str".to_vec()));

  // 验证后续无法再作为 Hash 读取
  assert_eq!(
    db.hlen("h_exp").unwrap_err().to_string(),
    wedb_embed::ERR_WRONG_TYPE
  );

  // 5. with_get 端到端零拷贝借用读取
  db.set("borrow_key", "borrow_val", [])?;
  let len = db.with_get("borrow_key", |slice| slice.len())?;
  assert_eq!(len, Some(10));
  let nonexist_res = db.with_get("nonexist_borrow", |_| true)?;
  assert_eq!(nonexist_res, None);

  // 6. with_getrange 端到端零拷贝借用读取
  let sub = db.with_getrange("borrow_key", 0..6, |s| s.to_vec())?;
  assert_eq!(sub, Some(b"borrow".to_vec()));
  let empty_sub = db.with_getrange("borrow_key", 100..200, |s| s.len())?;
  assert_eq!(empty_sub, Some(0));
  let nonexist_range = db.with_getrange("nonexist_borrow", 0..5, |_| true)?;
  assert_eq!(nonexist_range, None);

  // 7. 栈缓冲小字符串 append 与 setrange 深度边界验证 (len <= 55)
  db.set("small_k", "hello", [])?;
  assert_eq!(db.append("small_k", " world")?, 11);
  assert_eq!(db.get("small_k")?, Some(b"hello world".to_vec()));
  assert_eq!(db.setrange("small_k", 6, "rust")?, 11);
  assert_eq!(db.get("small_k")?, Some(b"hello rustd".to_vec()));

  // 8. LCS 栈缓冲快速路径 (len <= 64)
  let lcs_short_len = compute_lcs(b"quick_fox", b"quiet_box", [Lcs::Len])?;
  assert_eq!(lcs_short_len, StringLCSResult::Len(6)); // "qui_ox" (len 6)

  Ok(())
}
