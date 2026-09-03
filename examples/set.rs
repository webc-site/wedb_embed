//! # Set
//!
//! ## Overview
//! The Set structure represents an unordered collection of unique binary strings.
//! It supports member existence testing and high-performance set algebra (intersection, union, difference).
//!
//! ## Use Cases
//! - Unique user tags and classification labels
//! - Social graphs (following/followers, mutual connections)
//! - IP/device deduplication
//! - Set algebraic filtering (intersection/union/diff queries)
//!
//! ---
//!
//! # 集合
//!
//! ## 概述
//! 集合结构用于存储无序且唯一的字符串集合。
//! 支持高效的成员判断、随机抽样与集合代数运算（交集、并集、差集）。
//!
//! ## 使用场景
//! - 唯一用户标签与分类标记
//! - 社交关系图谱与共同好友计算
//! - 访问 IP 与设备去重
//! - 权限集合运算与差集过滤

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Add members and query existence/cardinality
  // 添加成员、存在判断、多成员查询与基数获取
  db.sadd(
    b"set:a",
    &[
      b"rust".as_slice(),
      b"go".as_slice(),
      b"python".as_slice(),
      b"c++".as_slice(),
    ],
  )?;
  assert!(db.sismember(b"set:a", b"rust")?);
  assert_eq!(
    db.smismember(b"set:a", &[b"rust".as_slice(), b"java".as_slice()])?,
    [true, false]
  );
  assert_eq!(db.scard(b"set:a")?, 4);
  assert_eq!(db.smembers(b"set:a")?.len(), 4);

  // Iteration, random sampling, and scanning
  // 遍历、随机抽样与游标扫描
  db.siter(b"set:a", |_| true)?;
  assert_eq!(db.srandmember(b"set:a", 2)?.len(), 2);
  assert!(!db.sscan(b"set:a", 0, None, Some(10))?.1.is_empty());

  // Move, remove, pop, and overwrite
  // 成员移动、移除、随机弹出与全量覆盖
  db.smove(b"set:a", b"set:b", b"c++")?;
  assert!(db.sismember(b"set:b", b"c++")?);
  assert_eq!(db.srem(b"set:a", &[b"python".as_slice()])?, 1);
  assert_eq!(db.spop(b"set:a", 1)?.len(), 1);
  db.overwrite_set(
    b"set:a",
    &[b"rust".as_slice(), b"go".as_slice(), b"zig".as_slice()],
  )?;

  // Set intersection operations
  // 集合交集计算与存储
  db.sadd(
    b"set:c",
    &[b"rust".as_slice(), b"zig".as_slice(), b"nim".as_slice()],
  )?;
  assert_eq!(
    db.sinter(&[b"set:a".as_slice(), b"set:c".as_slice()])?
      .len(),
    2
  );
  assert_eq!(
    db.sinterstore(b"set:inter", &[b"set:a".as_slice(), b"set:c".as_slice()])?,
    2
  );
  assert_eq!(
    db.sintercard(&[b"set:a".as_slice(), b"set:c".as_slice()], 10)?,
    2
  );

  // Set union operations
  // 集合并集计算与存储
  assert_eq!(
    db.sunion(&[b"set:a".as_slice(), b"set:c".as_slice()])?
      .len(),
    4
  );
  assert_eq!(
    db.sunionstore(b"set:union", &[b"set:a".as_slice(), b"set:c".as_slice()])?,
    4
  );
  assert_eq!(
    db.sunioncard(&[b"set:a".as_slice(), b"set:c".as_slice()], 10)?,
    4
  );

  // Set difference operations
  // 集合差集计算与存储
  assert_eq!(
    db.sdiff(&[b"set:a".as_slice(), b"set:c".as_slice()])?.len(),
    1
  );
  assert_eq!(
    db.sdiffstore(b"set:diff", &[b"set:a".as_slice(), b"set:c".as_slice()])?,
    1
  );
  assert_eq!(
    db.sdiffcard(&[b"set:a".as_slice(), b"set:c".as_slice()], 10)?,
    1
  );

  db.sadd_one(b"myset", b"one_elem")?;
  let _ = db.srem_one(b"myset", b"one_elem")?;
  let _ = db.spop_one(b"myset")?;
  let _ = db.srandmember_one(b"myset")?;

  println!("Set 示例全部接口执行成功");
  Ok(())
}
