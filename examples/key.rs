//! # Key Management
//!
//! ## Overview
//! Provides generic key-level operations spanning all data structures.
//! Supports key existence tests, pattern matching, data type queries, global TTL expiration, and cross-type deletion.
//!
//! ## Use Cases
//! - General keyspace lifecycle and expiry management
//! - Pattern-based key discovery and cleanup
//! - Storage metadata inspection and data type identification
//! - Multi-key atomic purging across diverse data models
//!
//! ---
//!
//! # 键空间与元数据管理
//!
//! ## 概述
//! 提供适用于所有数据结构的通用键级生命周期与元数据操作。
//! 支持键存在性检查、通配符模式匹配、数据类型识别、全局过期时间设置与跨类型删除。
//!
//! ## 使用场景
//! - 数据库键空间生命周期与过期维护
//! - 基于通配模式的键发现与归档清理
//! - 存储元数据检查与结构类型识别
//! - 跨数据结构的批量原子清理

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Prepare keys of different data types
  // 写入不同数据结构的测试键
  db.set(b"k_str", b"v_str", [])?;
  db.hset(b"k_hash", &[(b"f".as_slice(), b"v".as_slice())])?;
  db.lpush(b"k_list", &[b"v".as_slice()])?;

  // Existence, pattern matching, count, and type inspection
  // 键存在判断、通配符匹配、键总量统计与类型识别
  assert_eq!(
    db.exists(&[
      b"k_str".as_slice(),
      b"k_hash".as_slice(),
      b"nonexistent".as_slice()
    ])?,
    2
  );
  assert_eq!(db.keys("k_*")?.len(), 3);
  assert_eq!(db.key_count()?, 3);
  assert_eq!(db.type_of(b"k_str")?, "string");

  // Expiration management and persistence
  // 键过期时间设置、TTL查询、过期时间戳获取与持久化
  db.expire(b"k_str", 3600)?;
  assert!(db.ttl(b"k_str")? > 0);
  assert!(db.pttl(b"k_str")? > 0);
  assert!(db.get_key_expire_at(b"k_str")?.is_some());
  db.expireat(b"k_str", 2000000000000)?;
  assert!(db.persist(b"k_str")?);

  // Multi-key deletion across types
  // 批量删除多种数据类型的键
  assert_eq!(
    db.del(&[
      b"k_str".as_slice(),
      b"k_hash".as_slice(),
      b"k_list".as_slice()
    ])?,
    3
  );

  let _ = db.del_one(b"mykey")?;
  let _ = db.exists_one(b"mykey")?;
  let _ = db.pexpire(b"mykey", 60000)?;
  let _ = db.pexpireat(b"mykey", 2000000000000)?;
  let _ = db.expiretime(b"mykey")?;
  let _ = db.pexpiretime(b"mykey")?;

  println!("Key 示例全部接口执行成功");
  Ok(())
}
