use std::{fmt::Write, mem::take};

use rapidhash::RapidHashMap as HashMap;

use crate::search::{
  encoding::parse_vector_from_slice,
  meta::VectorType,
  tokenizer::{tokenize_text, unescape_tag_string},
};

/// Query abstract syntax tree node aligned with Apache Kvrocks kqir::Node.
/// 检索引擎查询语法树节点（对标 Apache Kvrocks kqir::Node）
#[derive(Debug, Clone, PartialEq)]
pub enum SearchQueryNode {
  Wildcard,
  Term {
    field: Option<String>,
    term: String,
    is_prefix: bool,
    is_fuzzy: bool,
    max_edits: u8,
  },
  Phrase {
    field: Option<String>,
    terms: Vec<String>,
    slop: usize,
    in_order: bool,
  },
  Tag {
    field: String,
    tags: Vec<String>,
  },
  NumericRange {
    field: String,
    min: f64,
    min_inclusive: bool,
    max: f64,
    max_inclusive: bool,
  },
  GeoFilter {
    field: String,
    lon: f64,
    lat: f64,
    radius_m: f64,
  },
  VectorKnn {
    field: String,
    k: usize,
    vector_param: String,
    vector: Option<Vec<f64>>,
  },
  VectorRange {
    field: String,
    radius: f64,
    vector_param: String,
    vector: Option<Vec<f64>>,
  },
  And(Vec<SearchQueryNode>),
  Or(Vec<SearchQueryNode>),
  Not(Box<SearchQueryNode>),
}

/// Parses RediSearch query syntax string into AST.
/// 解析 RediSearch 查询字符串
#[inline]
pub fn parse_search_query(query: &str) -> SearchQueryNode {
  let empty_params = HashMap::default();
  parse_search_query_with_params(query, &empty_params)
}

/// Parses RediSearch query string with parameter substitutions.
/// 解析带有参数化替换的 RediSearch 查询字符串
pub fn parse_search_query_with_params(
  query: &str,
  params: &HashMap<String, String>,
) -> SearchQueryNode {
  let trimmed = query.trim();
  if trimmed == "*" || trimmed.is_empty() {
    return SearchQueryNode::Wildcard;
  }

  // 检查 KNN 箭头表达式语法：`*=>[KNN 10 @vec $v]` 或 `(@tag:{books}) => [KNN 5 @vec $param]`
  if let Some(arrow_pos) = trimmed.find("=>") {
    let left_part = trimmed[..arrow_pos].trim();
    let right_part = trimmed[arrow_pos + 2..].trim();
    if right_part.starts_with('[') && right_part.ends_with(']') {
      let inner_knn = right_part[1..right_part.len() - 1].trim();
      let knn_parts: Vec<&str> = inner_knn.split_whitespace().collect();
      if knn_parts.len() >= 4 && knn_parts[0].eq_ignore_ascii_case("KNN") {
        let filter_node = if left_part == "*" || left_part.is_empty() {
          SearchQueryNode::Wildcard
        } else {
          parse_search_query_part(left_part, params)
        };

        let k_str = knn_parts[1];
        let k = if let Some(param_k) = k_str.strip_prefix('$') {
          params
            .get(param_k)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10)
        } else {
          k_str.parse::<usize>().unwrap_or(10)
        };

        let field = knn_parts[2].trim_start_matches('@').to_string();
        let param_name = knn_parts[3].trim_start_matches('$').to_string();

        let vector = params
          .get(&param_name)
          .and_then(|val| parse_vector_from_slice(val.as_bytes(), VectorType::Float64).ok());

        let knn_node = SearchQueryNode::VectorKnn {
          field,
          k,
          vector_param: param_name,
          vector,
        };

        if filter_node == SearchQueryNode::Wildcard {
          return knn_node;
        } else {
          return SearchQueryNode::And(vec![filter_node, knn_node]);
        }
      }
    }
  }

  // 处理顶层 OR 表达式 `|`
  let or_parts = split_top_level_or(trimmed);
  if or_parts.len() > 1 {
    let mut nodes: Vec<SearchQueryNode> = or_parts
      .into_iter()
      .map(|p| parse_search_query_part(p.trim(), params))
      .filter(|n| *n != SearchQueryNode::Wildcard)
      .collect();
    if nodes.len() == 1 {
      return nodes.swap_remove(0);
    } else if !nodes.is_empty() {
      return SearchQueryNode::Or(nodes);
    }
  }

  parse_search_query_part(trimmed, params)
}

fn split_top_level_or(s: &str) -> Vec<String> {
  let mut parts = Vec::new();
  let mut cur = String::with_capacity(s.len());
  let mut paren_depth = 0usize;
  let mut bracket_depth = 0usize;
  let mut brace_depth = 0usize;
  let mut in_quote = false;
  let mut escaped = false;

  for ch in s.chars() {
    if escaped {
      cur.push(ch);
      escaped = false;
      continue;
    }
    if ch == '\\' {
      escaped = true;
      cur.push(ch);
      continue;
    }

    if ch == '"' {
      in_quote = !in_quote;
      cur.push(ch);
    } else if in_quote {
      cur.push(ch);
    } else if ch == '{' && bracket_depth == 0 {
      brace_depth += 1;
      cur.push(ch);
    } else if ch == '}' && bracket_depth == 0 {
      brace_depth = brace_depth.saturating_sub(1);
      cur.push(ch);
    } else if ch == '[' && brace_depth == 0 {
      bracket_depth += 1;
      cur.push(ch);
    } else if ch == ']' && brace_depth == 0 {
      bracket_depth = bracket_depth.saturating_sub(1);
      cur.push(ch);
    } else if ch == '(' && bracket_depth == 0 && brace_depth == 0 {
      paren_depth += 1;
      cur.push(ch);
    } else if ch == ')' && bracket_depth == 0 && brace_depth == 0 {
      paren_depth = paren_depth.saturating_sub(1);
      cur.push(ch);
    } else if ch == '|' && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
      parts.push(take(&mut cur));
    } else {
      cur.push(ch);
    }
  }
  if !cur.is_empty() {
    parts.push(cur);
  }
  parts
}

fn parse_search_query_part(part: &str, params: &HashMap<String, String>) -> SearchQueryNode {
  let mut and_nodes = Vec::new();
  let tokens = split_query_tokens(part);

  for token in tokens {
    if token.is_empty() {
      continue;
    }
    if let Some(stripped) = token.strip_prefix('-') {
      if !stripped.is_empty() {
        let inner = parse_single_clause(stripped, params);
        and_nodes.push(SearchQueryNode::Not(Box::new(inner)));
      }
    } else {
      let inner = parse_single_clause(&token, params);
      and_nodes.push(inner);
    }
  }

  match and_nodes.len() {
    0 => SearchQueryNode::Wildcard,
    1 => and_nodes.swap_remove(0),
    _ => SearchQueryNode::And(and_nodes),
  }
}

fn split_query_tokens(s: &str) -> Vec<String> {
  let mut tokens = Vec::new();
  let mut cur = String::with_capacity(32);
  let mut paren_depth = 0usize;
  let mut bracket_depth = 0usize;
  let mut brace_depth = 0usize;
  let mut in_quote = false;
  let mut escaped = false;

  for ch in s.chars() {
    if escaped {
      cur.push(ch);
      escaped = false;
      continue;
    }
    if ch == '\\' {
      escaped = true;
      cur.push(ch);
      continue;
    }

    if ch == '"' {
      in_quote = !in_quote;
      cur.push(ch);
    } else if in_quote {
      cur.push(ch);
    } else if ch == '{' && bracket_depth == 0 {
      brace_depth += 1;
      cur.push(ch);
    } else if ch == '}' && bracket_depth == 0 {
      brace_depth = brace_depth.saturating_sub(1);
      cur.push(ch);
    } else if ch == '[' && brace_depth == 0 {
      bracket_depth += 1;
      cur.push(ch);
    } else if ch == ']' && brace_depth == 0 {
      bracket_depth = bracket_depth.saturating_sub(1);
      cur.push(ch);
    } else if ch == '(' && bracket_depth == 0 && brace_depth == 0 {
      paren_depth += 1;
      cur.push(ch);
    } else if ch == ')' && bracket_depth == 0 && brace_depth == 0 {
      paren_depth = paren_depth.saturating_sub(1);
      cur.push(ch);
    } else if ch.is_whitespace() && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
      if !cur.is_empty() {
        tokens.push(take(&mut cur));
      }
    } else {
      cur.push(ch);
    }
  }
  if !cur.is_empty() {
    tokens.push(cur);
  }
  tokens
}

fn split_tag_list(inner: &str, params: &HashMap<String, String>) -> Vec<String> {
  let mut tags = Vec::new();
  let mut cur = String::with_capacity(16);
  let mut in_quote = false;
  let mut escaped = false;

  for ch in inner.chars() {
    if escaped {
      cur.push(ch);
      escaped = false;
      continue;
    }
    if ch == '\\' {
      escaped = true;
      cur.push(ch);
      continue;
    }
    if ch == '"' || ch == '\'' {
      in_quote = !in_quote;
      cur.push(ch);
    } else if in_quote {
      cur.push(ch);
    } else if ch == '|' {
      let item = take(&mut cur);
      let parsed_tag = process_single_tag(&item, params);
      if !parsed_tag.is_empty() {
        tags.push(parsed_tag);
      }
    } else {
      cur.push(ch);
    }
  }
  if !cur.is_empty() {
    let parsed_tag = process_single_tag(&cur, params);
    if !parsed_tag.is_empty() {
      tags.push(parsed_tag);
    }
  }
  tags
}

fn process_single_tag(item: &str, params: &HashMap<String, String>) -> String {
  let trimmed = item.trim();
  if let Some(param_name) = trimmed.strip_prefix('$')
    && let Some(val) = params.get(param_name)
  {
    return unescape_tag_string(val.trim());
  }
  let unquoted = trimmed.trim_matches('"').trim_matches('\'');
  unescape_tag_string(unquoted)
}

fn parse_single_clause(clause: &str, params: &HashMap<String, String>) -> SearchQueryNode {
  let trimmed = clause.trim();
  if trimmed == "*" {
    return SearchQueryNode::Wildcard;
  }

  // 括号嵌套表达式
  if trimmed.starts_with('(') && trimmed.ends_with(')') {
    let inner = trimmed[1..trimmed.len() - 1].trim();
    return parse_search_query_with_params(inner, params);
  }

  // 字段前缀过滤 `@field:...`
  if trimmed.starts_with('@')
    && let Some(colon_pos) = trimmed.find(':')
  {
    let field = trimmed[1..colon_pos].to_string();
    let value_part = trimmed[colon_pos + 1..].trim();

    // 支持字段分组查询 `@field:(hello world)` 或 `@field:(a | b)`
    if value_part.starts_with('(') && value_part.ends_with(')') {
      let inner_grouped = &value_part[1..value_part.len() - 1].trim();
      let sub_node = parse_search_query_with_params(inner_grouped, params);
      return attach_field_to_node(sub_node, &field);
    }

    // 标签过滤 `@tag:{a | b | c}`
    if value_part.starts_with('{') && value_part.ends_with('}') {
      let inner = &value_part[1..value_part.len() - 1];
      let tags = split_tag_list(inner, params);
      return SearchQueryNode::Tag { field, tags };
    }

    // 数值范围过滤 `@num:[min max]` 或 `@num:[(min max]`
    if (value_part.starts_with('[') || value_part.starts_with('('))
      && (value_part.ends_with(']') || value_part.ends_with(')'))
    {
      let inner = value_part[1..value_part.len() - 1].trim();
      // 检查 VECTOR_RANGE 向量范围查询 `[VECTOR_RANGE radius $param]`
      if inner.to_ascii_uppercase().starts_with("VECTOR_RANGE") {
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.len() >= 3 {
          let radius = if let Some(param_r) = parts[1].strip_prefix('$') {
            params
              .get(param_r)
              .and_then(|v| v.parse::<f64>().ok())
              .unwrap_or(1.0)
          } else {
            parts[1].parse::<f64>().unwrap_or(1.0)
          };

          let param_name = parts[2].trim_start_matches('$').to_string();
          let vector = params
            .get(&param_name)
            .and_then(|val| parse_vector_from_slice(val.as_bytes(), VectorType::Float64).ok());
          return SearchQueryNode::VectorRange {
            field,
            radius,
            vector_param: param_name,
            vector,
          };
        }
      }

      let parts: Vec<&str> = inner.split_whitespace().collect();
      if parts.len() >= 2 {
        let (min, min_inc) = parse_num_bound(parts[0], params);
        let (max, max_inc) = parse_num_bound(parts[1], params);
        return SearchQueryNode::NumericRange {
          field,
          min,
          min_inclusive: min_inc,
          max,
          max_inclusive: max_inc,
        };
      }
    }

    // 短语精确匹配 `@field:"hello world"`
    if value_part.starts_with('"') && value_part.ends_with('"') && value_part.len() >= 2 {
      let phrase = &value_part[1..value_part.len() - 1];
      let terms = tokenize_text(phrase);
      if terms.len() > 1 {
        return SearchQueryNode::Phrase {
          field: Some(field),
          terms,
          slop: 0,
          in_order: true,
        };
      } else if let Some(first) = terms.into_iter().next() {
        return SearchQueryNode::Term {
          field: Some(field),
          term: first,
          is_prefix: false,
          is_fuzzy: false,
          max_edits: 0,
        };
      } else {
        return SearchQueryNode::Term {
          field: Some(field),
          term: String::new(),
          is_prefix: false,
          is_fuzzy: false,
          max_edits: 0,
        };
      }
    }

    // 模糊匹配 `@field:%%term%%` (2 edits) 或 `@field:%term%` (1 edit)
    if value_part.starts_with("%%") && value_part.ends_with("%%") && value_part.len() >= 4 {
      let term = value_part[2..value_part.len() - 2].to_lowercase();
      return SearchQueryNode::Term {
        field: Some(field),
        term,
        is_prefix: false,
        is_fuzzy: true,
        max_edits: 2,
      };
    } else if value_part.starts_with('%') && value_part.ends_with('%') && value_part.len() >= 2 {
      let term = value_part[1..value_part.len() - 1].to_lowercase();
      return SearchQueryNode::Term {
        field: Some(field),
        term,
        is_prefix: false,
        is_fuzzy: true,
        max_edits: 1,
      };
    }

    // 前缀匹配 `@field:term*`
    let is_prefix = value_part.ends_with('*');
    let term = if is_prefix {
      value_part[..value_part.len() - 1].to_lowercase()
    } else {
      value_part.to_lowercase()
    };
    return SearchQueryNode::Term {
      field: Some(field),
      term,
      is_prefix,
      is_fuzzy: false,
      max_edits: 0,
    };
  }

  // 全局短语精确匹配 `"hello world"`
  if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
    let phrase = &trimmed[1..trimmed.len() - 1];
    let terms = tokenize_text(phrase);
    if terms.len() > 1 {
      return SearchQueryNode::Phrase {
        field: None,
        terms,
        slop: 0,
        in_order: true,
      };
    } else if let Some(first) = terms.into_iter().next() {
      return SearchQueryNode::Term {
        field: None,
        term: first,
        is_prefix: false,
        is_fuzzy: false,
        max_edits: 0,
      };
    } else {
      return SearchQueryNode::Term {
        field: None,
        term: String::new(),
        is_prefix: false,
        is_fuzzy: false,
        max_edits: 0,
      };
    }
  }

  // 全局模糊匹配 `%%term%%` (2 edits) 或 `%term%` (1 edit)
  if trimmed.starts_with("%%") && trimmed.ends_with("%%") && trimmed.len() >= 4 {
    let term = trimmed[2..trimmed.len() - 2].to_lowercase();
    return SearchQueryNode::Term {
      field: None,
      term,
      is_prefix: false,
      is_fuzzy: true,
      max_edits: 2,
    };
  } else if trimmed.starts_with('%') && trimmed.ends_with('%') && trimmed.len() >= 2 {
    let term = trimmed[1..trimmed.len() - 1].to_lowercase();
    return SearchQueryNode::Term {
      field: None,
      term,
      is_prefix: false,
      is_fuzzy: true,
      max_edits: 1,
    };
  }

  // 全局前缀与词条匹配
  let is_prefix = trimmed.ends_with('*');
  let term = if is_prefix {
    trimmed[..trimmed.len() - 1].to_lowercase()
  } else {
    trimmed.to_lowercase()
  };
  SearchQueryNode::Term {
    field: None,
    term,
    is_prefix,
    is_fuzzy: false,
    max_edits: 0,
  }
}

fn attach_field_to_node(node: SearchQueryNode, field: &str) -> SearchQueryNode {
  match node {
    SearchQueryNode::Term {
      term,
      is_prefix,
      is_fuzzy,
      max_edits,
      ..
    } => SearchQueryNode::Term {
      field: Some(field.to_string()),
      term,
      is_prefix,
      is_fuzzy,
      max_edits,
    },
    SearchQueryNode::Phrase {
      terms,
      slop,
      in_order,
      ..
    } => SearchQueryNode::Phrase {
      field: Some(field.to_string()),
      terms,
      slop,
      in_order,
    },
    SearchQueryNode::And(children) => SearchQueryNode::And(
      children
        .into_iter()
        .map(|c| attach_field_to_node(c, field))
        .collect(),
    ),
    SearchQueryNode::Or(children) => SearchQueryNode::Or(
      children
        .into_iter()
        .map(|c| attach_field_to_node(c, field))
        .collect(),
    ),
    SearchQueryNode::Not(inner) => {
      SearchQueryNode::Not(Box::new(attach_field_to_node(*inner, field)))
    }
    other => other,
  }
}

fn parse_num_bound(bound_str: &str, params: &HashMap<String, String>) -> (f64, bool) {
  let clean = bound_str
    .trim()
    .trim_start_matches('[')
    .trim_end_matches(']')
    .trim_end_matches(')');

  let (val_str, inclusive) = if let Some(stripped) = clean.strip_prefix('(') {
    (stripped, false)
  } else {
    (clean, true)
  };

  let resolved_val = if let Some(param_name) = val_str.strip_prefix('$') {
    params
      .get(param_name)
      .map(String::as_str)
      .unwrap_or(val_str)
  } else {
    val_str
  };

  if resolved_val.eq_ignore_ascii_case("-inf") {
    (f64::NEG_INFINITY, true)
  } else if resolved_val.eq_ignore_ascii_case("+inf") || resolved_val.eq_ignore_ascii_case("inf") {
    (f64::INFINITY, true)
  } else {
    (resolved_val.parse::<f64>().unwrap_or(0.0), inclusive)
  }
}

/// Formats query execution plan string aligned with Apache Kvrocks FT.EXPLAIN.
/// 生成执行计划字符串（对标 Apache Kvrocks FT.EXPLAIN）
pub fn explain_search_query(node: &SearchQueryNode) -> String {
  let mut out = String::with_capacity(128);
  format_explain(node, &mut out, 0);
  out
}

/// Formats CLI query execution plan aligned with Apache Kvrocks FT.EXPLAINCLI.
/// 生成 CLI 格式化执行计划（对标 Apache Kvrocks FT.EXPLAINCLI）
#[inline]
pub fn explain_search_query_cli(node: &SearchQueryNode) -> String {
  explain_search_query(node)
}

fn format_explain(node: &SearchQueryNode, out: &mut String, depth: usize) {
  let indent = "  ".repeat(depth);
  match node {
    SearchQueryNode::Wildcard => {
      let _ = writeln!(out, "{indent}ALL");
    }
    SearchQueryNode::Term {
      field,
      term,
      is_prefix,
      is_fuzzy,
      ..
    } => {
      let f = field.as_deref().unwrap_or("*");
      let p = if *is_prefix {
        "*"
      } else if *is_fuzzy {
        "%"
      } else {
        ""
      };
      let _ = writeln!(out, "{indent}UNION <{f}:{term}{p}>");
    }
    SearchQueryNode::Phrase {
      field,
      terms,
      slop,
      in_order,
    } => {
      let f = field.as_deref().unwrap_or("*");
      let joined = terms.join(" ");
      let _ = writeln!(
        out,
        "{indent}PHRASE <{f}:\"{joined}\" slop={slop} in_order={in_order}>"
      );
    }
    SearchQueryNode::Tag { field, tags } => {
      let tag_list = tags.join(" | ");
      let _ = writeln!(out, "{indent}TAG <@{field}:{{{tag_list}}}>");
    }
    SearchQueryNode::NumericRange {
      field,
      min,
      min_inclusive,
      max,
      max_inclusive,
    } => {
      let left = if *min_inclusive { "[" } else { "(" };
      let right = if *max_inclusive { "]" } else { ")" };
      let _ = writeln!(out, "{indent}NUMERIC <@{field}:{left}{min} {max}{right}>");
    }
    SearchQueryNode::GeoFilter {
      field,
      lon,
      lat,
      radius_m,
    } => {
      let _ = writeln!(out, "{indent}GEO <@{field}:[{lon} {lat} {radius_m} m]>");
    }
    SearchQueryNode::VectorKnn {
      field,
      k,
      vector_param,
      ..
    } => {
      let _ = writeln!(out, "{indent}VECTOR KNN <@{field} k={k} ${vector_param}>");
    }
    SearchQueryNode::VectorRange {
      field,
      radius,
      vector_param,
      ..
    } => {
      let _ = writeln!(
        out,
        "{indent}VECTOR RANGE <@{field} radius={radius} ${vector_param}>"
      );
    }
    SearchQueryNode::And(nodes) => {
      let _ = writeln!(out, "{indent}INTERSECT {{");
      for n in nodes {
        format_explain(n, out, depth + 1);
      }
      let _ = writeln!(out, "{indent}}}");
    }
    SearchQueryNode::Or(nodes) => {
      let _ = writeln!(out, "{indent}UNION {{");
      for n in nodes {
        format_explain(n, out, depth + 1);
      }
      let _ = writeln!(out, "{indent}}}");
    }
    SearchQueryNode::Not(inner) => {
      let _ = writeln!(out, "{indent}NOT {{");
      format_explain(inner, out, depth + 1);
      let _ = writeln!(out, "{indent}}}");
    }
  }
}
