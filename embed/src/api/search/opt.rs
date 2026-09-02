use hipstr::HipStr;
use rapidhash::RapidHashMap as HashMap;

use super::meta::{IndexField, IndexOnDataType, SearchIndexSchema};

/// FT.CREATE command options.
/// FT.CREATE 命令选项
#[derive(Debug, Clone, PartialEq)]
pub struct FtCreate {
  pub index_name: String,
  pub on_data_type: IndexOnDataType,
  pub prefixes: Vec<String>,
  pub filter: Option<String>,
  pub default_score: f64,
  pub score_field: Option<String>,
  pub payload_field: Option<String>,
  pub language: Option<String>,
  pub language_field: Option<String>,
  pub max_text_fields: bool,
  pub no_offsets: bool,
  pub no_hl: bool,
  pub no_fields: bool,
  pub no_freqs: bool,
  pub stop_words: Vec<String>,
  pub fields: Vec<IndexField>,
}

impl Default for FtCreate {
  fn default() -> Self {
    Self {
      index_name: String::new(),
      on_data_type: IndexOnDataType::Hash,
      prefixes: Vec::new(),
      filter: None,
      default_score: 1.0,
      score_field: None,
      payload_field: None,
      language: None,
      language_field: None,
      max_text_fields: false,
      no_offsets: false,
      no_hl: false,
      no_fields: false,
      no_freqs: false,
      stop_words: Vec::new(),
      fields: Vec::new(),
    }
  }
}

impl From<FtCreate> for SearchIndexSchema {
  fn from(c: FtCreate) -> Self {
    Self {
      name: c.index_name,
      on_data_type: c.on_data_type,
      prefixes: c.prefixes,
      filter: c.filter,
      default_score: c.default_score,
      score_field: c.score_field,
      payload_field: c.payload_field,
      language: c.language,
      language_field: c.language_field,
      max_text_fields: c.max_text_fields,
      no_offsets: c.no_offsets,
      no_hl: c.no_hl,
      no_fields: c.no_fields,
      no_freqs: c.no_freqs,
      stop_words: c.stop_words,
      fields: c.fields,
    }
  }
}

/// FT.SEARCH command options.
/// FT.SEARCH 命令选项
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FtSearch {
  pub nocontent: bool,
  pub verbatim: bool,
  pub nostopwords: bool,
  pub withscores: bool,
  pub withpayloads: bool,
  pub withsortkeys: bool,
  pub filter: Vec<(String, f64, f64)>, // (field, min, max)
  pub geofilter: Vec<(String, f64, f64, f64, String)>, // (field, lon, lat, radius, unit)
  pub inkeys: Vec<String>,
  pub infields: Vec<String>,
  pub returns: Vec<(String, Option<String>)>, // (field, alias)
  pub sortby: Option<(String, bool)>,         // (field, asc)
  pub limit: Option<(usize, usize)>,          // (offset, count)
  pub params: HashMap<String, String>,
  pub dialect: Option<u32>,
  pub slop: Option<usize>,
  pub inorder: bool,
  pub language: Option<String>,
  pub expander: Option<String>,
  pub scorer: Option<String>,
  pub explain_cli: bool,
  pub timeout: Option<u64>,
}

/// FT.DROPINDEX command options.
/// FT.DROPINDEX 命令选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FtDropIndex {
  pub dd: bool, // Delete Documents
}

/// FT.INFO index schema definition.
/// FT.INFO 索引定义
#[derive(Debug, Clone, PartialEq)]
pub struct FtIndexDefinition {
  pub key_type: String,
  pub prefixes: Vec<String>,
  pub filter: Option<String>,
  pub default_score: f64,
  pub language: Option<String>,
}

/// FT.INFO field details.
/// FT.INFO 字段详情
#[derive(Debug, Clone, PartialEq)]
pub struct FtFieldInfo {
  pub identifier: String,
  pub attribute: Option<String>,
  pub field_type: String,
  pub properties: Vec<(String, String)>,
}

/// FT.INFO index details response.
/// FT.INFO 索引详情响应
#[derive(Debug, Clone, PartialEq)]
pub struct FtInfo {
  pub index_name: String,
  pub index_options: Vec<String>,
  pub index_definition: FtIndexDefinition,
  pub fields: Vec<FtFieldInfo>,
  pub num_docs: usize,
  pub max_doc_id: usize,
  pub num_terms: usize,
  pub num_records: usize,
  pub inverted_sz_mb: f64,
  pub vector_index_sz_mb: f64,
  pub total_inverted_index_blocks: usize,
  pub offset_vectors_sz_mb: f64,
  pub doc_table_size_mb: f64,
  pub sortable_values_size_mb: f64,
  pub key_table_size_mb: f64,
  pub records_per_doc_avg: f64,
  pub bytes_per_record_avg: f64,
  pub offsets_per_term_avg: f64,
  pub offset_bits_per_record_avg: f64,
  pub hash_indexing_failures: usize,
  pub indexing: bool,
  pub percent_indexed: f64,
}

/// FT.CONFIG command.
/// FT.CONFIG 命令
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FtConfigCommand {
  Get(String),
  Set(String, String),
  Help(String),
}

/// FT.SUGADD autocomplete suggestion add options.
/// FT.SUGADD 自动补全添加
#[derive(Debug, Clone, PartialEq)]
pub struct FtSugAdd {
  pub key: String,
  pub string: String,
  pub score: f64,
  pub incr: bool,
  pub payload: Option<String>,
}

/// FT.SUGGET autocomplete suggestion query options.
/// FT.SUGGET 自动补全检索
#[derive(Debug, Clone, PartialEq)]
pub struct FtSugGet {
  pub key: String,
  pub prefix: String,
  pub fuzzy: bool,
  pub withscores: bool,
  pub withpayloads: bool,
  pub max: Option<usize>,
}

/// Autocomplete suggestion item entry.
/// 建议项条目
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestionItem {
  pub string: String,
  pub score: f64,
  pub payload: Option<String>,
}

/// Single search hit document.
/// 单个搜索命中文档
#[derive(Debug, Clone, PartialEq)]
pub struct SearchDoc {
  pub id: HipStr<'static>,
  pub score: f64,
  pub payload: Option<Vec<u8>>,
  pub sort_key: Option<String>,
  pub fields: Vec<(HipStr<'static>, HipStr<'static>)>,
}

/// FT.AGGREGATE aggregation functions (aligned with RediSearch and Apache Kvrocks).
/// FT.AGGREGATE 聚合函数（对标 RediSearch 与 Apache Kvrocks 聚合支持）
#[derive(Debug, Clone, PartialEq)]
pub enum FtReducer {
  Count,
  Sum(String),
  Min(String),
  Max(String),
  Avg(String),
  CountDistinct(String),
  FirstValue(String),
  ToList(String),
}

/// FT.AGGREGATE grouping specification.
/// FT.AGGREGATE 分组规格
#[derive(Debug, Clone, PartialEq)]
pub struct FtGroupBy {
  pub fields: Vec<String>,
  pub reducers: Vec<(FtReducer, Option<String>)>, // (reducer, as_name)
}

/// FT.AGGREGATE command options.
/// FT.AGGREGATE 命令选项
#[derive(Debug, Clone, PartialEq)]
pub struct FtAggregate {
  pub query: String,
  pub load_fields: Vec<String>,
  pub groupbys: Vec<FtGroupBy>,
  pub sortby: Vec<(String, bool)>,  // (field, asc)
  pub apply: Vec<(String, String)>, // (expr, alias)
  pub filter: Option<String>,
  pub limit: Option<(usize, usize)>, // (offset, count)
  pub params: HashMap<String, String>,
  pub dialect: Option<u32>,
}

impl Default for FtAggregate {
  fn default() -> Self {
    Self {
      query: "*".to_string(),
      load_fields: Vec::new(),
      groupbys: Vec::new(),
      sortby: Vec::new(),
      apply: Vec::new(),
      filter: None,
      limit: None,
      params: HashMap::default(),
      dialect: None,
    }
  }
}

/// Single aggregate result row.
/// 单条聚合记录行
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AggregateRow {
  pub fields: Vec<(String, String)>,
}

/// Aggregate result collection.
/// 聚合结果集合
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AggregateResult {
  pub total_results: usize,
  pub rows: Vec<AggregateRow>,
}

/// FT._LIST command options.
/// FT._LIST 命令选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FtList;

/// FT.ALIASADD command options.
/// FT.ALIASADD 命令选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtAliasAdd {
  pub alias: String,
  pub index_name: String,
}

/// FT.ALIASDEL command options.
/// FT.ALIASDEL 命令选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtAliasDel {
  pub alias: String,
}

/// FT.ALIASUPDATE command options.
/// FT.ALIASUPDATE 命令选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtAliasUpdate {
  pub alias: String,
  pub index_name: String,
}

/// FT.TAGVALS command options.
/// FT.TAGVALS 命令选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtTagVals {
  pub index_name: String,
  pub field_name: String,
}

/// FT.SUGDEL command options.
/// FT.SUGDEL 命令选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtSugDel {
  pub key: String,
  pub string: String,
}

/// FT.SUGLEN command options.
/// FT.SUGLEN 命令选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtSugLen {
  pub key: String,
}

/// Search hit result collection.
/// 搜索结果集合
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchResult {
  pub total_results: usize,
  pub docs: Vec<SearchDoc>,
}
