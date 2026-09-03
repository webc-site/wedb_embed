use std::borrow::Cow;

use super::ast::{FilterExpr, FilterOp, PathSegment, SliceIndex};
use crate::error::{Error, Result};

/// Parses JSONPath query string into a list of borrowed path segments.
/// 解析 JSONPath 表达式为片段列表（零堆分配借用切片）
pub fn parse_json_path<'a>(path: &'a str) -> Result<Vec<PathSegment<'a>>> {
  let s = path.trim();
  if s.is_empty() || s == "$" || s == "." {
    return Ok(vec![PathSegment::Root]);
  }

  let bytes = s.as_bytes();
  let mut i = 0;
  let mut segments = Vec::new();

  if bytes[0] == b'$' {
    segments.push(PathSegment::Root);
    i += 1;
  }

  while i < bytes.len() {
    if bytes[i] == b'.' {
      i += 1;
      if i >= bytes.len() {
        return Err(Error::invalid_data("Invalid JSONPath: trailing dot"));
      }
      if bytes[i] == b'.' {
        // 递归下降 ..
        i += 1;
        if i >= bytes.len() {
          return Err(Error::invalid_data(
            "Invalid JSONPath: trailing recursive descent '..'",
          ));
        }
        if bytes[i] == b'*' {
          segments.push(PathSegment::Recursive(Box::new(PathSegment::Wildcard)));
          i += 1;
          continue;
        }
        if bytes[i] == b'[' {
          let bracket_seg = parse_bracket(s, &mut i)?;
          segments.push(PathSegment::Recursive(Box::new(bracket_seg)));
          continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
          i += 1;
        }
        let name = s[start..i].trim();
        if name.is_empty() {
          return Err(Error::invalid_data(
            "Invalid JSONPath: empty identifier after '..'",
          ));
        }
        segments.push(PathSegment::Recursive(Box::new(PathSegment::Field(
          Cow::Borrowed(name),
        ))));
        continue;
      }
      if bytes[i] == b'*' {
        segments.push(PathSegment::Wildcard);
        i += 1;
        continue;
      }
      if bytes[i] == b'[' {
        let bracket_seg = parse_bracket(s, &mut i)?;
        segments.push(bracket_seg);
        continue;
      }
      let start = i;
      while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
        i += 1;
      }
      let name = s[start..i].trim();
      if name.is_empty() {
        return Err(Error::invalid_data(
          "Invalid JSONPath: empty identifier after '.'",
        ));
      }
      if name == "*" {
        segments.push(PathSegment::Wildcard);
      } else {
        segments.push(PathSegment::Field(Cow::Borrowed(name)));
      }
    } else if bytes[i] == b'[' {
      let bracket_seg = parse_bracket(s, &mut i)?;
      segments.push(bracket_seg);
    } else {
      let start = i;
      while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
        i += 1;
      }
      let name = s[start..i].trim();
      if name.is_empty() {
        return Err(Error::invalid_data("Invalid JSONPath: unexpected token"));
      }
      if name == "*" {
        segments.push(PathSegment::Wildcard);
      } else {
        segments.push(PathSegment::Field(Cow::Borrowed(name)));
      }
    }
  }

  if segments.is_empty() {
    Ok(vec![PathSegment::Root])
  } else {
    Ok(segments)
  }
}

pub(crate) fn parse_bracket<'a>(s: &'a str, i: &mut usize) -> Result<PathSegment<'a>> {
  let bytes = s.as_bytes();
  if *i >= bytes.len() || bytes[*i] != b'[' {
    return Err(Error::invalid_data("Expected '['"));
  }
  *i += 1;
  let start = *i;
  let mut depth = 1;
  let mut in_single_quote = false;
  let mut in_double_quote = false;

  let mut i_inner = *i;
  while i_inner < bytes.len() && depth > 0 {
    match bytes[i_inner] {
      b'\\' if in_single_quote || in_double_quote => {
        i_inner += 1; // 跳过转义字符
      }
      b'\'' if !in_double_quote => {
        in_single_quote = !in_single_quote;
      }
      b'"' if !in_single_quote => {
        in_double_quote = !in_double_quote;
      }
      b'[' if !in_single_quote && !in_double_quote => {
        depth += 1;
      }
      b']' if !in_single_quote && !in_double_quote => {
        depth -= 1;
      }
      _ => {}
    }
    if depth > 0 {
      i_inner += 1;
    }
  }
  *i = i_inner;

  if depth > 0 || *i >= bytes.len() || bytes[*i] != b']' {
    return Err(Error::invalid_data("Invalid JSONPath: unclosed bracket"));
  }

  let inner = s[start..*i].trim();
  *i += 1; // 跳过 ']'

  parse_bracket_content(inner)
}

pub(crate) fn parse_bracket_content<'a>(inner: &'a str) -> Result<PathSegment<'a>> {
  let inner = inner.trim();
  if inner.is_empty() || inner == "*" {
    return Ok(PathSegment::Wildcard);
  }

  // 过滤表达式 [?(...)]
  if let Some(stripped) = inner.strip_prefix('?') {
    let expr_str = stripped
      .trim()
      .trim_start_matches('(')
      .trim_end_matches(')')
      .trim();
    if let Some(filter) = parse_filter_expr(expr_str) {
      return Ok(PathSegment::Filter(filter));
    }
    return Err(Error::invalid_data("Invalid filter expression in JSONPath"));
  }

  // 多选择器逗号分割 [a, b, c] 或 [0, 1]
  if inner.contains(',') {
    let parts = split_bracket_parts(inner);
    if parts.len() > 1 {
      let mut slice_indices = Vec::new();
      let mut field_names = Vec::new();
      let mut all_indices = true;
      let mut all_fields = true;

      for &part in &parts {
        let p = part.trim();
        if let Some(slice_idx) = parse_single_slice(p) {
          slice_indices.push(slice_idx);
          all_fields = false;
        } else {
          let unquoted = unquote_cow(p);
          field_names.push(unquoted);
          all_indices = false;
        }
      }

      if all_indices {
        return Ok(PathSegment::MultiIndex(slice_indices));
      }
      if all_fields {
        return Ok(PathSegment::MultiField(field_names));
      }
      return Ok(PathSegment::MultiField(
        parts.into_iter().map(unquote_cow).collect(),
      ));
    }
  }

  // 单个切片或索引
  if let Some(slice_idx) = parse_single_slice(inner) {
    match slice_idx {
      SliceIndex::Index(idx) => return Ok(PathSegment::Index(idx)),
      SliceIndex::Slice { start, stop, step } => {
        return Ok(PathSegment::MultiIndex(vec![SliceIndex::Slice {
          start,
          stop,
          step,
        }]));
      }
    }
  }

  // 单个字段名（支持引号或无引号）
  let unquoted = unquote_cow(inner);
  if !unquoted.is_empty() {
    Ok(PathSegment::Field(unquoted))
  } else {
    Err(Error::invalid_data("Invalid empty bracket content"))
  }
}

pub(crate) fn split_bracket_parts(s: &str) -> Vec<&str> {
  let mut parts = Vec::new();
  let mut start = 0;
  let mut in_single_quote = false;
  let mut in_double_quote = false;
  let bytes = s.as_bytes();
  let mut i = 0;

  while i < bytes.len() {
    match bytes[i] {
      b'\\' if in_single_quote || in_double_quote => {
        i += 1; // 跳过转义字符
      }
      b'\'' if !in_double_quote => {
        in_single_quote = !in_single_quote;
      }
      b'"' if !in_single_quote => {
        in_double_quote = !in_double_quote;
      }
      b',' if !in_single_quote && !in_double_quote => {
        parts.push(s[start..i].trim());
        start = i + 1;
      }
      _ => {}
    }
    i += 1;
  }
  let rest = s[start..].trim();
  if !rest.is_empty() {
    parts.push(rest);
  }
  parts
}

#[inline]
pub(crate) fn unquote_cow(s: &str) -> Cow<'_, str> {
  let s = s.trim();
  if (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    || (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
  {
    let inner = &s[1..s.len() - 1];
    if inner.contains('\\') {
      Cow::Owned(
        inner
          .replace("\\'", "'")
          .replace("\\\"", "\"")
          .replace("\\\\", "\\"),
      )
    } else {
      Cow::Borrowed(inner)
    }
  } else {
    Cow::Borrowed(s)
  }
}

pub(crate) fn parse_single_slice(s: &str) -> Option<SliceIndex> {
  let s = s.trim();
  if s.contains(':') {
    let mut parts = s.split(':');
    let p0 = parts.next()?;
    let p1 = parts.next()?;
    let p2 = parts.next();
    if parts.next().is_some() {
      return None;
    }
    let parse_part = |p: &str| -> Option<Option<isize>> {
      let t = p.trim();
      if t.is_empty() {
        Some(None)
      } else {
        t.parse::<isize>().ok().map(Some)
      }
    };
    let start = parse_part(p0)?;
    let stop = parse_part(p1)?;
    let step = match p2 {
      Some(p) => parse_part(p)?,
      None => None,
    };
    Some(SliceIndex::Slice { start, stop, step })
  } else {
    s.parse::<isize>().ok().map(SliceIndex::Index)
  }
}

pub(crate) fn parse_filter_expr(s: &str) -> Option<FilterExpr<'_>> {
  let s = s.trim();
  if let Some(stripped) = s.strip_prefix('!') {
    let inner = stripped.trim().strip_prefix('@')?;
    let inner = inner.trim().strip_prefix('.').unwrap_or(inner);
    let path = inner.split('.').map(Cow::Borrowed).collect();
    return Some(FilterExpr {
      path,
      op: FilterOp::NotExists,
    });
  }

  for op_str in &["==", "!=", "<=", ">=", "<", ">"] {
    if let Some(idx) = s.find(op_str) {
      let left = s[..idx].trim();
      let right = s[idx + op_str.len()..].trim();

      let left_path = left
        .strip_prefix('@')?
        .trim()
        .strip_prefix('.')
        .unwrap_or(left)
        .split('.')
        .map(Cow::Borrowed)
        .collect();

      let right_val = if (right.starts_with('\'') && right.ends_with('\''))
        || (right.starts_with('"') && right.ends_with('"'))
      {
        sonic_rs::json!(unquote_cow(right).as_ref())
      } else if right == "true" {
        sonic_rs::json!(true)
      } else if right == "false" {
        sonic_rs::json!(false)
      } else if right == "null" {
        sonic_rs::json!(null)
      } else if let Ok(num) = right.parse::<f64>() {
        if num.fract() == 0.0 && num >= (i64::MIN as f64) && num <= (i64::MAX as f64) {
          sonic_rs::json!(num as i64)
        } else {
          sonic_rs::json!(num)
        }
      } else {
        sonic_rs::json!(right)
      };

      let op = match *op_str {
        "==" => FilterOp::Eq(right_val),
        "!=" => FilterOp::Ne(right_val),
        "<" => FilterOp::Lt(right.parse().unwrap_or(0.0)),
        "<=" => FilterOp::Le(right.parse().unwrap_or(0.0)),
        ">" => FilterOp::Gt(right.parse().unwrap_or(0.0)),
        ">=" => FilterOp::Ge(right.parse().unwrap_or(0.0)),
        _ => FilterOp::Exists,
      };

      return Some(FilterExpr {
        path: left_path,
        op,
      });
    }
  }

  let left = s.strip_prefix('@')?;
  let left = left.trim().strip_prefix('.').unwrap_or(left);
  let path = left.split('.').map(Cow::Borrowed).collect();
  Some(FilterExpr {
    path,
    op: FilterOp::Exists,
  })
}
