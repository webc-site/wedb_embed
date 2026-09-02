use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, Partition, WeDb,
  api::list::compose_list_meta_key,
  key_composer::KeyComposer,
  list::{LPos, ListMeta},
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_list_metadata_and_indexing() -> Void {
  let meta = ListMeta::new(1700000000000, 100);
  assert_eq!(meta.head, ListMeta::INITIAL_INDEX);
  assert_eq!(meta.tail, ListMeta::INITIAL_INDEX);
  assert_eq!(meta.size(), 0);

  // 标准 42 字节编解码
  let enc = meta.encode();
  assert_eq!(enc.len(), ListMeta::ENCODED_SIZE);
  let dec = ListMeta::decode(&enc).expect("decode failed");
  assert_eq!(dec.head, ListMeta::INITIAL_INDEX);
  assert_eq!(dec.tail, ListMeta::INITIAL_INDEX);

  // Kvrocks 紧凑 41 字节编解码
  let kv_enc = meta.encode_kvrocks();
  assert_eq!(kv_enc.len(), ListMeta::KVROCKS_ENCODED_SIZE);
  let kv_dec = ListMeta::decode(&kv_enc).expect("decode kvrocks failed");
  assert_eq!(kv_dec.head, ListMeta::INITIAL_INDEX);
  assert_eq!(kv_dec.tail, ListMeta::INITIAL_INDEX);

  Ok(())
}

#[test]
fn test_list_push_and_pop() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert_eq!(db.rpush("lkey", &["a", "b", "c"])?, 3);
  assert_eq!(db.lpush("lkey", &["z"])?, 4);
  assert_eq!(db.llen("lkey")?, 4);

  // LPUSHX / RPUSHX
  assert_eq!(db.lpushx("lkey", &["first"])?, 5);
  assert_eq!(db.rpushx("lkey", &["last"])?, 6);
  assert_eq!(db.lpushx("nonexistent", &["val"])?, 0);
  assert_eq!(db.rpushx("nonexistent", &["val"])?, 0);

  let range = db.lrange("lkey", (0, -1))?;
  assert_eq!(
    range,
    vec![
      b"first".to_vec(),
      b"z".to_vec(),
      b"a".to_vec(),
      b"b".to_vec(),
      b"c".to_vec(),
      b"last".to_vec(),
    ]
  );

  assert_eq!(db.lpop("lkey", 1)?, vec![b"first".to_vec()]);
  assert_eq!(db.rpop("lkey", 1)?, vec![b"last".to_vec()]);
  assert_eq!(db.llen("lkey")?, 4);

  assert_eq!(
    db.lpop("lkey", 5)?,
    vec![b"z".to_vec(), b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
  );
  assert_eq!(db.llen("lkey")?, 0);
  assert_eq!(db.lpop("lkey", 1)?, Vec::<Vec<u8>>::new());

  Ok(())
}

#[test]
fn test_list_index_and_set() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("list_idx", &["v0", "v1", "v2", "v3"])?;

  assert_eq!(db.lindex("list_idx", 0)?, Some(b"v0".to_vec()));
  assert_eq!(db.lindex("list_idx", 2)?, Some(b"v2".to_vec()));
  assert_eq!(db.lindex("list_idx", -1)?, Some(b"v3".to_vec()));
  assert_eq!(db.lindex("list_idx", -4)?, Some(b"v0".to_vec()));
  assert_eq!(db.lindex("list_idx", -5)?, None);
  assert_eq!(db.lindex("list_idx", 100)?, None);

  db.lset("list_idx", 1, "v1_updated")?;
  assert_eq!(db.lindex("list_idx", 1)?, Some(b"v1_updated".to_vec()));

  db.lset("list_idx", -1, "v3_updated")?;
  assert_eq!(db.lindex("list_idx", 3)?, Some(b"v3_updated".to_vec()));

  assert!(db.lset("list_idx", 100, "out_of_range").is_err());
  assert!(db.lset("list_idx", -10, "out_of_range").is_err());
  assert!(db.lset("nonexistent_list", 0, "val").is_err());

  Ok(())
}

#[test]
fn test_list_lrange_edge_cases() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("lrkey", &["a", "b", "c", "d"])?; // len = 4

  // 各种边界与越界组合
  assert_eq!(
    db.lrange("lrkey", (0, 3))?,
    vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
  );
  assert_eq!(
    db.lrange("lrkey", (0, 100))?,
    vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
  );
  assert_eq!(
    db.lrange("lrkey", (-100, 100))?,
    vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
  );
  assert_eq!(
    db.lrange("lrkey", (-100, 1))?,
    vec![b"a".to_vec(), b"b".to_vec()]
  );
  assert_eq!(
    db.lrange("lrkey", (-2, -1))?,
    vec![b"c".to_vec(), b"d".to_vec()]
  );

  // 空集边界情况
  assert_eq!(db.lrange("lrkey", (0, -10))?, Vec::<Vec<u8>>::new());
  assert_eq!(db.lrange("lrkey", (-10, -5))?, Vec::<Vec<u8>>::new());
  assert_eq!(db.lrange("lrkey", (5, 10))?, Vec::<Vec<u8>>::new());
  assert_eq!(db.lrange("lrkey", (3, 1))?, Vec::<Vec<u8>>::new());
  assert_eq!(db.lrange("lrkey", (-1, -2))?, Vec::<Vec<u8>>::new());
  assert_eq!(
    db.lrange("nonexistent_lrkey", (0, -1))?,
    Vec::<Vec<u8>>::new()
  );

  Ok(())
}

#[test]
fn test_list_insert_and_rem_and_pos() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("lops", &["hello", "foo", "bar", "foo", "world"])?;

  // LINSERT BEFORE / AFTER
  assert_eq!(db.linsert("lops", true, "bar", "before_bar")?, 6);
  assert_eq!(db.linsert("lops", false, "bar", "after_bar")?, 7);
  assert_eq!(db.linsert("lops", true, "nonexistent", "val")?, -1);
  assert_eq!(db.linsert("nonexistent_key", true, "bar", "val")?, 0);

  // LPOS
  let pos_first = db.lpos("lops", "foo", [])?;
  assert_eq!(pos_first, vec![1]);

  let pos_rev = db.lpos("lops", "foo", [LPos::Rank(-1)])?;
  assert_eq!(pos_rev, vec![5]);

  let pos_all = db.lpos("lops", "foo", [LPos::Rank(1), LPos::Count(10)])?;
  assert_eq!(pos_all, vec![1, 5]);

  let pos_limit = db.lpos(
    "lops",
    "foo",
    [LPos::Rank(1), LPos::Count(10), LPos::MaxLen(3)],
  )?;
  assert_eq!(pos_limit, vec![1]);

  assert!(db.lpos("lops", "foo", [LPos::Rank(0)]).is_err());

  // LREM
  assert_eq!(db.lrem("lops", 1, "foo")?, 1);
  assert_eq!(db.lrem("lops", -1, "foo")?, 1);
  assert_eq!(db.lrem("lops", 0, "bar")?, 1);
  assert_eq!(db.llen("lops")?, 4);

  // LREM 删除全部
  db.rpush("del_all", &["x", "x", "x"])?;
  assert_eq!(db.lrem("del_all", 0, "x")?, 3);
  assert_eq!(db.llen("del_all")?, 0);

  Ok(())
}

#[test]
fn test_list_lmove_and_ltrim() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("src_list", &["1", "2", "3"])?;

  // 单列表轮转 (RPOPLPUSH)
  let rot = db.lmove("src_list", "src_list", false, true)?;
  assert_eq!(rot, Some(b"3".to_vec()));
  let range = db.lrange("src_list", (0, -1))?;
  assert_eq!(range, vec![b"3".to_vec(), b"1".to_vec(), b"2".to_vec()]);

  // RPOPLPUSH 别名指令
  let rpl = db.rpoplpush("src_list", "src_list")?;
  assert_eq!(rpl, Some(b"2".to_vec()));
  assert_eq!(
    db.lrange("src_list", (0, -1))?,
    vec![b"2".to_vec(), b"3".to_vec(), b"1".to_vec()]
  );

  // 双列表迁移
  let moved = db.lmove("src_list", "dst_list", true, false)?;
  assert_eq!(moved, Some(b"2".to_vec()));
  assert_eq!(db.llen("src_list")?, 2);
  assert_eq!(db.llen("dst_list")?, 1);

  // LTRIM
  db.ltrim("src_list", (0, 0))?;
  assert_eq!(db.llen("src_list")?, 1);
  assert_eq!(db.lindex("src_list", 0)?, Some(b"3".to_vec()));

  // LTRIM 清空
  db.ltrim("src_list", (5, 2))?;
  assert_eq!(db.llen("src_list")?, 0);

  Ok(())
}

#[test]
fn test_list_expiration_behavior() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("exp_list", &["e1", "e2", "e3"])?;
  assert_eq!(db.llen("exp_list")?, 3);

  // 手动写入已过期的元数据
  let kc = KeyComposer::default();
  let meta_k = compose_list_meta_key(&kc, b"exp_list");
  let mut exp_meta = ListMeta::new(1000, 1); // 过去的过期时间
  exp_meta.base.size = 3;
  db.meta().insert(&meta_k, &exp_meta.encode())?;

  // 所有只读指令应视作不存在
  assert_eq!(db.llen("exp_list")?, 0);
  assert_eq!(db.lrange("exp_list", (0, -1))?, Vec::<Vec<u8>>::new());
  assert_eq!(db.lindex("exp_list", 0)?, None);
  assert_eq!(db.lpop("exp_list", 1)?, Vec::<Vec<u8>>::new());
  assert_eq!(db.rpop("exp_list", 1)?, Vec::<Vec<u8>>::new());
  assert_eq!(db.lpos("exp_list", "e1", [])?, Vec::<i64>::new());
  assert_eq!(db.linsert("exp_list", true, "e1", "x")?, 0);
  assert_eq!(db.lrem("exp_list", 0, "e1")?, 0);
  assert_eq!(db.lpushx("exp_list", &["x"])?, 0);
  assert_eq!(db.rpushx("exp_list", &["x"])?, 0);
  assert!(db.lset("exp_list", 0, "x").is_err());
  assert_eq!(db.lmove("exp_list", "other", true, false)?, None);

  // 重新 LPUSH 应清理旧键并创建新列表
  assert_eq!(db.lpush("exp_list", &["fresh"])?, 1);
  assert_eq!(db.lrange("exp_list", (0, -1))?, vec![b"fresh".to_vec()]);

  Ok(())
}

#[test]
fn test_list_linsert_extremities() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("ins_list", &["head", "mid", "tail"])?;

  // 在 head 之前插入
  assert_eq!(db.linsert("ins_list", true, "head", "new_head")?, 4);
  assert_eq!(db.lindex("ins_list", 0)?, Some(b"new_head".to_vec()));

  // 在 tail 之后插入
  assert_eq!(db.linsert("ins_list", false, "tail", "new_tail")?, 5);
  assert_eq!(db.lindex("ins_list", -1)?, Some(b"new_tail".to_vec()));

  assert_eq!(
    db.lrange("ins_list", (0, -1))?,
    vec![
      b"new_head".to_vec(),
      b"head".to_vec(),
      b"mid".to_vec(),
      b"tail".to_vec(),
      b"new_tail".to_vec(),
    ]
  );

  Ok(())
}

#[test]
fn test_list_lmove_expired_dst() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("src", &["item1", "item2"])?;
  db.rpush("dst_exp", &["stale1", "stale2"])?;

  // 设置 dst_exp 过期
  let kc = KeyComposer::default();
  let meta_k = compose_list_meta_key(&kc, b"dst_exp");
  let mut exp_meta = ListMeta::new(1000, 1);
  exp_meta.base.size = 2;
  db.meta().insert(&meta_k, &exp_meta.encode())?;

  // LMOVE 到已过期的 dst
  let res = db.lmove("src", "dst_exp", true, false)?;
  assert_eq!(res, Some(b"item1".to_vec()));
  assert_eq!(db.llen("src")?, 1);
  assert_eq!(db.llen("dst_exp")?, 1);
  assert_eq!(db.lrange("dst_exp", (0, -1))?, vec![b"item1".to_vec()]);

  Ok(())
}

#[test]
fn test_list_ttl_and_persist() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("ttl_list", &["v1", "v2", "v3"])?;
  assert_eq!(db.ttl("ttl_list")?, -1);
  assert_eq!(db.ttl("nonexistent_list")?, -2);

  // 设置过期秒数
  assert!(db.expire("ttl_list", 300)?);
  let ttl = db.ttl("ttl_list")?;
  assert!(ttl > 0 && ttl <= 300);

  // 移除过期时间
  assert!(db.persist("ttl_list")?);
  assert_eq!(db.ttl("ttl_list")?, -1);
  assert!(!db.persist("ttl_list")?); // 已经是持久键返回 false

  // 设置绝对秒时间戳
  let future_sec = ts_::sec() + 600;
  assert!(db.expireat("ttl_list", future_sec)?);
  let ttl2 = db.ttl("ttl_list")?;
  assert!(ttl2 > 0);

  assert!(!db.expire("nonexistent_list", 100)?);
  assert!(!db.expireat("nonexistent_list", future_sec)?);
  assert!(!db.persist("nonexistent_list")?);

  Ok(())
}

#[test]
fn test_list_kvrocks_comprehensive_cases() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. LPOS 复杂组合
  db.rpush(
    "lpos_keys",
    &["a", "b", "c", "d", "b", "e", "b", "f", "b", "g"],
  )?; // 'b' at indices 1, 4, 6, 8
  assert_eq!(db.llen("lpos_keys")?, 10);

  // rank = 1
  assert_eq!(db.lpos("lpos_keys", "b", [])?, vec![1]);
  // rank = 2
  assert_eq!(db.lpos("lpos_keys", "b", [LPos::Rank(2)])?, vec![4]);
  // rank = -1 (倒数第 1 个)
  assert_eq!(db.lpos("lpos_keys", "b", [LPos::Rank(-1)])?, vec![8]);
  // rank = -2 (倒数第 2 个)
  assert_eq!(db.lpos("lpos_keys", "b", [LPos::Rank(-2)])?, vec![6]);
  // count = 0 (返回全部)
  assert_eq!(
    db.lpos("lpos_keys", "b", [LPos::Count(0)])?,
    vec![1, 4, 6, 8]
  );
  // count = 2
  assert_eq!(db.lpos("lpos_keys", "b", [LPos::Count(2)])?, vec![1, 4]);
  // rank = -1, count = 2
  assert_eq!(
    db.lpos("lpos_keys", "b", [LPos::Rank(-1), LPos::Count(2)])?,
    vec![8, 6]
  );
  // max_len 限制扫描范围
  assert_eq!(db.lpos("lpos_keys", "b", [LPos::MaxLen(3)])?, vec![1]);
  assert_eq!(
    db.lpos("lpos_keys", "b", [LPos::MaxLen(1)])?,
    Vec::<i64>::new()
  );

  // 2. LREM 大量元素两端位移测试
  db.rpush("lrem_test", &["x", "1", "2", "x", "3", "4", "x"])?;
  // 从左侧删除 1 个 x
  assert_eq!(db.lrem("lrem_test", 1, "x")?, 1);
  assert_eq!(
    db.lrange("lrem_test", (0, -1))?,
    vec![
      b"1".to_vec(),
      b"2".to_vec(),
      b"x".to_vec(),
      b"3".to_vec(),
      b"4".to_vec(),
      b"x".to_vec()
    ]
  );
  // 从右侧删除 1 个 x
  assert_eq!(db.lrem("lrem_test", -1, "x")?, 1);
  assert_eq!(
    db.lrange("lrem_test", (0, -1))?,
    vec![
      b"1".to_vec(),
      b"2".to_vec(),
      b"x".to_vec(),
      b"3".to_vec(),
      b"4".to_vec()
    ]
  );
  // 删除剩余所有 x
  assert_eq!(db.lrem("lrem_test", 0, "x")?, 1);
  assert_eq!(
    db.lrange("lrem_test", (0, -1))?,
    vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec(), b"4".to_vec()]
  );

  // 3. LTRIM 极端索引
  db.ltrim("lrem_test", (-3, -2))?; // 保留 [2, 3]
  assert_eq!(
    db.lrange("lrem_test", (0, -1))?,
    vec![b"2".to_vec(), b"3".to_vec()]
  );
  assert_eq!(db.llen("lrem_test")?, 2);

  Ok(())
}

#[test]
fn test_list_kvrocks_lmove_all_combinations() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. SrcLeftDstLeft
  db.rpush("src1", &["s1", "s2", "s3", "s4"])?;
  db.rpush("dst1", &["d1", "d2", "d3", "d4"])?;
  assert_eq!(db.lmove("src1", "dst1", true, true)?, Some(b"s1".to_vec()));
  assert_eq!(
    db.lrange("src1", (0, -1))?,
    vec![b"s2".to_vec(), b"s3".to_vec(), b"s4".to_vec()]
  );
  assert_eq!(
    db.lrange("dst1", (0, -1))?,
    vec![
      b"s1".to_vec(),
      b"d1".to_vec(),
      b"d2".to_vec(),
      b"d3".to_vec(),
      b"d4".to_vec()
    ]
  );

  // 2. SrcLeftDstRight
  db.rpush("src2", &["s1", "s2", "s3", "s4"])?;
  db.rpush("dst2", &["d1", "d2", "d3", "d4"])?;
  assert_eq!(db.lmove("src2", "dst2", true, false)?, Some(b"s1".to_vec()));
  assert_eq!(
    db.lrange("src2", (0, -1))?,
    vec![b"s2".to_vec(), b"s3".to_vec(), b"s4".to_vec()]
  );
  assert_eq!(
    db.lrange("dst2", (0, -1))?,
    vec![
      b"d1".to_vec(),
      b"d2".to_vec(),
      b"d3".to_vec(),
      b"d4".to_vec(),
      b"s1".to_vec()
    ]
  );

  // 3. SrcRightDstLeft
  db.rpush("src3", &["s1", "s2", "s3", "s4"])?;
  db.rpush("dst3", &["d1", "d2", "d3", "d4"])?;
  assert_eq!(db.lmove("src3", "dst3", false, true)?, Some(b"s4".to_vec()));
  assert_eq!(
    db.lrange("src3", (0, -1))?,
    vec![b"s1".to_vec(), b"s2".to_vec(), b"s3".to_vec()]
  );
  assert_eq!(
    db.lrange("dst3", (0, -1))?,
    vec![
      b"s4".to_vec(),
      b"d1".to_vec(),
      b"d2".to_vec(),
      b"d3".to_vec(),
      b"d4".to_vec()
    ]
  );

  // 4. SrcRightDstRight
  db.rpush("src4", &["s1", "s2", "s3", "s4"])?;
  db.rpush("dst4", &["d1", "d2", "d3", "d4"])?;
  assert_eq!(
    db.lmove("src4", "dst4", false, false)?,
    Some(b"s4".to_vec())
  );
  assert_eq!(
    db.lrange("src4", (0, -1))?,
    vec![b"s1".to_vec(), b"s2".to_vec(), b"s3".to_vec()]
  );
  assert_eq!(
    db.lrange("dst4", (0, -1))?,
    vec![
      b"d1".to_vec(),
      b"d2".to_vec(),
      b"d3".to_vec(),
      b"d4".to_vec(),
      b"s4".to_vec()
    ]
  );

  Ok(())
}

#[test]
fn test_list_kvrocks_pop_multi_excessive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // LPop count > size
  db.rpush("pop_test", &["1", "2", "3"])?;
  let popped = db.lpop("pop_test", 100)?;
  assert_eq!(popped, vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]);
  assert_eq!(db.llen("pop_test")?, 0);

  // RPop count > size
  db.rpush("pop_test_r", &["1", "2", "3"])?;
  let popped_r = db.rpop("pop_test_r", 100)?;
  assert_eq!(popped_r, vec![b"3".to_vec(), b"2".to_vec(), b"1".to_vec()]);
  assert_eq!(db.llen("pop_test_r")?, 0);

  Ok(())
}

#[test]
fn test_list_kvrocks_specific_trim_insert_rem() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let fields = [
    "0", "1", "2", "3", "4", "3", "6", "7", "3", "8", "9", "3", "9", "3", "9",
  ];
  db.rpush("spec_key", &fields)?;
  assert_eq!(db.llen("spec_key")?, 15);

  // LTRIM key 3 -3 (保留 index 3 到 12，共 10 个元素)
  db.ltrim("spec_key", (3, -3))?;
  assert_eq!(db.llen("spec_key")?, 10);
  assert_eq!(
    db.lrange("spec_key", (0, -1))?,
    vec![
      b"3".to_vec(),
      b"4".to_vec(),
      b"3".to_vec(),
      b"6".to_vec(),
      b"7".to_vec(),
      b"3".to_vec(),
      b"8".to_vec(),
      b"9".to_vec(),
      b"3".to_vec(),
      b"9".to_vec(),
    ]
  );

  // LINSERT 不存在 pivot -> -1
  assert_eq!(db.linsert("spec_key", true, "2", "3")?, -1);

  // LREM key 5 3 (删除前 5 个 "3"，实际有 4 个)
  assert_eq!(db.lrem("spec_key", 5, "3")?, 4);
  assert_eq!(db.llen("spec_key")?, 6);
  assert_eq!(
    db.lrange("spec_key", (0, -1))?,
    vec![
      b"4".to_vec(),
      b"6".to_vec(),
      b"7".to_vec(),
      b"8".to_vec(),
      b"9".to_vec(),
      b"9".to_vec(),
    ]
  );

  Ok(())
}

#[test]
fn test_list_binary_keys_and_values() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let bin_key = b"\xff\xfe\x00\x01_list";
  let bin_v1 = b"\x00\x01\x02\x03";
  let bin_v2 = b"\xff\xee\xdd\xcc";

  assert_eq!(db.rpush(bin_key, &[bin_v1, bin_v2])?, 2);
  assert_eq!(db.llen(bin_key)?, 2);
  assert_eq!(db.lindex(bin_key, 0)?, Some(bin_v1.to_vec()));
  assert_eq!(db.lindex(bin_key, 1)?, Some(bin_v2.to_vec()));
  assert_eq!(
    db.lrange(bin_key, (0, -1))?,
    vec![bin_v1.to_vec(), bin_v2.to_vec()]
  );

  assert_eq!(db.lpop(bin_key, 1)?, vec![bin_v1.to_vec()]);
  assert_eq!(db.llen(bin_key)?, 1);

  Ok(())
}

#[test]
fn test_list_wrongtype_protection() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 原生 String 存在时的冲突
  db.set("str_key", "string_value", [])?;
  assert!(db.lpush("str_key", &["elem"]).is_err());
  assert!(db.rpush("str_key", &["elem"]).is_err());
  assert!(db.llen("str_key").is_err());
  assert!(db.lrange("str_key", (0, -1)).is_err());
  assert!(db.lindex("str_key", 0).is_err());
  assert!(db.lpop("str_key", 1).is_err());
  assert!(db.rpop("str_key", 1).is_err());

  // 2. Hash 存在时的冲突
  db.hset("hash_key", &[("f1", "v1")])?;
  assert!(db.lpush("hash_key", &["elem"]).is_err());
  assert!(db.llen("hash_key").is_err());

  // 3. Set 存在时的冲突
  db.sadd("set_key", &["m1"])?;
  assert!(db.lpush("set_key", &["elem"]).is_err());
  assert!(db.llen("set_key").is_err());

  Ok(())
}

#[test]
fn test_list_extended_ttl_family() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("ext_ttl_list", &["e1", "e2"])?;
  assert_eq!(db.ttl("ext_ttl_list")?, -1);
  assert_eq!(db.pttl("ext_ttl_list")?, -1);
  assert_eq!(db.get_key_expire_at("ext_ttl_list")?, Some(0));

  // expire
  assert!(db.expire("ext_ttl_list", 50)?);
  let pttl = db.pttl("ext_ttl_list")?;
  assert!(pttl > 0 && pttl <= 50000);

  let exp_ms = db.get_key_expire_at("ext_ttl_list")?.unwrap();
  assert!(exp_ms > 0);

  // pexpireat
  let target_ms = exp_ms + 10000;
  assert!(db.pexpireat("ext_ttl_list", target_ms)?);
  assert_eq!(db.get_key_expire_at("ext_ttl_list")?, Some(target_ms));

  // key_persist
  assert!(db.persist("ext_ttl_list")?);
  assert_eq!(db.ttl("ext_ttl_list")?, -1);
  assert_eq!(db.pttl("ext_ttl_list")?, -1);
  assert_eq!(db.get_key_expire_at("ext_ttl_list")?, Some(0));

  // 不存在的 key
  assert_eq!(db.get_key_expire_at("no_such_list")?, None);
  assert_eq!(db.ttl("no_such_list")?, -2);
  assert_eq!(db.pttl("no_such_list")?, -2);

  Ok(())
}

#[test]
fn test_list_single_item_push_helpers() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert_eq!(db.rpush("single_k", &["a"])?, 1);
  assert_eq!(db.lpush("single_k", &["z"])?, 2);
  assert_eq!(db.rpushx("single_k", &["b"])?, 3);
  assert_eq!(db.lpushx("single_k", &["y"])?, 4);

  assert_eq!(db.rpushx("no_k", &["b"])?, 0);
  assert_eq!(db.lpushx("no_k", &["y"])?, 0);

  assert_eq!(
    db.lrange("single_k", (0, -1))?,
    vec![b"y".to_vec(), b"z".to_vec(), b"a".to_vec(), b"b".to_vec()]
  );

  assert_eq!(db.lpop("single_k", 1)?, vec![b"y".to_vec()]);
  assert_eq!(db.rpop("single_k", 1)?, vec![b"b".to_vec()]);
  assert_eq!(db.llen("single_k")?, 2);

  Ok(())
}

#[test]
fn test_list_lmove_self_single_elem_and_noop() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 单元素列表自我移动 (size == 1)
  db.rpush("single_elem", &["sole"])?;
  assert_eq!(
    db.lmove("single_elem", "single_elem", true, false)?,
    Some(b"sole".to_vec())
  );
  assert_eq!(db.llen("single_elem")?, 1);
  assert_eq!(db.lindex("single_elem", 0)?, Some(b"sole".to_vec()));

  // 同端到同端 (src_left == dst_left: true -> true / false -> false)
  db.rpush("multi_elem", &["1", "2", "3"])?;
  assert_eq!(
    db.lmove("multi_elem", "multi_elem", true, true)?,
    Some(b"1".to_vec())
  );
  assert_eq!(
    db.lrange("multi_elem", (0, -1))?,
    vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]
  );

  assert_eq!(
    db.lmove("multi_elem", "multi_elem", false, false)?,
    Some(b"3".to_vec())
  );
  assert_eq!(
    db.lrange("multi_elem", (0, -1))?,
    vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]
  );

  Ok(())
}

#[test]
fn test_list_lpos_count_zero_and_maxlen() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.rpush("lpos_k", &["a", "b", "c", "b", "d", "b", "e"])?;

  // 1. COUNT 0: 返回所有匹配项
  let all_matches = db.lpos("lpos_k", "b", [LPos::Count(0)])?;
  assert_eq!(all_matches, vec![1, 3, 5]);

  // 2. COUNT 2: 返回前 2 个匹配项
  let two_matches = db.lpos("lpos_k", "b", [LPos::Count(2)])?;
  assert_eq!(two_matches, vec![1, 3]);

  // 3. RANK -1 COUNT 0: 反向遍历返回所有匹配项（按从尾到头发现顺序，索引依然是绝对正向索引）
  let rev_all = db.lpos("lpos_k", "b", [LPos::Rank(-1), LPos::Count(0)])?;
  assert_eq!(rev_all, vec![5, 3, 1]);

  // 4. MAXLEN 限制扫描长度
  let maxlen_matches = db.lpos("lpos_k", "b", [LPos::Count(0), LPos::MaxLen(4)])?;
  // 仅扫描前 4 个元素 ("a", "b", "c", "b")
  assert_eq!(maxlen_matches, vec![1, 3]);

  Ok(())
}

#[test]
fn test_list_with_lindex_and_lmpop_and_kvrocks_suite() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. with_lindex 零拷贝借用读取
  db.rpush("lindex_k", &["item_zero", "item_one", "item_two"])?;
  let len0 = db.with_lindex("lindex_k", 0, |v| v.len())?;
  assert_eq!(len0, Some(9)); // "item_zero".len() == 9
  let is_two = db.with_lindex("lindex_k", -1, |v| v == b"item_two")?;
  assert_eq!(is_two, Some(true));
  let oob = db.with_lindex("lindex_k", 100, |v| v.len())?;
  assert_eq!(oob, None);
  let nonexist = db.with_lindex("nonexist_k", 0, |v| v.len())?;
  assert_eq!(nonexist, None);

  // 2. LMPOP 多键队列弹出 (Redis 7.0 / Kvrocks 对标)
  db.rpush("queue_b", &["b1", "b2", "b3"])?;
  db.rpush("queue_c", &["c1", "c2"])?;

  // 从 queue_a (不存在), queue_b, queue_c 依次探查并弹出 2 项
  let pop_res = db.lmpop(&["queue_a", "queue_b", "queue_c"], true, 2)?;
  assert!(pop_res.is_some());
  let (k, items) = pop_res.unwrap();
  assert_eq!(k, b"queue_b");
  assert_eq!(items, vec![b"b1".to_vec(), b"b2".to_vec()]);
  assert_eq!(db.llen("queue_b")?, 1);

  // 再次弹出 2 项（queue_b 仅剩 1 项，应弹出 1 项）
  let pop_res2 = db.lmpop(&["queue_a", "queue_b", "queue_c"], true, 2)?;
  assert!(pop_res2.is_some());
  let (k2, items2) = pop_res2.unwrap();
  assert_eq!(k2, b"queue_b");
  assert_eq!(items2, vec![b"b3".to_vec()]);
  assert_eq!(db.llen("queue_b")?, 0);

  // queue_b 已空，应自动落到 queue_c
  let pop_res3 = db.lmpop(&["queue_a", "queue_b", "queue_c"], false, 5)?;
  assert!(pop_res3.is_some());
  let (k3, items3) = pop_res3.unwrap();
  assert_eq!(k3, b"queue_c");
  assert_eq!(items3, vec![b"c2".to_vec(), b"c1".to_vec()]); // right pop 弹出
  assert_eq!(db.llen("queue_c")?, 0);

  // 全部为空返回 None
  let pop_none = db.lmpop(&["queue_a", "queue_b", "queue_c"], true, 2)?;
  assert!(pop_none.is_none());

  // 3. Kvrocks LPop / RPop 边界测试用例对标
  // 3.1 LPop / RPop 空列表
  assert!(db.lpop("empty_list", 1)?.is_empty());
  assert_eq!(db.lpop_one("empty_list")?, None);
  assert!(db.rpop("empty_list", 1)?.is_empty());
  assert_eq!(db.rpop_one("empty_list")?, None);

  // 3.2 LPop / RPop 单元素逐项弹出
  let fields = ["f0", "f1", "f2", "f3", "f4"];
  db.rpush("single_pop_k", &fields)?;
  for f in fields {
    assert_eq!(db.lpop_one("single_pop_k")?, Some(f.as_bytes().to_vec()));
  }
  assert_eq!(db.llen("single_pop_k")?, 0);
  assert_eq!(db.lpop_one("single_pop_k")?, None);

  db.rpush("single_rpop_k", &fields)?;
  for f in fields.iter().rev() {
    assert_eq!(db.rpop_one("single_rpop_k")?, Some(f.as_bytes().to_vec()));
  }
  assert_eq!(db.llen("single_rpop_k")?, 0);
  assert_eq!(db.rpop_one("single_rpop_k")?, None);

  // 3.3 PopMulti 数量超限测试 (PopMultiCountGreaterThanListSize)
  db.rpush("over_pop_k", &fields)?;
  let over_popped = db.lpop("over_pop_k", 100)?;
  assert_eq!(over_popped.len(), 5);
  assert_eq!(db.llen("over_pop_k")?, 0);

  db.rpush("over_rpop_k", &fields)?;
  let over_rpopped = db.rpop("over_rpop_k", 100)?;
  assert_eq!(over_rpopped.len(), 5);
  assert_eq!(over_rpopped[0], b"f4");
  assert_eq!(db.llen("over_rpop_k")?, 0);

  Ok(())
}
