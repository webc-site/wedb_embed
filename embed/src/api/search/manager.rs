use hipstr::HipStr;
use rapidhash::RapidHashMap as HashMap;

use crate::{
  error::{Error, Result},
  search::{
    ast::{explain_search_query, explain_search_query_cli, parse_search_query_with_params},
    index::InvertedIndex,
    meta::SearchIndexSchema,
    opt::{AggregateResult, FtAggregate, FtInfo, FtSearch, SearchResult, SuggestionItem},
    sug::SuggestionDict,
  },
};

/// Search index manager (aligned with Apache Kvrocks kqir::IndexMap and IndexManager).
/// 索引管理器（对标 Apache Kvrocks kqir::IndexMap 与 IndexManager）
#[derive(Debug, Clone, Default)]
pub struct SearchIndexManager {
  pub indexes: HashMap<HipStr<'static>, (SearchIndexSchema, InvertedIndex)>,
  pub aliases: HashMap<HipStr<'static>, HipStr<'static>>,
  pub suggestions: HashMap<HipStr<'static>, SuggestionDict>,
  pub configs: HashMap<String, String>,
}

impl SearchIndexManager {
  pub fn new() -> Self {
    let mut configs = HashMap::default();
    configs.insert("TIMEOUT".to_string(), "500".to_string());
    configs.insert("DEFAULT_DIALECT".to_string(), "2".to_string());
    configs.insert("MINPREFIX".to_string(), "2".to_string());
    configs.insert("MAXEXPANSIONS".to_string(), "200".to_string());
    Self {
      indexes: HashMap::default(),
      aliases: HashMap::default(),
      suggestions: HashMap::default(),
      configs,
    }
  }

  /// Creates an index (FT.CREATE).
  /// 创建索引（FT.CREATE）
  pub fn create_index(&mut self, schema: impl Into<SearchIndexSchema>) -> Result<()> {
    let schema = schema.into();
    let name = schema.name.clone();
    if self.indexes.contains_key(name.as_str()) {
      return Err(Error::invalid_data(format!(
        "Index '{name}' already exists"
      )));
    }
    self
      .indexes
      .insert(name.into(), (schema, InvertedIndex::new()));
    Ok(())
  }

  /// Drops an index (FT.DROPINDEX).
  /// 删除索引（FT.DROPINDEX）
  pub fn drop_index(&mut self, index_name: &str, dd: bool) -> Result<Vec<HipStr<'static>>> {
    let resolved_key = if let Some(target) = self.aliases.get(index_name) {
      target.clone()
    } else {
      HipStr::from(index_name)
    };

    let removed = self.indexes.remove(&resolved_key);
    if let Some((_, inverted)) = removed {
      // 清理对应的别名
      self.aliases.retain(|_, target| target != &resolved_key);

      if dd {
        let doc_ids: Vec<HipStr<'static>> = inverted.docs.keys().cloned().collect();
        Ok(doc_ids)
      } else {
        Ok(Vec::new())
      }
    } else {
      Err(Error::invalid_data(format!("Unknown index '{index_name}'")))
    }
  }

  /// Retrieves reference to index and inverted table.
  /// 获取索引及其倒排表引用
  #[inline]
  pub fn get_index(&self, index_name: &str) -> Option<&(SearchIndexSchema, InvertedIndex)> {
    let resolved = self.resolve_index_name(index_name);
    self.indexes.get(resolved)
  }

  /// Retrieves mutable reference to index and inverted table.
  /// 获取索引及其倒排表可变引用
  #[inline]
  pub fn get_index_mut(
    &mut self,
    index_name: &str,
  ) -> Option<&mut (SearchIndexSchema, InvertedIndex)> {
    if let Some(target) = self.aliases.get(index_name) {
      self.indexes.get_mut(target.as_str())
    } else {
      self.indexes.get_mut(index_name)
    }
  }

  /// Lists all index names in the current database (FT._LIST).
  /// 列出所有索引名称（FT._LIST）
  pub fn list_indexes(&self) -> Vec<String> {
    let mut list: Vec<String> = self.indexes.keys().map(HipStr::to_string).collect();
    list.sort();
    list
  }

  /// Resolves index name supporting index aliases.
  /// 解析索引名称（支持别名解析）
  #[inline]
  pub fn resolve_index_name<'a>(&'a self, alias_or_name: &'a str) -> &'a str {
    if let Some(target) = self.aliases.get(alias_or_name) {
      target.as_str()
    } else {
      alias_or_name
    }
  }

  /// Adds an alias for an index (FT.ALIASADD).
  /// 添加别名（FT.ALIASADD）
  pub fn add_alias(&mut self, alias: &str, index_name: &str) -> Result<()> {
    if self.aliases.contains_key(alias) {
      return Err(Error::invalid_data(format!(
        "Alias '{alias}' already exists"
      )));
    }
    if !self.indexes.contains_key(index_name) {
      return Err(Error::invalid_data(format!(
        "Index '{index_name}' not found"
      )));
    }
    self
      .aliases
      .insert(HipStr::from(alias), HipStr::from(index_name));
    Ok(())
  }

  /// Deletes an alias (FT.ALIASDEL).
  /// 删除别名（FT.ALIASDEL）
  pub fn del_alias(&mut self, alias: &str) -> Result<()> {
    if self.aliases.remove(alias).is_none() {
      return Err(Error::invalid_data(format!("Alias '{alias}' not found")));
    }
    Ok(())
  }

  /// Updates an alias to point to another index (FT.ALIASUPDATE).
  /// 更新别名（FT.ALIASUPDATE）
  pub fn update_alias(&mut self, alias: &str, index_name: &str) -> Result<()> {
    if !self.indexes.contains_key(index_name) {
      return Err(Error::invalid_data(format!(
        "Index '{index_name}' not found"
      )));
    }
    self
      .aliases
      .insert(HipStr::from(alias), HipStr::from(index_name));
    Ok(())
  }

  /// Retrieves all distinct tag values for a field (FT.TAGVALS).
  /// 获取标签字段值列表（FT.TAGVALS）
  pub fn tag_vals(&self, index_name: &str, field_name: &str) -> Result<Vec<String>> {
    if let Some((_, idx)) = self.get_index(index_name) {
      Ok(idx.tag_vals(field_name))
    } else {
      Err(Error::invalid_data(format!(
        "Index '{index_name}' not found"
      )))
    }
  }

  /// Executes search query (FT.SEARCH).
  /// 执行检索（FT.SEARCH）
  pub fn search(&self, index_name: &str, query: &str, opts: &FtSearch) -> Result<SearchResult> {
    let resolved = self.resolve_index_name(index_name);
    if let Some((schema, idx)) = self.indexes.get(resolved) {
      idx.search(schema, query, opts)
    } else {
      Err(Error::invalid_data(format!(
        "Index '{index_name}' not found"
      )))
    }
  }

  /// Executes aggregate query (FT.AGGREGATE).
  /// 执行聚合检索（FT.AGGREGATE）
  pub fn aggregate(&self, index_name: &str, opts: &FtAggregate) -> Result<AggregateResult> {
    let resolved = self.resolve_index_name(index_name);
    if let Some((schema, idx)) = self.indexes.get(resolved) {
      idx.aggregate(schema, opts)
    } else {
      Err(Error::invalid_data(format!(
        "Index '{index_name}' not found"
      )))
    }
  }

  /// Explains query execution plan (FT.EXPLAIN).
  /// 查询执行计划（FT.EXPLAIN）
  #[inline]
  pub fn explain(&self, query: &str, opts: &FtSearch) -> String {
    let ast = parse_search_query_with_params(query, &opts.params);
    explain_search_query(&ast)
  }

  /// Explains query execution plan in CLI format (FT.EXPLAINCLI).
  /// 查询 CLI 执行计划（FT.EXPLAINCLI）
  #[inline]
  pub fn explain_cli(&self, query: &str, opts: &FtSearch) -> String {
    let ast = parse_search_query_with_params(query, &opts.params);
    explain_search_query_cli(&ast)
  }

  /// Retrieves index information and statistics (FT.INFO).
  /// 获取索引信息（FT.INFO）
  pub fn info(&self, index_name: &str) -> Result<FtInfo> {
    if let Some((schema, idx)) = self.get_index(index_name) {
      Ok(idx.info(schema))
    } else {
      Err(Error::invalid_data(format!(
        "Index '{index_name}' not found"
      )))
    }
  }

  /// Retrieves a search engine configuration parameter (FT.CONFIG GET).
  /// 获取搜索引擎运行时配置参数
  pub fn config_get(&self, option: &str) -> Result<String> {
    let opt_upper = option.to_ascii_uppercase();
    if let Some(val) = self.configs.get(&opt_upper) {
      Ok(val.clone())
    } else {
      Err(Error::invalid_data(format!(
        "No such configuration option '{option}'"
      )))
    }
  }

  /// Sets a search engine configuration parameter (FT.CONFIG SET).
  /// 设置搜索引擎运行时配置参数
  pub fn config_set(&mut self, option: &str, value: &str) -> Result<()> {
    let opt_upper = option.to_ascii_uppercase();
    self.configs.insert(opt_upper, value.to_string());
    Ok(())
  }

  /// Displays help text for a configuration parameter (FT.CONFIG HELP).
  /// 查看配置参数帮助说明
  pub fn config_help(&self, option: &str) -> Result<String> {
    let opt_upper = option.to_ascii_uppercase();
    match opt_upper.as_str() {
      "TIMEOUT" => Ok("Query execution timeout in milliseconds".to_string()),
      "DEFAULT_DIALECT" => Ok("Default RediSearch dialect version".to_string()),
      "MINPREFIX" => Ok("Minimum prefix length for wildcard expansion".to_string()),
      "MAXEXPANSIONS" => Ok("Maximum number of prefix expansions".to_string()),
      _ => Ok(format!("Help information for {option}")),
    }
  }

  /// Adds a suggestion string to an auto-complete dictionary (FT.SUGADD).
  /// 向自动补全字典添加建议词条
  pub fn sug_add(
    &mut self,
    key: &str,
    string: &str,
    score: f64,
    incr: bool,
    payload: Option<String>,
  ) -> usize {
    let dict = self.suggestions.entry(HipStr::from(key)).or_default();
    dict.sug_add(string, score, incr, payload)
  }

  /// Retrieves auto-complete suggestions matching a prefix (FT.SUGGET).
  /// 获取匹配前缀的自动补全建议词条
  pub fn sug_get(
    &self,
    key: &str,
    prefix: &str,
    fuzzy: bool,
    withscores: bool,
    withpayloads: bool,
    max: Option<usize>,
  ) -> Vec<SuggestionItem> {
    if let Some(dict) = self.suggestions.get(key) {
      dict.sug_get(prefix, fuzzy, withscores, withpayloads, max)
    } else {
      Vec::new()
    }
  }

  /// Deletes a suggestion string from an auto-complete dictionary (FT.SUGDEL).
  /// 从自动补全字典删除指定建议词条
  pub fn sug_del(&mut self, key: &str, string: &str) -> bool {
    if let Some(dict) = self.suggestions.get_mut(key) {
      dict.sug_del(string)
    } else {
      false
    }
  }

  /// Returns the number of entries in an auto-complete dictionary (FT.SUGLEN).
  /// 获取自动补全字典中的词条总数
  pub fn sug_len(&self, key: &str) -> usize {
    if let Some(dict) = self.suggestions.get(key) {
      dict.sug_len()
    } else {
      0
    }
  }
}
