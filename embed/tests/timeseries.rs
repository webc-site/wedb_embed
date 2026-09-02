use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  timeseries::{
    AggregationType, Aggregator, BucketTimestampType, ChunkType, DuplicatePolicy, GroupReducerType,
    TSChunk, TSSample, TimeSeriesLabelFilter, TsCreate, TsFilter, TsMGet, TsMRange, TsRange,
    group_samples_and_reduce,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_chunk_compression_roundtrip() -> Void {
  let mut samples = Vec::new();
  let start_ts = 1700000000000u64;
  for i in 0..100 {
    let ts = start_ts + (i * 1000) + (i % 3) * 50;
    let v = 20.0 + (i as f64) * 0.1 + ((i % 5) as f64) * 0.05;
    samples.push(TSSample::new(ts, v));
  }

  // 边界与全 64 位差异样本测试
  samples.push(TSSample::new(start_ts + 200000, 0.0));
  samples.push(TSSample::new(
    start_ts + 300000,
    f64::from_bits(0x8000000000000001),
  ));
  samples.push(TSSample::new(
    start_ts + 400000,
    f64::from_bits(0x0000000000000001),
  ));

  let compressed = TSChunk::encode_compressed(&samples);
  assert!(!compressed.is_empty());
  assert!(compressed.len() < samples.len() * 16);

  let decompressed = TSChunk::decode_samples(&compressed)?;
  assert_eq!(decompressed.len(), samples.len());
  for (orig, dec) in samples.iter().zip(decompressed.iter()) {
    assert_eq!(orig.ts, dec.ts);
    assert_eq!(orig.v.to_bits(), dec.v.to_bits());
  }

  Ok(())
}

#[test]
fn test_timeseries_basic_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.ts_create(
    "tskey",
    [
      TsCreate::DuplicatePolicy(DuplicatePolicy::Block),
      TsCreate::Labels(vec![("sensor".to_string(), "temp".to_string())]),
    ],
  )?;
  assert!(
    db.ts_create("tskey", [TsCreate::DuplicatePolicy(DuplicatePolicy::Block)],)
      .is_err()
  );

  db.ts_add("tskey", 1000, 25.5, None, [])?;
  db.ts_add("tskey", 2000, 26.0, None, [])?;
  db.ts_add("tskey", 3000, 24.8, None, [])?;

  assert_eq!(db.ts_get("tskey")?, Some((3000, 24.8)));

  let range = db.ts_range_one("tskey", (1000, 2500))?;
  assert_eq!(range, vec![(1000, 25.5), (2000, 26.0)]);

  let revrange = db.ts_revrange_one("tskey", (1000, 2500))?;
  assert_eq!(revrange, vec![(2000, 26.0), (1000, 25.5)]);

  let full_range = db.ts_range_one("tskey", (0, 10000))?;
  assert_eq!(full_range.len(), 3);

  // 测试增减操作
  db.ts_incrby("tskey", 5.0, Some(4000), None)?;
  assert_eq!(db.ts_get("tskey")?, Some((4000, 29.8)));

  db.ts_decrby("tskey", 4.8, Some(5000), None)?;
  assert_eq!(db.ts_get("tskey")?, Some((5000, 25.0)));

  // 测试删除
  let deleted = db.ts_del("tskey", (1000, 2500))?;
  assert_eq!(deleted, 2);
  let after_del = db.ts_range_one("tskey", (0, 10000))?;
  assert_eq!(after_del.len(), 3);

  // 测试 TS.INFO
  let info = db.ts_info("tskey")?;
  assert_eq!(info.total_samples, 3);
  assert_eq!(info.first_timestamp, 3000);
  assert_eq!(info.last_timestamp, 5000);

  Ok(())
}

#[test]
fn test_timeseries_compressed_chunk_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let opt = [
    TsCreate::ChunkSize(1024),
    TsCreate::DuplicatePolicy(DuplicatePolicy::Last),
    TsCreate::Labels(vec![("region".to_string(), "us-west".to_string())]),
  ];
  db.ts_create("comp_ts", opt)?;

  db.ts_add("comp_ts", 1000, 10.0, None, [])?;
  db.ts_add("comp_ts", 2000, 20.0, None, [])?;
  db.ts_add("comp_ts", 3000, 30.0, None, [])?;

  assert_eq!(db.ts_get("comp_ts")?, Some((3000, 30.0)));
  let r = db.ts_range_one("comp_ts", (1000, 3000))?;
  assert_eq!(r, vec![(1000, 10.0), (2000, 20.0), (3000, 30.0)]);

  Ok(())
}

#[test]
fn test_timeseries_aggregations() {
  let samples = vec![
    (1000, 10.0),
    (1500, 20.0),
    (2000, 30.0),
    (2500, 40.0),
    (3000, 50.0),
  ];

  let agg_avg = Aggregator::new(AggregationType::Avg, 1000, 0);
  let res_avg = agg_avg.split_and_aggregate(&samples, None, false, BucketTimestampType::Start);
  assert_eq!(res_avg.len(), 3);
  assert_eq!(res_avg[0], (1000, 15.0)); // (10 + 20) / 2
  assert_eq!(res_avg[1], (2000, 35.0)); // (30 + 40) / 2
  assert_eq!(res_avg[2], (3000, 50.0)); // 50 / 1

  let agg_sum = Aggregator::new(AggregationType::Sum, 1000, 0);
  let res_sum = agg_sum.split_and_aggregate(&samples, None, false, BucketTimestampType::Start);
  assert_eq!(res_sum[0], (1000, 30.0));
  assert_eq!(res_sum[1], (2000, 70.0));

  let agg_min = Aggregator::new(AggregationType::Min, 1000, 0);
  let res_min = agg_min.split_and_aggregate(&samples, None, false, BucketTimestampType::Start);
  assert_eq!(res_min[0], (1000, 10.0));

  let agg_max = Aggregator::new(AggregationType::Max, 1000, 0);
  let res_max = agg_max.split_and_aggregate(&samples, None, false, BucketTimestampType::Start);
  assert_eq!(res_max[0], (1000, 20.0));

  let agg_range = Aggregator::new(AggregationType::Range, 1000, 0);
  let res_range = agg_range.split_and_aggregate(&samples, None, false, BucketTimestampType::Start);
  assert_eq!(res_range[0], (1000, 10.0)); // 20 - 10

  // 测试空桶填充 (EMPTY) 与时间戳对齐 (End / Mid)
  let sparse_samples = vec![(1000, 10.0), (4000, 40.0)];
  let agg_empty = Aggregator::new(AggregationType::Sum, 1000, 0);
  let res_empty =
    agg_empty.split_and_aggregate(&sparse_samples, None, true, BucketTimestampType::Start);
  assert_eq!(res_empty.len(), 4);
  assert_eq!(res_empty[0], (1000, 10.0));
  assert_eq!(res_empty[1], (2000, 0.0));
  assert_eq!(res_empty[2], (3000, 0.0));
  assert_eq!(res_empty[3], (4000, 40.0));
}

#[test]
fn test_timeseries_label_filter_and_mget_mrange() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let opt1 = [
    TsCreate::ChunkType(ChunkType::Uncompressed),
    TsCreate::DuplicatePolicy(DuplicatePolicy::Block),
    TsCreate::Labels(vec![
      ("sensor".to_string(), "temp".to_string()),
      ("loc".to_string(), "room1".to_string()),
    ]),
  ];
  db.ts_create("ts1", opt1)?;
  db.ts_add("ts1", 1000, 22.0, None, [])?;
  db.ts_add("ts1", 2000, 23.0, None, [])?;

  let opt2 = [
    TsCreate::ChunkType(ChunkType::Uncompressed),
    TsCreate::DuplicatePolicy(DuplicatePolicy::Block),
    TsCreate::Labels(vec![
      ("sensor".to_string(), "temp".to_string()),
      ("loc".to_string(), "room2".to_string()),
    ]),
  ];
  db.ts_create("ts2", opt2)?;
  db.ts_add("ts2", 1000, 24.0, None, [])?;
  db.ts_add("ts2", 2000, 25.0, None, [])?;

  // MGET
  let mget_opt = [
    TsMGet::WithLabels,
    TsMGet::Filters(vec!["sensor=temp".to_string()]),
  ];
  let mget_res = db.ts_mget(mget_opt)?;
  assert_eq!(mget_res.len(), 2);

  // QUERYINDEX
  let idx_res = db.ts_queryindex(&["sensor=temp".to_string(), "loc=room1".to_string()])?;
  assert_eq!(idx_res, vec!["ts1"]);

  // MRANGE with GROUPBY & REDUCE
  let mrange_opt = [
    TsMRange::Filters(vec!["sensor=temp".to_string()]),
    TsMRange::GroupBy("sensor".to_string(), GroupReducerType::Avg),
  ];

  let mrange_res = db.ts_mrange((0, 5000), mrange_opt.clone())?;
  assert_eq!(mrange_res.len(), 1);
  assert_eq!(mrange_res[0].name, "sensor=temp");
  assert_eq!(
    mrange_res[0].samples,
    vec![(1000, 23.0), (2000, 24.0)] // (22+24)/2, (23+25)/2
  );

  // MREVRANGE
  let mrev_res = db.ts_mrevrange((0, 5000), mrange_opt)?;
  assert_eq!(mrev_res[0].samples, vec![(2000, 24.0), (1000, 23.0)]);

  Ok(())
}

#[test]
fn test_timeseries_chunk_and_label_filter_details() -> Void {
  // 1. TSChunk encode/decode/split
  let samples = vec![
    TSSample::new(1000, 10.0),
    TSSample::new(2000, 20.0),
    TSSample::new(3000, 30.0),
  ];
  let raw = TSChunk::encode_uncompressed(&samples);
  assert_eq!(TSChunk::get_count(&raw), 3);
  assert_eq!(TSChunk::get_first_timestamp(&raw), Some(1000));

  let decoded = TSChunk::decode_samples(&raw)?;
  assert_eq!(decoded, samples);

  let chunks = TSChunk::upsert_and_split(
    &raw,
    &[TSSample::new(4000, 40.0)],
    DuplicatePolicy::Block,
    2,
    ChunkType::Uncompressed,
  )?;
  assert_eq!(chunks.len(), 2);

  // 2. TimeSeriesLabelFilter
  let mut filter = TimeSeriesLabelFilter::new();
  filter.add_filter("sensor=(temp,humidity)");
  filter.add_filter("region!=cn-north");
  assert!(filter.matches(&[
    ("sensor".to_string(), "temp".to_string()),
    ("region".to_string(), "us-east".to_string())
  ]));
  assert!(!filter.matches(&[("sensor".to_string(), "pressure".to_string())]));

  // 2.1 标签存在性过滤 (k!= 与 k=) 与空值边界测试
  let mut exist_filter = TimeSeriesLabelFilter::new();
  exist_filter.add_filter("env!="); // 必须存在标签 env
  exist_filter.add_filter("deprecated="); // 不得存在标签 deprecated
  assert!(exist_filter.matches(&[("env".to_string(), "".to_string())])); // 合法空值依然满足必须存在
  assert!(exist_filter.matches(&[("env".to_string(), "prod".to_string())]));
  assert!(!exist_filter.matches(&[("region".to_string(), "us".to_string())])); // 缺失 env
  assert!(!exist_filter.matches(&[
    ("env".to_string(), "prod".to_string()),
    ("deprecated".to_string(), "true".to_string())
  ])); // 存在 forbidden 的 deprecated 标签

  // 2.2 同标签复合过滤 (In 与 NotIn 联用)
  let mut compound_label_filter = TimeSeriesLabelFilter::new();
  compound_label_filter.add_filter("sensor=(temp,humidity)");
  compound_label_filter.add_filter("sensor!=humidity");
  assert!(compound_label_filter.matches(&[("sensor".to_string(), "temp".to_string())]));
  assert!(!compound_label_filter.matches(&[("sensor".to_string(), "humidity".to_string())]));

  // 3. group_samples_and_reduce
  let s1 = vec![(100, 10.0), (200, 20.0)];
  let s2 = vec![(100, 30.0), (200, 40.0)];
  let red = group_samples_and_reduce(&[s1, s2], GroupReducerType::Sum);
  assert_eq!(red, vec![(100, 40.0), (200, 60.0)]);

  Ok(())
}

#[test]
fn test_all_13_aggregators() {
  let samples = vec![
    (1000, 2.0),
    (1200, 4.0),
    (1400, 4.0),
    (1600, 4.0),
    (1800, 5.0),
    (1900, 5.0),
    (1950, 7.0),
    (1990, 9.0),
  ];

  let agg_sum = Aggregator::new(AggregationType::Sum, 2000, 0);
  assert_eq!(agg_sum.aggregate_samples(&samples), 40.0);

  let agg_min = Aggregator::new(AggregationType::Min, 2000, 0);
  assert_eq!(agg_min.aggregate_samples(&samples), 2.0);

  let agg_max = Aggregator::new(AggregationType::Max, 2000, 0);
  assert_eq!(agg_max.aggregate_samples(&samples), 9.0);

  let agg_count = Aggregator::new(AggregationType::Count, 2000, 0);
  assert_eq!(agg_count.aggregate_samples(&samples), 8.0);

  let agg_first = Aggregator::new(AggregationType::First, 2000, 0);
  assert_eq!(agg_first.aggregate_samples(&samples), 2.0);

  let agg_last = Aggregator::new(AggregationType::Last, 2000, 0);
  assert_eq!(agg_last.aggregate_samples(&samples), 9.0);

  let agg_avg = Aggregator::new(AggregationType::Avg, 2000, 0);
  assert_eq!(agg_avg.aggregate_samples(&samples), 5.0);

  let agg_range = Aggregator::new(AggregationType::Range, 2000, 0);
  assert_eq!(agg_range.aggregate_samples(&samples), 7.0);

  let agg_varp = Aggregator::new(AggregationType::VarP, 2000, 0);
  assert!((agg_varp.aggregate_samples(&samples) - 4.0).abs() < 1e-9);

  let agg_stdp = Aggregator::new(AggregationType::StdP, 2000, 0);
  assert!((agg_stdp.aggregate_samples(&samples) - 2.0).abs() < 1e-9);

  let agg_vars = Aggregator::new(AggregationType::VarS, 2000, 0);
  assert!((agg_vars.aggregate_samples(&samples) - (32.0 / 7.0)).abs() < 1e-9);

  let agg_stds = Aggregator::new(AggregationType::StdS, 2000, 0);
  assert!((agg_stds.aggregate_samples(&samples) - (32.0 / 7.0f64).sqrt()).abs() < 1e-9);

  let agg_twa = Aggregator::new(AggregationType::Twa, 2000, 0);
  let twa_val = agg_twa.aggregate_samples(&samples);
  assert!(twa_val > 0.0);
}

#[test]
fn test_duplicate_policies_and_createrule() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // DuplicatePolicy tests in chunk merging
  let mut s = vec![TSSample::new(100, 10.0)];
  let _ = TSChunk::merge_samples(&mut s, &[TSSample::new(100, 20.0)], DuplicatePolicy::First)?;
  assert_eq!(s[0].v, 10.0);

  let _ = TSChunk::merge_samples(&mut s, &[TSSample::new(100, 20.0)], DuplicatePolicy::Last)?;
  assert_eq!(s[0].v, 20.0);

  let _ = TSChunk::merge_samples(&mut s, &[TSSample::new(100, 5.0)], DuplicatePolicy::Min)?;
  assert_eq!(s[0].v, 5.0);

  let _ = TSChunk::merge_samples(&mut s, &[TSSample::new(100, 35.0)], DuplicatePolicy::Max)?;
  assert_eq!(s[0].v, 35.0);

  let _ = TSChunk::merge_samples(&mut s, &[TSSample::new(100, 15.0)], DuplicatePolicy::Sum)?;
  assert_eq!(s[0].v, 50.0);

  // Downstream rule creation
  db.ts_create_one("src_ts")?;
  db.ts_create_one("dst_ts")?;
  db.ts_createrule("src_ts", "dst_ts", AggregationType::Avg, 60000, Some(0))?;

  // Check non-existent rules fail gracefully
  assert!(
    db.ts_createrule("non_src", "dst_ts", AggregationType::Sum, 1000, None)
      .is_err()
  );
  assert!(
    db.ts_createrule("src_ts", "non_dst", AggregationType::Sum, 1000, None)
      .is_err()
  );

  // 测试自动触发下游更新
  db.ts_add("src_ts", 1000, 10.0, None, [])?;
  db.ts_add("src_ts", 2000, 20.0, None, [])?;
  assert_eq!(db.ts_get("dst_ts")?, Some((0, 15.0))); // Avg of (10+20)/2 at aligned bucket 0

  // 测试删除规则
  db.ts_deleterule("src_ts", "dst_ts")?;
  assert!(db.ts_deleterule("src_ts", "dst_ts").is_err());

  Ok(())
}

#[test]
fn test_timeseries_create_and_alter() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. TS.CREATE
  let create_opt = [
    TsCreate::RetentionTime(100000),
    TsCreate::ChunkSize(2048),
    TsCreate::DuplicatePolicy(DuplicatePolicy::Last),
    TsCreate::Labels(vec![
      ("sensor".to_string(), "temperature".to_string()),
      ("unit".to_string(), "celsius".to_string()),
    ]),
  ];
  db.ts_create("dev:sensor1", create_opt.clone())?;

  // 重复创建应报错
  assert!(db.ts_create("dev:sensor1", create_opt).is_err());

  // 验证初始 TS.INFO
  let info = db.ts_info("dev:sensor1")?;
  assert_eq!(info.retention_time, 100000);
  assert_eq!(info.chunk_size, 2048);
  assert_eq!(info.chunk_type, ChunkType::Compressed);
  assert_eq!(info.duplicate_policy, DuplicatePolicy::Last);
  assert_eq!(info.labels.len(), 2);

  // 2. TS.ALTER
  db.ts_alter(
    "dev:sensor1",
    Some(200000),
    Some(1024),
    Some(DuplicatePolicy::Max),
    Some(vec![("sensor".to_string(), "humidity".to_string())]),
  )?;

  let info_after = db.ts_info("dev:sensor1")?;
  assert_eq!(info_after.retention_time, 200000);
  assert_eq!(info_after.chunk_size, 1024);
  assert_eq!(info_after.duplicate_policy, DuplicatePolicy::Max);
  assert_eq!(
    info_after.labels,
    vec![("sensor".to_string(), "humidity".to_string())]
  );

  // 修改不存在的 key 应报错
  assert!(
    db.ts_alter("non_existing_key", Some(5000), None, None, None)
      .is_err()
  );

  Ok(())
}

#[test]
fn test_timeseries_retention_window_and_expiration() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let opt = [
    TsCreate::RetentionTime(5000),
    TsCreate::ChunkType(ChunkType::Uncompressed),
  ];
  db.ts_create("ts:retention", opt)?;

  // 写入新点
  db.ts_add("ts:retention", 10000, 100.0, None, [])?;
  db.ts_add("ts:retention", 12000, 120.0, None, [])?;

  // 尝试写入早于 retention_bound (12000 - 5000 = 7000) 的点 -> 应拒绝
  assert!(db.ts_add("ts:retention", 6000, 60.0, None, None).is_err());

  // 写入刚好在 retention 窗口内的点 -> 应成功
  assert!(db.ts_add("ts:retention", 8000, 80.0, None, None).is_ok());

  // 查询应只返回 retention 范围内的点 (8000, 10000, 12000)
  let samples = db.ts_range_one("ts:retention", (0, 20000))?;
  assert_eq!(samples.len(), 3);
  assert_eq!(samples[0], (8000, 80.0));
  assert_eq!(samples[1], (10000, 100.0));
  assert_eq!(samples[2], (12000, 120.0));

  Ok(())
}

#[test]
fn test_timeseries_madd_and_on_duplicate_options() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // TS.MADD
  let items = vec![
    ("m1", 1000u64, 10.0f64),
    ("m2", 1000u64, 20.0f64),
    ("m1", 2000u64, 15.0f64),
    ("m2", 2000u64, 25.0f64),
  ];
  let res = db.ts_madd(&items)?;
  assert_eq!(res.len(), 4);
  for r in &res {
    assert!(r.is_ok());
  }

  assert_eq!(db.ts_get("m1")?, Some((2000, 15.0)));
  assert_eq!(db.ts_get("m2")?, Some((2000, 25.0)));

  // 测试 TS.ADD with ON_DUPLICATE
  // 默认是 Block 策略，重复应报错
  assert!(db.ts_add("m1", 1000, 99.0, None, None).is_err());

  // 使用 ON_DUPLICATE 覆盖
  db.ts_add("m1", 1000, 99.0, Some(DuplicatePolicy::Last), None)?;
  assert_eq!(db.ts_range_one("m1", (1000, 1000))?, vec![(1000, 99.0)]);

  db.ts_add("m1", 1000, 1.0, Some(DuplicatePolicy::Sum), None)?;
  assert_eq!(db.ts_range_one("m1", (1000, 1000))?, vec![(1000, 100.0)]);

  db.ts_add("m1", 1000, 50.0, Some(DuplicatePolicy::Min), None)?;
  assert_eq!(db.ts_range_one("m1", (1000, 1000))?, vec![(1000, 50.0)]);

  db.ts_add("m1", 1000, 80.0, Some(DuplicatePolicy::Max), None)?;
  assert_eq!(db.ts_range_one("m1", (1000, 1000))?, vec![(1000, 80.0)]);

  Ok(())
}

#[test]
fn test_timeseries_incrby_decrby_boundaries() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // INCRBY on non-existent key creates it
  db.ts_incrby("metric:counter", 10.5, Some(1000), None)?;
  assert_eq!(db.ts_get("metric:counter")?, Some((1000, 10.5)));

  // INCRBY with equal or higher timestamp
  db.ts_incrby("metric:counter", 5.0, Some(2000), None)?;
  assert_eq!(db.ts_get("metric:counter")?, Some((2000, 15.5)));

  // DECRBY
  db.ts_decrby("metric:counter", 3.5, Some(3000), None)?;
  assert_eq!(db.ts_get("metric:counter")?, Some((3000, 12.0)));

  // Timestamp lower than latest existing timestamp must error
  assert!(
    db.ts_incrby("metric:counter", 1.0, Some(2500), None)
      .is_err()
  );

  Ok(())
}

#[test]
fn test_timeseries_del_cascade_and_compaction_restrictions() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.ts_create("src_del", [TsCreate::RetentionTime(10000)])?;
  db.ts_create_one("dst_del")?;
  db.ts_createrule("src_del", "dst_del", AggregationType::Sum, 2000, Some(0))?;

  // Add source samples
  db.ts_add("src_del", 1000, 10.0, None, [])?;
  db.ts_add("src_del", 1500, 20.0, None, [])?;
  db.ts_add("src_del", 2000, 30.0, None, [])?;
  db.ts_add("src_del", 2500, 40.0, None, [])?;
  db.ts_add("src_del", 10000, 50.0, None, [])?; // last_time = 10000, retention = 10000 (retention_bound = 0)

  // Delete [1000, 1500] in source
  let del_cnt = db.ts_del("src_del", (1000, 1500))?;
  assert_eq!(del_cnt, 2);

  // Source range check
  let src_rem = db.ts_range_one("src_del", (0, 10000))?;
  assert_eq!(src_rem, vec![(2000, 30.0), (2500, 40.0), (10000, 50.0)]);

  // Downstream bucket at 0 had (10+20)=30, after deletion it becomes empty, deleted from dst
  let dst_samples = db.ts_range_one("dst_del", (0, 10000))?;
  assert!(!dst_samples.iter().any(|s| s.0 == 0));

  Ok(())
}

#[test]
fn test_timeseries_advanced_mrange_and_queryindex_filters() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let s1 = [TsCreate::Labels(vec![
    ("sensor".to_string(), "temp".to_string()),
    ("area".to_string(), "zoneA".to_string()),
    ("country".to_string(), "CN".to_string()),
  ])];
  let s2 = [TsCreate::Labels(vec![
    ("sensor".to_string(), "temp".to_string()),
    ("area".to_string(), "zoneB".to_string()),
    ("country".to_string(), "US".to_string()),
  ])];
  let s3 = [TsCreate::Labels(vec![
    ("sensor".to_string(), "humidity".to_string()),
    ("area".to_string(), "zoneA".to_string()),
    ("country".to_string(), "CN".to_string()),
  ])];

  db.ts_create("k1", s1)?;
  db.ts_create("k2", s2)?;
  db.ts_create("k3", s3)?;

  db.ts_add("k1", 100, 10.0, None, [])?;
  db.ts_add("k2", 100, 20.0, None, [])?;
  db.ts_add("k3", 100, 30.0, None, [])?;

  // 1. QUERYINDEX with quoted values and list matchers
  let idx1 = db.ts_queryindex(&[
    "sensor=(temp,humidity)".to_string(),
    "country=CN".to_string(),
  ])?;
  assert_eq!(idx1, vec!["k1", "k3"]);

  let idx2 = db.ts_queryindex(&["area!=zoneB".to_string()])?;
  assert_eq!(idx2, vec!["k1", "k3"]);

  // 2. MRANGE with SELECTED_LABELS and GROUPBY REDUCE
  let mut selected_labels = rapidhash::RapidHashSet::default();
  selected_labels.insert("area".to_string());

  let mrange_opt = [
    TsMRange::WithLabels,
    TsMRange::SelectedLabels(selected_labels.into_iter().collect()),
    TsMRange::Filters(vec!["sensor=temp".to_string()]),
    TsMRange::GroupBy("sensor".to_string(), GroupReducerType::Sum),
  ];

  let mrange_res = db.ts_mrange((0, 200), mrange_opt)?;
  assert_eq!(mrange_res.len(), 1);
  assert_eq!(mrange_res[0].name, "sensor=temp");
  assert_eq!(mrange_res[0].samples, vec![(100, 30.0)]); // 10 + 20
  assert!(
    mrange_res[0]
      .labels
      .iter()
      .any(|(k, v)| k == "__reducer__" && v == "sum")
  );
  assert!(mrange_res[0].labels.iter().any(|(k, _)| k == "__source__"));

  Ok(())
}

#[test]
fn test_timeseries_empty_bucket_last_carry_forward() {
  let samples = vec![(1000, 10.0), (4000, 40.0)];
  let agg_last = Aggregator::new(AggregationType::Last, 1000, 0);

  let res_last = agg_last.split_and_aggregate(&samples, None, true, BucketTimestampType::Start);
  assert_eq!(res_last.len(), 4);
  assert_eq!(res_last[0], (1000, 10.0));
  assert_eq!(res_last[1], (2000, 10.0)); // 继承前一个桶的值
  assert_eq!(res_last[2], (3000, 10.0)); // 继承前一个桶的值
  assert_eq!(res_last[3], (4000, 40.0));
}

#[test]
fn test_timeseries_multi_sample_chunk_splitting_and_queries() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let opt = [
    TsCreate::ChunkSize(3),
    TsCreate::DuplicatePolicy(DuplicatePolicy::Last),
    TsCreate::Labels(vec![("type".to_string(), "cpu".to_string())]),
  ];
  db.ts_create("metric:cpu", opt)?;

  // 连续写入 10 个样本，跨多个 chunk
  for i in 1..=10 {
    db.ts_add("metric:cpu", i * 1000, i as f64 * 10.0, None, [])?;
  }

  let info = db.ts_info("metric:cpu")?;
  assert_eq!(info.total_samples, 10);
  assert_eq!(info.first_timestamp, 1000);
  assert_eq!(info.last_timestamp, 10000);

  // 范围查询验证全部数据
  let all = db.ts_range_one("metric:cpu", (0, 20000))?;
  assert_eq!(all.len(), 10);
  for (idx, (ts, v)) in all.iter().enumerate() {
    assert_eq!(*ts, ((idx + 1) * 1000) as u64);
    assert_eq!(*v, (idx + 1) as f64 * 10.0);
  }

  // 乱序回填样本 (3500) 以及覆盖更新 (5000)
  db.ts_add("metric:cpu", 3500, 35.0, None, [])?;
  db.ts_add("metric:cpu", 5000, 55.0, None, [])?;

  let updated_range = db.ts_range_one("metric:cpu", (3000, 6000))?;
  assert_eq!(
    updated_range,
    vec![
      (3000, 30.0),
      (3500, 35.0),
      (4000, 40.0),
      (5000, 55.0),
      (6000, 60.0)
    ]
  );

  Ok(())
}

#[test]
fn test_all_12_group_reducers_matrix() {
  let s1 = vec![(100, 10.0), (200, 20.0)];
  let s2 = vec![(100, 30.0), (200, 40.0)];
  let all = vec![s1, s2];

  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Sum),
    vec![(100, 40.0), (200, 60.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Avg),
    vec![(100, 20.0), (200, 30.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Min),
    vec![(100, 10.0), (200, 20.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Max),
    vec![(100, 30.0), (200, 40.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Range),
    vec![(100, 20.0), (200, 20.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Count),
    vec![(100, 2.0), (200, 2.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::VarP),
    vec![(100, 100.0), (200, 100.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::StdP),
    vec![(100, 10.0), (200, 10.0)]
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::VarS),
    vec![(100, 200.0), (200, 200.0)]
  );
  assert!(
    (group_samples_and_reduce(&all, GroupReducerType::StdS)[0].1 - 200.0f64.sqrt()).abs() < 1e-9
  );
  assert_eq!(
    group_samples_and_reduce(&all, GroupReducerType::Twa),
    vec![(100, 20.0), (200, 30.0)]
  );
}

#[test]
fn test_timeseries_incrby_decrby_with_create_options() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let create_opt = [
    TsCreate::RetentionTime(3600000),
    TsCreate::ChunkSize(1024),
    TsCreate::DuplicatePolicy(DuplicatePolicy::Last),
    TsCreate::Labels(vec![
      ("metric".to_string(), "counter".to_string()),
      ("host".to_string(), "server1".to_string()),
    ]),
  ];

  // 1. INCRBY 创建非存在 key 并附加选项
  let ts1 = db.ts_incrby("ctr:1", 5.0, Some(1000), create_opt)?;
  assert_eq!(ts1, 1000);

  let info = db.ts_info("ctr:1")?;
  assert_eq!(info.retention_time, 3600000);
  assert_eq!(info.chunk_type, ChunkType::Compressed);
  assert_eq!(
    info.labels,
    vec![
      ("metric".to_string(), "counter".to_string()),
      ("host".to_string(), "server1".to_string())
    ]
  );

  // 2. 连续 INCRBY / DECRBY
  let ts2 = db.ts_incrby("ctr:1", 10.0, Some(2000), None)?;
  assert_eq!(ts2, 2000);

  let ts3 = db.ts_decrby("ctr:1", 3.0, Some(3000), None)?;
  assert_eq!(ts3, 3000);

  let get_res = db.ts_get("ctr:1")?;
  assert_eq!(get_res, Some((3000, 12.0))); // 5 + 10 - 3 = 12

  let range_res = db.ts_range_one("ctr:1", (0, 5000))?;
  assert_eq!(range_res, vec![(1000, 5.0), (2000, 15.0), (3000, 12.0)]);

  Ok(())
}

#[test]
fn test_timeseries_zero_copy_uncompressed_and_compressed_timestamps() {
  let samples = vec![
    TSSample::new(100, 1.0),
    TSSample::new(200, 2.0),
    TSSample::new(300, 3.0),
    TSSample::new(400, 4.0),
  ];

  // 未压缩 Chunk
  let uncomp_chunk = TSChunk::encode_uncompressed(&samples);
  assert_eq!(TSChunk::get_first_timestamp(&uncomp_chunk), Some(100));
  assert_eq!(TSChunk::get_last_timestamp(&uncomp_chunk), Some(400));
  assert_eq!(TSChunk::get_count(&uncomp_chunk), 4);

  // Gorilla 压缩 Chunk
  let comp_chunk = TSChunk::encode_compressed(&samples);
  assert_eq!(TSChunk::get_first_timestamp(&comp_chunk), Some(100));
  assert_eq!(TSChunk::get_last_timestamp(&comp_chunk), Some(400));
  assert_eq!(TSChunk::get_count(&comp_chunk), 4);
}

#[test]
fn test_multi_sample_batch_duplicate_policies() -> Void {
  // 测试 new_samples 内部含重复时间戳的各种策略归并
  let base = vec![TSSample::new(100, 10.0), TSSample::new(300, 30.0)];
  let new_with_dups = vec![
    TSSample::new(200, 20.0),
    TSSample::new(200, 25.0),
    TSSample::new(300, 35.0),
  ];

  // 1. Last 策略
  let mut s_last = base.clone();
  let stats_last = TSChunk::merge_samples(&mut s_last, &new_with_dups, DuplicatePolicy::Last)?;
  assert_eq!(
    s_last,
    vec![
      TSSample::new(100, 10.0),
      TSSample::new(200, 25.0),
      TSSample::new(300, 35.0),
    ]
  );
  assert_eq!(stats_last.inserted, 1);
  assert_eq!(stats_last.updated, 2);

  // 2. First 策略
  let mut s_first = base.clone();
  let _ = TSChunk::merge_samples(&mut s_first, &new_with_dups, DuplicatePolicy::First)?;
  assert_eq!(
    s_first,
    vec![
      TSSample::new(100, 10.0),
      TSSample::new(200, 20.0),
      TSSample::new(300, 30.0),
    ]
  );

  // 3. Max 策略
  let mut s_max = base.clone();
  let _ = TSChunk::merge_samples(&mut s_max, &new_with_dups, DuplicatePolicy::Max)?;
  assert_eq!(
    s_max,
    vec![
      TSSample::new(100, 10.0),
      TSSample::new(200, 25.0),
      TSSample::new(300, 35.0),
    ]
  );

  // 4. Min 策略
  let mut s_min = base.clone();
  let _ = TSChunk::merge_samples(&mut s_min, &new_with_dups, DuplicatePolicy::Min)?;
  assert_eq!(
    s_min,
    vec![
      TSSample::new(100, 10.0),
      TSSample::new(200, 20.0),
      TSSample::new(300, 30.0),
    ]
  );

  // 5. Sum 策略
  let mut s_sum = base.clone();
  let _ = TSChunk::merge_samples(&mut s_sum, &new_with_dups, DuplicatePolicy::Sum)?;
  assert_eq!(
    s_sum,
    vec![
      TSSample::new(100, 10.0),
      TSSample::new(200, 45.0), // 20 + 25
      TSSample::new(300, 65.0), // 30 + 35
    ]
  );

  // 6. Block 策略 (冲突时报错)
  let mut s_block = base.clone();
  assert!(TSChunk::merge_samples(&mut s_block, &new_with_dups, DuplicatePolicy::Block).is_err());

  Ok(())
}

#[test]
fn test_timeseries_get_latest_sample_zero_alloc() -> Void {
  let samples = vec![
    TSSample::new(100, 1.5),
    TSSample::new(200, 2.5),
    TSSample::new(300, 3.5),
  ];

  // 1. 未压缩块
  let uncomp = TSChunk::encode_uncompressed(&samples);
  assert_eq!(TSChunk::get_latest_sample(&uncomp)?, Some((300, 3.5)));

  // 2. 压缩块
  let comp = TSChunk::encode_compressed(&samples);
  assert_eq!(TSChunk::get_latest_sample(&comp)?, Some((300, 3.5)));

  // 3. 空块
  let empty_chunk = TSChunk::encode_uncompressed(&[]);
  assert_eq!(TSChunk::get_latest_sample(&empty_chunk)?, None);

  Ok(())
}

#[test]
fn test_timeseries_mget_selected_labels_missing_keys() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let opt = [
    TsCreate::ChunkType(ChunkType::Uncompressed),
    TsCreate::Labels(vec![
      ("region".to_string(), "us-west".to_string()),
      ("env".to_string(), "prod".to_string()),
    ]),
  ];
  db.ts_create("ts_sel_test", opt)?;
  db.ts_add("ts_sel_test", 1000, 42.0, None, [])?;

  let mut selected_labels = rapidhash::RapidHashSet::default();
  selected_labels.insert("region".to_string());
  selected_labels.insert("missing_tag".to_string());

  let mget_opt = [
    TsMGet::SelectedLabels(selected_labels.into_iter().collect()),
    TsMGet::Filters(vec!["env=prod".to_string()]),
  ];
  let res = db.ts_mget(mget_opt)?;
  assert_eq!(res.len(), 1);
  assert_eq!(res[0].name, "ts_sel_test");
  assert_eq!(res[0].sample, Some((1000, 42.0)));
  // 选中的 missing_tag 应输出对应键且值为空字符串
  assert!(
    res[0]
      .labels
      .iter()
      .any(|(k, v)| k == "region" && v == "us-west")
  );
  assert!(
    res[0]
      .labels
      .iter()
      .any(|(k, v)| k == "missing_tag" && v.is_empty())
  );

  Ok(())
}

#[test]
fn test_timeseries_del_compaction_retention_boundary() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 创建源时序，retention = 5000
  db.ts_create("src_ret", [TsCreate::RetentionTime(5000)])?;
  db.ts_create_one("dst_ret")?;
  // 创建降采样规则：bucket_duration = 2000, alignment = 0
  db.ts_createrule("src_ret", "dst_ret", AggregationType::Sum, 2000, Some(0))?;

  // 写入最新点 10000 -> retention_bound = 10000 - 5000 = 5000
  db.ts_add("src_ret", 6000, 10.0, None, [])?;
  db.ts_add("src_ret", 10000, 20.0, None, [])?;

  // 尝试删除 from = 5500:
  // 5500 自身 >= 5000，但是其 aligned bucket left 是 4000 < 5000 (retention_bound)
  // 根据 kvrocks 规则，这会影响超出 retention 范围的降采样桶，必须拒绝！
  assert!(db.ts_del("src_ret", (5500, 7000)).is_err());

  // 如果从 6000 删除：
  // aligned bucket left 是 6000 >= 5000，允许删除！
  assert!(db.ts_del("src_ret", (6000, 7000)).is_ok());

  Ok(())
}

#[test]
fn test_timeseries_arbitrary_binary_keys_and_downstream_rules() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 构造包含 \x00, \xFF, :, 换行等极端二进制的源键与目标键
  let bin_src = b"ts\x00src:key\xff\x00\x01";
  let bin_dst = b"ts\x00dst:key\xff\x00\x02";

  db.ts_create_one(bin_src)?;
  db.ts_create_one(bin_dst)?;

  // 2. 创建下游降采样规则（对标 1 字节 \xFF 隔离码与 OPPV 长度分帧）
  db.ts_createrule(bin_src, bin_dst, AggregationType::Sum, 1000, Some(0))?;

  // 3. 写入数据并验证自动降采样级联更新
  db.ts_add(bin_src, 100, 42.5, None, [])?;
  db.ts_add(bin_src, 200, 57.5, None, [])?;

  // 目标键应当在对齐时间戳 0 处聚合出 42.5 + 57.5 = 100.0
  assert_eq!(db.ts_get(bin_dst)?, Some((0, 100.0)));

  // 4. 查询源键 TS.INFO，验证规则列表被正确解析
  let info = db.ts_info(bin_src)?;
  assert_eq!(info.total_samples, 2);
  assert_eq!(info.first_timestamp, 100);
  assert_eq!(info.last_timestamp, 200);
  assert_eq!(info.downstream_rules.len(), 1);

  // 5. 级联删除验证：DEL 源键会级联清除数据点和下游规则
  assert_eq!(db.del(&[bin_src])?, 1);
  assert_eq!(db.ts_get(bin_src)?, None);
  // 目标键仍保留已聚合数据
  assert_eq!(db.ts_get(bin_dst)?, Some((0, 100.0)));

  Ok(())
}

#[test]
fn test_timeseries_alp_compression_and_queries() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let opt = [TsCreate::Labels(vec![
    ("sensor".to_string(), "temperature".to_string()),
    ("device".to_string(), "d101".to_string()),
  ])];
  db.ts_create("sensor:temp:alp", opt)?;

  // 写入 500 个带十进制小数精度的温度读数
  let mut expected = Vec::with_capacity(500);
  for i in 0..500 {
    let ts = 1000 + i * 10;
    let v = 20.0 + (i % 150) as f64 * 0.1;
    db.ts_add("sensor:temp:alp", ts, v, None, [])?;
    expected.push((ts, v));
  }

  // 1. TS.INFO 验证
  let info = db.ts_info("sensor:temp:alp")?;
  assert_eq!(info.total_samples, 500);
  assert_eq!(info.first_timestamp, 1000);
  assert_eq!(info.last_timestamp, 1000 + 499 * 10);
  assert_eq!(info.chunk_type, ChunkType::Compressed);

  // 2. TS.GET 验证最新点
  assert_eq!(
    db.ts_get("sensor:temp:alp")?,
    Some((1000 + 499 * 10, 20.0 + (499 % 150) as f64 * 0.1))
  );

  // 3. TS.RANGE 完整范围查询
  let range_res = db.ts_range_one("sensor:temp:alp", (0, 100_000))?;
  assert_eq!(range_res.len(), 500);
  for (act, exp) in range_res.iter().zip(expected.iter()) {
    assert_eq!(act.0, exp.0);
    assert_eq!(act.1.to_bits(), exp.1.to_bits());
  }

  // 4. TS.RANGE 聚合降采样查询 (Avg, bucket = 100, alignment = 0)
  let opt_agg = [TsRange::Aggregation(AggregationType::Avg, 100)];
  let agg_res = db.ts_range("sensor:temp:alp", (1000, 2000), opt_agg)?;
  assert!(!agg_res.is_empty());

  Ok(())
}

#[test]
fn test_timeseries_filter_by_ts_optimized() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 TsFilter 数据结构的基础特性与不变量
  let raw_ts = vec![5000, 1000, 3000, 1000, 4000, 2000, 3000];
  let filter = TsFilter::new(raw_ts);
  assert_eq!(filter.len(), 5);
  assert_eq!(filter.as_slice(), &[1000, 2000, 3000, 4000, 5000]);
  assert!(filter.contains(1000));
  assert!(filter.contains(3000));
  assert!(!filter.contains(2500));
  assert!(!filter.contains(6000));

  // 2. 测试 matches_range 区间命中（Chunk剪枝）与异常区间守卫
  assert!(filter.matches_range(500, 1500)); // 包含 1000
  assert!(filter.matches_range(2000, 2000)); // 包含 2000
  assert!(!filter.matches_range(1500, 1800)); // 无命中
  assert!(!filter.matches_range(6000, 7000)); // 均在右侧，无命中
  assert!(!filter.matches_range(100, 500)); // 均在左侧，无命中
  assert!(!filter.matches_range(3000, 2000)); // 倒置区间快速排斥

  // 2.1 测试 clamp_range 精确有效极值收缩与提前截断
  assert_eq!(filter.clamp_range(0, 10000), Some((1000, 5000)));
  assert_eq!(filter.clamp_range(1500, 3500), Some((2000, 3000)));
  assert_eq!(filter.clamp_range(2000, 2000), Some((2000, 2000)));
  assert_eq!(filter.clamp_range(2100, 2900), None); // 区间内无点，直接截断
  assert_eq!(filter.clamp_range(6000, 8000), None);
  assert_eq!(filter.clamp_range(100, 500), None);
  assert_eq!(filter.clamp_range(500, 100), None);

  // 3. 测试 filter_samples 双指针原地高效过滤
  let mut test_samples = vec![
    (1000, 10.0),
    (1500, 15.0),
    (2000, 20.0),
    (2500, 25.0),
    (3000, 30.0),
    (4000, 40.0),
    (6000, 60.0),
  ];
  filter.filter_samples(&mut test_samples, Some((15.0, 35.0)));
  // 命中时间戳：1000, 2000, 3000, 4000
  // 同时满足值 [15.0, 35.0]：2000 (20.0), 3000 (30.0)
  assert_eq!(test_samples, vec![(2000, 20.0), (3000, 30.0)]);

  // 4. 数据库 TS.RANGE 集成测试
  db.ts_create("sensor:filter_ts", [])?;
  for i in 1..=20 {
    let ts = i * 100;
    db.ts_add("sensor:filter_ts", ts, ts as f64, None, [])?;
  }

  // 4.1 乱序与重复输入 TsFilter 过滤打点
  let query_filter = TsFilter::from([700, 300, 900, 300, 1500, 8888]);
  let range_res = db.ts_range(
    "sensor:filter_ts",
    (0, 3000),
    [TsRange::FilterByTs(query_filter)],
  )?;
  assert_eq!(
    range_res,
    vec![(300, 300.0), (700, 700.0), (900, 900.0), (1500, 1500.0)]
  );

  // 4.2 复合过滤：FilterByTs + FilterByValue
  let compound_res = db.ts_range(
    "sensor:filter_ts",
    (0, 3000),
    [
      TsRange::FilterByTs(TsFilter::from(vec![200, 400, 600, 800])),
      TsRange::FilterByValue(300.0, 700.0),
    ],
  )?;
  assert_eq!(compound_res, vec![(400, 400.0), (600, 600.0)]);

  // 4.3 TS.MRANGE 过滤测试
  db.ts_create(
    "sensor:mrange:1",
    [TsCreate::Labels(vec![("type".into(), "metric".into())])],
  )?;
  db.ts_create(
    "sensor:mrange:2",
    [TsCreate::Labels(vec![("type".into(), "metric".into())])],
  )?;
  for ts in [100, 200, 300, 400] {
    db.ts_add("sensor:mrange:1", ts, (ts * 2) as f64, None, [])?;
    db.ts_add("sensor:mrange:2", ts, (ts * 3) as f64, None, [])?;
  }

  let mrange_res = db.ts_mrange(
    (0, 1000),
    [
      TsMRange::Filters(vec!["type=metric".into()]),
      TsMRange::FilterByTs(TsFilter::from([200, 300])),
    ],
  )?;
  assert_eq!(mrange_res.len(), 2);
  for item in mrange_res {
    assert_eq!(item.samples.len(), 2);
    assert_eq!(item.samples[0].0, 200);
    assert_eq!(item.samples[1].0, 300);
  }

  Ok(())
}

#[test]
fn test_timeseries_expiration_and_edge_optimizations() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 验证过期时序键在 ts_mget、ts_mrange、ts_queryindex 中的正确过滤
  db.ts_create(
    "sensor:live",
    [TsCreate::Labels(vec![
      ("app".into(), "web".into()),
      ("status".into(), "ok".into()),
    ])],
  )?;
  db.ts_create(
    "sensor:dead",
    [TsCreate::Labels(vec![
      ("app".into(), "web".into()),
      ("status".into(), "expired".into()),
    ])],
  )?;

  db.ts_add("sensor:live", 1000, 10.0, None, [])?;
  db.ts_add("sensor:dead", 1000, 99.0, None, [])?;

  // 将 sensor:dead 设为已过期 (expire = 0s)
  assert!(db.expire("sensor:dead", 0)?);

  // 1.1 ts_queryindex 应当排除已过期的 dead
  let keys = db.ts_queryindex(&["app=web".to_string()])?;
  assert_eq!(keys, vec!["sensor:live"]);

  // 1.2 ts_mget 应当排除已过期的 dead，且高效不触发额外点查
  let mget_res = db.ts_mget([TsMGet::Filters(vec!["app=web".to_string()])])?;
  assert_eq!(mget_res.len(), 1);
  assert_eq!(mget_res[0].name, "sensor:live");
  assert_eq!(mget_res[0].sample, Some((1000, 10.0)));

  // 1.3 ts_mrange 应当排除已过期的 dead
  let mrange_res = db.ts_mrange((0, 2000), [TsMRange::Filters(vec!["app=web".to_string()])])?;
  assert_eq!(mrange_res.len(), 1);
  assert_eq!(mrange_res[0].name, "sensor:live");

  // 2. 多分组 GroupBy 确定性排序验证 (按名称升序稳定返回)
  db.ts_create(
    "device:c",
    [TsCreate::Labels(vec![("zone".into(), "c".into())])],
  )?;
  db.ts_create(
    "device:a",
    [TsCreate::Labels(vec![("zone".into(), "a".into())])],
  )?;
  db.ts_create(
    "device:b",
    [TsCreate::Labels(vec![("zone".into(), "b".into())])],
  )?;

  db.ts_add("device:c", 100, 30.0, None, [])?;
  db.ts_add("device:a", 100, 10.0, None, [])?;
  db.ts_add("device:b", 100, 20.0, None, [])?;

  let grouped_res = db.ts_mrange(
    (0, 200),
    [
      TsMRange::Filters(vec!["zone!=".into()]),
      TsMRange::GroupBy("zone".into(), GroupReducerType::Sum),
    ],
  )?;
  assert_eq!(grouped_res.len(), 3);
  assert_eq!(grouped_res[0].name, "zone=a");
  assert_eq!(grouped_res[1].name, "zone=b");
  assert_eq!(grouped_res[2].name, "zone=c");

  // 3. Chunk 乱序插入与向前扩展首时间戳测试
  db.ts_create("sensor:prepend", [])?;
  db.ts_add("sensor:prepend", 1000, 10.0, None, [])?;
  db.ts_add("sensor:prepend", 2000, 20.0, None, [])?;
  db.ts_add("sensor:prepend", 3000, 30.0, None, [])?;
  // 向头部插入更早的时间戳 500 (测试 chunk 首时间戳更新与旧 key 正确迁移)
  db.ts_add("sensor:prepend", 500, 5.0, None, [])?;

  let samples = db.ts_range_one("sensor:prepend", (0, 5000))?;
  assert_eq!(
    samples,
    vec![(500, 5.0), (1000, 10.0), (2000, 20.0), (3000, 30.0)]
  );

  // 4. 状态机转义与边界测试
  let mut filter = TimeSeriesLabelFilter::new();
  assert!(filter.add_filter("host=\"server\\\"1\""));
  assert_eq!(filter.len(), 1);

  Ok(())
}
