//! # Sorted Set (ZSet)
//!
//! ## Overview
//! The Sorted Set (ZSet) is a collection of unique members associated with floating-point scores.
//! Members are ordered by score in ascending order, enabling fast rank, range, and score queries.
//!
//! ## Use Cases
//! - Real-time gaming leaderboards and ranking systems
//! - Priority task queues and delayed scheduling
//! - Sliding window rate limiters
//! - Price/index range filtering
//!
//! ---
//!
//! # 有序集合
//!
//! ## 概述
//! 有序集合结构存储唯一的字符串成员及其关联的浮点分值。
//! 元素按分值自动升序排列，支持根据排名、分值与字典序进行高速范围查找。
//!
//! ## 使用场景
//! - 实时游戏排行榜与积分排名系统
//! - 优先级任务队列与延迟调度
//! - 滑动窗口限流器
//! - 价格与指标分值区间过滤

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  zset::{Aggregate, RangeLex, RangeScore, ZAdd},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Add members, count, score queries, and increments
  // 添加成员、基数统计、分值查询与分值自增
  db.zadd(
    b"leaderboard",
    &[
      (100.0, b"alice".as_slice()),
      (200.0, b"bob".as_slice()),
      (300.0, b"charlie".as_slice()),
    ],
    [ZAdd::Nx],
  )?;
  assert_eq!(db.zcard(b"leaderboard")?, 3);
  assert_eq!(db.zscore(b"leaderboard", b"alice")?, Some(100.0));
  assert_eq!(
    db.zmscore(b"leaderboard", &[b"alice".as_slice(), b"bob".as_slice()])?,
    [Some(100.0), Some(200.0)]
  );
  assert_eq!(
    db.zmget(b"leaderboard", &[b"alice".as_slice(), b"bob".as_slice()])?
      .len(),
    2
  );
  assert_eq!(db.zincrby(b"leaderboard", 50.0, b"alice")?, 150.0);

  // Rank queries: standard and reverse
  // 正向与反向排名查询
  assert!(db.zrank(b"leaderboard", b"alice")?.is_some());
  assert!(db.zrank_with_score(b"leaderboard", b"alice")?.is_some());
  assert!(db.zrevrank(b"leaderboard", b"alice")?.is_some());
  assert!(db.zrevrank_with_score(b"leaderboard", b"alice")?.is_some());

  // Index range queries: forward, reverse, and fetch all
  // 按索引正向与反向范围获取
  assert_eq!(db.zrange(b"leaderboard", b"0", b"-1", [])?.len(), 3);
  assert_eq!(db.zrevrange(b"leaderboard", (0, -1))?.len(), 3);
  assert_eq!(db.zget_all(b"leaderboard")?.len(), 3);

  // Score range queries and count
  // 按分值区间统计与范围查询
  let score_spec = RangeScore::new(100.0, 250.0);
  assert_eq!(db.zcount(b"leaderboard", score_spec)?, 2);
  assert_eq!(db.zrangebyscore(b"leaderboard", score_spec)?.len(), 2);
  assert_eq!(db.zrevrangebyscore(b"leaderboard", score_spec)?.len(), 2);

  // Lexicographical range queries and count
  // 按字典序区间统计与范围查询
  let lex_spec = RangeLex::unbounded();
  let _ = db.zlexcount(b"leaderboard", &lex_spec)?;
  let _ = db.zrangebylex(b"leaderboard", &lex_spec)?;
  let _ = db.zrangebylex_with_scores(b"leaderboard", &lex_spec)?;
  let _ = db.zrevrangebylex(b"leaderboard", &lex_spec)?;
  let _ = db.zrevrangebylex_with_scores(b"leaderboard", &lex_spec)?;

  // Streaming iterators: full, members, score range, and lex range
  // 全量、成员、分值与字典序流式迭代器
  db.ziter(b"leaderboard", |_, _| true)?;
  db.ziter_rev(b"leaderboard", |_, _| true)?;
  db.ziter_members(b"leaderboard", |_, _| true)?;
  db.ziter_members_rev(b"leaderboard", |_, _| true)?;
  db.ziter_range_byscore(b"leaderboard", &score_spec, |_, _| true)?;
  db.ziter_range_byscore_rev(b"leaderboard", &score_spec, |_, _| true)?;
  db.ziter_range_bylex(b"leaderboard", &lex_spec, |_, _| true)?;
  db.ziter_range_bylex_rev(b"leaderboard", &lex_spec, |_, _| true)?;

  // Extremum pop, random sampling, and generalized range spec
  // 极值弹出、随机抽样与泛型范围查询
  let _ = db.zrange(b"leaderboard", b"-inf", b"+inf", [])?;
  let _ = db.zrandmember(b"leaderboard", 1)?;
  assert_eq!(db.zpopmin(b"leaderboard", 1)?.len(), 1);
  assert_eq!(db.zpopmax(b"leaderboard", 1)?.len(), 1);
  let _ = db.bzpopmin(&[b"leaderboard".as_slice()])?;
  let _ = db.bzpopmax(&[b"leaderboard".as_slice()])?;

  // Overwrite and range deletion by rank, score, or lex
  // 全量覆盖以及按排名、分值与字典序范围删除
  db.overwrite_zset(
    b"leaderboard",
    &[
      (b"dave".as_slice(), 400.0),
      (b"eva".as_slice(), 500.0),
      (b"frank".as_slice(), 600.0),
    ],
  )?;
  let _ = db.zremrangebyrank(b"leaderboard", (0, 0))?;
  let _ = db.zremrangebyscore(b"leaderboard", RangeScore::new(450.0, 550.0))?;
  let _ = db.zremrangebylex(b"leaderboard", RangeLex::default())?;
  db.zadd(b"leaderboard", &[(700.0, b"grace".as_slice())], [])?;
  db.zrem(b"leaderboard", &[b"grace".as_slice()])?;

  // Sorted set algebra and scanning
  // 有序集合交并差运算与游标扫描
  db.zadd(
    b"z1",
    &[(10.0, b"a".as_slice()), (20.0, b"b".as_slice())],
    [],
  )?;
  db.zadd(
    b"z2",
    &[(20.0, b"b".as_slice()), (30.0, b"c".as_slice())],
    [],
  )?;
  let _ = db.zunion(
    &[(b"z1".as_slice(), 1.0), (b"z2".as_slice(), 1.0)],
    Aggregate::Sum,
  )?;
  let _ = db.zunionstore(
    b"z_u",
    &[(b"z1".as_slice(), 1.0), (b"z2".as_slice(), 1.0)],
    Aggregate::Sum,
  )?;
  let _ = db.zinter(
    &[(b"z1".as_slice(), 1.0), (b"z2".as_slice(), 1.0)],
    Aggregate::Sum,
  )?;
  let _ = db.zinterstore(
    b"z_i",
    &[(b"z1".as_slice(), 1.0), (b"z2".as_slice(), 1.0)],
    Aggregate::Sum,
  )?;
  let _ = db.zintercard(&[b"z1".as_slice(), b"z2".as_slice()], 10)?;
  let _ = db.zdiff(&[b"z1".as_slice(), b"z2".as_slice()])?;
  let _ = db.zdiffstore(b"z_d", &[b"z1".as_slice(), b"z2".as_slice()])?;
  let _ = db.zscan(b"z1", 0, None, Some(10))?;

  db.zadd_one(b"leaderboard", 1000.0, b"one_m", [])?;
  let _ = db.zrangebyrank(b"leaderboard", (0, -1))?;
  let _ = db.zpopmin_one(b"leaderboard")?;
  let _ = db.zpopmax_one(b"leaderboard")?;
  let _ = db.zrem_one(b"leaderboard", b"one_m")?;

  println!("ZSet 示例全部接口执行成功");
  Ok(())
}
