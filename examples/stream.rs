//! # Stream & Consumer Groups
//!
//! ## Overview
//! Append-only log data structure with time-based entry IDs.
//! Supports consumer groups, message acknowledgement (XACK), pending entry lists (PEL), and consumer claiming (XAUTOCLAIM).
//!
//! ## Use Cases
//! - Event-driven architectures and distributed event sourcing
//! - Persistent multi-consumer message queue systems
//! - Real-time activity feeds with exactly-once / at-least-once delivery guarantees
//! - Fault-tolerant worker task claiming and crash recovery
//!
//! ---
//!
//! # 消息流与消费组
//!
//! ## 概述
//! 基于单调自增时间戳标识的持久化追加日志数据结构。
//! 原生支持多消费组负载分发、消息确认机制、待处理列表追踪与故障转移自动认领。
//!
//! ## 使用场景
//! - 事件驱动架构与分布式事件溯源
//! - 支持多消费者的持久化消息队列
//! - 具备至少一次投递保证的实时活动流
//! - 具备故障恢复与超时认领的工作流分配

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  stream::{
    StreamAdd, StreamAutoClaim, StreamClaim, StreamId, StreamPending, StreamRange, StreamTrim,
  },
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Append entries and check length
  // 追加日志条目、获取末尾标识与流长度
  let id1 = db.xadd(
    b"stream_1",
    (),
    &[
      (b"sensor".as_slice(), b"temp".as_slice()),
      (b"val".as_slice(), b"23.5".as_slice()),
    ],
  )?;
  let id2 = db.xadd(
    b"stream_1",
    StreamAdd::default(),
    &[
      (b"sensor".as_slice(), b"humidity".as_slice()),
      (b"val".as_slice(), b"60.0".as_slice()),
    ],
  )?;

  assert_eq!(db.xlast_id(b"stream_1")?, id2);
  assert_eq!(db.xlen(b"stream_1")?, 2);

  // Range queries across entry IDs
  // 正向与反向时间区间范围读取
  assert_eq!(
    db.xrange(b"stream_1", (StreamId::min(), StreamId::max()))?
      .len(),
    2
  );
  assert_eq!(
    db.xrevrange(b"stream_1", (StreamId::max(), StreamId::min()))?
      .len(),
    2
  );
  assert_eq!(db.xrange(b"stream_1", StreamRange::default())?.len(), 2);

  // Standalone stream reading
  // 独立读取与多流并发读取
  assert_eq!(db.xread(b"stream_1", StreamId::min(), Some(5))?.len(), 2);
  assert_eq!(
    db.xread_streams(&[("stream_1", StreamId::min())], Some(5))?
      .len(),
    1
  );

  // Consumer group management
  // 消费组与消费者创建、信息查询与进度指针重置
  db.xgroup_create(b"stream_1", "workers", "0-0", false, None)?;
  let _ = db.xgroup_create_consumer(b"stream_1", "workers", "c1")?;
  assert_eq!(db.xinfo_groups(b"stream_1")?.len(), 1);
  assert_eq!(db.xinfo_consumers(b"stream_1", "workers")?.len(), 1);
  db.xgroup_set_id(b"stream_1", "workers", "0-0", None)?;

  // Consumer group reading, acknowledging, and claiming
  // 消费组消息读取、确认、待处理统计与超时认领
  assert_eq!(
    db.xreadgroup(b"stream_1", "workers", "c1", ">", Some(5), false)?
      .len(),
    2
  );
  let id3 = db.xadd(
    b"stream_1",
    (),
    &[
      (b"sensor".as_slice(), b"press".as_slice()),
      (b"val".as_slice(), b"101.3".as_slice()),
    ],
  )?;
  assert_eq!(
    db.xreadgroup_streams("workers", "c1", &[("stream_1", ">")], Some(5), false)?
      .len(),
    1
  );

  assert!(db.xpending_summary(b"stream_1", "workers")?.pending_number > 0);
  assert!(
    !db
      .xpending_range(b"stream_1", "workers", StreamPending::default())?
      .is_empty()
  );

  let _ = db.xclaim(
    b"stream_1",
    "workers",
    "c1",
    0,
    &[id1],
    StreamClaim::default(),
  )?;
  let _ = db.xautoclaim(b"stream_1", "workers", "c1", StreamAutoClaim::default())?;
  assert!(db.xack(b"stream_1", "workers", &[id1, id2])? > 0);

  // Stream info, trimming, deletion, and group destruction
  // 流元信息、修剪裁剪、条目删除与消费组销毁
  assert!(db.xinfo_stream(b"stream_1", true, None)?.size > 0);
  db.xsetid(b"stream_1", StreamId::new(id3.ms + 1000, 0), None, None)?;
  let _ = db.xtrim(b"stream_1", StreamTrim::default())?;
  let _ = db.xdel(b"stream_1", &[id1])?;
  let _ = db.xgroup_del_consumer(b"stream_1", "workers", "c1")?;
  let _ = db.xgroup_destroy(b"stream_1", "workers")?;

  println!("Stream 示例全部接口执行成功");
  Ok(())
}
