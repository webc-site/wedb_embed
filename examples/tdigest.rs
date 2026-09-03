//! # T-Digest
//!
//! ## Overview
//! A probabilistic data structure for highly accurate online percentile and quantile estimation.
//! Particularly effective at extreme quantiles (P99, P99.9) while consuming constant, bounded memory.
//!
//! ## Use Cases
//! - Service Level Agreement (SLA) latency tracking (P50, P90, P99, P99.9)
//! - Financial transaction value and risk distribution monitoring
//! - Sensor anomaly threshold evaluation
//! - Robust trimmed-mean calculations rejecting outliers
//!
//! ---
//!
//! # 分位数统计 (T-Digest)
//!
//! ## 概述
//! 用于高精度在线分位数与分位排名估算的概率数据结构。
//! 在极值分位数（如 P99、P99.9）处具有极高估算精度，且内存占用恒定可控。
//!
//! ## 使用场景
//! - 服务服务等级协议响应延迟监控
//! - 金融交易金额与风险指标分布统计
//! - 传感器异常阈值评估
//! - 剔除极端离群值的截断均值计算

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Create structure and add observations
  // 创建结构并批量添加样本值
  db.tdigest_create(b"latencies", 100.0)?;
  db.tdigest_add(b"latencies", &[10.0, 20.0, 30.0, 40.0, 50.0, 90.0, 99.0])?;

  // Minimum and maximum observations
  // 获取样本最小值与最大值
  assert_eq!(db.tdigest_min(b"latencies")?, 10.0);
  assert_eq!(db.tdigest_max(b"latencies")?, 99.0);

  // Quantiles and cumulative distribution functions
  // 多分位数估算与累计分布函数查询
  assert_eq!(
    db.tdigest_quantile(b"latencies", &[0.5, 0.95, 0.99])?.len(),
    3
  );
  assert_eq!(db.tdigest_cdf(b"latencies", &[25.0, 50.0])?.len(), 2);

  // Forward and reverse rank estimations
  // 按值估算排名、反向排名与按排名反查数值
  assert_eq!(db.tdigest_rank(b"latencies", &[30.0])?, [2]);
  assert_eq!(db.tdigest_revrank(b"latencies", &[30.0])?, [4]);
  assert!(db.tdigest_byrank(b"latencies", &[2])?[0].is_some());
  assert!(db.tdigest_byrevrank(b"latencies", &[2])?[0].is_some());

  // Trimmed mean and metadata inspection
  // 截断均值计算与压缩参数元信息
  assert!(
    db.tdigest_trimmed_mean(b"latencies", 0.1, 0.9)?
      .unwrap_or_default()
      > 0.0
  );
  assert_eq!(db.tdigest_info(b"latencies")?.compression, 100);

  // Merging multiple digests and reset
  // 多结构合并与重置
  db.tdigest_create(b"latencies_b", 100.0)?;
  db.tdigest_add(b"latencies_b", &[60.0, 70.0, 80.0])?;
  db.tdigest_create(b"latencies_merged", 100.0)?;
  db.tdigest_merge(
    b"latencies_merged".as_slice(),
    &[b"latencies".as_slice(), b"latencies_b".as_slice()],
    [],
  )?;

  db.tdigest_reset(b"latencies_b")?;

  db.tdigest_add_one(b"latencies_merged", 25.5)?;
  let _ = db.tdigest_quantile_one(b"latencies_merged", 0.5)?;
  let _ = db.tdigest_cdf_one(b"latencies_merged", 25.5)?;
  let _ = db.tdigest_rank_one(b"latencies_merged", 25.5)?;
  let _ = db.tdigest_revrank_one(b"latencies_merged", 25.5)?;
  let _ = db.tdigest_byrank_one(b"latencies_merged", 0)?;
  let _ = db.tdigest_byrevrank_one(b"latencies_merged", 0)?;

  println!("T-Digest 示例全部接口执行成功");
  Ok(())
}
