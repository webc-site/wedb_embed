//! # JSON Document
//!
//! ## Overview
//! Provides native JSON document storage with full JSONPath query and update support.
//! Allows fine-grained manipulation of sub-paths without re-serializing entire documents.
//!
//! ## Use Cases
//! - Semi-structured document and profile storage
//! - Dynamic config management with deep path mutations
//! - Partial updates of complex nested business entities
//! - Schema-free analytics records
//!
//! ---
//!
//! # JSON 文档
//!
//! ## 概述
//! 提供原生 JSON 文档存储，全面支持 JSONPath 路径查询与局部更新。
//! 允许对深层嵌套路径进行读写而无需全量重新序列化整个文档。
//!
//! ## 使用场景
//! - 半结构化业务文档与用户配置
//! - 支持深层路径更新的动态配置中心
//! - 复杂嵌套业务实体的局部修改
//! - 无固定模式的应用数据存储

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  json::{JsonArrIndex, JsonSet},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Setting and formatted retrieval
  // 设置文档、带选项写入与格式化获取
  db.json_set_one(
    b"doc:1",
    "$",
    r#"{"user":{"name":"Alice","age":25,"active":true,"tags":["rust","db"]}}"#,
  )?;
  db.json_set(b"doc:1", "$.user.age", "26", [JsonSet::Xx])?;
  assert!(db.json_get_one(b"doc:1", "$.user.name")?.is_some());
  assert!(
    db.json_get_formatted(b"doc:1", &["$.user"], Some("  "), Some("\n"), Some(" "))?
      .is_some()
  );

  // Batch operations on JSON documents
  // 多文档批量设置与批量路径读取
  db.json_mset(&[(b"doc:2", "$", r#"{"val":100}"#)])?;
  assert_eq!(db.json_mget(&[b"doc:1", b"doc:2"], "$.user.name")?.len(), 2);

  // Type inspection and numerical modifications
  // 节点类型检测与数值原子计算
  let _ = db.json_type(b"doc:1", Some("$.user.name"))?;
  let _ = db.json_numincrby(b"doc:1", "$.user.age", "1.0")?;
  let _ = db.json_nummultby(b"doc:1", "$.user.age", "2.0")?;

  // String operations inside JSON documents
  // 字符串节点追加与长度计算
  let _ = db.json_strappend(b"doc:1", Some("$.user.name"), r#"" Smith""#)?;
  let _ = db.json_strlen(b"doc:1", Some("$.user.name"))?;

  // Array manipulation
  // 数组追加、插入、索引查找、长度、弹出与截断
  db.json_arrappend(b"doc:1", "$.user.tags", &[r#""lsm""#])?;
  db.json_arrinsert(b"doc:1", "$.user.tags", 0, &[r#""core""#])?;
  let _ = db.json_arrindex(
    b"doc:1",
    "$.user.tags",
    r#""rust""#,
    [JsonArrIndex::Start(0)],
  )?;
  let _ = db.json_arrlen(b"doc:1", Some("$.user.tags"))?;
  let _ = db.json_arrpop(b"doc:1", Some("$.user.tags"), None)?;
  let _ = db.json_arrtrim(b"doc:1", "$.user.tags", 0, 1)?;

  // Object inspection, boolean toggle, and memory debug
  // 布尔值翻转、对象合并、键列表、对象长度与内存信息
  let _ = db.json_toggle(b"doc:1", Some("$.user.active"))?;
  let _ = db.json_merge(b"doc:1", "$.user", r#"{"city":"Shanghai"}"#)?;
  let _ = db.json_objkeys(b"doc:1", Some("$.user"))?;
  let _ = db.json_objlen(b"doc:1", Some("$.user"))?;
  let _ = db.json_debug_memory(b"doc:1", None)?;
  let _ = db.json_info(b"doc:1")?;

  // Cleanup and deletion
  // 容器清空、忽略删除与全量删除
  db.json_clear(b"doc:1", Some("$.user.tags"))?;
  db.json_del(b"doc:1", Some("$.user.city"))?;
  db.json_del(b"doc:1", None)?;

  let _ = db.json_get(b"doc", &["$"], [])?;
  db.json_mset_one(b"doc", "$", r#"{"x":1}"#)?;

  println!("JSON 示例全部接口执行成功");
  Ok(())
}
