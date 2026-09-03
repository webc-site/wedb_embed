//! # SortedInt (Compact Integer Set)
//!
//! ## Overview
//! A highly optimized sorted integer set tailored for 64-bit unsigned integers.
//! Eliminates string serialization overhead and provides fast membership lookups, range queries, and rank operations.
//!
//! ## Use Cases
//! - Large-scale ID collections (user IDs, article IDs, entity primary keys)
//! - Direct integer inverted-index postings
//! - Numeric range slicing without string conversion overhead
//! - Fast integer existence filtering
//!
//! ---
//!
//! # 紧凑整型集合
//!
//! ## 概述
//! 针对 64 位无符号整数高度优化的紧凑有序集合。
//! 消除字符串编解码开销，提供高效的整型成员判断、数值范围切片与排名统计。
//!
//! ## 使用场景
//! - 海量数值主键集合存储（如用户 ID、文章 ID 列表）
//! - 倒排索引整数倒排链
//! - 无需字符串解析的纯数值范围检索
//! - 高性能整数存在性过滤

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb, sortedint::SortedintRange};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Add integers, check existence, and get members/cardinality
  // 批量添加整数、存在判断、基数统计与全量成员获取
  db.si_add(b"si:1", &[100, 200, 300, 400, 500])?;
  assert!(db.si_exists(b"si:1", 200)?);
  assert!(db.si_exists(b"si:1", 300)?);
  assert_eq!(db.si_card(b"si:1")?, 5);
  assert_eq!(db.si_members(b"si:1")?, [100, 200, 300, 400, 500]);

  // Batch existence test and streaming iterator
  // 批量存在判断与流式迭代器
  assert_eq!(
    db.si_mexist(b"si:1", &[100, 250, 300])?,
    [true, false, true]
  );
  db.si_iter(b"si:1", |_| true)?;

  // Index range and rank queries
  // 按偏移范围正反向获取与排名反查
  assert_eq!(db.si_range(b"si:1", 0, 0, 3, false)?.len(), 3);
  assert_eq!(db.si_rev_range(b"si:1", 0, 0, 3)?.len(), 3);
  assert_eq!(db.si_rank(b"si:1", 200)?, Some(1));
  assert_eq!(db.si_revrank(b"si:1", 200)?, Some(3));

  // Value range queries and counting
  // 按数值范围区间计数与范围切片
  let spec = SortedintRange {
    min: 200,
    max: 400,
    ..Default::default()
  };
  assert_eq!(db.si_count(b"si:1", spec)?, 3);
  assert_eq!(db.si_range_by_value(b"si:1", spec)?, [200, 300, 400]);
  assert_eq!(db.si_rev_range_by_value(b"si:1", spec)?, [400, 300, 200]);

  // Range deletion and clearing
  // 按数值、排名范围删除与全量清空
  let _ = db.si_rem_range_by_value(
    b"si:1",
    SortedintRange {
      min: 100,
      max: 150,
      ..Default::default()
    },
  )?;
  let _ = db.si_rem_range_by_value(
    b"si:1",
    SortedintRange {
      min: 150,
      max: 250,
      ..Default::default()
    },
  )?;
  let _ = db.si_rem_range_by_rank(b"si:1", (0, 0))?;
  let _ = db.si_rem(b"si:1", &[500])?;
  let _ = db.del(&[b"si:1".as_slice()])?;

  db.si_add_one(b"my_sortedint", 100)?;
  let _ = db.si_rem_one(b"my_sortedint", 100)?;

  println!("SortedInt 示例全部接口执行成功");
  Ok(())
}
