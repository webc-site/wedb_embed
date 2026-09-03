//! # HyperLogLog (HLL)
//!
//! ## Overview
//! A probabilistic data structure used for cardinality estimation of massive unique datasets.
//! Uses a fixed small memory footprint (typically 12KB) to count billions of distinct items with ~0.81% error.
//!
//! ## Use Cases
//! - Daily and monthly unique visitor (UV) metrics
//! - Unique search query counting
//! - Real-time network flow and unique IP tracking
//! - Multi-period cardinality merging (daily into weekly/monthly)
//!
//! ---
//!
//! # 基数统计 (HyperLogLog)
//!
//! ## 概述
//! 用于对海量不重复数据集进行基数估算的概率数据结构。
//! 仅占用约 12KB 极小固定内存空间，即可完成数十亿级独立元素的近似统计，标准误差约 0.81%。
//!
//! ## 使用场景
//! - 网站每日与每月独立访客统计
//! - 搜索引擎独立查询词量统计
//! - 网络流量与独立访问来源去重计数
//! - 跨周期基数合并统计（如按日合并为按周或按月）

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Add elements and estimate cardinality
  // 添加元素与多键基数统计
  db.pfadd(
    b"hll_day1",
    &[
      b"user_1".as_slice(),
      b"user_2".as_slice(),
      b"user_3".as_slice(),
    ],
  )?;
  db.pfadd(
    b"hll_day2",
    &[
      b"user_3".as_slice(),
      b"user_4".as_slice(),
      b"user_5".as_slice(),
    ],
  )?;

  assert_eq!(db.pfcount(&[b"hll_day1"])?, 3);
  assert_eq!(db.pfcount(&[b"hll_day1", b"hll_day2"])?, 5);

  // Merge multiple HyperLogLog structures
  // 多基数集合并
  db.pfmerge(
    b"hll_merged".as_slice(),
    &[b"hll_day1".as_slice(), b"hll_day2".as_slice()],
  )?;
  assert_eq!(db.pfcount(&[b"hll_merged"])?, 5);

  // Algorithm self-testing
  // 算法正确性自检
  assert!(db.pfselftest());

  db.pfadd_one(b"hll_single", b"user1")?;
  let _ = db.pfcount_one(b"hll_single")?;

  println!("HyperLogLog 示例全部接口执行成功");
  Ok(())
}
