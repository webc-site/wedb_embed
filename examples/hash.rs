//! # Hash Map
//!
//! ## Overview
//! The Hash structure represents a collection of field-value pairs under a single key.
//! It supports individual field-level TTL expiration and subkey indexing.
//!
//! ## Use Cases
//! - User profile and account attributes
//! - Object entity representation
//! - Field-level cache invalidation
//! - Configuration dictionaries
//!
//! ---
//!
//! # 哈希表
//!
//! ## 概述
//! 哈希表结构用于在单个键下存储多个字段与值的映射关系。
//! 原生支持独立的字段级过期时间与高效子键检索。
//!
//! ## 使用场景
//! - 用户信息与账户属性存储
//! - 结构化对象实体映射
//! - 字段级独立缓存失效
//! - 配置字典与动态元数据管理

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  hash::{HGetEx, HSet, HashLengthMode, RangeLex},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Basic field get, set, exists, and length
  // 基础字段读取、设置、存在判断与长度获取
  db.hset(
    b"hash:1",
    &[
      (b"name".as_slice(), b"Alice".as_slice()),
      (b"age".as_slice(), b"20".as_slice()),
    ],
  )?;
  assert_eq!(db.hget(b"hash:1", b"name")?.as_deref(), Some(&b"Alice"[..]));
  assert!(db.hexists(b"hash:1", b"name")?);
  assert_eq!(db.hlen(b"hash:1")?, 2);
  assert_eq!(db.hlen_with_mode(b"hash:1", HashLengthMode::Accurate)?, 2);

  // Fetch all, keys, and values
  // 获取全部字段、键列表与值列表
  assert_eq!(db.hgetall(b"hash:1")?.len(), 2);
  assert_eq!(db.hkeys(b"hash:1")?.len(), 2);
  assert_eq!(db.hvals(b"hash:1")?.len(), 2);

  // Field numerical increments
  // 字段数值自增与浮点增减
  assert_eq!(db.hincrby(b"hash:1", b"age", 1)?, 21);
  db.hset(b"hash:1", &[(b"score".as_slice(), b"88.5".as_slice())])?;
  assert!((db.hincrbyfloat(b"hash:1", b"score", 1.5)? - 90.0).abs() < 1e-6);

  // Batch operations and random field sampling
  // 批量操作、字段长度、条件设置与随机抽样
  db.hset(
    b"hash:1",
    &[
      (b"city".as_slice(), b"Beijing".as_slice()),
      (b"dept".as_slice(), b"Eng".as_slice()),
    ],
  )?;
  assert_eq!(
    db.hmget(b"hash:1", &[b"name".as_slice(), b"city".as_slice()])?
      .len(),
    2
  );
  assert_eq!(db.hstrlen(b"hash:1", b"name")?, 5);
  assert!(db.hsetnx(b"hash:1", b"new_f", b"v")?);
  assert!(!db.hrandfield(b"hash:1", 2, true)?.is_empty());

  // Streaming iteration and cursor scan
  // 流式迭代与游标扫描
  db.hiter(b"hash:1", |_, _| true)?;
  assert!(!db.hscan(b"hash:1", 0, 10, None)?.1.is_empty());

  // Field-level TTL expiration
  // 字段级过期控制与时间戳设置
  db.hexpire(b"hash:1", &[b"city".as_slice()], 3600, [])?;
  db.hpexpire(b"hash:1", &[b"dept".as_slice()], 3_600_000, [])?;
  db.hexpireat(b"hash:1", &[b"city".as_slice()], 2_000_000_000, [])?;
  db.hpexpireat(b"hash:1", &[b"dept".as_slice()], 2_000_000_000_000, [])?;
  // Field TTL inspection and persistence
  // 字段过期时间查询与移除过期
  assert_eq!(db.httl(b"hash:1", &[b"dept".as_slice()])?.len(), 1);
  assert_eq!(db.hpttl(b"hash:1", &[b"dept".as_slice()])?.len(), 1);
  assert_eq!(db.hexpiretime(b"hash:1", &[b"dept".as_slice()])?.len(), 1);
  assert_eq!(db.hpexpiretime(b"hash:1", &[b"dept".as_slice()])?.len(), 1);
  let _ = db.hpersist(b"hash:1", &[b"dept".as_slice()])?;

  // Extended get/delete and expire options
  // 扩展获取、删除与带过期字段设置
  assert_eq!(db.hgetdel(b"hash:1", &[b"new_f".as_slice()])?.len(), 1);
  let _ = db.hsetex(
    b"hash:1",
    &[(b"f_ex".as_slice(), b"v_ex".as_slice())],
    [HSet::Ex(3600)],
  )?;
  let _ = db.hgetex(b"hash:1", b"f_ex", [HGetEx::Persist])?;

  // Lexicographical range query and deletion
  // 字典序范围查询与字段删除
  assert!(!db.hrangebylex(b"hash:1", RangeLex::unbounded())?.is_empty());
  assert_eq!(
    db.hdel(b"hash:1", &[b"dept".as_slice(), b"city".as_slice()])?,
    2
  );

  db.hset_one(b"myhash", b"f_one", b"v_one")?;
  let _ = db.hdel_one(b"myhash", b"f_one")?;
  let _ = db.hexpire_one(b"myhash", b"f_exp", 60, [])?;
  let _ = db.hpexpire_one(b"myhash", b"f_exp", 60000, [])?;
  let _ = db.hexpireat_one(b"myhash", b"f_exp", 2000000000, [])?;
  let _ = db.hpexpireat_one(b"myhash", b"f_exp", 2000000000000, [])?;
  let _ = db.httl_one(b"myhash", b"f_exp")?;
  let _ = db.hpttl_one(b"myhash", b"f_exp")?;
  let _ = db.hexpiretime_one(b"myhash", b"f_exp")?;
  let _ = db.hpexpiretime_one(b"myhash", b"f_exp")?;
  let _ = db.hpersist_one(b"myhash", b"f_exp")?;
  let _ = db.hgetdel_one(b"myhash", b"f_exp")?;
  db.hsetex_one(b"myhash", b"f_ex", b"v", [wedb_embed::HSet::Ex(60)])?;
  let _ = db.hmget_ex(b"myhash", &[b"f_ex"], [])?;
  let _ = db.hrange_by_lex(b"myhash", wedb_embed::RangeLex::default())?;

  println!("Hash 示例全部接口执行成功");
  Ok(())
}
