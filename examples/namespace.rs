//! # Multi-Tenant Namespace and Database Selection
//!
//! ## Overview
//! WeDb provides two levels of multi-tenant data isolation:
//! - Numerical namespaces (`u64`) for complete tenant partition and lifecycle isolation.
//! - Database numbering (`u64`) within each namespace (similar to Redis `SELECT db`).
//!
//! Operations use `wedb.ns(ns_id)?.db(db_id)?`, where passing `None` or `()` allocates auto-increment IDs, `rm()` deletes the current scope cascadingly, and `iter(begin)` enables streaming discovery.
//!
//! ## Use Cases
//! - Multi-tenant cloud SaaS database isolation
//! - Auto-increment tenant ID and database ID allocation
//! - Dynamic switching between tenant namespaces and databases
//! - Streaming discovery of active namespaces and databases from a given offset
//! - Scoped data purge and tenant-level cascading deletion and catalog deregistration
//!
//! ---
//!
//! # 多租户命名空间与多库选择
//!
//! ## 概述
//! WeDb 提供两级多租户数据逻辑隔离架构：
//! - 纯数字命名空间（`u64`）实现租户级物理前缀隔离与目录编排。
//! - 命名空间内的多数据库编号（`u64`），对标 Redis `SELECT db` 库选择语义。
//!
//! 支持通过 `wedb.ns(ns_id)?.db(db_id)?` 统一寻址与创建（传入数字寻址已存在 ID，传入 `None` 或 `()` 自动分配自增 ID），通过 `rm` 级联清空并注销，并通过 `iter` 从指定偏移纯流式发现实际存在的命名空间与数据库。
//!
//! ## 使用场景
//! - 多租户云原生 SaaS 数据库隔离
//! - 自动递增租户与数据库 ID 分配
//! - 租户命名空间与具体多库之间的灵活切换
//! - 纯流式发现所有实际存在的命名空间与数据库索引
//! - 库级与租户级级联数据清空与目录注销（rm）

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{Fjall, WeDb};

fn main() -> Result<()> {
  let dir = tempdir()?;

  // Open storage engine with optimal defaults and initialize WeDb
  // 使用最优化默认配置打开存储引擎并初始化 WeDb 实例
  let engine = Fjall::open(dir.path())?;
  let wedb = WeDb::new(engine);
  let db = wedb.ns(0)?.db(0)?;

  // Initial handle points to default namespace (0) and default database (0)
  // 初始句柄指向默认命名空间与默认数据库
  assert_eq!(db.ns_id(), 0);
  assert_eq!(db.id(), 0);
  assert!(db.is_default());

  // Allocate new unique namespace IDs automatically by passing None
  // 传入 None 自动分配全局唯一且递增的租户命名空间
  let ns_tenant1 = wedb.ns(None)?;
  let ns_tenant2 = wedb.ns(None)?;
  assert_eq!(ns_tenant1.id(), 1);
  assert_eq!(ns_tenant2.id(), 2);

  // Allocate next database ID automatically in default namespace by passing None
  // 在默认命名空间中传入 None 自动分配递增数据库
  let db_0_1 = wedb.ns(0)?.db(None)?;
  let db_0_2 = wedb.ns(0)?.db(None)?;
  assert_eq!(db_0_1.ns_id(), 0);
  assert_eq!(db_0_1.id(), 1);
  assert_eq!(db_0_2.ns_id(), 0);
  assert_eq!(db_0_2.id(), 2);

  // Allocate next database ID automatically in tenant 1 namespace by passing None
  // 在租户 1 命名空间中传入 None 自动分配递增数据库
  let db_t1_1 = ns_tenant1.db(None)?;
  let db_t1_2 = ns_tenant1.db(None)?;
  assert_eq!(db_t1_1.ns_id(), 1);
  assert_eq!(db_t1_1.id(), 1);
  assert_eq!(db_t1_2.ns_id(), 1);
  assert_eq!(db_t1_2.id(), 2);

  // Open specific database by ID
  // 寻址打开指定编号数据库
  let db_t1_5 = ns_tenant1.db(5)?;
  let db_t2_0 = wedb.ns(2)?.db(0)?;
  let db_t2_3 = wedb.ns(2)?.db(3)?;
  assert_eq!(db_t1_5.ns_id(), 1);
  assert_eq!(db_t1_5.id(), 5);
  assert_eq!(db_t2_0.ns_id(), 2);
  assert_eq!(db_t2_0.id(), 0);
  assert_eq!(db_t2_3.ns_id(), 2);
  assert_eq!(db_t2_3.id(), 3);

  // Write isolated keys across namespaces and databases
  // 在不同命名空间与数据库中写入完全隔离的数据
  db.set(b"user:name", b"root_admin", [])?;
  db_0_1.set(b"user:name", b"db1_admin", [])?;
  db_t1_1.set(b"user:name", b"alice", [])?;
  db_t1_1.hset(b"profile", &[(b"role", b"admin")])?;
  db_t1_5.set(b"user:name", b"alice_backup", [])?;
  db_t2_0.set(b"user:name", b"bob", [])?;
  db_t2_3.set(b"user:name", b"bob_staging", [])?;

  // Verify full data isolation
  // 验证跨命名空间与跨库的数据完全隔离
  assert_eq!(db.get(b"user:name")?, Some(b"root_admin".to_vec()));
  assert_eq!(db_0_1.get(b"user:name")?, Some(b"db1_admin".to_vec()));
  assert_eq!(db_t1_1.get(b"user:name")?, Some(b"alice".to_vec()));
  assert_eq!(db_t1_1.hget(b"profile", b"role")?, Some(b"admin".to_vec()));
  assert_eq!(db_t1_5.get(b"user:name")?, Some(b"alice_backup".to_vec()));
  assert_eq!(db_t2_0.get(b"user:name")?, Some(b"bob".to_vec()));
  assert_eq!(db_t2_3.get(b"user:name")?, Some(b"bob_staging".to_vec()));
  assert_eq!(db_t2_0.hget(b"profile", b"role")?, None);

  // Stream iterate all active namespaces from beginning
  // 纯流式迭代从起始位置开始的所有实际存在的命名空间
  let namespaces: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
  assert_eq!(namespaces, vec![0, 1, 2]);

  // Stream iterate namespaces from specified begin offset
  // 从指定起始偏移开始流式迭代命名空间
  let namespaces_offset: Vec<u64> = wedb.iter(1).map(|ns| ns.id()).collect();
  assert_eq!(namespaces_offset, vec![1, 2]);

  // Stream iterate all activated databases in tenant 1 from beginning
  // 纯流式迭代租户 1 下从起始位置开始的所有实际存在的数据库索引
  let t1_dbs: Vec<u64> = ns_tenant1.iter(0).collect();
  assert_eq!(t1_dbs, vec![0, 1, 2, 5]);

  // Stream iterate databases in tenant 1 from specified begin offset
  // 从指定起始偏移开始流式迭代租户 1 下的数据库索引
  let t1_dbs_offset: Vec<u64> = ns_tenant1.iter(2).collect();
  assert_eq!(t1_dbs_offset, vec![2, 5]);

  // Remove single database and verify catalog deregistration
  // 删除单个数据库并验证目录注销
  let removed_db_count = db_t1_5.rm()?;
  assert!(removed_db_count > 0);
  assert_eq!(db_t1_5.get(b"user:name")?, None);
  assert_eq!(db_t1_1.get(b"user:name")?, Some(b"alice".to_vec()));

  // Verify removed database does not appear in iteration
  // 验证已删除的数据库不再出现在迭代器中
  let t1_dbs_after_rm: Vec<u64> = ns_tenant1.iter(0).collect();
  assert_eq!(t1_dbs_after_rm, vec![0, 1, 2]);

  // Remove entire namespace and verify catalog deregistration
  // 删除整个租户命名空间并验证目录注销
  let removed_ns_count = ns_tenant1.rm()?;
  assert!(removed_ns_count > 0);
  assert_eq!(db_t1_1.get(b"user:name")?, None);
  assert_eq!(db_t1_1.hget(b"profile", b"role")?, None);

  // Verify removed namespace does not appear in iteration
  // 验证已删除的命名空间不再出现在迭代器中
  let namespaces_after_rm: Vec<u64> = wedb.iter(0).map(|ns| ns.id()).collect();
  assert_eq!(namespaces_after_rm, vec![0, 2]);

  // Verify other tenant and default namespace remain intact
  // 验证其他租户与默认命名空间的数据完好无损
  assert_eq!(db.get(b"user:name")?, Some(b"root_admin".to_vec()));
  assert_eq!(db_0_1.get(b"user:name")?, Some(b"db1_admin".to_vec()));
  assert_eq!(db_t2_0.get(b"user:name")?, Some(b"bob".to_vec()));
  assert_eq!(db_t2_3.get(b"user:name")?, Some(b"bob_staging".to_vec()));

  println!(
    "Namespace and database lifecycle demonstrated successfully:\n  - Remaining Namespaces: {:?}\n  - Root User: {}\n  - Tenant 2 User: {}",
    namespaces_after_rm,
    String::from_utf8_lossy(&db.get(b"user:name")?.unwrap()),
    String::from_utf8_lossy(&db_t2_0.get(b"user:name")?.unwrap())
  );

  Ok(())
}
