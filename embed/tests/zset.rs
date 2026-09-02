use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, KeyComposer, Partition, WeDb,
  api::zset::{compose_zset_key, compose_zset_meta_key, compose_zset_score_key},
  zset::{
    Aggregate, RangeLex, RangeScore, ZAdd, ZRange, ZSetMeta, decode_sortable_f64,
    encode_sortable_f64,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_zset_metadata_and_score_codec() -> Void {
  let meta = ZSetMeta::new(1700000000000, 202, 88);

  // 标准 26 字节
  let enc = meta.encode();
  assert_eq!(enc.len(), ZSetMeta::ENCODED_SIZE);
  let dec = ZSetMeta::decode(&enc).expect("decode failed");
  assert_eq!(dec.base.size, 88);

  // Kvrocks 紧凑 25 字节
  let kv_enc = meta.encode_kvrocks();
  assert_eq!(kv_enc.len(), ZSetMeta::KVROCKS_ENCODED_SIZE);
  let kv_dec = ZSetMeta::decode(&kv_enc).expect("decode kvrocks failed");
  assert_eq!(kv_dec.base.size, 88);

  // 浮点数可排序保序编码测试（包含 -inf, -0.0, 0.0, +inf）
  let scores = [
    f64::NEG_INFINITY,
    -1000.5,
    -100.5,
    -1.0,
    -0.0,
    0.0,
    0.5,
    100.5,
    1000.5,
    f64::INFINITY,
  ];

  for i in 0..scores.len() - 1 {
    let enc1 = encode_sortable_f64(scores[i]);
    let enc2 = encode_sortable_f64(scores[i + 1]);
    assert!(
      enc1 <= enc2,
      "failed for {} vs {}",
      scores[i],
      scores[i + 1]
    );
    let dec = decode_sortable_f64(enc1);
    if scores[i].is_nan() {
      assert!(dec.is_nan());
    } else {
      assert_eq!(dec.to_bits(), scores[i].to_bits());
    }
  }

  Ok(())
}

#[test]
fn test_zset_basic_ops_and_binary_safety() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 二进制安全测试（包含 \x00, \xff 等任意二进制字节）
  let bin_m1 = b"bin\x00key\xff";
  let bin_m2 = b"\x00\x01\x02\xfe\xff";
  let bin_m3 = b"regular_user";

  assert_eq!(
    db.zadd(
      "z_bin",
      &[
        (10.0, bin_m1.as_slice()),
        (20.0, bin_m2.as_slice()),
        (15.0, bin_m3.as_slice())
      ],
      []
    )?,
    3
  );
  assert_eq!(db.zcard("z_bin")?, 3);
  assert_eq!(db.zscore("z_bin", bin_m1)?, Some(10.0));
  assert_eq!(db.zscore("z_bin", bin_m2)?, Some(20.0));
  assert_eq!(db.zscore("z_bin", bin_m3)?, Some(15.0));
  assert_eq!(db.zscore("z_bin", b"nonexistent")?, None);

  let mget = db.zmget(
    "z_bin",
    &[bin_m1.as_slice(), bin_m2.as_slice(), b"not_found"],
  )?;
  assert_eq!(mget.len(), 2);
  assert_eq!(mget.get(bin_m1.as_slice()), Some(&10.0));
  assert_eq!(mget.get(bin_m2.as_slice()), Some(&20.0));

  assert_eq!(
    db.zmscore(
      "z_bin",
      &[bin_m1.as_slice(), b"not_found", bin_m2.as_slice()]
    )?,
    vec![Some(10.0), None, Some(20.0)]
  );

  let range = db.zrange("z_bin", b"0", b"-1", [])?;
  assert_eq!(
    range,
    vec![
      (bin_m1.to_vec(), 10.0),
      (bin_m3.to_vec(), 15.0),
      (bin_m2.to_vec(), 20.0),
    ]
  );

  let revrange = db.zrevrange("z_bin", (0, 1))?;
  assert_eq!(
    revrange,
    vec![(bin_m2.to_vec(), 20.0), (bin_m3.to_vec(), 15.0)]
  );

  assert_eq!(db.zrank("z_bin", bin_m1)?, Some(0));
  assert_eq!(db.zrank("z_bin", bin_m3)?, Some(1));
  assert_eq!(db.zrank("z_bin", bin_m2)?, Some(2));
  assert_eq!(db.zrank_with_score("z_bin", bin_m2)?, Some((2, 20.0)));

  assert_eq!(db.zrevrank("z_bin", bin_m2)?, Some(0));
  assert_eq!(db.zrevrank_with_score("z_bin", bin_m2)?, Some((0, 20.0)));

  assert_eq!(db.zincrby("z_bin", 25.0, bin_m1)?, 35.0);
  assert_eq!(db.zscore("z_bin", bin_m1)?, Some(35.0));

  assert_eq!(db.zrem("z_bin", &[bin_m2.as_slice(), b"nonexistent"])?, 1);
  assert_eq!(db.zcard("z_bin")?, 2);
  assert_eq!(db.zscore("z_bin", bin_m2)?, None);

  Ok(())
}

#[test]
fn test_zset_options_nx_xx_gt_lt_ch_and_incompatible() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 互斥标志校验
  assert!(
    db.zadd("z_opts", &[(10.0, "m1")], [ZAdd::Nx, ZAdd::Xx])
      .is_err()
  );
  assert!(
    db.zadd("z_opts", &[(10.0, "m1")], [ZAdd::Gt, ZAdd::Lt])
      .is_err()
  );
  assert!(
    db.zadd("z_opts", &[(10.0, "m1")], [ZAdd::Nx, ZAdd::Gt])
      .is_err()
  );

  db.zadd("z_opts", &[(100.0, "m1")], [])?;

  // NX: 只添加新元素，不更新已有元素
  assert_eq!(
    db.zadd("z_opts", &[(200.0, "m1"), (50.0, "m2")], [ZAdd::Nx])?,
    1
  );
  assert_eq!(db.zscore("z_opts", "m1")?, Some(100.0));
  assert_eq!(db.zscore("z_opts", "m2")?, Some(50.0));

  // XX: 只更新已有元素，不添加新元素
  assert_eq!(
    db.zadd("z_opts", &[(300.0, "m1"), (10.0, "m3")], [ZAdd::Xx])?,
    0
  );
  assert_eq!(db.zscore("z_opts", "m1")?, Some(300.0));
  assert_eq!(db.zscore("z_opts", "m3")?, None);

  // GT: 仅当新分数大于旧分数时更新
  db.zadd("z_opts", &[(250.0, "m1")], [ZAdd::Gt])?;
  assert_eq!(db.zscore("z_opts", "m1")?, Some(300.0)); // 未变
  db.zadd("z_opts", &[(350.0, "m1")], [ZAdd::Gt])?;
  assert_eq!(db.zscore("z_opts", "m1")?, Some(350.0)); // 成功更新

  // LT: 仅当新分数小于旧分数时更新
  db.zadd("z_opts", &[(400.0, "m1")], [ZAdd::Lt])?;
  assert_eq!(db.zscore("z_opts", "m1")?, Some(350.0)); // 未变
  db.zadd("z_opts", &[(150.0, "m1")], [ZAdd::Lt])?;
  assert_eq!(db.zscore("z_opts", "m1")?, Some(150.0)); // 成功更新

  // CH: 返回发生改变的元素数量（新增 + 分数修改）
  let ch = db.zadd("z_opts", &[(180.0, "m1"), (90.0, "m4")], [ZAdd::Ch])?;
  assert_eq!(ch, 2);

  // 重复 member 去重测试：后出现者覆盖先出现者
  let ch_dup = db.zadd(
    "z_opts_dup",
    &[(10.0, "dup"), (20.0, "dup"), (30.0, "dup")],
    [],
  )?;
  assert_eq!(ch_dup, 1);
  assert_eq!(db.zscore("z_opts_dup", "dup")?, Some(30.0));

  Ok(())
}

#[test]
fn test_zset_ranges_and_pop_and_algebra() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.zadd(
    "z_range",
    &[(1.0, "a"), (2.0, "b"), (3.0, "c"), (4.0, "d"), (5.0, "e")],
    [],
  )?;

  // 负索引规整化边界测试（card=5，-5为首元素，-6及更小为空）
  assert_eq!(db.zrange("z_range", b"0", b"-6", [])?, Vec::new());
  assert_eq!(
    db.zrange("z_range", b"0", b"-5", [])?,
    vec![(b"a".to_vec(), 1.0)]
  );
  assert_eq!(db.zrange("z_range", b"-2", b"-1", [])?.len(), 2);
  assert_eq!(db.zrevrange("z_range", (0, -6))?, Vec::new());
  assert_eq!(
    db.zrevrange("z_range", (0, -5))?,
    vec![(b"e".to_vec(), 5.0)]
  );

  // ZRANGEBYSCORE & ZCOUNT
  let score_spec = RangeScore {
    min: 2.0,
    max: 4.0,
    minex: true,
    maxex: false,
    offset: 0,
    count: None,
  };
  assert_eq!(db.zcount("z_range", score_spec)?, 2); // (2.0, 4.0] -> 3.0, 4.0
  let by_score = db.zrangebyscore("z_range", score_spec)?;
  assert_eq!(by_score, vec![(b"c".to_vec(), 3.0), (b"d".to_vec(), 4.0)]);

  let rev_by_score = db.zrevrangebyscore("z_range", score_spec)?;
  assert_eq!(
    rev_by_score,
    vec![(b"d".to_vec(), 4.0), (b"c".to_vec(), 3.0)]
  );

  // ZRANGEBYLEX & ZLEXCOUNT
  let lex_spec = RangeLex {
    min: b"b".to_vec(),
    max: b"d".to_vec(),
    minex: false,
    maxex: true,
    min_infinite: false,
    max_infinite: false,
    offset: 0,
    count: None,
    reversed: false,
  };
  assert_eq!(db.zlexcount("z_range", &lex_spec)?, 2); // [b, d) -> b, c
  let by_lex = db.zrangebylex("z_range", &lex_spec)?;
  assert_eq!(by_lex, vec![b"b".to_vec(), b"c".to_vec()]);

  let rev_by_lex = db.zrevrangebylex("z_range", &lex_spec)?;
  assert_eq!(rev_by_lex, vec![b"c".to_vec(), b"b".to_vec()]);

  // 统一 ZRANGE 规格测试
  let zrange_score_spec = [ZRange::ByScore, ZRange::Rev, ZRange::Limit(0, 2)];
  let unified_res = db.zrange("z_range", b"2.0", b"5.0", zrange_score_spec)?;
  assert_eq!(unified_res.len(), 2);
  assert_eq!(unified_res[0].0, b"e");

  // ZPOPMIN & ZPOPMAX & BZPOPMIN & BZPOPMAX
  let bzpop = db.bzpopmin(&["nonexistent_zset", "z_range"])?;
  assert_eq!(bzpop, Some((b"z_range".to_vec(), b"a".to_vec(), 1.0)));
  assert_eq!(db.zcard("z_range")?, 4);

  let popmax = db.zpopmax("z_range", 1)?;
  assert_eq!(popmax, vec![(b"e".to_vec(), 5.0)]);
  assert_eq!(db.zcard("z_range")?, 3);

  // ZRANDMEMBER
  let rand_items = db.zrandmember("z_range", 2)?;
  assert_eq!(rand_items.len(), 2);
  let rand_rep = db.zrandmember("z_range", -4)?;
  assert_eq!(rand_rep.len(), 4);

  // ZUNION & ZINTER & ZDIFF
  db.zadd("z_A", &[(10.0, "x"), (20.0, "y")], [])?;
  db.zadd("z_B", &[(30.0, "y"), (40.0, "z")], [])?;

  let diff = db.zdiff(&["z_A", "z_B"])?;
  assert_eq!(diff, vec![(b"x".to_vec(), 10.0)]);
  assert_eq!(db.zdiffstore("z_diff_dst", &["z_A", "z_B"])?, 1);

  let union_res = db.zunion(&[("z_A", 1.0), ("z_B", 2.0)], Aggregate::Sum)?;
  assert_eq!(union_res.len(), 3);
  assert_eq!(
    db.zunionstore("z_union_dst", &[("z_A", 1.0), ("z_B", 2.0)], Aggregate::Sum)?,
    3
  );

  let inter_res = db.zinter(&[("z_A", 1.0), ("z_B", 1.0)], Aggregate::Max)?;
  assert_eq!(inter_res, vec![(b"y".to_vec(), 30.0)]);
  assert_eq!(
    db.zinterstore("z_inter_dst", &[("z_A", 1.0), ("z_B", 1.0)], Aggregate::Max)?,
    1
  );
  assert_eq!(db.zintercard(&["z_A", "z_B"], 0)?, 1);
  assert_eq!(db.zintercard(&["z_A", "z_B"], 1)?, 1);

  // ZREMRANGEBYRANK & ZREMRANGEBYSCORE & ZREMRANGEBYLEX
  assert_eq!(db.zremrangebyrank("z_range", (0, 0))?, 1);
  assert_eq!(db.zcard("z_range")?, 2);

  let rem_score_spec = RangeScore::new(2.5, 3.5);
  assert_eq!(db.zremrangebyscore("z_range", rem_score_spec)?, 1);
  assert_eq!(db.zcard("z_range")?, 1);

  let rem_lex_spec = RangeLex::new(b"a", b"z");
  assert_eq!(db.zremrangebylex("z_range", &rem_lex_spec)?, 1);
  assert_eq!(db.zcard("z_range")?, 0);

  // ZSCAN
  let (cur, page) = db.zscan("z_union_dst", 0, None, Some(10))?;
  assert_eq!(page.len(), 3);
  assert_eq!(cur, 0);

  Ok(())
}

#[test]
fn test_zset_extended_edge_cases() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 同分数按 member 字典序排序测试
  db.zadd(
    "z_equal_scores",
    &[
      (10.0, "charlie"),
      (10.0, "alice"),
      (10.0, "bob"),
      (10.0, "david"),
    ],
    [],
  )?;
  let all = db.zget_all("z_equal_scores")?;
  assert_eq!(
    all,
    vec![
      (b"alice".to_vec(), 10.0),
      (b"bob".to_vec(), 10.0),
      (b"charlie".to_vec(), 10.0),
      (b"david".to_vec(), 10.0),
    ]
  );

  assert_eq!(db.zrank("z_equal_scores", "alice")?, Some(0));
  assert_eq!(db.zrank("z_equal_scores", "bob")?, Some(1));
  assert_eq!(db.zrank("z_equal_scores", "charlie")?, Some(2));
  assert_eq!(db.zrank("z_equal_scores", "david")?, Some(3));
  assert_eq!(db.zrank("z_equal_scores", "none")?, None);

  assert_eq!(db.zrevrank("z_equal_scores", "david")?, Some(0));
  assert_eq!(db.zrevrank("z_equal_scores", "charlie")?, Some(1));
  assert_eq!(db.zrevrank("z_equal_scores", "bob")?, Some(2));
  assert_eq!(db.zrevrank("z_equal_scores", "alice")?, Some(3));
  assert_eq!(db.zrevrank("z_equal_scores", "none")?, None);

  // 2. 极值浮点数与边界情况
  db.zadd(
    "z_floats",
    &[
      (f64::NEG_INFINITY, "neg_inf"),
      (-0.0, "neg_zero"),
      (0.0, "pos_zero"),
      (1e-50, "tiny"),
      (1e50, "huge"),
      (f64::INFINITY, "pos_inf"),
    ],
    [],
  )?;

  assert_eq!(db.zcard("z_floats")?, 6);
  let float_all = db.zget_all("z_floats")?;
  assert_eq!(float_all[0].0, b"neg_inf");
  assert_eq!(float_all[float_all.len() - 1].0, b"pos_inf");

  // 3. ZPOPMIN / ZPOPMAX 多批次测试
  let popped_min = db.zpopmin("z_floats", 2)?;
  assert_eq!(popped_min.len(), 2);
  assert_eq!(popped_min[0].0, b"neg_inf");
  assert_eq!(db.zcard("z_floats")?, 4);

  let popped_max = db.zpopmax("z_floats", 2)?;
  assert_eq!(popped_max.len(), 2);
  assert_eq!(popped_max[0].0, b"pos_inf");
  assert_eq!(db.zcard("z_floats")?, 2);

  // 4. ZINCRBY 不存在时默认从 0.0 开始累加
  assert_eq!(db.zincrby("z_new_incr", 42.5, "item1")?, 42.5);
  assert_eq!(db.zscore("z_new_incr", "item1")?, Some(42.5));
  assert_eq!(db.zincrby("z_new_incr", -10.5, "item1")?, 32.0);
  assert_eq!(db.zscore("z_new_incr", "item1")?, Some(32.0));

  // 5. ZINTER / ZUNION 聚合算法全覆盖 (SUM, MIN, MAX)
  db.zadd("z_set1", &[(10.0, "a"), (20.0, "b")], [])?;
  db.zadd("z_set2", &[(30.0, "a"), (15.0, "b")], [])?;

  let inter_min = db.zinter(&[("z_set1", 1.0), ("z_set2", 1.0)], Aggregate::Min)?;
  assert_eq!(
    inter_min,
    vec![(b"a".to_vec(), 10.0), (b"b".to_vec(), 15.0)]
  );

  let inter_max = db.zinter(&[("z_set1", 1.0), ("z_set2", 1.0)], Aggregate::Max)?;
  assert_eq!(
    inter_max,
    vec![(b"b".to_vec(), 20.0), (b"a".to_vec(), 30.0)]
  );

  let union_min = db.zunion(&[("z_set1", 1.0), ("z_set2", 1.0)], Aggregate::Min)?;
  assert_eq!(
    union_min,
    vec![(b"a".to_vec(), 10.0), (b"b".to_vec(), 15.0)]
  );

  // 6. ZINTERCARD 边界与限制测试
  assert_eq!(db.zintercard(&["z_set1", "z_set2"], 0)?, 2);
  assert_eq!(db.zintercard(&["z_set1", "z_set2"], 1)?, 1);
  assert_eq!(db.zintercard(&["z_set1", "z_set2"], 10)?, 2);
  assert_eq!(db.zintercard(&["z_set1", "nonexistent"], 5)?, 0);

  // 7. ZMSCORE 与 ZMGET 批量点查与空集返回
  let zmscores = db.zmscore("z_set1", &["a", "b", "c"])?;
  assert_eq!(zmscores, vec![Some(10.0), Some(20.0), None]);
  let zmscores_empty = db.zmscore("empty_zset", &["a", "b"])?;
  assert_eq!(zmscores_empty, vec![None, None]);

  let zmget_empty = db.zmget("empty_zset", &["a", "b"])?;
  assert!(zmget_empty.is_empty());

  Ok(())
}

#[test]
fn test_zset_single_pass_rem_ranges_and_boundary() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. ZREMRANGEBYRANK
  db.zadd(
    "z_rem_rank",
    &[
      (10.0, "m1"),
      (20.0, "m2"),
      (30.0, "m3"),
      (40.0, "m4"),
      (50.0, "m5"),
    ],
    [],
  )?;
  // 删除 rank 1..=3 (m2, m3, m4)
  assert_eq!(db.zremrangebyrank("z_rem_rank", (1, 3))?, 3);
  assert_eq!(db.zcard("z_rem_rank")?, 2);
  let remaining = db.zget_all("z_rem_rank")?;
  assert_eq!(
    remaining,
    vec![(b"m1".to_vec(), 10.0), (b"m5".to_vec(), 50.0)]
  );
  // 再次点查已被删除的 member，确保 member_key 和 score_key 均被物理删除
  assert_eq!(db.zscore("z_rem_rank", "m2")?, None);
  assert_eq!(db.zscore("z_rem_rank", "m3")?, None);
  assert_eq!(db.zscore("z_rem_rank", "m4")?, None);
  assert_eq!(db.zscore("z_rem_rank", "m1")?, Some(10.0));
  assert_eq!(db.zscore("z_rem_rank", "m5")?, Some(50.0));

  // 删除剩余所有 (0..=-1)
  assert_eq!(db.zremrangebyrank("z_rem_rank", (0, -1))?, 2);
  assert_eq!(db.zcard("z_rem_rank")?, 0);
  assert!(db.zget_all("z_rem_rank")?.is_empty());

  // 2. ZREMRANGEBYSCORE
  db.zadd(
    "z_rem_score",
    &[(1.5, "a"), (2.5, "b"), (3.5, "c"), (4.5, "d"), (5.5, "e")],
    [],
  )?;
  // 开区间 (2.0, 5.0) -> 删除 b(2.5), c(3.5), d(4.5)
  let spec = RangeScore {
    min: 2.0,
    max: 5.0,
    minex: true,
    maxex: true,
    ..Default::default()
  };
  assert_eq!(db.zremrangebyscore("z_rem_score", spec)?, 3);
  assert_eq!(db.zcard("z_rem_score")?, 2);
  assert_eq!(
    db.zget_all("z_rem_score")?,
    vec![(b"a".to_vec(), 1.5), (b"e".to_vec(), 5.5)]
  );

  // 3. ZREMRANGEBYLEX
  db.zadd(
    "z_rem_lex",
    &[
      (0.0, "alpha"),
      (0.0, "beta"),
      (0.0, "gamma"),
      (0.0, "delta"),
      (0.0, "omega"),
    ],
    [],
  )?;
  // 闭区间 [beta, gamma] -> 删除 beta, delta, gamma (按字典序: alpha, beta, delta, gamma, omega)
  let lex_spec = RangeLex::from_bounds(b"[beta", b"[gamma", 0, None)?;
  assert_eq!(db.zremrangebylex("z_rem_lex", &lex_spec)?, 3);
  assert_eq!(db.zcard("z_rem_lex")?, 2);
  assert_eq!(
    db.zget_all("z_rem_lex")?,
    vec![(b"alpha".to_vec(), 0.0), (b"omega".to_vec(), 0.0)]
  );

  Ok(())
}

#[test]
fn test_zset_expired_purging_and_isolation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 手动插入一个已过期的 ZSetMeta
  let kc = KeyComposer::default();
  let meta_k = compose_zset_meta_key(&kc, b"z_expired");
  let old_meta = ZSetMeta::new(1000, 101, 2); // 1970年，必定过期
  db.meta().insert(&meta_k, &old_meta.encode())?;

  // 插入旧残留数据
  let s_key1 = compose_zset_score_key(&kc, b"z_expired", 10.0, b"ghost1");
  let m_key1 = compose_zset_key(&kc, b"z_expired", b"ghost1");
  db.data().insert(&s_key1, b"")?;
  db.data().insert(&m_key1, &encode_sortable_f64(10.0))?;

  // 此时查询已过期 key 应该返回空 / 0
  assert_eq!(db.zcard("z_expired")?, 0);
  assert_eq!(db.zscore("z_expired", "ghost1")?, None);
  assert!(db.zget_all("z_expired")?.is_empty());

  // 重新通过 ZADD 写入新数据，触发 prepare_zset_meta_for_write 的过期自动清理
  assert_eq!(db.zadd("z_expired", &[(99.0, "new_member")], [])?, 1);
  assert_eq!(db.zcard("z_expired")?, 1);
  assert_eq!(db.zscore("z_expired", "new_member")?, Some(99.0));
  assert_eq!(db.zscore("z_expired", "ghost1")?, None);

  let all = db.zget_all("z_expired")?;
  assert_eq!(all, vec![(b"new_member".to_vec(), 99.0)]);

  Ok(())
}

#[test]
fn test_zset_wrongtype_checks() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试与 String 类型的互斥
  db.set("str_key", "string_val", [])?;
  let err_zadd = db.zadd("str_key", &[(10.0, "member")], []);
  assert!(err_zadd.is_err());
  assert!(
    err_zadd
      .unwrap_err()
      .to_string()
      .contains("WRONGTYPE Operation against a key holding the wrong kind of value")
  );

  let err_incr = db.zincrby("str_key", 5.0, "member");
  assert!(err_incr.is_err());

  let err_overwrite = db.overwrite_zset("str_key", &[("member", 10.0)]);
  assert!(err_overwrite.is_err());

  // 2. 测试与其他复杂类型 (如 Set) 的互斥
  db.sadd("set_key", &["member1", "member2"])?;
  let err_set_zadd = db.zadd("set_key", &[(1.0, "member1")], []);
  assert!(err_set_zadd.is_err());

  Ok(())
}

#[test]
fn test_zset_range_spec_extended_options() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.zadd(
    "z_range_ext",
    &[
      (10.0, "a"),
      (20.0, "b"),
      (30.0, "c"),
      (40.0, "d"),
      (50.0, "e"),
    ],
    [],
  )?;

  // 1. ZRANGEBYLEX with scores
  let lex_spec = RangeLex {
    min: b"b".to_vec(),
    max: b"d".to_vec(),
    ..Default::default()
  };
  let lex_with_scores = db.zrangebylex_with_scores("z_range_ext", &lex_spec)?;
  assert_eq!(
    lex_with_scores,
    vec![
      (b"b".to_vec(), 20.0),
      (b"c".to_vec(), 30.0),
      (b"d".to_vec(), 40.0),
    ]
  );

  let rev_lex_with_scores = db.zrevrangebylex_with_scores("z_range_ext", &lex_spec)?;
  assert_eq!(
    rev_lex_with_scores,
    vec![
      (b"d".to_vec(), 40.0),
      (b"c".to_vec(), 30.0),
      (b"b".to_vec(), 20.0),
    ]
  );

  // 2. 统一 ZRANGE 规范中的 BYLEX + REV + WITHSCORES
  let zrange_lex_rev_spec = [ZRange::ByLex, ZRange::Rev, ZRange::WithScores];
  let unified_lex_res = db.zrange("z_range_ext", b"[d", b"[b", zrange_lex_rev_spec)?;
  assert_eq!(
    unified_lex_res,
    vec![
      (b"d".to_vec(), 40.0),
      (b"c".to_vec(), 30.0),
      (b"b".to_vec(), 20.0),
    ]
  );

  // 3. 统一 ZRANGE 规范中的 BYSCORE + REV (Redis CLI 传参顺序 <max> <min>)
  let zrange_score_rev_spec = [
    ZRange::ByScore,
    ZRange::Rev,
    ZRange::WithScores,
    ZRange::Limit(1, 2),
  ];
  // 逆序查找 [45, 15] 之间，跳过 1 个 (40)，取 2 个 (30, 20)
  let unified_score_res = db.zrange("z_range_ext", b"45.0", b"15.0", zrange_score_rev_spec)?;
  assert_eq!(
    unified_score_res,
    vec![(b"c".to_vec(), 30.0), (b"b".to_vec(), 20.0)]
  );

  // 4. 字典序边界严格解析测试 (对标 Kvrocks ParseRangeLex)
  assert!(RangeLex::from_bounds(b"-", b"+", 0, None).is_ok());
  assert!(RangeLex::from_bounds(b"[a", b"(z", 0, None).is_ok());
  assert!(RangeLex::from_bounds(b"+", b"-", 0, None).is_err()); // '+' 非法作为 min
  assert!(RangeLex::from_bounds(b"a", b"z", 0, None).is_err()); // 缺少 '[' 或 '(' 前缀
  assert!(RangeLex::from_bounds(b"-", b"-", 0, None).is_err()); // '-' 非法作为 max

  // 5. 分数边界解析测试 (对标 Kvrocks ParseRangeScore)
  let spec_score_inf = RangeScore::from_bounds("-inf", "+inf", 0, None)?;
  assert_eq!(spec_score_inf.min, f64::NEG_INFINITY);
  assert_eq!(spec_score_inf.max, f64::INFINITY);

  let spec_score_ex = RangeScore::from_bounds("(10.5", "(20.5", 0, None)?;
  assert!(spec_score_ex.minex);
  assert!(spec_score_ex.maxex);
  assert_eq!(spec_score_ex.min, 10.5);
  assert_eq!(spec_score_ex.max, 20.5);

  Ok(())
}

#[test]
fn test_zset_ttl_and_streaming_reverse_iterators() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试流式逆序迭代器与极速 ZREVRANK
  db.zadd(
    "z_rev_stream",
    &[
      (10.0, "m1"),
      (20.0, "m2"),
      (30.0, "m3"),
      (40.0, "m4"),
      (50.0, "m5"),
    ],
    [],
  )?;

  let mut rev_scores = Vec::new();
  db.ziter_rev("z_rev_stream", |m, s| {
    rev_scores.push((m.to_vec(), s));
    true
  })?;
  assert_eq!(
    rev_scores,
    vec![
      (b"m5".to_vec(), 50.0),
      (b"m4".to_vec(), 40.0),
      (b"m3".to_vec(), 30.0),
      (b"m2".to_vec(), 20.0),
      (b"m1".to_vec(), 10.0),
    ]
  );

  assert_eq!(db.zrevrank("z_rev_stream", "m5")?, Some(0));
  assert_eq!(
    db.zrevrank_with_score("z_rev_stream", "m5")?,
    Some((0, 50.0))
  );
  assert_eq!(db.zrevrank("z_rev_stream", "m1")?, Some(4));
  assert_eq!(db.zrevrank("z_rev_stream", "m3")?, Some(2));
  assert_eq!(db.zrevrank("z_rev_stream", "nonexistent")?, None);

  // 2. 逆序流式截取与提前终止 (ZREVRANGEBYSCORE & ZREVRANGEBYLEX)
  let score_spec = RangeScore {
    min: 20.0,
    max: 40.0,
    minex: false,
    maxex: false,
    offset: 0,
    count: Some(2),
  };
  let rev_score_items = db.zrevrangebyscore("z_rev_stream", score_spec)?;
  assert_eq!(
    rev_score_items,
    vec![(b"m4".to_vec(), 40.0), (b"m3".to_vec(), 30.0)]
  );

  // 空区间提前中断 (min > max 或 min == max && minex)
  let empty_score_spec = RangeScore {
    min: 50.0,
    max: 20.0,
    ..Default::default()
  };
  assert!(
    db.zrangebyscore("z_rev_stream", empty_score_spec)?
      .is_empty()
  );
  assert!(
    db.zrevrangebyscore("z_rev_stream", empty_score_spec)?
      .is_empty()
  );
  assert_eq!(db.zcount("z_rev_stream", empty_score_spec)?, 0);
  assert_eq!(db.zremrangebyscore("z_rev_stream", empty_score_spec)?, 0);

  // 3. TTL 生命周期管理测试 (EXPIRE, EXPIREAT, TTL, PTTL, KEY_PERSIST)
  let non_exist_ttl = db.ttl("nonexistent_key")?;
  assert_eq!(non_exist_ttl, -2);
  let non_exist_pttl = db.pttl("nonexistent_key")?;
  assert_eq!(non_exist_pttl, -2);

  assert_eq!(db.ttl("z_rev_stream")?, -1); // 永不过期
  assert_eq!(db.pttl("z_rev_stream")?, -1);
  assert_eq!(db.get_key_expire_at("z_rev_stream")?, Some(0));

  // 设置过期时间
  assert!(db.expire("z_rev_stream", 3600)?);
  let ttl = db.ttl("z_rev_stream")?;
  assert!(ttl > 0 && ttl <= 3600);
  let pttl = db.pttl("z_rev_stream")?;
  assert!(pttl > 0 && pttl <= 3600 * 1000);

  let expire_time = db.get_key_expire_at("z_rev_stream")?.unwrap();
  assert!(expire_time > 0);

  // 持久化移除过期时间
  assert!(db.persist("z_rev_stream")?);
  assert_eq!(db.ttl("z_rev_stream")?, -1);
  assert_eq!(db.pttl("z_rev_stream")?, -1);
  assert!(!db.persist("z_rev_stream")?); // 再次 persist 返回 false

  // 4. 任意二进制非 UTF-8 Key 安全测试
  let bin_key = b"\x80\xfe\xff\x00_zset";
  assert_eq!(db.zadd(bin_key, &[(1.0, "v1"), (2.0, "v2")], [])?, 2);
  assert_eq!(db.zcard(bin_key)?, 2);
  assert_eq!(db.zscore(bin_key, "v1")?, Some(1.0));
  assert_eq!(db.zscore(bin_key, "v2")?, Some(2.0));
  assert_eq!(db.zrank(bin_key, "v1")?, Some(0));
  assert_eq!(db.zrevrank(bin_key, "v1")?, Some(1));
  assert_eq!(db.zpopmax(bin_key, 1)?, vec![(b"v2".to_vec(), 2.0)]);
  assert_eq!(db.zcard(bin_key)?, 1);

  Ok(())
}

#[test]
fn test_zset_precise_range_seek_optimization() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 构造具有大量元素的数据集（1000 个元素，分数 0.0 到 999.0）
  let mut pairs = Vec::with_capacity(1000);
  for i in 0..1000 {
    pairs.push((i as f64, format!("member_{i:04}")));
  }
  db.zadd("z_big", &pairs, [])?;
  assert_eq!(db.zcard("z_big")?, 1000);

  // 2. 测试中间范围查询 [500.0, 505.0]
  let spec_inc = RangeScore {
    min: 500.0,
    max: 505.0,
    minex: false,
    maxex: false,
    offset: 0,
    count: None,
  };
  assert_eq!(db.zcount("z_big", spec_inc)?, 6);
  let items_inc = db.zrangebyscore("z_big", spec_inc)?;
  assert_eq!(items_inc.len(), 6);
  assert_eq!(items_inc[0].0, b"member_0500");
  assert_eq!(items_inc[5].0, b"member_0505");

  // 3. 测试开区间 (500.0, 505.0)
  let spec_ex = RangeScore {
    min: 500.0,
    max: 505.0,
    minex: true,
    maxex: true,
    offset: 0,
    count: None,
  };
  assert_eq!(db.zcount("z_big", spec_ex)?, 4);
  let items_ex = db.zrangebyscore("z_big", spec_ex)?;
  assert_eq!(items_ex.len(), 4);
  assert_eq!(items_ex[0].0, b"member_0501");
  assert_eq!(items_ex[3].0, b"member_0504");

  // 4. 逆序范围查询 [500.0, 505.0] 带 offset 和 limit
  let spec_rev_limit = RangeScore {
    min: 500.0,
    max: 505.0,
    minex: false,
    maxex: false,
    offset: 1,
    count: Some(2),
  };
  let rev_items = db.zrevrangebyscore("z_big", spec_rev_limit)?;
  assert_eq!(rev_items.len(), 2);
  assert_eq!(rev_items[0].0, b"member_0504");
  assert_eq!(rev_items[1].0, b"member_0503");

  // 5. 字典序范围查询 [member_0200, member_0205)
  let lex_spec = RangeLex {
    min: b"member_0200".to_vec(),
    max: b"member_0205".to_vec(),
    minex: false,
    maxex: true,
    min_infinite: false,
    max_infinite: false,
    offset: 0,
    count: None,
    reversed: false,
  };
  assert_eq!(db.zlexcount("z_big", &lex_spec)?, 5);
  let lex_items = db.zrangebylex("z_big", &lex_spec)?;
  assert_eq!(lex_items.len(), 5);
  assert_eq!(lex_items[0], b"member_0200");
  assert_eq!(lex_items[4], b"member_0204");

  // 6. 逆序字典序范围查询 (member_0200, member_0205]
  let rev_lex_spec = RangeLex {
    min: b"member_0200".to_vec(),
    max: b"member_0205".to_vec(),
    minex: true,
    maxex: false,
    min_infinite: false,
    max_infinite: false,
    offset: 0,
    count: None,
    reversed: true,
  };
  let rev_lex_items = db.zrevrangebylex("z_big", &rev_lex_spec)?;
  assert_eq!(rev_lex_items.len(), 5);
  assert_eq!(rev_lex_items[0], b"member_0205");
  assert_eq!(rev_lex_items[4], b"member_0201");

  // 7. 删除指定分数区间 [100.0, 199.0] (100 个元素)
  let rem_spec = RangeScore::new(100.0, 199.0);
  assert_eq!(db.zremrangebyscore("z_big", rem_spec)?, 100);
  assert_eq!(db.zcard("z_big")?, 900);
  assert_eq!(db.zscore("z_big", "member_0100")?, None);
  assert_eq!(db.zscore("z_big", "member_0199")?, None);
  assert_eq!(db.zscore("z_big", "member_0099")?, Some(99.0));
  assert_eq!(db.zscore("z_big", "member_0200")?, Some(200.0));

  // 8. 删除指定字典序区间 [member_0300, member_0399] (100 个元素)
  let rem_lex = RangeLex::new(b"member_0300", b"member_0399");
  assert_eq!(db.zremrangebylex("z_big", &rem_lex)?, 100);
  assert_eq!(db.zcard("z_big")?, 800);
  assert_eq!(db.zscore("z_big", "member_0300")?, None);
  assert_eq!(db.zscore("z_big", "member_0399")?, None);
  assert_eq!(db.zscore("z_big", "member_0299")?, Some(299.0));
  assert_eq!(db.zscore("z_big", "member_0400")?, Some(400.0));

  // 9. 零值 (+0.0 / -0.0) 范围测试
  db.zadd(
    "z_zero",
    &[
      (0.0, "zero_1"),
      (-0.0, "zero_2"),
      (1.0, "one"),
      (-1.0, "neg_one"),
    ],
    [],
  )?;
  let zero_spec = RangeScore::new(0.0, 0.0);
  assert_eq!(db.zcount("z_zero", zero_spec)?, 2);
  let zero_items = db.zrangebyscore("z_zero", zero_spec)?;
  assert_eq!(zero_items.len(), 2);

  let zero_ex_spec = RangeScore {
    min: 0.0,
    max: 10.0,
    minex: true,
    maxex: false,
    offset: 0,
    count: None,
  };
  let zero_ex_items = db.zrangebyscore("z_zero", zero_ex_spec)?;
  assert_eq!(zero_ex_items.len(), 1);
  assert_eq!(zero_ex_items[0].0, b"one");

  Ok(())
}

#[test]
fn test_zset_binary_key_with_ff_boundary() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let key_ff = b"key_ends_with_ff\xff\xff";
  db.zadd(
    key_ff,
    &[
      (10.0, b"m1".as_slice()),
      (20.0, b"m2".as_slice()),
      (30.0, b"m3\xff".as_slice()),
    ],
    [],
  )?;

  assert_eq!(db.zcard(key_ff)?, 3);
  let spec = RangeScore::new(f64::NEG_INFINITY, f64::INFINITY);
  let items = db.zrangebyscore(key_ff, spec)?;
  assert_eq!(items.len(), 3);
  assert_eq!(items[0].0, b"m1");
  assert_eq!(items[1].0, b"m2");
  assert_eq!(items[2].0, b"m3\xff");

  let lex_spec = RangeLex::unbounded();
  let lex_items = db.zrangebylex(key_ff, &lex_spec)?;
  assert_eq!(lex_items.len(), 3);
  assert_eq!(lex_items[0], b"m1");
  assert_eq!(lex_items[1], b"m2");
  assert_eq!(lex_items[2], b"m3\xff");

  Ok(())
}

#[test]
fn test_zset_kvrocks_comprehensive_suite_zmpop_and_zscan_by_member() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 单元素写入与点查零内存分配路径验证
  let ret1 = db.zadd_one("single_z", 10.5, "m1", [])?;
  assert_eq!(ret1, 1);
  assert_eq!(db.zcard("single_z")?, 1);
  assert_eq!(db.zscore("single_z", "m1")?, Some(10.5));
  assert_eq!(
    db.zmscore("single_z", &["m1", "m_non"])?,
    vec![Some(10.5), None]
  );
  let mget_res = db.zmget("single_z", &["m1", "m_non"])?;
  assert_eq!(mget_res.len(), 1);
  assert_eq!(mget_res.get(b"m1".as_slice()), Some(&10.5));

  // zincrby
  let new_sc = db.zincrby("single_z", 5.5, "m1")?;
  assert_eq!(new_sc, 16.0);
  assert_eq!(db.zscore("single_z", "m1")?, Some(16.0));

  // zrem_one
  let rem1 = db.zrem_one("single_z", "m1")?;
  assert_eq!(rem1, 1);
  assert_eq!(db.zcard("single_z")?, 0);
  assert_eq!(db.zscore("single_z", "m1")?, None);

  // 2. 多元素与 Tie-breaker 同分测试 (Score 相同按 member 字典序排列)
  db.zadd(
    "tie_z",
    &[(10.0, "c"), (10.0, "a"), (10.0, "b"), (20.0, "d")],
    [],
  )?;

  assert_eq!(db.zrank("tie_z", "a")?, Some(0));
  assert_eq!(db.zrank("tie_z", "b")?, Some(1));
  assert_eq!(db.zrank("tie_z", "c")?, Some(2));
  assert_eq!(db.zrank("tie_z", "d")?, Some(3));
  assert_eq!(db.zrank("tie_z", "non_exist")?, None);

  assert_eq!(db.zrevrank("tie_z", "d")?, Some(0));
  assert_eq!(db.zrevrank("tie_z", "c")?, Some(1));
  assert_eq!(db.zrevrank("tie_z", "b")?, Some(2));
  assert_eq!(db.zrevrank("tie_z", "a")?, Some(3));

  // 3. ZMPOP 多键批量弹出测试 (Redis 7.0 对齐)
  db.zadd("z_pop_1", &[(1.0, "x1"), (2.0, "x2"), (3.0, "x3")], [])?;
  db.zadd("z_pop_2", &[(10.0, "y1"), (20.0, "y2")], [])?;

  // 空键首先被跳过，优先弹出第一个非空键
  let pop_min_res = db.zmpop(&["empty_1", "z_pop_1", "z_pop_2"], true, 2)?;
  assert!(pop_min_res.is_some());
  let (pop_k, pop_items) = pop_min_res.unwrap();
  assert_eq!(pop_k, b"z_pop_1");
  assert_eq!(pop_items.len(), 2);
  assert_eq!(pop_items[0], (b"x1".to_vec(), 1.0));
  assert_eq!(pop_items[1], (b"x2".to_vec(), 2.0));
  assert_eq!(db.zcard("z_pop_1")?, 1);

  // MAX 弹出
  let pop_max_res = db.zmpop(&["z_pop_1"], false, 5)?;
  let (pop_k2, pop_items2) = pop_max_res.unwrap();
  assert_eq!(pop_k2, b"z_pop_1");
  assert_eq!(pop_items2, vec![(b"x3".to_vec(), 3.0)]);
  assert_eq!(db.zcard("z_pop_1")?, 0);

  // 4. ZRANDMEMBER 快速路径与随机采样
  db.zadd(
    "z_rand",
    &[(1.0, "r1"), (2.0, "r2"), (3.0, "r3"), (4.0, "r4")],
    [],
  )?;
  let r_one = db.zrandmember_one("z_rand")?;
  assert!(r_one.is_some());
  let (r_member, r_score) = r_one.unwrap();
  assert!([b"r1".as_slice(), b"r2", b"r3", b"r4"].contains(&r_member.as_slice()));
  assert!([1.0, 2.0, 3.0, 4.0].contains(&r_score));

  let r_dup = db.zrandmember("z_rand", -10)?;
  assert_eq!(r_dup.len(), 10);

  // 5. ZSCAN_BY_MEMBER 范围游标分页测试 (对标 Kvrocks ZSet::Scan)
  db.zadd(
    "z_scan_page",
    &[
      (100.0, "alpha"),
      (200.0, "beta"),
      (300.0, "charlie"),
      (400.0, "delta"),
      (500.0, "echo"),
    ],
    [],
  )?;

  // 第一页 limit 2
  let (next_cur, page1) = db.zscan_by_member("z_scan_page", None, None, Some(2))?;
  assert_eq!(page1.len(), 2);
  assert_eq!(page1[0].0, b"alpha");
  assert_eq!(page1[1].0, b"beta");
  assert_eq!(next_cur.as_deref(), Some(b"beta".as_slice()));

  // 第二页从 "beta" 之后继续
  let (next_cur2, page2) = db.zscan_by_member("z_scan_page", next_cur.as_deref(), None, Some(2))?;
  assert_eq!(page2.len(), 2);
  assert_eq!(page2[0].0, b"charlie");
  assert_eq!(page2[1].0, b"delta");
  assert_eq!(next_cur2.as_deref(), Some(b"delta".as_slice()));

  // 第三页
  let (next_cur3, page3) =
    db.zscan_by_member("z_scan_page", next_cur2.as_deref(), None, Some(2))?;
  assert_eq!(page3.len(), 1);
  assert_eq!(page3[0].0, b"echo");
  assert_eq!(next_cur3, None);

  // 6. ZINTERCARD 单 Key O(1) 优化与多 Key 截断
  assert_eq!(db.zintercard(&["z_scan_page"], 0)?, 5);
  assert_eq!(db.zintercard(&["z_scan_page"], 3)?, 3);
  assert_eq!(db.zintercard(&["z_scan_page", "non_exist_key"], 0)?, 0);

  // 7. ZDIFF 对齐 Kvrocks 测试用例 (Diff 与 DiffStore)
  db.zadd(
    "k1",
    &[(-100.1, "a"), (-100.1, "b"), (0.0, "c"), (1.234, "d")],
    [],
  )?;
  db.zadd("k2", &[(-150.1, "c")], [])?;
  db.zadd("k3", &[(-1000.1, "a"), (-100.1, "c"), (8000.9, "e")], [])?;

  let diff_res = db.zdiff(&["k1", "k2", "k3"])?;
  assert_eq!(diff_res.len(), 2);
  assert_eq!(diff_res[0], (b"b".to_vec(), -100.1));
  assert_eq!(diff_res[1], (b"d".to_vec(), 1.234));

  let diffstore_cnt = db.zdiffstore("zdiff_dst", &["k1", "k2"])?;
  assert_eq!(diffstore_cnt, 3);
  assert_eq!(db.zcard("zdiff_dst")?, 3);

  // 8. ZREMRANGEBYRANK 全量删除快速路径
  let rem_full = db.zremrangebyrank("zdiff_dst", (0, -1))?;
  assert_eq!(rem_full, 3);
  assert_eq!(db.zcard("zdiff_dst")?, 0);

  Ok(())
}
