use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  stream::{
    NextStreamEntryIdStrategy, StreamAdd, StreamAutoClaim, StreamClaim, StreamConsumerGroupMeta,
    StreamConsumerMeta, StreamId, StreamMeta, StreamPending, StreamRange, StreamTrim,
    check_lag_valid,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_stream_id_and_strategy() -> Void {
  // 基础 StreamId 测试
  let mut id = StreamId::new(100, 0);
  assert_eq!(id.to_string_id(), "100-0");
  id.increment()?;
  assert_eq!(id, StreamId::new(100, 1));

  let parsed = StreamId::parse("200-5")?;
  assert_eq!(parsed, StreamId::new(200, 5));

  let (r_start, ex_s) = StreamId::parse_range_start("(100-0")?;
  assert!(ex_s);
  assert_eq!(r_start, StreamId::new(100, 0));

  let (r_end, ex_e) = StreamId::parse_range_end("[200")?;
  assert!(!ex_e);
  assert_eq!(r_end, StreamId::new(200, u64::MAX));

  // 边界与自增溢出校验
  let mut max_seq_id = StreamId::new(100, u64::MAX);
  max_seq_id.increment()?;
  assert_eq!(max_seq_id, StreamId::new(101, 0));

  let mut max_id = StreamId::max();
  assert!(max_id.increment().is_err());
  assert_eq!(max_id, StreamId::min());

  // 策略测试
  let strat_auto = NextStreamEntryIdStrategy::parse("*")?;
  let gen_id = strat_auto.generate_id(StreamId::new(100, 5), 1000)?;
  assert_eq!(gen_id, StreamId::new(1000, 0));

  let strat_any_seq = NextStreamEntryIdStrategy::parse("100-*")?;
  let gen_id2 = strat_any_seq.generate_id(StreamId::new(100, 5), 1000)?;
  assert_eq!(gen_id2, StreamId::new(100, 6));

  let strat_spec_seq = NextStreamEntryIdStrategy::parse("*-10")?;
  let gen_id3 = strat_spec_seq.generate_id(StreamId::new(500, 0), 1000)?;
  assert_eq!(gen_id3, StreamId::new(1000, 10));

  Ok(())
}

#[test]
fn test_stream_meta_encoding_122b() -> Void {
  let mut meta = StreamMeta::new(5000, 12345);
  meta.last_generated_id = StreamId::new(100, 1);
  meta.recorded_first_entry_id = StreamId::new(50, 0);
  meta.max_deleted_entry_id = StreamId::new(80, 0);
  meta.first_entry_id = StreamId::new(50, 0);
  meta.last_entry_id = StreamId::new(100, 1);
  meta.entries_added = 10;
  meta.group_number = 2;
  meta.base.size = 6;

  assert_eq!(StreamMeta::ENCODED_SIZE, 122);
  let bytes = meta.encode();
  assert_eq!(bytes.len(), 122);

  let decoded = StreamMeta::decode(&bytes).expect("decode failed");
  assert_eq!(decoded, meta);

  // 消费者组与消费者元数据编码测试
  let group_meta = StreamConsumerGroupMeta {
    consumer_number: 3,
    pending_number: 7,
    last_delivered_id: StreamId::new(99, 0),
    entries_read: 5,
    lag: 1,
  };
  assert_eq!(StreamConsumerGroupMeta::ENCODED_SIZE, 48);
  let g_bytes = group_meta.encode();
  assert_eq!(StreamConsumerGroupMeta::decode(&g_bytes), Some(group_meta));

  let consumer_meta = StreamConsumerMeta {
    pending_number: 2,
    last_attempted_interaction_ms: 1000,
    last_successful_interaction_ms: 1000,
  };
  assert_eq!(StreamConsumerMeta::ENCODED_SIZE, 24);
  let c_bytes = consumer_meta.encode();
  assert_eq!(StreamConsumerMeta::decode(&c_bytes), Some(consumer_meta));

  Ok(())
}

#[test]
fn test_stream_xadd_xlen_xrange() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let id1 = db.xadd(
    "mystream",
    Some(StreamId::new(1000, 0)),
    &[("sensor", "1"), ("temp", "25")],
  )?;
  assert_eq!(id1, StreamId::new(1000, 0));

  let id2 = db.xadd(
    "mystream",
    Some(StreamId::new(1000, 1)),
    &[("sensor", "2"), ("temp", "26")],
  )?;
  assert_eq!(id2, StreamId::new(1000, 1));

  let id3 = db.xadd(
    "mystream",
    Some(StreamId::new(2000, 0)),
    &[("sensor", "3"), ("temp", "27")],
  )?;
  assert_eq!(id3, StreamId::new(2000, 0));

  assert_eq!(db.xlen("mystream")?, 3);
  assert_eq!(db.xlast_id("mystream")?, StreamId::new(2000, 0));

  // 递增验证：不能添加更小的 ID
  assert!(
    db.xadd("mystream", Some(StreamId::new(1500, 0)), &[("k", "v")])
      .is_err()
  );

  let entries = db.xrange("mystream", (StreamId::new(1000, 0), StreamId::new(1000, 1)))?;
  assert_eq!(entries.len(), 2);
  assert_eq!(entries[0].0, StreamId::new(1000, 0));
  assert_eq!(entries[1].0, StreamId::new(1000, 1));

  let all_entries = db.xrange("mystream", (StreamId::min(), StreamId::max(), 2))?;
  assert_eq!(all_entries.len(), 2);

  let rev_entries = db.xrevrange("mystream", (StreamId::max(), StreamId::min(), 2))?;
  assert_eq!(rev_entries.len(), 2);
  assert_eq!(rev_entries[0].0, StreamId::new(2000, 0));

  // 多流联合 XREAD 测试
  db.xadd("s2", Some(StreamId::new(3000, 0)), &[("s2k", "s2v")])?;
  let multi_read = db.xread_streams(
    &[
      ("mystream", StreamId::new(1000, 0)),
      ("s2", StreamId::min()),
    ],
    Some(10),
  )?;
  assert_eq!(multi_read.len(), 2);
  assert_eq!(multi_read[0].name, "mystream");
  assert_eq!(multi_read[0].entries.len(), 2);
  assert_eq!(multi_read[1].name, "s2");
  assert_eq!(multi_read[1].entries.len(), 1);

  Ok(())
}

#[test]
fn test_stream_xtrim_and_xdel() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=10 {
    db.xadd(
      "s_trim",
      Some(StreamId::new(i * 100, 0)),
      &[("idx", &format!("{i}"))],
    )?;
  }
  assert_eq!(db.xlen("s_trim")?, 10);

  // MAXLEN 裁剪至 5 条
  let deleted = db.xtrim("s_trim", StreamTrim::maxlen(5))?;
  assert_eq!(deleted, 5);
  assert_eq!(db.xlen("s_trim")?, 5);

  let info = db.xinfo_stream("s_trim", false, None)?;
  assert_eq!(info.size, 5);
  assert_eq!(info.first_entry.unwrap().0, StreamId::new(600, 0));
  assert_eq!(info.last_entry.unwrap().0, StreamId::new(1000, 0));

  // XDEL 同时删除首条、中间和尾条
  let xdel_cnt = db.xdel(
    "s_trim",
    &[
      StreamId::new(600, 0),
      StreamId::new(800, 0),
      StreamId::new(1000, 0),
    ],
  )?;
  assert_eq!(xdel_cnt, 3);
  assert_eq!(db.xlen("s_trim")?, 2);

  let info2 = db.xinfo_stream("s_trim", false, None)?;
  assert_eq!(info2.first_entry.unwrap().0, StreamId::new(700, 0));
  assert_eq!(info2.last_entry.unwrap().0, StreamId::new(900, 0));
  assert_eq!(info2.max_deleted_entry_id, StreamId::new(1000, 0));

  Ok(())
}

#[test]
fn test_stream_consumer_groups_full_lifecycle() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=5 {
    db.xadd(
      "s_group",
      Some(StreamId::new(i * 100, 0)),
      &[("msg", &format!("val_{i}"))],
    )?;
  }

  // 1. 创建消费组
  db.xgroup_create("s_group", "g1", "0-0", false, Some(0))?;
  let groups = db.xinfo_groups("s_group")?;
  assert_eq!(groups.len(), 1);
  assert_eq!(groups[0].0, "g1");

  // 2. 创建 Consumer
  db.xgroup_create_consumer("s_group", "g1", "c1")?;
  let consumers = db.xinfo_consumers("s_group", "g1")?;
  assert_eq!(consumers.len(), 1);
  assert_eq!(consumers[0].0, "c1");

  // 3. XREADGROUP 读取最新未交付消息
  let read_entries = db.xreadgroup("s_group", "g1", "c1", ">", Some(2), false)?;
  assert_eq!(read_entries.len(), 2);
  assert_eq!(read_entries[0].0, StreamId::new(100, 0));
  assert_eq!(read_entries[1].0, StreamId::new(200, 0));

  // 多流 XREADGROUP 测试
  let multi_group_read = db.xreadgroup_streams("g1", "c1", &[("s_group", ">")], Some(1), false)?;
  assert_eq!(multi_group_read.len(), 1);
  assert_eq!(multi_group_read[0].entries[0].0, StreamId::new(300, 0));

  // 4. XPENDING 摘要与范围查询
  let pending_sum = db.xpending_summary("s_group", "g1")?;
  assert_eq!(pending_sum.pending_number, 3);
  assert_eq!(pending_sum.first_entry_id, StreamId::new(100, 0));
  assert_eq!(pending_sum.last_entry_id, StreamId::new(300, 0));

  let nacks = db.xpending_range("s_group", "g1", StreamPending::default())?;
  assert_eq!(nacks.len(), 3);

  // 5. XACK 确认第一条消息
  let ack_cnt = db.xack("s_group", "g1", &[StreamId::new(100, 0)])?;
  assert_eq!(ack_cnt, 1);
  let pending_sum2 = db.xpending_summary("s_group", "g1")?;
  assert_eq!(pending_sum2.pending_number, 2);

  // 6. XCLAIM 转移待确认条目到 c2
  db.xgroup_create_consumer("s_group", "g1", "c2")?;
  let claim_res = db.xclaim(
    "s_group",
    "g1",
    "c2",
    0,
    &[StreamId::new(200, 0)],
    StreamClaim::default(),
  )?;
  assert_eq!(claim_res.entries.len(), 1);
  assert_eq!(claim_res.entries[0].0, StreamId::new(200, 0));

  // 7. XAUTOCLAIM 自动转移
  let auto_res = db.xautoclaim(
    "s_group",
    "g1",
    "c1",
    StreamAutoClaim::new(0, StreamId::min())
      .count(10)
      .just_id(false),
  )?;
  assert_eq!(auto_res.entries.len(), 2);
  assert_eq!(auto_res.entries[0].0, StreamId::new(200, 0));
  assert_eq!(auto_res.entries[1].0, StreamId::new(300, 0));

  // 8. 销毁组
  let destroyed = db.xgroup_destroy("s_group", "g1")?;
  assert!(destroyed);
  assert_eq!(db.xinfo_groups("s_group")?.len(), 0);

  Ok(())
}

#[test]
fn test_stream_xsetid_and_lag_calc() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 在空流上 XSETID (需提供 entries_added 与 max_deleted_id)
  assert!(
    db.xsetid("s_setid", StreamId::new(1000, 0), None, None)
      .is_err()
  );
  assert!(
    db.xsetid("s_setid", StreamId::new(1000, 0), Some(10), None)
      .is_err()
  );

  db.xsetid(
    "s_setid",
    StreamId::new(1000, 0),
    Some(10),
    Some(StreamId::new(500, 0)),
  )?;
  let info = db.xinfo_stream("s_setid", false, None)?;
  assert_eq!(info.last_generated_id, StreamId::new(1000, 0));
  assert_eq!(info.entries_added, 10);
  assert_eq!(info.max_deleted_entry_id, StreamId::new(500, 0));

  // 验证已有流上 XSETID 规则
  db.xadd("s_setid", Some(StreamId::new(2000, 0)), &[("field", "val")])?;
  assert!(
    db.xsetid("s_setid", StreamId::new(1500, 0), None, None)
      .is_err()
  );

  // 验证 Lag 计算全部分支
  // 1. entries_added == 0
  let empty_stream = StreamMeta::new(0, 1);
  let mut g_meta = StreamConsumerGroupMeta::default();
  check_lag_valid(&empty_stream, &mut g_meta);
  assert_eq!(g_meta.lag, 0);

  // 2. entries_read != -1 无墓碑
  let mut stream_meta = StreamMeta::new(0, 1);
  stream_meta.entries_added = 20;
  stream_meta.base.size = 10;
  stream_meta.first_entry_id = StreamId::new(100, 0);
  stream_meta.last_entry_id = StreamId::new(200, 0);

  let mut group_meta = StreamConsumerGroupMeta {
    consumer_number: 1,
    pending_number: 0,
    last_delivered_id: StreamId::new(150, 0),
    entries_read: 15,
    lag: 0,
  };
  check_lag_valid(&stream_meta, &mut group_meta);
  assert_eq!(group_meta.lag, 5); // 20 - 15 = 5

  // 3. 有墓碑时回退估算 (id == first_entry_id: entries_added - size + 1)
  let mut stream_with_tombstone = StreamMeta::new(0, 1);
  stream_with_tombstone.entries_added = 10;
  stream_with_tombstone.base.size = 5;
  stream_with_tombstone.first_entry_id = StreamId::new(100, 0);
  stream_with_tombstone.last_entry_id = StreamId::new(200, 0);
  stream_with_tombstone.max_deleted_entry_id = StreamId::min();

  let mut g_meta2 = StreamConsumerGroupMeta {
    consumer_number: 1,
    pending_number: 0,
    last_delivered_id: StreamId::new(100, 0),
    entries_read: -1,
    lag: 0,
  };
  check_lag_valid(&stream_with_tombstone, &mut g_meta2);
  assert_eq!(g_meta2.lag, 10 - (10 - 5 + 1)); // entries_read=6, lag=4

  // 4. id < first_entry_id (entries_added - size)
  let mut g_meta3 = StreamConsumerGroupMeta {
    consumer_number: 1,
    pending_number: 0,
    last_delivered_id: StreamId::new(50, 0),
    entries_read: -1,
    lag: 0,
  };
  check_lag_valid(&stream_with_tombstone, &mut g_meta3);
  assert_eq!(g_meta3.lag, 10 - (10 - 5)); // entries_read=5, lag=5

  // 5. 无效判定 (id > last_entry_id)
  let mut g_meta4 = StreamConsumerGroupMeta {
    consumer_number: 1,
    pending_number: 0,
    last_delivered_id: StreamId::new(300, 0),
    entries_read: -1,
    lag: 0,
  };
  check_lag_valid(&stream_with_tombstone, &mut g_meta4);
  assert_eq!(g_meta4.lag, u64::MAX);

  Ok(())
}

#[test]
fn test_stream_xtrim_limit_and_xautoclaim_cleanup() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=20 {
    db.xadd(
      "s_adv",
      Some(StreamId::new(i * 10, 0)),
      &[("k", &format!("v{i}"))],
    )?;
  }
  assert_eq!(db.xlen("s_adv")?, 20);

  // XTRIM with LIMIT
  let trim_opts = StreamTrim::maxlen(10).with_limit(5);
  let trimmed = db.xtrim("s_adv", trim_opts)?;
  assert_eq!(trimmed, 5);
  assert_eq!(db.xlen("s_adv")?, 15);

  let info = db.xinfo_stream("s_adv", true, Some(10))?;
  assert_eq!(info.size, 15);
  assert_eq!(info.first_entry.unwrap().0, StreamId::new(60, 0));
  assert_eq!(info.entries.len(), 10);

  // 消费组与 XAUTOCLAIM 孤立 PEL 自动清理测试
  db.xgroup_create("s_adv", "g_adv", "0-0", false, None)?;
  let read_entries = db.xreadgroup("s_adv", "g_adv", "c_adv", ">", Some(5), false)?;
  assert_eq!(read_entries.len(), 5);

  // 删除其中 2 条消息使其在 PEL 中变成孤立条目 (dangling)
  let del_cnt = db.xdel("s_adv", &[StreamId::new(60, 0), StreamId::new(70, 0)])?;
  assert_eq!(del_cnt, 2);

  // XAUTOCLAIM 应该识别并清理已删除的条目，同时转移剩余存活条目
  let auto_res = db.xautoclaim(
    "s_adv",
    "g_adv",
    "c_adv2",
    StreamAutoClaim::new(0, StreamId::min()).count(10),
  )?;
  assert_eq!(auto_res.deleted_ids.len(), 2);
  assert_eq!(auto_res.entries.len(), 3);

  // 验证消费组与消费者元数据中的 pending_number 同步扣减
  let p_sum = db.xpending_summary("s_adv", "g_adv")?;
  assert_eq!(p_sum.pending_number, 3);

  let consumers = db.xinfo_consumers("s_adv", "g_adv")?;
  let c2 = consumers.iter().find(|c| c.0 == "c_adv2").unwrap();
  assert_eq!(c2.1.pending_number, 3);

  Ok(())
}

#[test]
fn test_stream_edge_cases_and_all_cmds() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. XADD NOMKSTREAM 边缘测试
  let nomk_opt = StreamAdd::auto().nomkstream(true);
  assert!(
    db.xadd("non_existent_stream", nomk_opt, &[("k", "v")])
      .is_err()
  );

  // 2. 0-0 ID 校验
  let id_0_0_opt = StreamAdd::with_id(StreamId::new(0, 0));
  assert!(db.xadd("s_edge", id_0_0_opt, &[("k", "v")]).is_err());

  // 3. 正常添加与各种策略生成
  let id1 = db.xadd("s_edge", Some(StreamId::new(100, 1)), &[("k1", "v1")])?;
  assert_eq!(id1, StreamId::new(100, 1));

  // ID <= last_generated_id 校验
  assert!(
    db.xadd("s_edge", Some(StreamId::new(100, 1)), &[("k", "v")])
      .is_err()
  );
  assert!(
    db.xadd("s_edge", Some(StreamId::new(99, 5)), &[("k", "v")])
      .is_err()
  );

  // 特定时间戳任意序号策略 (100-*)
  let strat_any = NextStreamEntryIdStrategy::parse("100-*")?;
  let id2 = db.xadd(
    "s_edge",
    StreamAdd::with_strategy(strat_any),
    &[("k2", "v2")],
  )?;
  assert_eq!(id2, StreamId::new(100, 2));

  // 4. XRANGE 边界条件
  let empty_range = db.xrange("s_edge", (StreamId::new(200, 0), StreamId::new(100, 0)))?;
  assert!(empty_range.is_empty());

  let count_zero = db.xrange("s_edge", (StreamId::min(), StreamId::max(), 0))?;
  assert!(count_zero.is_empty());

  let ex_invalid_start = StreamRange {
    start: StreamId::max(),
    end: StreamId::max(),
    count: None,
    reverse: false,
    exclude_start: true,
    exclude_end: false,
  };
  assert!(db.xrange("s_edge", ex_invalid_start).is_err());

  let ex_invalid_end = StreamRange {
    start: StreamId::min(),
    end: StreamId::min(),
    count: None,
    reverse: false,
    exclude_start: false,
    exclude_end: true,
  };
  assert!(db.xrange("s_edge", ex_invalid_end).is_err());

  // 5. XREADGROUP NOACK 与 自身 PEL 读取
  db.xgroup_create("s_edge", "g_edge", "0-0", false, None)?;
  // NOACK 读取
  let noack_entries = db.xreadgroup("s_edge", "g_edge", "c_noack", ">", Some(1), true)?;
  assert_eq!(noack_entries.len(), 1);
  let p_sum = db.xpending_summary("s_edge", "g_edge")?;
  assert_eq!(p_sum.pending_number, 0); // NOACK 不产生 PEL

  // 带 ACK 方式读取下一条
  let ack_entries = db.xreadgroup("s_edge", "g_edge", "c_ack", ">", Some(1), false)?;
  assert_eq!(ack_entries.len(), 1);
  assert_eq!(ack_entries[0].0, StreamId::new(100, 2));

  // 读取自身 PEL 待确认消息 (start_id != ">")
  let own_pending = db.xreadgroup("s_edge", "g_edge", "c_ack", "0-0", Some(10), false)?;
  assert_eq!(own_pending.len(), 1);
  assert_eq!(own_pending[0].0, StreamId::new(100, 2));

  // 6. XCLAIM 选项验证 (JUSTID, with_retry_count, with_last_id)
  let claim_opt = StreamClaim::new(0)
    .with_retry_count(5)
    .just_id(true)
    .with_last_id(StreamId::new(500, 0));
  let claim_res = db.xclaim(
    "s_edge",
    "g_edge",
    "c_target",
    0,
    &[StreamId::new(100, 2)],
    claim_opt,
  )?;
  assert_eq!(claim_res.ids.len(), 1);
  assert_eq!(claim_res.ids[0], StreamId::new(100, 2));
  assert!(claim_res.entries.is_empty()); // just_id 不返回 payload

  // 验证 XPENDING 筛选器 (consumer 与 idle)
  let pend_opt = StreamPending::range(StreamId::min(), StreamId::max(), 10).consumer("c_target");
  let target_nacks = db.xpending_range("s_edge", "g_edge", pend_opt)?;
  assert_eq!(target_nacks.len(), 1);
  assert_eq!(target_nacks[0].pel_entry.last_delivery_count, 5);
  assert_eq!(target_nacks[0].pel_entry.consumer_name, "c_target");

  // 7. XGROUP DELCONSUMER
  let del_pel = db.xgroup_del_consumer("s_edge", "g_edge", "c_target")?;
  assert_eq!(del_pel, 1);
  let p_sum_after_del = db.xpending_summary("s_edge", "g_edge")?;
  assert_eq!(p_sum_after_del.pending_number, 0);

  Ok(())
}

#[test]
fn test_stream_zero_copy_and_extended_options() -> Void {
  use wedb_embed::stream::{decode_stream_entry_fields_borrowed, encode_stream_entry_pairs};

  let fields = [("key1", "val1"), ("field2", "hello world")];
  let encoded = encode_stream_entry_pairs(&fields);
  let borrowed = decode_stream_entry_fields_borrowed(&encoded).expect("borrowed decode failed");
  assert_eq!(borrowed.len(), 2);
  assert_eq!(borrowed[0], ("key1", "val1"));
  assert_eq!(borrowed[1], ("field2", "hello world"));

  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=5 {
    db.xadd(
      "s_ext",
      Some(StreamId::new(i * 1000, 0)),
      &[("seq", &format!("{i}"))],
    )?;
  }

  // xinfo_stream with count = Some(0) returning all entries
  let info_full_all = db.xinfo_stream("s_ext", true, Some(0))?;
  assert_eq!(info_full_all.entries.len(), 5);

  // XGROUP with non-existent stream without mkstream fails
  assert!(
    db.xgroup_create("non_existent_s", "g1", "0-0", false, None)
      .is_err()
  );

  // XGROUP with mkstream succeeds
  db.xgroup_create("non_existent_s", "g1", "0-0", true, None)?;
  assert_eq!(db.xlen("non_existent_s")?, 0);

  // XPENDING with exclude_start and exclude_end
  db.xadd("non_existent_s", Some(StreamId::new(100, 0)), &[("k", "v")])?;
  db.xadd("non_existent_s", Some(StreamId::new(200, 0)), &[("k", "v")])?;
  db.xadd("non_existent_s", Some(StreamId::new(300, 0)), &[("k", "v")])?;
  let _ = db.xreadgroup("non_existent_s", "g1", "c1", ">", Some(10), false)?;

  let pending_ex = db.xpending_range(
    "non_existent_s",
    "g1",
    StreamPending::range(StreamId::new(100, 0), StreamId::new(300, 0), 10)
      .exclude_start(true)
      .exclude_end(true),
  )?;
  assert_eq!(pending_ex.len(), 1);
  assert_eq!(pending_ex[0].id, StreamId::new(200, 0));

  Ok(())
}

#[test]
fn test_stream_point_query_and_delconsumer_errors() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let s_name = "s_point";
  let id1 = db.xadd(s_name, Some(StreamId::new(100, 0)), &[("f1", "v1")])?;
  let _id2 = db.xadd(s_name, Some(StreamId::new(200, 0)), &[("f2", "v2")])?;

  // 1. XRANGE 单点直接点查测试 (start == end)
  let p_res = db.xrange(
    s_name,
    StreamRange {
      start: id1,
      end: id1,
      count: None,
      reverse: false,
      exclude_start: false,
      exclude_end: false,
    },
  )?;
  assert_eq!(p_res.len(), 1);
  assert_eq!(p_res[0].0, id1);
  assert_eq!(p_res[0].1[0], ("f1".to_string(), "v1".to_string()));

  // exclude_start 时单点查询直接返回空
  let p_res_ex = db.xrange(
    s_name,
    StreamRange {
      start: id1,
      end: id1,
      count: None,
      reverse: false,
      exclude_start: true,
      exclude_end: false,
    },
  )?;
  assert!(p_res_ex.is_empty());

  // 不存在的 ID 单点查询返回空
  let p_res_none = db.xrange(
    s_name,
    StreamRange {
      start: StreamId::new(150, 0),
      end: StreamId::new(150, 0),
      count: None,
      reverse: false,
      exclude_start: false,
      exclude_end: false,
    },
  )?;
  assert!(p_res_none.is_empty());

  // 2. XGROUP DELCONSUMER 严格错误校验
  // 非法 key 报错
  assert!(db.xgroup_del_consumer("non_key", "g1", "c1").is_err());

  // 存在 key 但不存在 group 报错
  assert!(db.xgroup_del_consumer(s_name, "non_group", "c1").is_err());

  // 存在 group 但不存在 consumer 返回 0
  db.xgroup_create(s_name, "g1", "0-0", false, None)?;
  let del_0 = db.xgroup_del_consumer(s_name, "g1", "non_c")?;
  assert_eq!(del_0, 0);

  Ok(())
}

#[test]
fn test_stream_minid_trim_and_force_claim() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=5 {
    db.xadd(
      "s_minid",
      Some(StreamId::new(i * 100, 0)),
      &[("i", &format!("{i}"))],
    )?;
  }
  assert_eq!(db.xlen("s_minid")?, 5);

  // MINID 裁剪：保留 >= 300-0 的元素，裁剪掉 100-0 和 200-0
  let trimmed = db.xtrim("s_minid", StreamTrim::minid(StreamId::new(300, 0)))?;
  assert_eq!(trimmed, 2);
  assert_eq!(db.xlen("s_minid")?, 3);

  let info = db.xinfo_stream("s_minid", false, None)?;
  assert_eq!(info.first_entry.unwrap().0, StreamId::new(300, 0));
  assert_eq!(info.size, 3);

  // FORCE 模式认领
  db.xgroup_create("s_minid", "g1", "0-0", false, None)?;
  let claim_res = db.xclaim(
    "s_minid",
    "g1",
    "c_force",
    0,
    &[StreamId::new(300, 0)],
    StreamClaim::new(0).force(true),
  )?;
  assert_eq!(claim_res.entries.len(), 1);
  assert_eq!(claim_res.entries[0].0, StreamId::new(300, 0));

  let p_sum = db.xpending_summary("s_minid", "g1")?;
  assert_eq!(p_sum.pending_number, 1);

  Ok(())
}

#[test]
fn test_stream_dedup_and_multi_consumer_autoclaim() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 XDEL / XACK / XCLAIM 重复 ID 去重
  let id1 = db.xadd("s_dedup", Some(StreamId::new(100, 0)), &[("k", "v1")])?;
  let id2 = db.xadd("s_dedup", Some(StreamId::new(200, 0)), &[("k", "v2")])?;
  let id3 = db.xadd("s_dedup", Some(StreamId::new(300, 0)), &[("k", "v3")])?;
  assert_eq!(db.xlen("s_dedup")?, 3);

  // XGROUP + 读取产生 PEL
  db.xgroup_create("s_dedup", "g1", "0-0", false, None)?;
  let r = db.xreadgroup("s_dedup", "g1", "c1", ">", Some(3), false)?;
  assert_eq!(r.len(), 3);
  assert_eq!(db.xpending_summary("s_dedup", "g1")?.pending_number, 3);

  // 重复 ID XACK 去重验证
  let ack_cnt = db.xack("s_dedup", "g1", &[id1, id1, id1])?;
  assert_eq!(ack_cnt, 1);
  assert_eq!(db.xpending_summary("s_dedup", "g1")?.pending_number, 2);

  // 重复 ID XCLAIM 去重验证
  let claim_res = db.xclaim(
    "s_dedup",
    "g1",
    "c2",
    0,
    &[id2, id2, id2],
    StreamClaim::default(),
  )?;
  assert_eq!(claim_res.entries.len(), 1);
  assert_eq!(claim_res.entries[0].0, id2);

  // 重复 ID XDEL 去重验证
  let del_cnt = db.xdel("s_dedup", &[id3, id3, id3])?;
  assert_eq!(del_cnt, 1);
  assert_eq!(db.xlen("s_dedup")?, 2);

  // 2. 测试多消费者孤立 PEL 条目清理与 pending_number 联动扣减
  // 当前状态：c1 有 0 个 pending，c2 有 id2 (1 个 pending)。
  // 现在让 c1 读 id2（自身 PEL），再加新消息给 c1 和 c3
  let id4 = db.xadd("s_dedup", Some(StreamId::new(400, 0)), &[("k", "v4")])?;
  let id5 = db.xadd("s_dedup", Some(StreamId::new(500, 0)), &[("k", "v5")])?;
  let _ = db.xreadgroup("s_dedup", "g1", "c1", ">", Some(1), false)?; // c1 获得 id4
  let _ = db.xreadgroup("s_dedup", "g1", "c3", ">", Some(1), false)?; // c3 获得 id5

  let p_sum_before = db.xpending_summary("s_dedup", "g1")?;
  assert_eq!(p_sum_before.pending_number, 4); // c2(id2), c1(id3 dangling), c1(id4), c3(id5)

  // 删除 id4 (属于 c1) 和 id5 (属于 c3)
  let del_dangling = db.xdel("s_dedup", &[id4, id5])?;
  assert_eq!(del_dangling, 2);

  // c4 执行 XAUTOCLAIM：id3, id4 和 id5 作为已删除孤立条目被自动清理，id2 被转移至 c4
  let auto_res = db.xautoclaim(
    "s_dedup",
    "g1",
    "c4",
    StreamAutoClaim::new(0, StreamId::min()).count(10),
  )?;
  assert_eq!(auto_res.entries.len(), 1);
  assert_eq!(auto_res.entries[0].0, id2);
  assert_eq!(auto_res.deleted_ids.len(), 3); // id3, id4, id5

  // 验证各消费者 pending_number 正确扣减
  let consumers = db.xinfo_consumers("s_dedup", "g1")?;
  for (name, meta) in consumers {
    if name == "c1" || name == "c2" || name == "c3" {
      assert_eq!(
        meta.pending_number, 0,
        "consumer {name} pending should be 0"
      );
    } else if name == "c4" {
      assert_eq!(meta.pending_number, 1, "consumer c4 pending should be 1");
    }
  }
  assert_eq!(db.xpending_summary("s_dedup", "g1")?.pending_number, 1);

  Ok(())
}

#[test]
fn test_stream_full_trim_and_reverse_xrange_window() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=10 {
    db.xadd(
      "s_full",
      Some(StreamId::new(i * 100, 0)),
      &[("val", &format!("{i}"))],
    )?;
  }
  assert_eq!(db.xlen("s_full")?, 10);

  // XREVRANGE 各种 count 限制测试
  let rev_3 = db.xrevrange("s_full", (StreamId::new(800, 0), StreamId::new(300, 0), 3))?;
  assert_eq!(rev_3.len(), 3);
  assert_eq!(rev_3[0].0, StreamId::new(800, 0));
  assert_eq!(rev_3[1].0, StreamId::new(700, 0));
  assert_eq!(rev_3[2].0, StreamId::new(600, 0));

  // 全量裁剪测试 (maxlen 0)
  let trimmed = db.xtrim("s_full", StreamTrim::maxlen(0))?;
  assert_eq!(trimmed, 10);
  assert_eq!(db.xlen("s_full")?, 0);

  let info = db.xinfo_stream("s_full", false, None)?;
  assert_eq!(info.size, 0);
  assert!(info.first_entry.is_none());
  assert!(info.last_entry.is_none());
  assert_eq!(info.max_deleted_entry_id, StreamId::new(1000, 0));

  Ok(())
}
