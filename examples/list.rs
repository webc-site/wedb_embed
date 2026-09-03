//! # List
//!
//! ## Overview
//! The List structure is an ordered sequence of binary strings, supporting head and tail pushes/pops.
//! It implements dual-ended queue semantics and element positioning.
//!
//! ## Use Cases
//! - Asynchronous task queues and job processing
//! - Activity timelines and recent feed generation
//! - Fixed-capacity circular buffers via LTRIM
//! - Inter-thread work distribution
//!
//! ---
//!
//! # 列表
//!
//! ## 概述
//! 列表结构是有序的字符串序列，支持头尾两端的高效推入与弹出。
//! 实现了完整的双端队列语义、范围切片与元素定位功能。
//!
//! ## 使用场景
//! - 异步任务队列与作业处理
//! - 动态时间线与最新动态流
//! - 基于列表裁剪的定长消息缓冲区
//! - 生产者-消费者工作分发

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Push elements to head and tail
  // 双端推入元素
  db.lpush(b"mylist", &[b"item_b", b"item_a"])?;
  db.lpush(b"mylist", &[b"item_head"])?;
  db.rpush(b"mylist", &[b"item_c", b"item_d"])?;
  db.rpush(b"mylist", &[b"item_tail"])?;

  // Length, index, and range queries
  // 长度、索引与范围查询
  assert_eq!(db.llen(b"mylist")?, 6);
  assert_eq!(db.lindex(b"mylist", 0)?.as_deref(), Some(&b"item_head"[..]));
  assert_eq!(db.lrange(b"mylist", (0, -1))?.len(), 6);

  // Conditional push operations
  // 键存在时的条件推入
  db.lpushx(b"mylist", &[b"x_head"])?;
  db.lpushx(b"mylist", &[b"x_head2"])?;
  db.rpushx(b"mylist", &[b"x_tail"])?;
  db.rpushx(b"mylist", &[b"x_tail2"])?;

  // Insert, update, and find element position
  // 元素插入、按索引修改与位置定位
  db.linsert(b"mylist", true, b"item_a", b"inserted_before_a")?;
  db.lset(b"mylist", 0, b"first_elem")?;
  let _ = db.lpos(b"mylist", b"item_c", [])?;

  // Pop elements from head and tail
  // 双端弹出单个与多个元素
  assert_eq!(db.lpop(b"mylist", 2)?.len(), 2);
  assert_eq!(db.lpop(b"mylist", 1)?.len(), 1);
  assert_eq!(db.rpop(b"mylist", 2)?.len(), 2);
  assert_eq!(db.rpop(b"mylist", 1)?.len(), 1);

  // Element removal, trimming, and moving
  // 按值移除、列表裁剪与跨列表移动
  db.lpush(b"mylist", &[b"dup", b"dup"])?;
  db.lrem(b"mylist", 1, b"dup")?;
  db.ltrim(b"mylist", (0, 2))?;

  db.lmove(b"mylist", b"other_list", false, true)?;
  let _ = db.rpoplpush(b"other_list", b"mylist")?;

  db.lpush_one(b"mylist", b"single_head")?;
  db.rpush_one(b"mylist", b"single_tail")?;
  db.lpushx_one(b"mylist", b"single_xhead")?;
  db.rpushx_one(b"mylist", b"single_xtail")?;
  let _ = db.lpop_one(b"mylist")?;
  let _ = db.rpop_one(b"mylist")?;
  let _ = db.lpos_one(b"mylist", b"single_head", [])?;

  println!("List 示例全部接口执行成功");
  Ok(())
}
