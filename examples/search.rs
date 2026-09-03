//! # Full-Text Search & Vector Retrieval (RediSearch & Vector Search)
//!
//! ## Overview
//! Integrated search engine supporting schema definition, inverted full-text search, tag filtering,
//! numerical sorting, aggregation, and HNSW vector similarity search for AI embeddings.
//!
//! ## Use Cases
//! - E-commerce product search with faceted tag filters and price sorting
//! - Retrieval-Augmented Generation (RAG) and semantic vector search using embeddings
//! - Real-time query autocompletion suggestions (sug_add / sug_get)
//! - Aggregations and grouping pipelines over indexed JSON/Hash documents
//!
//! ---
//!
//! # 全文检索与向量检索
//!
//! ## 概述
//! 集成式检索引擎，支持索引模式定义、倒排全文检索、标签过滤、数值排序、聚合管道与面向人工智能向量嵌入的 HNSW 近似最近邻检索。
//!
//! ## 使用场景
//! - 电商商品检索、多维度标签过滤与价格区间排序
//! - 基于向量嵌入的检索增强生成与语义检索
//! - 搜索框实时自动补全建议字典
//! - 针对索引化结构文档的统计聚合管道

use anyhow::Result;
use wedb_embed::search::{
  DistanceMetric, FtAggregate, FtCreate, FtSearch, IndexField, IndexFieldType, IndexOnDataType,
  SearchIndexManager, SearchIndexSchema, VectorAlgorithm, VectorFieldMetadata, VectorType,
};

fn main() -> Result<()> {
  let mut search_mgr = SearchIndexManager::new();

  // Create search index schemas with text, tag, and vector fields
  // 创建包含文本、标签与 HNSW 向量字段的检索索引
  let mut schema_opts = FtCreate {
    index_name: "idx_articles".to_string(),
    on_data_type: IndexOnDataType::Json,
    ..Default::default()
  };
  schema_opts.prefixes.push("article:".to_string());
  schema_opts.fields.push(IndexField {
    name: "title".to_string(),
    alias: None,
    field_type: IndexFieldType::Text,
    weight: 1.0,
    sortable: false,
    noindex: false,
    separator: None,
    case_sensitive: false,
    unf: false,
    vector_meta: None,
  });
  schema_opts.fields.push(IndexField {
    name: "tag".to_string(),
    alias: None,
    field_type: IndexFieldType::Tag,
    weight: 1.0,
    sortable: false,
    noindex: false,
    separator: Some(','),
    case_sensitive: false,
    unf: false,
    vector_meta: None,
  });
  schema_opts.fields.push(IndexField {
    name: "vec".to_string(),
    alias: None,
    field_type: IndexFieldType::Vector,
    weight: 1.0,
    sortable: false,
    noindex: false,
    separator: None,
    case_sensitive: false,
    unf: false,
    vector_meta: Some(VectorFieldMetadata {
      algorithm: VectorAlgorithm::Hnsw,
      vector_type: VectorType::Float32,
      dim: 4,
      distance_metric: DistanceMetric::Cosine,
      m: 16,
      ef_construction: 200,
      ef_runtime: 10,
      initial_cap: 1000,
      epsilon: 0.01,
      num_levels: 16,
    }),
  });
  search_mgr.create_index(schema_opts)?;

  let schema_manual = SearchIndexSchema::from(FtCreate {
    index_name: "idx_books".to_string(),
    on_data_type: IndexOnDataType::Hash,
    ..Default::default()
  });
  search_mgr.create_index(schema_manual)?;

  // Index listing, resolution, and instance lookup
  // 索引列表、别名解析与索引实例获取
  assert!(!search_mgr.list_indexes().is_empty());
  assert_eq!(
    search_mgr.resolve_index_name("idx_articles"),
    "idx_articles"
  );
  assert!(search_mgr.get_index("idx_articles").is_some());
  assert!(search_mgr.get_index_mut("idx_articles").is_some());

  // Alias management: add, update, and delete
  // 索引别名添加、修改与删除
  search_mgr.add_alias("alias_art", "idx_articles")?;
  search_mgr.update_alias("alias_art", "idx_books")?;
  search_mgr.del_alias("alias_art")?;

  // Search, aggregation, explain plans, tag values, and info
  // 全文检索、聚合管道、查询执行计划、标签值与索引元信息
  let _ = search_mgr.search("idx_articles", "@title:database", &FtSearch::default())?;
  let _ = search_mgr.aggregate("idx_articles", &FtAggregate::default())?;
  let _ = search_mgr.explain("@title:database", &FtSearch::default());
  let _ = search_mgr.explain_cli("@title:database", &FtSearch::default());
  let _ = search_mgr.tag_vals("idx_articles", "tag")?;
  let _ = search_mgr.info("idx_articles")?;

  // Configuration parameters: get, set, and help
  // 检索引擎配置参数获取、修改与说明
  assert!(!search_mgr.config_get("TIMEOUT")?.is_empty());
  search_mgr.config_set("TIMEOUT", "1000")?;
  assert!(!search_mgr.config_help("TIMEOUT")?.is_empty());

  // Autocompletion suggestion dictionary: add, get, length, and delete
  // 自动补全建议字典添加、检索、长度与删除
  search_mgr.sug_add("sug_dict", "wedb_embed", 1.0, false, None);
  search_mgr.sug_add("sug_dict", "wedb_client", 0.8, false, None);
  assert_eq!(search_mgr.sug_len("sug_dict"), 2);
  assert_eq!(
    search_mgr
      .sug_get("sug_dict", "wedb", false, true, false, Some(5))
      .len(),
    2
  );
  assert!(search_mgr.sug_del("sug_dict", "wedb_client"));

  // Drop index
  // 删除索引
  let _ = search_mgr.drop_index("idx_books", false)?;

  println!("SearchIndexManager 示例全部接口执行成功");
  Ok(())
}
