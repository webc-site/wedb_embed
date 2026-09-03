//! # TimeSeries
//!
//! ## Overview
//! Dedicated time-series engine with Gorilla compression and timestamp-value storage.
//! Features multi-metric downsampling, compaction retention windows, and real-time aggregation rules.
//!
//! ## Use Cases
//! - IoT sensor data collection and environmental monitoring
//! - Infrastructure host/container metrics (CPU, memory, network)
//! - Financial market tick feeds and candlestick aggregations
//! - Application performance monitoring (APM)
//!
//! ---
//!
//! # 时序数据
//!
//! ## 概述
//! 专用的时序数据存储引擎，采用时序压缩算法与高效的时间戳-数值存储。
//! 支持多维度标签索引、降采样压缩窗口与自动化流式聚合规则。
//!
//! ## 使用场景
//! - 物联网传感器遥测与环境监控数据收集
//! - 服务器与容器基础设施监控指标
//! - 金融行情分时数据与走势聚合
//! - 应用性能监控与链路延迟统计

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  timeseries::{AggregationType, DuplicatePolicy, TsCreate, TsMGet, TsMRange, TsRange},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Create time series with options and labels
  // 创建带标签与选项的时序序列
  db.ts_create(
    b"ts_cpu",
    [
      TsCreate::DuplicatePolicy(DuplicatePolicy::Last),
      TsCreate::Labels(vec![
        ("sensor".to_string(), "temp".to_string()),
        ("region".to_string(), "us-east".to_string()),
      ]),
    ],
  )?;
  db.ts_create_one(b"ts_mem")?;

  // Insert sample points
  // 插入采样点、带去重选项插入与批量插入
  db.ts_add(b"ts_cpu", 1000, 25.0, None, [])?;
  db.ts_add(b"ts_cpu", 2000, 26.5, Some(DuplicatePolicy::Last), [])?;
  let _ = db.ts_madd(&[(b"ts_cpu", 3000, 28.0), (b"ts_mem", 1000, 1024.0)])?;

  // Counter increments and decrements
  // 时序数值增减
  db.ts_incrby(b"ts_cpu", 1.0, Some(4000), [])?;
  db.ts_incrby(b"ts_cpu", 0.5, Some(5000), [])?;
  db.ts_decrby(b"ts_cpu", 0.5, Some(6000), [])?;
  db.ts_decrby(b"ts_cpu", 1.0, Some(7000), [])?;

  // Point get and range queries with aggregation
  // 最新点获取、正反向范围与聚合查询
  assert!(db.ts_get(b"ts_cpu")?.is_some());
  assert!(!db.ts_range(b"ts_cpu", (0, 10000), [])?.is_empty());
  assert!(!db.ts_revrange(b"ts_cpu", (0, 10000), [])?.is_empty());
  let _ = db.ts_range(
    b"ts_cpu",
    (0, 10000),
    [TsRange::Aggregation(AggregationType::Avg, 2000)],
  )?;

  // Metadata alterations, inspection, and label index queries
  // 序列属性修改、元信息查询与标签索引过滤
  db.ts_alter(
    b"ts_cpu",
    Some(86400000),
    None,
    Some(DuplicatePolicy::Block),
    None,
  )?;
  assert!(db.ts_info(b"ts_cpu")?.total_samples > 0);
  assert_eq!(db.ts_queryindex(&["sensor=temp".to_string()])?, ["ts_cpu"]);

  // Aggregation rules: create and delete
  // 自动化降采样聚合规则创建与删除
  db.ts_create(
    b"ts_cpu_avg",
    [TsCreate::DuplicatePolicy(DuplicatePolicy::Last)],
  )?;
  db.ts_createrule(b"ts_cpu", b"ts_cpu_avg", AggregationType::Avg, 5000, None)?;
  db.ts_deleterule(b"ts_cpu", b"ts_cpu_avg")?;

  // Multi-series range queries and sample point deletion
  // 多序列查询与采样点范围删除
  let _ = db.ts_mget([TsMGet::Filters(vec!["sensor=temp".to_string()])])?;

  let mrange_opt = [TsMRange::Filters(vec!["sensor=temp".to_string()])];
  let _ = db.ts_mrange((0, 10000), mrange_opt.clone())?;
  let _ = db.ts_mrevrange((0, 10000), mrange_opt)?;
  assert_eq!(db.ts_del(b"ts_cpu", (1000, 2000))?, 2);

  let _ = db.ts_madd_one(b"ts_metric", 2000, 20.0)?;
  let _ = db.ts_range_one(b"ts_metric", (0, 3000))?;
  let _ = db.ts_revrange_one(b"ts_metric", (0, 3000))?;

  println!("TimeSeries 示例全部接口执行成功");
  Ok(())
}
