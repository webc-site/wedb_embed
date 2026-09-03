//! # String (Key-Value)
//!
//! ## Overview
//! The String type is the fundamental binary-safe key-value data structure in wedb_embed.
//! It supports standard Redis string operations, atomic increments, TTL expiration, and CAS/CAD.
//!
//! ## Use Cases
//! - Session caching and token validation
//! - Distributed locking via NX/XX options and CAS
//! - Atomic high-throughput counters
//! - Binary payload storage with TTL
//! - Longest Common Subsequence (LCS) analysis
//!
//! ---
//!
//! # 字符串与键值
//!
//! ## 概述
//! 字符串是 wedb_embed 中最基础的二进制安全键值数据结构。
//! 支持完整的 Redis 字符串语义、原子自增、TTL 过期控制与比较交换操作。
//!
//! ## 使用场景
//! - 会话缓存与令牌校验
//! - 基于条件写入与比较交换的分布式锁
//! - 高吞吐原子计数器
//! - 带过期时间的二进制数据存储
//! - 最长公共子序列计算与文本比对

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  string::{DelEx, GetEx, StringMSet, StringSet},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Basic get, set, and get_with_expire
  // 基础读取、设置与带过期时间获取
  db.set(b"key1", b"hello world", [])?;
  assert_eq!(db.get(b"key1")?.as_deref(), Some(&b"hello world"[..]));

  let (v, exp) = db.get_with_expire(b"key1")?;
  assert_eq!(v.as_deref(), Some(&b"hello world"[..]));
  assert_eq!(exp, 0);

  // Opt and conditional writes: set_with, setex, setnx, setxx
  // 选项与条件写入
  db.set_with(b"key_args", b"val_args", &StringSet::default())?;
  assert_eq!(db.get(b"key_args")?.as_deref(), Some(&b"val_args"[..]));

  db.setex(b"temp_key", b"temp_val", 60_000)?;
  assert!(db.setnx(b"key_nx", b"nx_val")?);
  assert!(db.setxx(b"key_nx", b"xx_val", 0)?);

  // Extended get and conditional delete: getex, delex, getset, getdel
  // 扩展获取与条件删除
  let _ = db.getex(b"key_nx", [GetEx::Persist])?;
  assert!(db.delex(b"key_nx", [DelEx::IfEq(b"xx_val")])?);

  assert_eq!(
    db.getset(b"key1", b"new_hello")?.as_deref(),
    Some(&b"hello world"[..])
  );
  assert_eq!(db.getdel(b"key1")?.as_deref(), Some(&b"new_hello"[..]));

  // Atomic counters: incr, decr, incrby, decrby, incrby_ex, incrbyfloat
  // 原子计数器增减
  db.set(b"counter", b"10", [])?;
  assert_eq!(db.incr(b"counter")?, 11);
  assert_eq!(db.decr(b"counter")?, 10);
  assert_eq!(db.incrby(b"counter", 5)?, 15);
  assert_eq!(db.decrby(b"counter", 3)?, 12);
  assert_eq!(db.incrby_ex(b"counter", 8, 0, true)?, 20);

  db.set(b"float_c", b"10.5", [])?;
  assert!((db.incrbyfloat(b"float_c", 2.25)? - 12.75).abs() < 1e-6);
  assert!((db.incrbyfloat_ex(b"float_c", 0.25, 0, true)? - 13.0).abs() < 1e-6);

  // String manipulation: strlen, append, getrange, setrange
  // 字符串切片与追加
  db.set(b"greeting", b"Hello", [])?;
  assert_eq!(db.append(b"greeting", b" World")?, 11);
  assert_eq!(db.strlen(b"greeting")?, 11);
  assert_eq!(&db.getrange(b"greeting", (0, 4))?[..], b"Hello");
  let _ = db.setrange(b"greeting", 6, b"Rust!")?;
  assert_eq!(db.get(b"greeting")?.as_deref(), Some(&b"Hello Rust!"[..]));

  // Batch multi-key operations: mset, mget, mset_with, msetex, msetnx
  // 多键批量读写与设置
  db.mset(&[(b"m1", b"v1"), (b"m2", b"v2")])?;
  assert_eq!(db.mget(&[b"m1".as_slice(), b"m2".as_slice()])?.len(), 2);
  assert!(db.mset_with(&[(b"m3", b"v3")], StringMSet::default())?);
  db.msetex(&[(b"m4", b"v4")], 60_000)?;
  assert!(db.msetnx(&[(b"m5", b"v5")])?);

  // Compare-And-Swap & Compare-And-Delete: cas, cad
  // 原子比较交换与比较删除
  db.set(b"cas_key", b"state_0", [])?;
  assert_eq!(db.cas(b"cas_key", b"state_0", b"state_1", 0)?, 1);
  assert_eq!(db.cad(b"cas_key", b"state_1")?, 1);

  // Content digest & Longest Common Subsequence: digest, lcs
  // 校验摘要与最长公共子序列计算
  db.set(b"d_key", b"sample content", [])?;
  assert!(db.digest(b"d_key")?.is_some());

  db.set(b"str_a", b"OHMYMISTAKE", [])?;
  db.set(b"str_b", b"HEYMYMOMENT", [])?;
  let _ = db.lcs(b"str_a", b"str_b", [])?;

  println!("String 示例全部接口执行成功");
  db.set_one(b"k_one", b"v_one")?;
  Ok(())
}
