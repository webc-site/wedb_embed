//! # Bloom & Cuckoo Filter
//!
//! ## Overview
//! Probabilistic data structures for fast set membership testing with low memory overhead.
//! Includes Bloom filters for standard insertion/testing and Cuckoo filters which additionally support element deletion.
//!
//! ## Use Cases
//! - Cache penetration defense (pre-filtering nonexistent keys)
//! - Web crawler URL deduplication
//! - Recommendation system viewed-history filtering
//! - Membership testing with item deletion requirements (Cuckoo Filter)
//!
//! ---
//!
//! # 布隆与布谷鸟过滤器
//!
//! ## 概述
//! 用于高空间效率与常量时间集合成员存在性判断的概率数据结构。
//! 包含标准单向插入的布隆过滤器与额外支持元素删除的布谷鸟过滤器。
//!
//! ## 使用场景
//! - 缓存穿透防护与非命中请求前置过滤
//! - 网页爬虫海量链接去重
//! - 推荐系统已读历史记录过滤
//! - 需要动态删除元素的去重场景

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  bloom::{BfReserve, CfInsert, CfReserve},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Bloom filter: reservation, addition, and existence checks
  // 布隆过滤器预分配、添加与存在性判断
  db.bf_reserve(b"bf_1", 0.01, 1000, [BfReserve::Expansion(2)])?;
  assert!(db.bf_add(b"bf_1", b"alpha")?);
  assert_eq!(
    db.bf_madd(b"bf_1", &[b"beta".as_slice(), b"gamma".as_slice()])?,
    [true, true]
  );
  assert!(db.bf_exists(b"bf_1", b"alpha")?);
  assert_eq!(
    db.bf_mexists(b"bf_1", &[b"alpha".as_slice(), b"unknown".as_slice()])?,
    [true, false]
  );

  // Bloom filter insert options, metadata, and cardinality
  // 布隆过滤器高级插入、元信息查询与元素估算
  let _ = db.bf_insert(b"bf_1", &[b"delta"], [])?;
  let info = db.bf_info(b"bf_1")?;
  assert!(info.size >= 3);
  assert!(db.bf_card(b"bf_1")? >= 3);

  // Cuckoo filter: reservation, addition, and insert options
  // 布谷鸟过滤器预分配、添加与条件插入
  db.cf_reserve(
    b"cf_1",
    1000,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(500),
      CfReserve::Expansion(1),
    ],
  )?;
  db.cf_reserve(
    b"cf_ext",
    1000,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(500),
      CfReserve::Expansion(1),
      CfReserve::PageSize(4096),
    ],
  )?;
  assert!(db.cf_add(b"cf_1", b"elem_1")?);
  let _ = db.cf_addnx(b"cf_1", b"elem_2")?;
  let _ = db.cf_insert(b"cf_1", &[b"elem_3"], [])?;
  let _ = db.cf_insert(b"cf_1", &[b"elem_4"], [CfInsert::Nx])?;

  // Cuckoo filter existence, count, item deletion, and metadata
  // 布谷鸟过滤器存在判断、计数、元素删除与信息查询
  assert!(db.cf_exists(b"cf_1", b"elem_1")?);
  assert_eq!(
    db.cf_mexists(b"cf_1", &[b"elem_1".as_slice(), b"unknown".as_slice()])?,
    [true, false]
  );
  assert!(db.cf_count(b"cf_1", b"elem_1")? >= 1);
  assert!(db.cf_del(b"cf_1", b"elem_1")?);
  let _ = db.cf_info(b"cf_1")?;

  db.bf_reserve_one(b"bf_single", 0.01, 1000)?;
  db.bf_insert_one(b"bf_single", b"item1", [])?;
  db.cf_reserve_one(b"cf_single", 1000)?;
  db.cf_insert_one(b"cf_single", b"item1", [])?;
  db.cf_insertnx_one(b"cf_single", b"item2", [])?;
  let _ = db.cf_insertnx(b"cf_single", &[b"item3"], [])?;

  println!("Bloom & Cuckoo 示例全部接口执行成功");
  Ok(())
}
