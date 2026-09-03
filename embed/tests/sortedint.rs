use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, Partition, SortedintMeta, SortedintRange, WeDb, api::sortedint::compose_si_meta_key,
  parse_range_spec,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_sortedint_basic_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert_eq!(db.si_add("sikey", &[10, 20, 30, 40, 50])?, 5);
  assert_eq!(db.si_add("sikey", &[20, 60])?, 1);
  assert_eq!(db.si_card("sikey")?, 6);

  assert!(db.si_exists("sikey", 20)?);
  assert!(!db.si_exists("sikey", 99)?);

  let mexist = db.si_mexist("sikey", &[10, 25, 30, 99])?;
  assert_eq!(mexist, vec![true, false, true, false]);

  let members = db.si_members("sikey")?;
  assert_eq!(members, vec![10, 20, 30, 40, 50, 60]);

  let range = db.si_range("sikey", 0, 0, 10, false)?;
  assert_eq!(range, vec![10, 20, 30, 40, 50, 60]);

  let page = db.si_range("sikey", 0, 2, 2, false)?;
  assert_eq!(page, vec![30, 40]);

  let cursor_range = db.si_range("sikey", 30, 0, 2, false)?;
  assert_eq!(cursor_range, vec![40, 50]);

  let rev_range = db.si_rev_range("sikey", 0, 0, 10)?;
  assert_eq!(rev_range, vec![60, 50, 40, 30, 20, 10]);

  let rev_cursor = db.si_rev_range("sikey", 40, 0, 2)?;
  assert_eq!(rev_cursor, vec![30, 20]);

  assert_eq!(db.si_rem("sikey", &[20, 40, 99])?, 2);
  assert_eq!(db.si_card("sikey")?, 4);
  assert_eq!(db.si_members("sikey")?, vec![10, 30, 50, 60]);

  Ok(())
}

#[test]
fn test_sortedint_range_by_value() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.si_add("sikey", &[10, 20, 30, 40, 50, 60, 70, 80])?;

  // [20, 60]
  let spec1 = parse_range_spec("20", "60")?;
  let r1 = db.si_range_by_value("sikey", spec1)?;
  assert_eq!(r1, vec![20, 30, 40, 50, 60]);

  // (20, (60)
  let spec2 = parse_range_spec("(20", "(60")?;
  let r2 = db.si_range_by_value("sikey", spec2)?;
  assert_eq!(r2, vec![30, 40, 50]);

  // [20, (60)
  let spec2_half = parse_range_spec("[20", "(60")?;
  let r2_half = db.si_range_by_value("sikey", spec2_half)?;
  assert_eq!(r2_half, vec![20, 30, 40, 50]);

  // (20, 60]
  let spec2_half2 = parse_range_spec("(20", "[60")?;
  let r2_half2 = db.si_range_by_value("sikey", spec2_half2)?;
  assert_eq!(r2_half2, vec![30, 40, 50, 60]);

  // -inf, +inf with offset & count
  let mut spec3 = parse_range_spec("-inf", "+inf")?;
  spec3.offset = 2;
  spec3.count = Some(3);
  let r3 = db.si_range_by_value("sikey", spec3)?;
  assert_eq!(r3, vec![30, 40, 50]);

  // Reversed range by value
  let mut spec_rev = parse_range_spec("20", "60")?;
  spec_rev.reversed = true;
  let r_rev = db.si_range_by_value("sikey", spec_rev)?;
  assert_eq!(r_rev, vec![60, 50, 40, 30, 20]);

  Ok(())
}

#[test]
fn test_sortedint_rank_revrank_and_count() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.si_add("rank_k", &[10, 20, 30, 40, 50])?;

  assert_eq!(db.si_rank("rank_k", 10)?, Some(0));
  assert_eq!(db.si_rank("rank_k", 30)?, Some(2));
  assert_eq!(db.si_rank("rank_k", 50)?, Some(4));
  assert_eq!(db.si_rank("rank_k", 25)?, None);
  assert_eq!(db.si_rank("rank_k", 99)?, None);

  assert_eq!(db.si_revrank("rank_k", 50)?, Some(0));
  assert_eq!(db.si_revrank("rank_k", 30)?, Some(2));
  assert_eq!(db.si_revrank("rank_k", 10)?, Some(4));
  assert_eq!(db.si_revrank("rank_k", 99)?, None);

  let spec = parse_range_spec("20", "40")?;
  assert_eq!(db.si_count("rank_k", spec)?, 3);

  let spec_ex = parse_range_spec("(20", "(40")?;
  assert_eq!(db.si_count("rank_k", spec_ex)?, 1);

  let spec_all = parse_range_spec("-inf", "+inf")?;
  assert_eq!(db.si_count("rank_k", spec_all)?, 5);

  let spec_none = parse_range_spec("100", "200")?;
  assert_eq!(db.si_count("rank_k", spec_none)?, 0);

  Ok(())
}

#[test]
fn test_sortedint_rem_range_by_value_and_rank() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.si_add("rem_k", &[10, 20, 30, 40, 50, 60, 70, 80])?;

  // Remove by value (20, 50] -> 30, 40, 50 removed (3 items)
  let spec = parse_range_spec("(20", "50")?;
  assert_eq!(db.si_rem_range_by_value("rem_k", spec)?, 3);
  assert_eq!(db.si_card("rem_k")?, 5);
  assert_eq!(
    db.si_range("rem_k", 0, 0, 10, false)?,
    vec![10, 20, 60, 70, 80]
  );

  // Remove by rank 0, 1 -> 10, 20 removed (2 items)
  assert_eq!(db.si_rem_range_by_rank("rem_k", (0, 1))?, 2);
  assert_eq!(db.si_card("rem_k")?, 3);
  assert_eq!(db.si_range("rem_k", 0, 0, 10, false)?, vec![60, 70, 80]);

  // Remove by negative rank -1, -1 -> 80 removed (1 item)
  assert_eq!(db.si_rem_range_by_rank("rem_k", (-1, -1))?, 1);
  assert_eq!(db.si_card("rem_k")?, 2);
  assert_eq!(db.si_range("rem_k", 0, 0, 10, false)?, vec![60, 70]);

  // Remove remaining elements: full cleanup check
  assert_eq!(db.si_rem_range_by_rank("rem_k", (0, -1))?, 2);
  assert_eq!(db.si_card("rem_k")?, 0);
  assert!(!db.si_exists("rem_k", 60)?);
  assert_eq!(db.si_mexist("rem_k", &[60, 70])?, vec![false, false]);

  Ok(())
}

#[test]
fn test_sortedint_duplicates_and_64bit_boundaries() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Duplicates within the same add call
  assert_eq!(db.si_add("dup_k", &[100, 100, 200, 100, 300, 200])?, 3);
  assert_eq!(db.si_card("dup_k")?, 3);

  // Single item add optimization path
  assert_eq!(db.si_add("dup_k", &[400])?, 1);
  assert_eq!(db.si_add("dup_k", &[400])?, 0);
  assert_eq!(db.si_card("dup_k")?, 4);

  // Single item rem optimization path
  assert_eq!(db.si_rem("dup_k", &[400])?, 1);
  assert_eq!(db.si_rem("dup_k", &[400])?, 0);
  assert_eq!(db.si_card("dup_k")?, 3);

  // Duplicates within the same rem call
  assert_eq!(db.si_rem("dup_k", &[100, 100, 200, 100])?, 2);
  assert_eq!(db.si_card("dup_k")?, 1);

  // Large 64-bit integers & boundary ordering
  let large_ids = [
    0u64,
    1,
    1000,
    1 << 32,
    1 << 60,
    (1 << 63) - 1,
    1 << 63,
    u64::MAX,
  ];
  assert_eq!(db.si_add("large_k", &large_ids)?, 8);
  assert_eq!(db.si_card("large_k")?, 8);

  let range_all = db.si_range("large_k", 0, 0, 100, false)?;
  assert_eq!(range_all, large_ids.to_vec());

  let rev_all = db.si_rev_range("large_k", 0, 0, 100)?;
  let mut expected_rev = large_ids.to_vec();
  expected_rev.reverse();
  assert_eq!(rev_all, expected_rev);

  // Non-existent key operations
  assert_eq!(db.si_card("non_exist")?, 0);
  assert_eq!(db.si_mexist("non_exist", &[1, 2])?, vec![false, false]);
  assert_eq!(
    db.si_range("non_exist", 0, 0, 10, false)?,
    Vec::<u64>::new()
  );
  assert_eq!(db.si_members("non_exist")?, Vec::<u64>::new());
  assert_eq!(db.si_rem("non_exist", &[1, 2])?, 0);
  assert_eq!(db.si_rank("non_exist", 10)?, None);
  assert_eq!(db.si_revrank("non_exist", 10)?, None);
  assert_eq!(db.si_rem_range_by_rank("non_exist", (0, 10))?, 0);

  Ok(())
}

#[test]
fn test_sortedint_edge_cases_and_clear() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. limit = 0 and count = 0 edge conditions
  db.si_add("edge_k", &[10, 20, 30, 40, 50])?;
  assert_eq!(db.si_range("edge_k", 0, 0, 0, false)?, Vec::<u64>::new());
  assert_eq!(db.si_rev_range("edge_k", 0, 0, 0)?, Vec::<u64>::new());

  let mut spec_count_0 = parse_range_spec("10", "50")?;
  spec_count_0.count = Some(0);
  assert_eq!(
    db.si_range_by_value("edge_k", spec_count_0)?,
    Vec::<u64>::new()
  );
  assert_eq!(
    db.si_rev_range_by_value("edge_k", spec_count_0)?,
    Vec::<u64>::new()
  );

  // 2. Empty range cases (min > max, min == max with exclusive)
  let spec_min_gt_max = parse_range_spec("100", "50")?;
  assert_eq!(
    db.si_range_by_value("edge_k", spec_min_gt_max)?,
    Vec::<u64>::new()
  );
  assert_eq!(
    db.si_rev_range_by_value("edge_k", spec_min_gt_max)?,
    Vec::<u64>::new()
  );

  let spec_ex_eq = parse_range_spec("(30", "30")?;
  assert_eq!(
    db.si_range_by_value("edge_k", spec_ex_eq)?,
    Vec::<u64>::new()
  );
  let spec_eq_ex = parse_range_spec("30", "(30")?;
  assert_eq!(
    db.si_range_by_value("edge_k", spec_eq_ex)?,
    Vec::<u64>::new()
  );

  // Extreme boundaries
  let spec_max_ex = SortedintRange::all().with_min(u64::MAX, true);
  assert_eq!(
    db.si_range_by_value("edge_k", spec_max_ex)?,
    Vec::<u64>::new()
  );
  let spec_zero_ex = SortedintRange::all().with_max(0, true);
  assert_eq!(
    db.si_range_by_value("edge_k", spec_zero_ex)?,
    Vec::<u64>::new()
  );

  // 3. Sliding window reverse pagination tests with various offset & limit combinations
  assert_eq!(db.si_rev_range("edge_k", 0, 0, 2)?, vec![50, 40]);
  assert_eq!(db.si_rev_range("edge_k", 0, 2, 2)?, vec![30, 20]);
  assert_eq!(db.si_rev_range("edge_k", 0, 4, 2)?, vec![10]);
  assert_eq!(db.si_rev_range("edge_k", 0, 10, 2)?, Vec::<u64>::new());

  // Reversed range by value with offset & count
  let mut spec_rev_page = parse_range_spec("20", "50")?;
  spec_rev_page.reversed = true;
  spec_rev_page.offset = 1;
  spec_rev_page.count = Some(2);
  assert_eq!(db.si_range_by_value("edge_k", spec_rev_page)?, vec![40, 30]);

  // 4. si_rem_range_by_value test
  let spec_rem = parse_range_spec("20", "40")?;
  assert_eq!(db.si_rem_range_by_value("edge_k", spec_rem)?, 3);
  assert_eq!(db.si_card("edge_k")?, 2);
  assert_eq!(db.si_range("edge_k", 0, 0, 10, false)?, vec![10, 50]);

  // 5. del full cleanup test
  assert_eq!(db.del(&["edge_k"])?, 1);
  assert_eq!(db.si_card("edge_k")?, 0);
  assert!(!db.si_exists("edge_k", 10)?);
  assert!(!db.si_exists("edge_k", 50)?);
  assert_eq!(db.si_range("edge_k", 0, 0, 10, false)?, Vec::<u64>::new());
  assert_eq!(db.del(&["edge_k"])?, 0);

  Ok(())
}

#[test]
fn test_sortedint_advanced_features_and_builder() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.si_add("adv_k", &[10, 20, 30, 40, 50, 60, 70])?;

  // Builder test
  let spec_builder = SortedintRange::all().with_offset(2).with_count(3);
  assert_eq!(
    db.si_range_by_value("adv_k", spec_builder)?,
    vec![30, 40, 50]
  );

  let spec_builder_rev = SortedintRange::all()
    .with_offset(1)
    .with_count(2)
    .with_reversed(true);
  assert_eq!(
    db.si_range_by_value("adv_k", spec_builder_rev)?,
    vec![60, 50]
  );

  // Reverse without count (unlimited)
  let mut spec_rev_unlimited = SortedintRange::all().with_reversed(true);
  spec_rev_unlimited.offset = 3;
  assert_eq!(
    db.si_range_by_value("adv_k", spec_rev_unlimited)?,
    vec![40, 30, 20, 10]
  );

  // Empty del test
  assert_eq!(db.del(&["non_existent_key"])?, 0);

  Ok(())
}

#[test]
fn test_sortedint_streaming_iter_and_namespace() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. Streaming iterator test
  db.si_add("iter_k", &[10, 20, 30, 40, 50])?;
  let mut collected = Vec::new();
  db.si_iter("iter_k", |id| {
    collected.push(id);
    true
  })?;
  assert_eq!(collected, vec![10, 20, 30, 40, 50]);

  // Early termination in streaming iterator
  let mut stopped_early = Vec::new();
  db.si_iter("iter_k", |id| {
    stopped_early.push(id);
    id < 30
  })?;
  assert_eq!(stopped_early, vec![10, 20, 30]);

  // 2. Multi-tenant namespace isolation test
  let tenant1 = db.wedb().ns(1)?.db(0)?;
  let tenant2 = db.wedb().ns(2)?.db(0)?;

  assert_eq!(tenant1.si_add("ns_key", &[100, 200, 300])?, 3);
  assert_eq!(tenant2.si_add("ns_key", &[300, 400])?, 2);
  assert_eq!(db.si_add("ns_key", &[500, 600])?, 2);

  assert_eq!(tenant1.si_card("ns_key")?, 3);
  assert_eq!(tenant2.si_card("ns_key")?, 2);
  assert_eq!(db.si_card("ns_key")?, 2);

  assert!(tenant1.si_exists("ns_key", 100)?);
  assert!(!tenant2.si_exists("ns_key", 100)?);
  assert!(tenant2.si_exists("ns_key", 400)?);

  assert_eq!(tenant1.si_members("ns_key")?, vec![100, 200, 300]);
  assert_eq!(tenant2.si_members("ns_key")?, vec![300, 400]);
  assert_eq!(db.si_members("ns_key")?, vec![500, 600]);

  let spec_ns = SortedintRange::all().with_min(150, false);
  assert_eq!(
    tenant1.si_range_by_value("ns_key", spec_ns)?,
    vec![200, 300]
  );

  assert_eq!(tenant1.si_rem("ns_key", &[100])?, 1);
  assert_eq!(tenant1.si_card("ns_key")?, 2);
  assert_eq!(tenant2.si_card("ns_key")?, 2);

  assert_eq!(tenant1.del(&["ns_key"])?, 1);
  assert_eq!(tenant1.si_card("ns_key")?, 0);
  assert_eq!(tenant2.si_card("ns_key")?, 2);

  Ok(())
}

#[test]
fn test_sortedint_large_dataset_and_cursor_range() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 插入 1000 个递增整数：10, 20, 30, ..., 10000
  let ids: Vec<u64> = (1..=1000).map(|i| i * 10).collect();
  assert_eq!(db.si_add("scale_k", &ids)?, 1000);
  assert_eq!(db.si_card("scale_k")?, 1000);

  // 1. 正序 cursor 查找：cursor = 5000, offset = 0, limit = 5 -> 应该返回 5010, 5020, 5030, 5040, 5050
  let r_cursor = db.si_range("scale_k", 5000, 0, 5, false)?;
  assert_eq!(r_cursor, vec![5010, 5020, 5030, 5040, 5050]);

  // 正序 cursor + offset 查找：cursor = 5000, offset = 2, limit = 3 -> 5030, 5040, 5050
  let r_cursor_off = db.si_range("scale_k", 5000, 2, 3, false)?;
  assert_eq!(r_cursor_off, vec![5030, 5040, 5050]);

  // 2. 逆序 cursor 查找：cursor = 5000, offset = 0, limit = 5 -> 应该返回 4990, 4980, 4970, 4960, 4950
  let rev_cursor = db.si_rev_range("scale_k", 5000, 0, 5)?;
  assert_eq!(rev_cursor, vec![4990, 4980, 4970, 4960, 4950]);

  // 逆序 cursor + offset 查找：cursor = 5000, offset = 3, limit = 2 -> 4960, 4950
  let rev_cursor_off = db.si_rev_range("scale_k", 5000, 3, 2)?;
  assert_eq!(rev_cursor_off, vec![4960, 4950]);

  // 3. 范围查询：[4980, 5020]
  let spec_mid = parse_range_spec("4980", "5020")?;
  assert_eq!(
    db.si_range_by_value("scale_k", spec_mid)?,
    vec![4980, 4990, 5000, 5010, 5020]
  );

  // 逆序范围查询：[4980, 5020]
  let mut spec_mid_rev = parse_range_spec("4980", "5020")?;
  spec_mid_rev.reversed = true;
  assert_eq!(
    db.si_range_by_value("scale_k", spec_mid_rev)?,
    vec![5020, 5010, 5000, 4990, 4980]
  );

  // 统计范围元素数量
  assert_eq!(db.si_count("scale_k", spec_mid)?, 5);

  // 删除范围 [4980, 5020]
  assert_eq!(db.si_rem_range_by_value("scale_k", spec_mid)?, 5);
  assert_eq!(db.si_card("scale_k")?, 995);
  assert_eq!(db.si_count("scale_k", spec_mid)?, 0);

  Ok(())
}

#[test]
fn test_sortedint_expiration_and_recreation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 插入初始数据
  db.si_add("exp_k", &[10, 20, 30])?;
  assert_eq!(db.si_card("exp_k")?, 3);

  // 模拟过期元数据：设置 expire_at = 1 (毫秒戳，远早于当前时间)
  let meta_k = compose_si_meta_key(&db.kc(), b"exp_k");
  let expired_meta = SortedintMeta::new(1, 1, 3);
  db.meta().insert(&meta_k, &expired_meta.encode())?;

  // 过期后读取应视为空
  assert_eq!(db.si_card("exp_k")?, 0);
  assert_eq!(db.si_members("exp_k")?, Vec::<u64>::new());
  assert!(!db.si_exists("exp_k", 10)?);

  // 重新写入：旧残留数据应被清理并成功建立新集合
  assert_eq!(db.si_add("exp_k", &[10, 40, 50])?, 3);
  assert_eq!(db.si_card("exp_k")?, 3);
  assert_eq!(db.si_members("exp_k")?, vec![10, 40, 50]);
  assert!(!db.si_exists("exp_k", 20)?);
  assert!(!db.si_exists("exp_k", 30)?);

  Ok(())
}

#[test]
fn test_sortedint_wrongtype_checks() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 原生 String 键类型冲突
  db.set("str_key", "hello", [])?;
  assert!(db.si_add("str_key", &[10, 20]).is_err());
  assert!(db.si_rem("str_key", &[10]).is_err());
  assert!(db.si_card("str_key").is_err());
  assert!(db.si_exists("str_key", 10).is_err());
  assert!(db.si_mexist("str_key", &[10, 20]).is_err());
  assert!(db.si_members("str_key").is_err());
  assert!(db.si_range("str_key", 0, 0, 10, false).is_err());
  let spec = parse_range_spec("-inf", "+inf")?;
  assert!(db.si_range_by_value("str_key", spec).is_err());
  assert!(db.si_count("str_key", spec).is_err());

  // 2. Hash 键类型冲突
  db.hset("h_key", &[("field", "val")])?;
  assert!(db.si_add("h_key", &[10]).is_err());
  assert!(db.si_card("h_key").is_err());
  assert!(db.si_members("h_key").is_err());

  // 3. Set 键类型冲突
  db.sadd("s_key", &[b"mem1".as_slice()])?;
  assert!(db.si_add("s_key", &[10]).is_err());
  assert!(db.si_card("s_key").is_err());

  // 4. ZSet 键类型冲突
  db.zadd("z_key", &[(100.0, b"member".as_slice())], [])?;
  assert!(db.si_add("z_key", &[10]).is_err());
  assert!(db.si_card("z_key").is_err());

  Ok(())
}

#[test]
fn test_sortedint_binary_keys() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let bin_key = b"\x00my\xff\xfe\x00key\x01";

  assert_eq!(db.si_add(bin_key, &[10, 20, 30])?, 3);
  assert_eq!(db.si_card(bin_key)?, 3);
  assert!(db.si_exists(bin_key, 20)?);
  assert!(!db.si_exists(bin_key, 99)?);
  assert_eq!(db.si_members(bin_key)?, vec![10, 20, 30]);

  let mexist = db.si_mexist(bin_key, &[10, 25, 30])?;
  assert_eq!(mexist, vec![true, false, true]);

  assert_eq!(db.si_rem(bin_key, &[20])?, 1);
  assert_eq!(db.si_card(bin_key)?, 2);
  assert_eq!(db.si_members(bin_key)?, vec![10, 30]);

  assert_eq!(db.del(&[bin_key])?, 1);
  assert_eq!(db.si_card(bin_key)?, 0);

  Ok(())
}

#[test]
fn test_sortedint_range_spec_parsing_errors() -> Void {
  // 1. +inf on min or -inf on max
  let err1 = parse_range_spec("+inf", "10").unwrap_err();
  assert!(err1.to_string().contains("ERR min > max"));

  let err2 = parse_range_spec("10", "-inf").unwrap_err();
  assert!(err2.to_string().contains("ERR min > max"));

  let err3 = parse_range_spec("+inf", "-inf").unwrap_err();
  assert!(err3.to_string().contains("ERR min > max"));

  // 2. non-integer on min or max
  let err4 = parse_range_spec("abc", "10").unwrap_err();
  assert!(err4.to_string().contains("ERR the min isn't integer"));

  let err5 = parse_range_spec("10", "xyz").unwrap_err();
  assert!(err5.to_string().contains("ERR the max isn't integer"));

  let err6 = parse_range_spec("(abc", "10").unwrap_err();
  assert!(err6.to_string().contains("ERR the min isn't integer"));

  let err7 = parse_range_spec("10", "(xyz").unwrap_err();
  assert!(err7.to_string().contains("ERR the max isn't integer"));

  // Negative numbers for unsigned 64-bit int
  let err8 = parse_range_spec("-10", "10").unwrap_err();
  assert!(err8.to_string().contains("ERR the min isn't integer"));

  let err9 = parse_range_spec("10", "-5").unwrap_err();
  assert!(err9.to_string().contains("ERR the max isn't integer"));

  Ok(())
}

#[test]
fn test_sortedint_fast_path_and_boundary_pruning() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试全范围 rank 删除快路径
  db.si_add("si_full_rank", &[1, 2, 3, 4, 5])?;
  assert_eq!(db.si_card("si_full_rank")?, 5);
  assert_eq!(db.si_rem_range_by_rank("si_full_rank", (0, 4))?, 5);
  assert_eq!(db.si_card("si_full_rank")?, 0);

  // 2. 测试全范围 value 删除快路径
  db.si_add("si_full_val", &[10, 20, 30])?;
  assert_eq!(db.si_card("si_full_val")?, 3);
  let all_spec = SortedintRange::all();
  assert_eq!(db.si_rem_range_by_value("si_full_val", all_spec)?, 3);
  assert_eq!(db.si_card("si_full_val")?, 0);

  // 3. 测试 Bound::Excluded 精确边界过滤
  db.si_add("si_bound", &[100, 200, 300, 400, 500])?;
  let spec_ex = SortedintRange::default()
    .with_min(200, true)
    .with_max(400, true);
  let res_ex = db.si_range_by_value("si_bound", spec_ex)?;
  assert_eq!(res_ex, vec![300]);
  assert_eq!(db.si_count("si_bound", spec_ex)?, 1);

  // 4. 测试 si_rank 在不存在元素时的早停行为
  assert_eq!(db.si_rank("si_bound", 250)?, None);
  assert_eq!(db.si_rank("si_bound", 300)?, Some(2));
  assert_eq!(db.si_revrank("si_bound", 300)?, Some(2));

  Ok(())
}
