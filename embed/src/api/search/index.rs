use std::{cmp::Ordering, collections::BTreeMap, str};

use hipstr::HipStr;
use rapidhash::{RapidHashMap as HashMap, RapidHashSet as HashSet};
use sonic_rs::{JsonContainerTrait, JsonValueTrait};

use crate::{
  error::Result,
  geo::GeoShape,
  json::json_path_query,
  search::{
    ast::{SearchQueryNode, parse_search_query_with_params},
    encoding::{
      compute_vector_distance, decode_sortable_f64, encode_sortable_f64, parse_vector_from_slice,
    },
    hnsw::HnswGraph,
    meta::{DistanceMetric, IndexFieldType, IndexOnDataType, SearchIndexSchema, VectorType},
    opt::{
      AggregateResult, AggregateRow, FtAggregate, FtFieldInfo, FtIndexDefinition, FtInfo,
      FtReducer, FtSearch, SearchDoc, SearchResult,
    },
    tokenizer::{levenshtein_distance, tokenize_tags, tokenize_text},
  },
  string::format_float,
};

/// Extracts indexable entries from document returning (field_name, term_or_encoded_val).
/// 提取文档中的索引项（返回 `(field_name, term_or_encoded_val)` 列表）
pub fn extract_doc_terms(
  schema: &SearchIndexSchema,
  doc_id: &str,
  raw_doc: &[u8],
) -> Vec<(String, String)> {
  let _ = doc_id;
  let mut terms = Vec::new();

  match schema.on_data_type {
    IndexOnDataType::Json => {
      if let Ok(json_v) = sonic_rs::from_slice::<sonic_rs::Value>(raw_doc) {
        for field in &schema.fields {
          if field.noindex {
            continue;
          }
          let field_name = &field.name;
          let matched_nodes: Vec<&sonic_rs::Value> =
            if !field_name.contains('.') && !field_name.contains('[') {
              if let Some(obj) = json_v.as_object()
                && let Some(node) = obj.get(field_name)
              {
                vec![node]
              } else {
                Vec::new()
              }
            } else {
              let path = if field_name.starts_with('$') {
                field_name.to_string()
              } else {
                format!("$.{field_name}")
              };
              json_path_query(&json_v, &path)
            };
          for node in matched_nodes {
            match field.field_type {
              IndexFieldType::Text => {
                if let Some(s) = node.as_str() {
                  for word in tokenize_text(s) {
                    terms.push((field.name.clone(), word));
                  }
                }
              }
              IndexFieldType::Tag => {
                let sep = field.separator.unwrap_or(',');
                if let Some(s) = node.as_str() {
                  for tag in tokenize_tags(s, sep, field.case_sensitive) {
                    terms.push((field.name.clone(), tag));
                  }
                } else if let Some(arr) = node.as_array() {
                  for item in arr {
                    if let Some(s) = item.as_str() {
                      let tag_str = if field.case_sensitive {
                        s.to_string()
                      } else {
                        s.to_lowercase()
                      };
                      terms.push((field.name.clone(), tag_str));
                    }
                  }
                }
              }
              IndexFieldType::Numeric => {
                if let Some(num) = node.as_f64() {
                  terms.push((field.name.clone(), encode_sortable_f64(num)));
                }
              }
              IndexFieldType::Vector => {}
              IndexFieldType::Geo => {
                if let Some(s) = node.as_str() {
                  let parts: Vec<&str> = s.split(',').collect();
                  if parts.len() == 2 {
                    terms.push((field.name.clone(), s.to_string()));
                  }
                }
              }
            }
          }
        }
      }
    }
    IndexOnDataType::Hash => {
      if let Ok(json_v) = sonic_rs::from_slice::<sonic_rs::Value>(raw_doc) {
        if let Some(obj) = json_v.as_object() {
          for field in &schema.fields {
            if field.noindex {
              continue;
            }
            if let Some(val) = obj.get(&field.name) {
              match field.field_type {
                IndexFieldType::Text => {
                  if let Some(s) = val.as_str() {
                    for word in tokenize_text(s) {
                      terms.push((field.name.clone(), word));
                    }
                  }
                }
                IndexFieldType::Tag => {
                  let sep = field.separator.unwrap_or(',');
                  if let Some(s) = val.as_str() {
                    for tag in tokenize_tags(s, sep, field.case_sensitive) {
                      terms.push((field.name.clone(), tag));
                    }
                  }
                }
                IndexFieldType::Numeric => {
                  if let Some(num) = val.as_f64() {
                    terms.push((field.name.clone(), encode_sortable_f64(num)));
                  } else if let Some(s) = val.as_str()
                    && let Ok(num) = s.parse::<f64>()
                  {
                    terms.push((field.name.clone(), encode_sortable_f64(num)));
                  }
                }
                IndexFieldType::Vector => {}
                IndexFieldType::Geo => {
                  if let Some(s) = val.as_str() {
                    terms.push((field.name.clone(), s.to_string()));
                  }
                }
              }
            }
          }
        }
      } else if let Ok(s) = str::from_utf8(raw_doc) {
        for word in tokenize_text(s) {
          if let Some(first_field) = schema.fields.first() {
            terms.push((first_field.name.clone(), word));
          }
        }
      }
    }
  }

  terms
}

/// Inverted index posting entry.
/// 倒排记录条目（Posting）
#[derive(Debug, Clone, PartialEq)]
pub struct Posting {
  pub doc_id: HipStr<'static>,
  pub score: f64,
  pub positions: Vec<u32>,
  pub payload: Option<Vec<u8>>,
}

/// Stored document entry tuple: (field-value map, base score, payload data).
/// 文档存储内容元组：(字段键值映射, 文档基础得分, 附带载荷数据)
pub type StoredDoc = (
  HashMap<HipStr<'static>, HipStr<'static>>,
  f64,
  Option<Vec<u8>>,
);

/// Inverted index engine (aligned with Apache Kvrocks GlobalIndexer and IndexUpdater).
/// 倒排索引引擎（对标 Apache Kvrocks GlobalIndexer 与 IndexUpdater）
#[derive(Debug, Clone, Default)]
pub struct InvertedIndex {
  /// Text inverted index mapping: field_name -> (term -> (doc_id -> Posting)).
  /// 文本倒排索引：field_name -> (term -> (doc_id -> Posting))
  pub text_index: HashMap<String, HashMap<String, HashMap<HipStr<'static>, Posting>>>,
  /// Tag inverted index mapping: field_name -> (tag -> Set<doc_id>).
  /// 标签倒排索引：field_name -> (tag -> Set<doc_id>)
  pub tag_index: HashMap<String, HashMap<String, HashSet<HipStr<'static>>>>,
  /// Numeric index mapping: field_name -> (sortable_f64_hex -> Set<doc_id>).
  /// 数值索引：field_name -> (sortable_f64_hex -> Set<doc_id>)
  pub numeric_index: HashMap<String, BTreeMap<String, HashSet<HipStr<'static>>>>,
  /// Vector index mapping: field_name -> (doc_id -> vector).
  /// 向量索引：field_name -> (doc_id -> vector)
  pub vector_index: HashMap<String, HashMap<HipStr<'static>, Vec<f64>>>,
  /// HNSW vector index graph mapping: field_name -> HnswGraph.
  /// HNSW 向量索引图：field_name -> HnswGraph
  pub hnsw_index: HashMap<String, HnswGraph>,
  /// Geospatial index mapping: field_name -> (doc_id -> (lon, lat)).
  /// 空间索引：field_name -> (doc_id -> (lon, lat))
  pub geo_index: HashMap<String, HashMap<HipStr<'static>, (f64, f64)>>,
  /// Document store mapping: doc_id -> StoredDoc.
  /// 文档全量存储：doc_id -> StoredDoc
  pub docs: HashMap<HipStr<'static>, StoredDoc>,
}

impl InvertedIndex {
  #[inline]
  pub fn new() -> Self {
    Self::default()
  }

  /// Indexes document fields into text, tag, numeric, geo, and vector indices.
  /// 针对文档写入索引
  pub fn index_doc(
    &mut self,
    schema: &SearchIndexSchema,
    doc_id: &str,
    raw_doc: &[u8],
    doc_score: Option<f64>,
    payload: Option<Vec<u8>>,
  ) -> Result<()> {
    let doc_key = HipStr::from(doc_id);

    // 若文档已存在，先清除历史倒排记录与图节点以避免脏数据
    if self.docs.contains_key(&doc_key) {
      self.delete_doc(schema, doc_id);
    }

    let score = doc_score.unwrap_or(schema.default_score);
    let mut stored_fields = HashMap::default();

    if let Ok(json_v) = sonic_rs::from_slice::<sonic_rs::Value>(raw_doc) {
      if let Some(obj) = json_v.as_object() {
        for (k, v) in obj.iter() {
          let k_str = HipStr::from(k);
          let v_str = if let Some(s) = v.as_str() {
            HipStr::from(s)
          } else {
            HipStr::from(v.to_string())
          };
          stored_fields.insert(k_str, v_str);
        }
      }
    } else if let Ok(s) = str::from_utf8(raw_doc) {
      stored_fields.insert(HipStr::from("body"), HipStr::from(s));
    }

    // 提取并更新各字段索引
    for field in &schema.fields {
      if field.noindex {
        continue;
      }
      let field_name_str = field.name.as_str();

      match field.field_type {
        IndexFieldType::Text => {
          if let Some(val) = stored_fields.get(field_name_str) {
            let words = tokenize_text(val.as_str());
            let field_map = self.text_index.entry(field.name.to_string()).or_default();
            for (pos, word) in words.into_iter().enumerate() {
              let entry = field_map
                .entry(word)
                .or_default()
                .entry(doc_key.clone())
                .or_insert_with(|| Posting {
                  doc_id: doc_key.clone(),
                  score: field.weight * score,
                  positions: Vec::new(),
                  payload: payload.clone(),
                });
              entry.positions.push(pos as u32);
            }
          }
        }
        IndexFieldType::Tag => {
          if let Some(val) = stored_fields.get(field_name_str) {
            let sep = field.separator.unwrap_or(',');
            let tags = tokenize_tags(val.as_str(), sep, field.case_sensitive);
            let field_map = self.tag_index.entry(field.name.to_string()).or_default();
            for tag in tags {
              field_map.entry(tag).or_default().insert(doc_key.clone());
            }
          }
        }
        IndexFieldType::Numeric => {
          if let Some(val) = stored_fields.get(field_name_str)
            && let Ok(num) = val.parse::<f64>()
          {
            let hex = encode_sortable_f64(num);
            self
              .numeric_index
              .entry(field.name.to_string())
              .or_default()
              .entry(hex)
              .or_default()
              .insert(doc_key.clone());
          }
        }
        IndexFieldType::Vector => {
          if let Some(val) = stored_fields.get(field_name_str) {
            let vec_meta = field.vector_meta.clone().unwrap_or_default();
            let vec_type = vec_meta.vector_type;
            if let Ok(vec) = parse_vector_from_slice(val.as_bytes(), vec_type) {
              self
                .vector_index
                .entry(field.name.to_string())
                .or_default()
                .insert(doc_key.clone(), vec.clone());

              let hnsw = self
                .hnsw_index
                .entry(field.name.to_string())
                .or_insert_with(|| {
                  HnswGraph::new(
                    vec_meta.dim,
                    vec_meta.distance_metric,
                    vec_meta.m,
                    vec_meta.ef_construction,
                    vec_meta.ef_runtime,
                    vec_meta.epsilon,
                  )
                });
              let _ = hnsw.insert(doc_key.clone(), vec);
            }
          }
        }
        IndexFieldType::Geo => {
          if let Some(val) = stored_fields.get(field_name_str) {
            let parts: Vec<&str> = val.as_str().split(',').collect();
            if parts.len() == 2
              && let Ok(lon) = parts[0].trim().parse::<f64>()
              && let Ok(lat) = parts[1].trim().parse::<f64>()
            {
              self
                .geo_index
                .entry(field.name.to_string())
                .or_default()
                .insert(doc_key.clone(), (lon, lat));
            }
          }
        }
      }
    }

    self.docs.insert(doc_key, (stored_fields, score, payload));
    Ok(())
  }

  /// Removes document entries from all indices.
  /// 删除指定文档索引
  pub fn delete_doc(&mut self, schema: &SearchIndexSchema, doc_id: &str) -> bool {
    let doc_key = HipStr::from(doc_id);
    if self.docs.remove(&doc_key).is_none() {
      return false;
    }

    for field_map in self.text_index.values_mut() {
      for postings in field_map.values_mut() {
        postings.remove(&doc_key);
      }
    }
    for field_map in self.tag_index.values_mut() {
      for set in field_map.values_mut() {
        set.remove(&doc_key);
      }
    }
    for num_map in self.numeric_index.values_mut() {
      for set in num_map.values_mut() {
        set.remove(&doc_key);
      }
    }
    for vec_map in self.vector_index.values_mut() {
      vec_map.remove(&doc_key);
    }
    for hnsw in self.hnsw_index.values_mut() {
      hnsw.delete(doc_id);
    }
    for geo_map in self.geo_index.values_mut() {
      geo_map.remove(&doc_key);
    }

    let _ = schema;
    true
  }

  /// Retrieves all distinct tag values for a tag field aligned with FT.TAGVALS.
  /// 获取标签字段的所有独立值（对标 FT.TAGVALS）
  pub fn tag_vals(&self, field_name: &str) -> Vec<String> {
    if let Some(map) = self.tag_index.get(field_name) {
      let mut vals: Vec<String> = map
        .iter()
        .filter(|(_, set)| !set.is_empty())
        .map(|(k, _)| k.clone())
        .collect();
      vals.sort();
      vals
    } else {
      Vec::new()
    }
  }

  /// Evaluates query AST node to retrieve matching document IDs.
  /// 评估查询节点获取匹配的文档集合
  pub fn eval_query_node(
    &self,
    schema: &SearchIndexSchema,
    node: &SearchQueryNode,
    opts: &FtSearch,
  ) -> HashSet<HipStr<'static>> {
    match node {
      SearchQueryNode::Wildcard => self.docs.keys().cloned().collect(),
      SearchQueryNode::Term {
        field,
        term,
        is_prefix,
        is_fuzzy,
        max_edits,
      } => {
        let mut matched = HashSet::default();

        let search_in_field_map =
          |field_map: &HashMap<String, HashMap<HipStr<'static>, Posting>>,
           matched: &mut HashSet<HipStr<'static>>| {
            if *is_prefix {
              for (t, postings) in field_map {
                if t.starts_with(term.as_str()) {
                  for doc_id in postings.keys() {
                    matched.insert(doc_id.clone());
                  }
                }
              }
            } else if *is_fuzzy {
              let threshold = (*max_edits).max(1) as usize;
              for (t, postings) in field_map {
                if levenshtein_distance(t, term) <= threshold {
                  for doc_id in postings.keys() {
                    matched.insert(doc_id.clone());
                  }
                }
              }
            } else if let Some(postings) = field_map.get(term.as_str()) {
              for doc_id in postings.keys() {
                matched.insert(doc_id.clone());
              }
            }
          };

        if let Some(f) = field {
          if let Some(field_map) = self.text_index.get(f) {
            search_in_field_map(field_map, &mut matched);
          }
        } else if !opts.infields.is_empty() {
          for f in &opts.infields {
            if let Some(field_map) = self.text_index.get(f) {
              search_in_field_map(field_map, &mut matched);
            }
          }
        } else {
          for field_map in self.text_index.values() {
            search_in_field_map(field_map, &mut matched);
          }
        }
        matched
      }
      SearchQueryNode::Phrase {
        field,
        terms,
        slop,
        in_order,
      } => {
        if terms.is_empty() {
          return HashSet::default();
        }

        let check_field_phrase = |field_map: &HashMap<
          String,
          HashMap<HipStr<'static>, Posting>,
        >|
         -> HashSet<HipStr<'static>> {
          let mut field_matched = HashSet::default();
          let mut candidate_postings = Vec::with_capacity(terms.len());
          for t in terms {
            if let Some(postings) = field_map.get(t.as_str()) {
              candidate_postings.push(postings);
            } else {
              return field_matched;
            }
          }

          // 寻找包含所有词条的共同文档
          let first_postings = &candidate_postings[0];
          for (doc_id, p0) in first_postings.iter() {
            let mut all_present = true;
            let mut doc_positions = Vec::with_capacity(terms.len());
            doc_positions.push(&p0.positions);

            for next_postings in &candidate_postings[1..] {
              if let Some(pn) = next_postings.get(doc_id) {
                doc_positions.push(&pn.positions);
              } else {
                all_present = false;
                break;
              }
            }

            if all_present && verify_phrase_positions(&doc_positions, *slop, *in_order) {
              field_matched.insert(doc_id.clone());
            }
          }
          field_matched
        };

        let mut matched = HashSet::default();
        if let Some(f) = field {
          if let Some(field_map) = self.text_index.get(f) {
            matched.extend(check_field_phrase(field_map));
          }
        } else if !opts.infields.is_empty() {
          for f in &opts.infields {
            if let Some(field_map) = self.text_index.get(f) {
              matched.extend(check_field_phrase(field_map));
            }
          }
        } else {
          for field_map in self.text_index.values() {
            matched.extend(check_field_phrase(field_map));
          }
        }
        matched
      }
      SearchQueryNode::Tag { field, tags } => {
        let mut matched = HashSet::default();
        if let Some(field_map) = self.tag_index.get(field) {
          let is_case_sensitive = schema
            .get_field(field)
            .map(|f| f.case_sensitive)
            .unwrap_or(false);

          for tag in tags {
            if let Some(prefix) = tag.strip_suffix('*') {
              let prefix_query = if is_case_sensitive {
                prefix.to_string()
              } else {
                prefix.to_lowercase()
              };
              for (t, docs) in field_map {
                if t.starts_with(&prefix_query) {
                  for d in docs {
                    matched.insert(d.clone());
                  }
                }
              }
            } else {
              let exact_tag = if is_case_sensitive {
                tag.to_string()
              } else {
                tag.to_lowercase()
              };
              if let Some(docs) = field_map.get(&exact_tag) {
                for d in docs {
                  matched.insert(d.clone());
                }
              }
            }
          }
        }
        matched
      }
      SearchQueryNode::NumericRange {
        field,
        min,
        min_inclusive,
        max,
        max_inclusive,
      } => {
        let mut matched = HashSet::default();
        if let Some(btree) = self.numeric_index.get(field) {
          let min_hex = encode_sortable_f64(*min);
          let max_hex = encode_sortable_f64(*max);

          for (hex, doc_set) in btree.range(min_hex..=max_hex) {
            if let Some(val) = decode_sortable_f64(hex) {
              let pass_min = if *min_inclusive {
                val >= *min
              } else {
                val > *min
              };
              let pass_max = if *max_inclusive {
                val <= *max
              } else {
                val < *max
              };
              if pass_min && pass_max {
                for d in doc_set {
                  matched.insert(d.clone());
                }
              }
            }
          }
        }
        matched
      }
      SearchQueryNode::GeoFilter {
        field,
        lon,
        lat,
        radius_m,
      } => {
        let mut matched = HashSet::default();
        if let Some(field_map) = self.geo_index.get(field) {
          let shape = GeoShape::new_circular(*lon, *lat, *radius_m);
          for (doc_id, (p_lon, p_lat)) in field_map {
            if shape.contains_point(*p_lon, *p_lat) {
              matched.insert(doc_id.clone());
            }
          }
        }
        matched
      }
      SearchQueryNode::VectorKnn {
        field,
        k,
        vector_param,
        vector,
      } => {
        let mut matched = HashSet::default();
        let query_vec = vector.clone().or_else(|| {
          let vec_type = schema
            .get_field(field)
            .and_then(|f| f.vector_meta.as_ref())
            .map(|m| m.vector_type)
            .unwrap_or(VectorType::Float64);
          opts.params.get(vector_param).and_then(|val| {
            parse_vector_from_slice(val.as_bytes(), vec_type)
              .ok()
              .or_else(|| parse_vector_from_slice(val.as_bytes(), VectorType::Float64).ok())
              .or_else(|| parse_vector_from_slice(val.as_bytes(), VectorType::Float32).ok())
          })
        });

        if let Some(q_vec) = query_vec {
          if let Some(hnsw) = self.hnsw_index.get(field)
            && let Ok(res) = hnsw.search_knn(&q_vec, *k, None)
          {
            for (_, doc_id) in res {
              matched.insert(doc_id);
            }
            return matched;
          }

          if let Some(field_map) = self.vector_index.get(field) {
            let metric = schema
              .get_field(field)
              .and_then(|f| f.vector_meta.as_ref())
              .map(|m| m.distance_metric)
              .unwrap_or(DistanceMetric::Cosine);

            let mut distances: Vec<(f64, HipStr<'static>)> = Vec::new();
            for (doc_id, v) in field_map {
              if let Ok(dist) = compute_vector_distance(&q_vec, v, metric) {
                distances.push((dist, doc_id.clone()));
              }
            }
            distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
            for (_, doc_id) in distances.into_iter().take(*k) {
              matched.insert(doc_id);
            }
          }
        }
        matched
      }
      SearchQueryNode::VectorRange {
        field,
        radius,
        vector_param,
        vector,
      } => {
        let mut matched = HashSet::default();
        let query_vec = vector.clone().or_else(|| {
          let vec_type = schema
            .get_field(field)
            .and_then(|f| f.vector_meta.as_ref())
            .map(|m| m.vector_type)
            .unwrap_or(VectorType::Float64);
          opts.params.get(vector_param).and_then(|val| {
            parse_vector_from_slice(val.as_bytes(), vec_type)
              .ok()
              .or_else(|| parse_vector_from_slice(val.as_bytes(), VectorType::Float64).ok())
              .or_else(|| parse_vector_from_slice(val.as_bytes(), VectorType::Float32).ok())
          })
        });

        if let Some(q_vec) = query_vec {
          if let Some(hnsw) = self.hnsw_index.get(field)
            && let Ok(res) = hnsw.search_range(&q_vec, *radius, None)
          {
            for (_, doc_id) in res {
              matched.insert(doc_id);
            }
            return matched;
          }

          if let Some(field_map) = self.vector_index.get(field) {
            let metric = schema
              .get_field(field)
              .and_then(|f| f.vector_meta.as_ref())
              .map(|m| m.distance_metric)
              .unwrap_or(DistanceMetric::Cosine);

            for (doc_id, v) in field_map {
              if let Ok(dist) = compute_vector_distance(&q_vec, v, metric)
                && dist <= *radius
              {
                matched.insert(doc_id.clone());
              }
            }
          }
        }
        matched
      }
      SearchQueryNode::And(nodes) => {
        if nodes.is_empty() {
          return HashSet::default();
        }
        let mut sets: Vec<HashSet<HipStr<'static>>> = nodes
          .iter()
          .map(|n| self.eval_query_node(schema, n, opts))
          .collect();
        // 经典 IR 优化：按集合大小升序排序，最小候选集优先求交
        sets.sort_by_key(HashSet::len);
        let mut iter = sets.into_iter();
        if let Some(mut base) = iter.next() {
          for next_set in iter {
            base.retain(|id| next_set.contains(id));
            if base.is_empty() {
              break;
            }
          }
          base
        } else {
          HashSet::default()
        }
      }
      SearchQueryNode::Or(nodes) => {
        let mut union_set = HashSet::default();
        for n in nodes {
          union_set.extend(self.eval_query_node(schema, n, opts));
        }
        union_set
      }
      SearchQueryNode::Not(inner) => {
        let exclude_set = self.eval_query_node(schema, inner, opts);
        self
          .docs
          .keys()
          .filter(|k| !exclude_set.contains(*k))
          .cloned()
          .collect()
      }
    }
  }

  /// Executes full-text search aligned with Apache Kvrocks FT.SEARCH.
  /// 执行全文检索（对标 Apache Kvrocks FT.SEARCH）
  pub fn search(
    &self,
    schema: &SearchIndexSchema,
    query: &str,
    opts: &FtSearch,
  ) -> Result<SearchResult> {
    let ast = parse_search_query_with_params(query, &opts.params);

    // 检查是否包含 KNN 向量预过滤逻辑（Hybrid Vector Search）
    let mut candidate_ids = if let SearchQueryNode::And(ref nodes) = ast
      && let Some(knn_idx) = nodes
        .iter()
        .position(|n| matches!(n, SearchQueryNode::VectorKnn { .. }))
    {
      let knn_node = &nodes[knn_idx];
      let other_nodes: Vec<SearchQueryNode> = nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != knn_idx)
        .map(|(_, n)| n.clone())
        .collect();

      let prefiltered = if other_nodes.is_empty() {
        self.docs.keys().cloned().collect()
      } else if other_nodes.len() == 1 {
        self.eval_query_node(schema, &other_nodes[0], opts)
      } else {
        self.eval_query_node(schema, &SearchQueryNode::And(other_nodes), opts)
      };

      if let SearchQueryNode::VectorKnn {
        field,
        k,
        vector_param,
        vector,
      } = knn_node
      {
        let query_vec = vector.clone().or_else(|| {
          opts
            .params
            .get(vector_param)
            .and_then(|val| parse_vector_from_slice(val.as_bytes(), VectorType::Float64).ok())
        });

        let mut knn_matched = HashSet::default();
        if let Some(q_vec) = query_vec
          && let Some(field_map) = self.vector_index.get(field)
        {
          let metric = schema
            .get_field(field)
            .and_then(|f| f.vector_meta.as_ref())
            .map(|m| m.distance_metric)
            .unwrap_or(DistanceMetric::Cosine);

          let mut distances: Vec<(f64, HipStr<'static>)> = Vec::new();
          for doc_id in &prefiltered {
            if let Some(v) = field_map.get(doc_id)
              && let Ok(dist) = compute_vector_distance(&q_vec, v, metric)
            {
              distances.push((dist, doc_id.clone()));
            }
          }
          distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
          for (_, doc_id) in distances.into_iter().take(*k) {
            knn_matched.insert(doc_id);
          }
        }
        knn_matched
      } else {
        self.eval_query_node(schema, &ast, opts)
      }
    } else {
      self.eval_query_node(schema, &ast, opts)
    };

    // 外部过滤条件：INKEYS
    if !opts.inkeys.is_empty() {
      let inkey_set: HashSet<HipStr<'static>> = opts
        .inkeys
        .iter()
        .map(|k| HipStr::from(k.as_str()))
        .collect();
      candidate_ids.retain(|id| inkey_set.contains(id));
    }

    // 外部过滤条件：FILTER (numeric min max)
    for (f_name, min_v, max_v) in &opts.filter {
      if let Some(btree) = self.numeric_index.get(f_name) {
        let min_hex = encode_sortable_f64(*min_v);
        let max_hex = encode_sortable_f64(*max_v);
        let mut filter_docs = HashSet::default();
        for (_, set) in btree.range(min_hex..=max_hex) {
          for d in set {
            filter_docs.insert(d.clone());
          }
        }
        candidate_ids.retain(|id| filter_docs.contains(id));
      } else {
        candidate_ids.clear();
      }
    }

    // 外部过滤条件：GEOFILTER (field lon lat radius unit)
    for (f_name, lon, lat, radius, unit_str) in &opts.geofilter {
      let factor = match unit_str.to_ascii_lowercase().as_str() {
        "km" => 1000.0,
        "m" => 1.0,
        "mi" => 1609.344,
        "ft" => 0.3048,
        _ => 1.0,
      };
      let radius_m = radius * factor;
      let shape = GeoShape::new_circular(*lon, *lat, radius_m);
      if let Some(field_map) = self.geo_index.get(f_name) {
        candidate_ids.retain(|id| {
          if let Some(&(p_lon, p_lat)) = field_map.get(id) {
            shape.contains_point(p_lon, p_lat)
          } else {
            false
          }
        });
      } else {
        candidate_ids.clear();
      }
    }

    let total_results = candidate_ids.len();

    // 候选文档排序
    let mut candidate_list: Vec<HipStr<'static>> = candidate_ids.into_iter().collect();

    if let Some((ref sort_field, asc)) = opts.sortby {
      candidate_list.sort_by(|a, b| {
        let doc_a = self.docs.get(a);
        let doc_b = self.docs.get(b);
        let val_a = doc_a.and_then(|(f, ..)| f.get(sort_field.as_str()));
        let val_b = doc_b.and_then(|(f, ..)| f.get(sort_field.as_str()));

        let ord = match (val_a, val_b) {
          (Some(sa), Some(sb)) => {
            if let (Ok(na), Ok(nb)) = (sa.parse::<f64>(), sb.parse::<f64>()) {
              na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
            } else {
              sa.cmp(sb)
            }
          }
          (Some(_), None) => Ordering::Greater,
          (None, Some(_)) => Ordering::Less,
          (None, None) => a.cmp(b),
        };
        if asc { ord } else { ord.reverse() }
      });
    } else {
      // 默认按文档基础权重/得分从高到低排序，分数一致按 ID 字母序
      candidate_list.sort_by(|a, b| {
        let score_a = self.docs.get(a).map(|(_, s, _)| *s).unwrap_or(1.0);
        let score_b = self.docs.get(b).map(|(_, s, _)| *s).unwrap_or(1.0);
        score_b
          .partial_cmp(&score_a)
          .unwrap_or(Ordering::Equal)
          .then_with(|| a.cmp(b))
      });
    }

    // 分页 LIMIT offset count
    let (offset, count) = opts.limit.unwrap_or((0, 10));
    let paged_keys = candidate_list.into_iter().skip(offset).take(count);

    let mut docs = Vec::new();
    for key in paged_keys {
      if let Some((stored_fields, score, payload)) = self.docs.get(&key) {
        let mut doc_fields = Vec::new();
        if !opts.nocontent {
          if !opts.returns.is_empty() {
            for (req_f, alias_opt) in &opts.returns {
              let out_name = alias_opt
                .as_deref()
                .map(HipStr::from)
                .unwrap_or_else(|| HipStr::from(req_f.as_str()));
              if let Some(val) = stored_fields.get(req_f.as_str()) {
                doc_fields.push((out_name, val.clone()));
              }
            }
          } else {
            for (k, v) in stored_fields {
              doc_fields.push((k.clone(), v.clone()));
            }
          }
        }

        let sort_key = if opts.withsortkeys
          && let Some((ref sf, _)) = opts.sortby
        {
          stored_fields.get(sf.as_str()).map(HipStr::to_string)
        } else {
          None
        };

        docs.push(SearchDoc {
          id: key.clone(),
          score: *score,
          payload: if opts.withpayloads {
            payload.clone()
          } else {
            None
          },
          sort_key,
          fields: doc_fields,
        });
      }
    }

    Ok(SearchResult {
      total_results,
      docs,
    })
  }

  /// Executes aggregate query aligned with FT.AGGREGATE.
  /// 执行聚合检索（对标 FT.AGGREGATE 与 Apache Kvrocks 聚合支持）
  pub fn aggregate(
    &self,
    schema: &SearchIndexSchema,
    opts: &FtAggregate,
  ) -> Result<AggregateResult> {
    let search_opts = FtSearch {
      params: opts.params.clone(),
      ..Default::default()
    };
    let search_res = self.search(schema, &opts.query, &search_opts)?;

    // 如果没有 GROUPBY，直接提取所有文档字段形成行
    if opts.groupbys.is_empty() {
      let mut rows = Vec::with_capacity(search_res.docs.len());
      for doc in &search_res.docs {
        let mut row_fields = Vec::new();
        if !opts.load_fields.is_empty() {
          for req_f in &opts.load_fields {
            if let Some((_, v)) = doc.fields.iter().find(|(k, _)| k.as_str() == req_f) {
              row_fields.push((req_f.clone(), v.to_string()));
            }
          }
        } else {
          for (k, v) in &doc.fields {
            row_fields.push((k.to_string(), v.to_string()));
          }
        }
        rows.push(AggregateRow { fields: row_fields });
      }

      // 排序
      if !opts.sortby.is_empty() {
        rows.sort_by(|a, b| {
          for (f, asc) in &opts.sortby {
            let va = a
              .fields
              .iter()
              .find(|(k, _)| k == f)
              .map(|(_, v)| v.as_str());
            let vb = b
              .fields
              .iter()
              .find(|(k, _)| k == f)
              .map(|(_, v)| v.as_str());
            let ord = match (va, vb) {
              (Some(sa), Some(sb)) => {
                if let (Ok(na), Ok(nb)) = (sa.parse::<f64>(), sb.parse::<f64>()) {
                  na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
                } else {
                  sa.cmp(sb)
                }
              }
              (Some(_), None) => Ordering::Greater,
              (None, Some(_)) => Ordering::Less,
              (None, None) => Ordering::Equal,
            };
            if ord != Ordering::Equal {
              return if *asc { ord } else { ord.reverse() };
            }
          }
          Ordering::Equal
        });
      }

      let total_results = rows.len();
      let (offset, count) = opts.limit.unwrap_or((0, total_results));
      let paged_rows: Vec<AggregateRow> = rows.into_iter().skip(offset).take(count).collect();

      return Ok(AggregateResult {
        total_results,
        rows: paged_rows,
      });
    }

    // 有 GROUPBY 逻辑
    let mut grouped_rows = Vec::new();
    for groupby in &opts.groupbys {
      let mut groups: HashMap<Vec<String>, Vec<&SearchDoc>> = HashMap::default();
      for doc in &search_res.docs {
        let mut group_key = Vec::with_capacity(groupby.fields.len());
        for gf in &groupby.fields {
          let val = doc
            .fields
            .iter()
            .find(|(k, _)| k.as_str() == gf.as_str())
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
          group_key.push(val);
        }
        groups.entry(group_key).or_default().push(doc);
      }

      for (group_key, docs) in groups {
        let mut row_fields = Vec::new();
        for (gf, gval) in groupby.fields.iter().zip(group_key) {
          row_fields.push((gf.clone(), gval));
        }

        for (reducer, as_alias) in &groupby.reducers {
          let alias = as_alias.as_deref().unwrap_or(match reducer {
            FtReducer::Count => "count",
            FtReducer::Sum(f) => f.as_str(),
            FtReducer::Min(f) => f.as_str(),
            FtReducer::Max(f) => f.as_str(),
            FtReducer::Avg(f) => f.as_str(),
            FtReducer::CountDistinct(f) => f.as_str(),
            FtReducer::FirstValue(f) => f.as_str(),
            FtReducer::ToList(f) => f.as_str(),
          });

          match reducer {
            FtReducer::Count => {
              let mut itoa_buf = itoa::Buffer::new();
              row_fields.push((alias.to_string(), itoa_buf.format(docs.len()).to_string()));
            }
            FtReducer::Sum(f) => {
              let mut sum = 0.0f64;
              for doc in &docs {
                if let Some((_, v)) = doc.fields.iter().find(|(k, _)| k.as_str() == f.as_str())
                  && let Ok(n) = v.parse::<f64>()
                {
                  sum += n;
                }
              }
              row_fields.push((alias.to_string(), format_float(sum)));
            }
            FtReducer::Min(f) => {
              let mut min = f64::INFINITY;
              for doc in &docs {
                if let Some((_, v)) = doc.fields.iter().find(|(k, _)| k.as_str() == f.as_str())
                  && let Ok(n) = v.parse::<f64>()
                  && n < min
                {
                  min = n;
                }
              }
              let out_val = if min == f64::INFINITY {
                "0".to_string()
              } else {
                format_float(min)
              };
              row_fields.push((alias.to_string(), out_val));
            }
            FtReducer::Max(f) => {
              let mut max = f64::NEG_INFINITY;
              for doc in &docs {
                if let Some((_, v)) = doc.fields.iter().find(|(k, _)| k.as_str() == f.as_str())
                  && let Ok(n) = v.parse::<f64>()
                  && n > max
                {
                  max = n;
                }
              }
              let out_val = if max == f64::NEG_INFINITY {
                "0".to_string()
              } else {
                format_float(max)
              };
              row_fields.push((alias.to_string(), out_val));
            }
            FtReducer::Avg(f) => {
              let mut sum = 0.0f64;
              let mut count = 0usize;
              for doc in &docs {
                if let Some((_, v)) = doc.fields.iter().find(|(k, _)| k.as_str() == f.as_str())
                  && let Ok(n) = v.parse::<f64>()
                {
                  sum += n;
                  count += 1;
                }
              }
              let avg = if count > 0 { sum / (count as f64) } else { 0.0 };
              row_fields.push((alias.to_string(), format_float(avg)));
            }
            FtReducer::CountDistinct(f) => {
              let mut set = HashSet::default();
              for doc in &docs {
                if let Some((_, v)) = doc.fields.iter().find(|(k, _)| k.as_str() == f.as_str()) {
                  set.insert(v.as_str());
                }
              }
              let mut itoa_buf = itoa::Buffer::new();
              row_fields.push((alias.to_string(), itoa_buf.format(set.len()).to_string()));
            }
            FtReducer::FirstValue(f) => {
              let first_val = docs
                .first()
                .and_then(|doc| doc.fields.iter().find(|(k, _)| k.as_str() == f.as_str()))
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
              row_fields.push((alias.to_string(), first_val));
            }
            FtReducer::ToList(f) => {
              let list: Vec<&str> = docs
                .iter()
                .filter_map(|doc| {
                  doc
                    .fields
                    .iter()
                    .find(|(k, _)| k.as_str() == f.as_str())
                    .map(|(_, v)| v.as_str())
                })
                .collect();
              row_fields.push((alias.to_string(), list.join(",")));
            }
          }
        }

        grouped_rows.push(AggregateRow { fields: row_fields });
      }
    }

    // 排序
    if !opts.sortby.is_empty() {
      grouped_rows.sort_by(|a, b| {
        for (f, asc) in &opts.sortby {
          let va = a
            .fields
            .iter()
            .find(|(k, _)| k == f)
            .map(|(_, v)| v.as_str());
          let vb = b
            .fields
            .iter()
            .find(|(k, _)| k == f)
            .map(|(_, v)| v.as_str());
          let ord = match (va, vb) {
            (Some(sa), Some(sb)) => {
              if let (Ok(na), Ok(nb)) = (sa.parse::<f64>(), sb.parse::<f64>()) {
                na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
              } else {
                sa.cmp(sb)
              }
            }
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
          };
          if ord != Ordering::Equal {
            return if *asc { ord } else { ord.reverse() };
          }
        }
        Ordering::Equal
      });
    }

    let total_results = grouped_rows.len();
    let (offset, count) = opts.limit.unwrap_or((0, total_results));
    let paged_rows: Vec<AggregateRow> = grouped_rows.into_iter().skip(offset).take(count).collect();

    Ok(AggregateResult {
      total_results,
      rows: paged_rows,
    })
  }

  /// Retrieves index statistics and schema info aligned with FT.INFO.
  /// 获取索引详细统计信息（对标 FT.INFO）
  pub fn info(&self, schema: &SearchIndexSchema) -> FtInfo {
    let mut index_opts = Vec::new();
    if schema.no_offsets {
      index_opts.push("NOOFFSETS".to_string());
    }
    if schema.no_hl {
      index_opts.push("NOHL".to_string());
    }
    if schema.no_fields {
      index_opts.push("NOFIELDS".to_string());
    }
    if schema.no_freqs {
      index_opts.push("NOFREQS".to_string());
    }

    let field_infos = schema
      .fields
      .iter()
      .map(|f| {
        let mut props = Vec::new();
        if f.sortable {
          props.push(("SORTABLE".to_string(), "true".to_string()));
        }
        if f.noindex {
          props.push(("NOINDEX".to_string(), "true".to_string()));
        }
        if f.unf {
          props.push(("UNF".to_string(), "true".to_string()));
        }
        if (f.weight - 1.0).abs() > 1e-6 {
          let mut zmij_buf = zmij::Buffer::new();
          props.push(("WEIGHT".to_string(), zmij_buf.format(f.weight).to_string()));
        }
        if let Some(sep) = f.separator {
          props.push(("SEPARATOR".to_string(), sep.to_string()));
        }
        if f.case_sensitive {
          props.push(("CASESENSITIVE".to_string(), "true".to_string()));
        }
        if let Some(ref vm) = f.vector_meta {
          let mut itoa_buf = itoa::Buffer::new();
          props.push(("ALGORITHM".to_string(), vm.algorithm.as_str().to_string()));
          props.push(("TYPE".to_string(), vm.vector_type.as_str().to_string()));
          props.push(("DIM".to_string(), itoa_buf.format(vm.dim).to_string()));
          props.push((
            "DISTANCE_METRIC".to_string(),
            vm.distance_metric.as_str().to_string(),
          ));
          props.push(("M".to_string(), itoa_buf.format(vm.m).to_string()));
          props.push((
            "EF_CONSTRUCTION".to_string(),
            itoa_buf.format(vm.ef_construction).to_string(),
          ));
          props.push((
            "EF_RUNTIME".to_string(),
            itoa_buf.format(vm.ef_runtime).to_string(),
          ));
          let mut zmij_buf = zmij::Buffer::new();
          props.push((
            "EPSILON".to_string(),
            zmij_buf.format(vm.epsilon).to_string(),
          ));
        }
        FtFieldInfo {
          identifier: f.name.to_string(),
          attribute: f.alias.clone(),
          field_type: f.field_type.as_str().to_string(),
          properties: props,
        }
      })
      .collect();

    let num_terms = self.text_index.values().map(HashMap::len).sum::<usize>();

    let num_records = self
      .text_index
      .values()
      .map(|fm| fm.values().map(HashMap::len).sum::<usize>())
      .sum::<usize>();

    FtInfo {
      index_name: schema.name.to_string(),
      index_options: index_opts,
      index_definition: FtIndexDefinition {
        key_type: schema.on_data_type.as_str().to_string(),
        prefixes: schema.prefixes.clone(),
        filter: schema.filter.clone(),
        default_score: schema.default_score,
        language: schema.language.clone(),
      },
      fields: field_infos,
      num_docs: self.docs.len(),
      max_doc_id: self.docs.len(),
      num_terms,
      num_records,
      inverted_sz_mb: (num_terms * 64) as f64 / 1_048_576.0,
      vector_index_sz_mb: (self.vector_index.len() * 128) as f64 / 1_048_576.0,
      total_inverted_index_blocks: num_terms,
      offset_vectors_sz_mb: 0.0,
      doc_table_size_mb: (self.docs.len() * 128) as f64 / 1_048_576.0,
      sortable_values_size_mb: 0.0,
      key_table_size_mb: 0.0,
      records_per_doc_avg: 1.0,
      bytes_per_record_avg: 128.0,
      offsets_per_term_avg: 1.0,
      offset_bits_per_record_avg: 8.0,
      hash_indexing_failures: 0,
      indexing: false,
      percent_indexed: 1.0,
    }
  }
}

fn verify_phrase_positions(positions: &[&Vec<u32>], slop: usize, in_order: bool) -> bool {
  if positions.is_empty() {
    return true;
  }
  if positions.len() == 1 {
    return !positions[0].is_empty();
  }

  if in_order {
    for &start_pos in positions[0] {
      let mut curr_pos = start_pos;
      let mut matched = true;
      for next_list in &positions[1..] {
        if let Some(&next_pos) = next_list
          .iter()
          .find(|&&p| p > curr_pos && (p - curr_pos - 1) as usize <= slop)
        {
          curr_pos = next_pos;
        } else {
          matched = false;
          break;
        }
      }
      if matched {
        return true;
      }
    }
    false
  } else {
    // 无序短语匹配：寻找任意邻近位置
    for &start_pos in positions[0] {
      let mut matched = true;
      for next_list in &positions[1..] {
        if !next_list
          .iter()
          .any(|&p| (p as i64 - start_pos as i64).unsigned_abs() as usize <= slop + 1)
        {
          matched = false;
          break;
        }
      }
      if matched {
        return true;
      }
    }
    false
  }
}
