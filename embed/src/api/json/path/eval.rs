use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait, Value};

use super::{
  ast::{FilterExpr, FilterOp, PathSegment, SliceIndex},
  parser::parse_json_path,
};
use crate::error::Result;

/// Evaluates JSONPath filter expression against a candidate JSON node.
/// 评估过滤器表达式
pub(crate) fn eval_filter_expr(val: &Value, filter: &FilterExpr<'_>) -> bool {
  let mut cur = val;
  for seg in &filter.path {
    if let Some(obj) = cur.as_object() {
      if let Some(next) = obj.get(seg) {
        cur = next;
      } else {
        return matches!(filter.op, FilterOp::NotExists);
      }
    } else {
      return matches!(filter.op, FilterOp::NotExists);
    }
  }

  match &filter.op {
    FilterOp::Exists => !cur.is_null() && cur.as_bool().unwrap_or(true),
    FilterOp::NotExists => cur.is_null() || !cur.as_bool().unwrap_or(true),
    FilterOp::Eq(target) => {
      if let (Some(a), Some(b)) = (cur.as_f64(), target.as_f64()) {
        a == b
      } else {
        cur == target
      }
    }
    FilterOp::Ne(target) => {
      if let (Some(a), Some(b)) = (cur.as_f64(), target.as_f64()) {
        a != b
      } else {
        cur != target
      }
    }
    FilterOp::Lt(target) => cur.as_f64().is_some_and(|v| v < *target),
    FilterOp::Le(target) => cur.as_f64().is_some_and(|v| v <= *target),
    FilterOp::Gt(target) => cur.as_f64().is_some_and(|v| v > *target),
    FilterOp::Ge(target) => cur.as_f64().is_some_and(|v| v >= *target),
  }
}

/// Normalizes single array index handling negative offset and boundary conditions.
/// 归一化单个数组索引（处理负索引与越界）
#[inline]
pub fn normalize_index(len: usize, idx: isize) -> Option<usize> {
  let len_isize = len as isize;
  let actual = if idx < 0 { len_isize + idx } else { idx };
  if actual >= 0 && (actual as usize) < len {
    Some(actual as usize)
  } else {
    None
  }
}

/// Computes list of absolute indices for slice or index given array length.
/// 计算切片或索引在指定数组长度下的绝对下标列表（通用复用，消除重复）
pub fn resolve_slice_indices(len: usize, item: &SliceIndex, out: &mut Vec<usize>) {
  let len_isize = len as isize;
  if len_isize == 0 {
    return;
  }
  match item {
    SliceIndex::Index(idx) => {
      if let Some(actual) = normalize_index(len, *idx) {
        out.push(actual);
      }
    }
    SliceIndex::Slice { start, stop, step } => {
      let step_val = step.unwrap_or(1);
      if step_val == 0 {
        return;
      }
      if step_val > 0 {
        let s = match start {
          Some(v) if *v < 0 => (len_isize + *v).max(0),
          Some(v) => (*v).min(len_isize),
          None => 0,
        };
        let e = match stop {
          Some(v) if *v < 0 => (len_isize + *v).max(0),
          Some(v) => (*v).min(len_isize),
          None => len_isize,
        };
        let mut i = s;
        while i < e {
          if i >= 0 && (i as usize) < len {
            out.push(i as usize);
          }
          i += step_val;
        }
      } else {
        let s = match start {
          Some(v) if *v < 0 => len_isize + *v,
          Some(v) => (*v).min(len_isize - 1),
          None => len_isize - 1,
        };
        let e = match stop {
          Some(v) if *v < 0 => len_isize + *v,
          Some(v) => *v,
          None => -1,
        };
        let mut i = s;
        while i > e {
          if i >= 0 && (i as usize) < len {
            out.push(i as usize);
          }
          i += step_val;
        }
      }
    }
  }
}

/// Evaluates standard array slice with positive/negative step and boundary protection.
/// 标准数组切片求值（支持正负步长与边界保护）
pub fn eval_slice(
  arr: &[Value],
  start: Option<isize>,
  stop: Option<isize>,
  step: Option<isize>,
) -> Vec<&Value> {
  let mut indices = Vec::new();
  let slice_item = SliceIndex::Slice { start, stop, step };
  resolve_slice_indices(arr.len(), &slice_item, &mut indices);
  indices.into_iter().map(|idx| &arr[idx]).collect()
}

/// Extracts simple single-level field name with zero-heap-allocation fast path.
/// 提取单层简单字段名（零堆分配快速路径，支持 `$.field`、`.field`、`field`）
#[inline]
pub fn extract_simple_field(path: &str) -> Option<&str> {
  let s = path.trim();
  let candidate = if let Some(sub) = s.strip_prefix("$.") {
    sub
  } else if let Some(sub) = s.strip_prefix('.') {
    sub
  } else if !s.starts_with('$') && !s.starts_with('[') {
    s
  } else {
    return None;
  };

  let candidate = candidate.trim();
  if candidate.is_empty()
    || candidate == "*"
    || candidate.contains([
      '.', '[', ']', '*', '?', '(', ')', '\'', '"', '\\', ' ', '\t', '\n', '\r',
    ])
  {
    None
  } else {
    Some(candidate)
  }
}

/// Recursively traverses and collects immutable JSON node references.
/// 递归定位不可变节点引用
pub fn get_path_values<'a>(root: &'a Value, path: &str) -> Result<Vec<&'a Value>> {
  let s = path.trim();
  if s.is_empty() || s == "$" || s == "." {
    return Ok(vec![root]);
  }

  // 单层字段零语法树分配快速通道（对标 $.field 或 .field 或 field）
  if let Some(field) = extract_simple_field(s) {
    if let Some(obj) = root.as_object()
      && let Some(v) = obj.get(&field)
    {
      return Ok(vec![v]);
    } else {
      return Ok(Vec::new());
    }
  }

  let segments = parse_json_path(path)?;
  let mut current = vec![root];

  for seg in &segments {
    match seg {
      PathSegment::Root => {}
      PathSegment::Field(name) => {
        let mut next = Vec::new();
        for node in current {
          if let Some(obj) = node.as_object()
            && let Some(v) = obj.get(name)
          {
            next.push(v);
          }
        }
        current = next;
      }
      PathSegment::MultiField(names) => {
        let mut next = Vec::new();
        for node in current {
          if let Some(obj) = node.as_object() {
            for name in names {
              if let Some(v) = obj.get(name) {
                next.push(v);
              }
            }
          }
        }
        current = next;
      }
      PathSegment::Index(idx) => {
        let mut next = Vec::new();
        for node in current {
          if let Some(arr) = node.as_array()
            && let Some(actual) = normalize_index(arr.len(), *idx)
          {
            next.push(&arr[actual]);
          }
        }
        current = next;
      }
      PathSegment::MultiIndex(slices) => {
        let mut next = Vec::new();
        for node in current {
          if let Some(arr) = node.as_array() {
            let mut indices = Vec::new();
            for item in slices {
              resolve_slice_indices(arr.len(), item, &mut indices);
            }
            for idx in indices {
              if idx < arr.len() {
                next.push(&arr[idx]);
              }
            }
          }
        }
        current = next;
      }
      PathSegment::Wildcard => {
        let mut next = Vec::new();
        for node in current {
          if let Some(obj) = node.as_object() {
            for (_, v) in obj.iter() {
              next.push(v);
            }
          } else if let Some(arr) = node.as_array() {
            for v in arr.iter() {
              next.push(v);
            }
          }
        }
        current = next;
      }
      PathSegment::Filter(filter) => {
        let mut next = Vec::new();
        for node in current {
          if let Some(arr) = node.as_array() {
            for v in arr.iter() {
              if eval_filter_expr(v, filter) {
                next.push(v);
              }
            }
          }
        }
        current = next;
      }
      PathSegment::Recursive(target_seg) => {
        let mut next = Vec::new();
        for node in current {
          collect_recursive_matching(node, target_seg, &mut next);
        }
        current = next;
      }
    }
  }

  Ok(current)
}

fn collect_recursive_matching<'a>(
  node: &'a Value,
  target: &PathSegment<'_>,
  out: &mut Vec<&'a Value>,
) {
  match target {
    PathSegment::Wildcard => {
      if let Some(obj) = node.as_object() {
        for (_, v) in obj.iter() {
          out.push(v);
        }
      } else if let Some(arr) = node.as_array() {
        for v in arr.iter() {
          out.push(v);
        }
      }
    }
    PathSegment::Field(name) => {
      if let Some(obj) = node.as_object()
        && let Some(v) = obj.get(name)
      {
        out.push(v);
      }
    }
    PathSegment::Index(idx) => {
      if let Some(arr) = node.as_array()
        && let Some(actual) = normalize_index(arr.len(), *idx)
      {
        out.push(&arr[actual]);
      }
    }
    PathSegment::Filter(filter) => {
      if let Some(arr) = node.as_array() {
        for v in arr.iter() {
          if eval_filter_expr(v, filter) {
            out.push(v);
          }
        }
      }
    }
    _ => {}
  }

  if let Some(obj) = node.as_object() {
    for (_, child) in obj.iter() {
      collect_recursive_matching(child, target, out);
    }
  } else if let Some(arr) = node.as_array() {
    for child in arr.iter() {
      collect_recursive_matching(child, target, out);
    }
  }
}

/// Recursively modifies matching JSON value nodes in-place.
/// 就地递归修改 JSON 节点
pub fn mutate_path_values<F>(root: &mut Value, path: &str, mut f: F) -> Result<usize>
where
  F: FnMut(&mut Value),
{
  let s = path.trim();
  if s.is_empty() || s == "$" || s == "." {
    f(root);
    return Ok(1);
  }

  // 单层字段零语法树分配快速通道
  if let Some(field) = extract_simple_field(s) {
    if let Some(obj) = root.as_object_mut()
      && let Some(v) = obj.get_mut(&field)
    {
      f(v);
      return Ok(1);
    } else {
      return Ok(0);
    }
  }

  let segments = parse_json_path(path)?;
  Ok(mutate_recursive(root, &segments, &mut f))
}

fn mutate_recursive<F>(node: &mut Value, segments: &[PathSegment<'_>], f: &mut F) -> usize
where
  F: FnMut(&mut Value),
{
  if segments.is_empty() || (segments.len() == 1 && segments[0] == PathSegment::Root) {
    f(node);
    return 1;
  }

  let rest = if segments[0] == PathSegment::Root {
    &segments[1..]
  } else {
    &segments[0..]
  };

  if rest.is_empty() {
    f(node);
    return 1;
  }

  let head = &rest[0];
  let tail = &rest[1..];

  let mut count = 0;
  match head {
    PathSegment::Root => mutate_recursive(node, tail, f),
    PathSegment::Field(name) => {
      if tail.is_empty() {
        if let Some(obj) = node.as_object_mut()
          && let Some(v) = obj.get_mut(name)
        {
          f(v);
          count += 1;
        }
      } else if let Some(obj) = node.as_object_mut()
        && let Some(v) = obj.get_mut(name)
      {
        count += mutate_recursive(v, tail, f);
      }
      count
    }
    PathSegment::MultiField(names) => {
      if let Some(obj) = node.as_object_mut() {
        for name in names {
          if tail.is_empty() {
            if let Some(v) = obj.get_mut(name) {
              f(v);
              count += 1;
            }
          } else if let Some(v) = obj.get_mut(name) {
            count += mutate_recursive(v, tail, f);
          }
        }
      }
      count
    }
    PathSegment::Index(idx) => {
      if let Some(arr) = node.as_array_mut()
        && let Some(actual) = normalize_index(arr.len(), *idx)
      {
        let v = &mut arr[actual];
        if tail.is_empty() {
          f(v);
          count += 1;
        } else {
          count += mutate_recursive(v, tail, f);
        }
      }
      count
    }
    PathSegment::MultiIndex(slices) => {
      if let Some(arr) = node.as_array_mut() {
        let mut target_indices = Vec::new();
        for item in slices {
          resolve_slice_indices(arr.len(), item, &mut target_indices);
        }
        for idx in target_indices {
          if idx < arr.len() {
            let v = &mut arr[idx];
            if tail.is_empty() {
              f(v);
              count += 1;
            } else {
              count += mutate_recursive(v, tail, f);
            }
          }
        }
      }
      count
    }
    PathSegment::Wildcard => {
      if let Some(obj) = node.as_object_mut() {
        for (_, v) in obj.iter_mut() {
          if tail.is_empty() {
            f(v);
            count += 1;
          } else {
            count += mutate_recursive(v, tail, f);
          }
        }
      } else if let Some(arr) = node.as_array_mut() {
        for v in arr.iter_mut() {
          if tail.is_empty() {
            f(v);
            count += 1;
          } else {
            count += mutate_recursive(v, tail, f);
          }
        }
      }
      count
    }
    PathSegment::Filter(filter) => {
      if let Some(arr) = node.as_array_mut() {
        for v in arr.iter_mut() {
          if eval_filter_expr(v, filter) {
            if tail.is_empty() {
              f(v);
              count += 1;
            } else {
              count += mutate_recursive(v, tail, f);
            }
          }
        }
      }
      count
    }
    PathSegment::Recursive(target_seg) => mutate_recursive_descent(node, target_seg, tail, f),
  }
}

fn mutate_recursive_descent<F>(
  node: &mut Value,
  target: &PathSegment<'_>,
  tail: &[PathSegment<'_>],
  f: &mut F,
) -> usize
where
  F: FnMut(&mut Value),
{
  let mut count = 0;
  let mut direct_segments = vec![target.clone()];
  direct_segments.extend_from_slice(tail);
  count += mutate_recursive(node, &direct_segments, f);

  if let Some(obj) = node.as_object_mut() {
    for (_, child) in obj.iter_mut() {
      count += mutate_recursive_descent(child, target, tail, f);
    }
  } else if let Some(arr) = node.as_array_mut() {
    for child in arr.iter_mut() {
      count += mutate_recursive_descent(child, target, tail, f);
    }
  }

  count
}

/// Recursively deletes matching JSON value nodes in-place.
/// 就地删除匹配路径的节点
pub fn delete_path_values(root: &mut Value, path: &str) -> Result<usize> {
  let s = path.trim();
  if s.is_empty() || s == "$" || s == "." {
    return Ok(1);
  }

  // 单层字段零语法树分配快速通道
  if let Some(field) = extract_simple_field(s) {
    if let Some(obj) = root.as_object_mut()
      && obj.remove(&field).is_some()
    {
      return Ok(1);
    } else {
      return Ok(0);
    }
  }

  let segments = parse_json_path(path)?;
  if segments.is_empty() || (segments.len() == 1 && segments[0] == PathSegment::Root) {
    return Ok(1);
  }
  let rest = if segments[0] == PathSegment::Root {
    &segments[1..]
  } else {
    &segments[0..]
  };
  Ok(delete_recursive(root, rest))
}

fn delete_recursive(node: &mut Value, segments: &[PathSegment<'_>]) -> usize {
  if segments.is_empty() {
    return 0;
  }
  let head = &segments[0];
  let tail = &segments[1..];

  if tail.is_empty() {
    match head {
      PathSegment::Field(name) => {
        if let Some(obj) = node.as_object_mut()
          && obj.remove(name).is_some()
        {
          1
        } else {
          0
        }
      }
      PathSegment::MultiField(names) => {
        let mut count = 0;
        if let Some(obj) = node.as_object_mut() {
          for name in names {
            if obj.remove(name).is_some() {
              count += 1;
            }
          }
        }
        count
      }
      PathSegment::Index(idx) => {
        if let Some(arr) = node.as_array_mut()
          && let Some(actual) = normalize_index(arr.len(), *idx)
        {
          arr.remove(actual);
          1
        } else {
          0
        }
      }
      PathSegment::MultiIndex(slices) => {
        let mut count = 0;
        if let Some(arr) = node.as_array_mut() {
          let mut indices = Vec::new();
          for item in slices {
            resolve_slice_indices(arr.len(), item, &mut indices);
          }
          indices.sort_unstable();
          indices.dedup();
          for idx in indices.into_iter().rev() {
            if idx < arr.len() {
              arr.remove(idx);
              count += 1;
            }
          }
        }
        count
      }
      PathSegment::Wildcard => {
        if let Some(obj) = node.as_object_mut() {
          let len = obj.len();
          obj.clear();
          len
        } else if let Some(arr) = node.as_array_mut() {
          let len = arr.len();
          arr.clear();
          len
        } else {
          0
        }
      }
      PathSegment::Filter(filter) => {
        let mut count = 0;
        if let Some(arr) = node.as_array_mut() {
          let mut to_remove = Vec::new();
          for (i, item) in arr.iter().enumerate() {
            if eval_filter_expr(item, filter) {
              to_remove.push(i);
            }
          }
          for idx in to_remove.into_iter().rev() {
            arr.remove(idx);
            count += 1;
          }
        }
        count
      }
      PathSegment::Recursive(inner) => delete_recursive_descent(node, inner, &[]),
      _ => 0,
    }
  } else {
    let mut count = 0;
    match head {
      PathSegment::Field(name) => {
        if let Some(obj) = node.as_object_mut()
          && let Some(child) = obj.get_mut(name)
        {
          count += delete_recursive(child, tail);
        }
      }
      PathSegment::MultiField(names) => {
        if let Some(obj) = node.as_object_mut() {
          for name in names {
            if let Some(child) = obj.get_mut(name) {
              count += delete_recursive(child, tail);
            }
          }
        }
      }
      PathSegment::Index(idx) => {
        if let Some(arr) = node.as_array_mut()
          && let Some(actual) = normalize_index(arr.len(), *idx)
        {
          count += delete_recursive(&mut arr[actual], tail);
        }
      }
      PathSegment::MultiIndex(slices) => {
        if let Some(arr) = node.as_array_mut() {
          let mut indices = Vec::new();
          for item in slices {
            resolve_slice_indices(arr.len(), item, &mut indices);
          }
          indices.sort_unstable();
          indices.dedup();
          for idx in indices {
            if idx < arr.len() {
              count += delete_recursive(&mut arr[idx], tail);
            }
          }
        }
      }
      PathSegment::Wildcard => {
        if let Some(obj) = node.as_object_mut() {
          for (_, child) in obj.iter_mut() {
            count += delete_recursive(child, tail);
          }
        } else if let Some(arr) = node.as_array_mut() {
          for child in arr.iter_mut() {
            count += delete_recursive(child, tail);
          }
        }
      }
      PathSegment::Filter(filter) => {
        if let Some(arr) = node.as_array_mut() {
          for item in arr.iter_mut() {
            if eval_filter_expr(item, filter) {
              count += delete_recursive(item, tail);
            }
          }
        }
      }
      PathSegment::Recursive(inner) => {
        count += delete_recursive_descent(node, inner, tail);
      }
      _ => {}
    }
    count
  }
}

fn delete_recursive_descent(
  node: &mut Value,
  target: &PathSegment<'_>,
  tail: &[PathSegment<'_>],
) -> usize {
  let mut count = 0;

  if tail.is_empty() {
    match target {
      PathSegment::Field(name) => {
        if let Some(obj) = node.as_object_mut()
          && obj.remove(name).is_some()
        {
          count += 1;
        }
      }
      PathSegment::Index(idx) => {
        if let Some(arr) = node.as_array_mut() {
          let len = arr.len() as isize;
          let actual = if *idx < 0 { len + *idx } else { *idx };
          if actual >= 0 && (actual as usize) < arr.len() {
            arr.remove(actual as usize);
            count += 1;
          }
        }
      }
      PathSegment::Wildcard => {
        if let Some(obj) = node.as_object_mut() {
          count += obj.len();
          obj.clear();
        } else if let Some(arr) = node.as_array_mut() {
          count += arr.len();
          arr.clear();
        }
      }
      PathSegment::Filter(filter) => {
        if let Some(arr) = node.as_array_mut() {
          let mut to_remove = Vec::new();
          for (i, item) in arr.iter().enumerate() {
            if eval_filter_expr(item, filter) {
              to_remove.push(i);
            }
          }
          for i in to_remove.into_iter().rev() {
            arr.remove(i);
            count += 1;
          }
        }
      }
      _ => {}
    }
  } else {
    match target {
      PathSegment::Field(name) => {
        if let Some(obj) = node.as_object_mut()
          && let Some(child) = obj.get_mut(name)
        {
          count += delete_recursive(child, tail);
        }
      }
      PathSegment::Index(idx) => {
        if let Some(arr) = node.as_array_mut() {
          let len = arr.len() as isize;
          let actual = if *idx < 0 { len + *idx } else { *idx };
          if actual >= 0 && (actual as usize) < arr.len() {
            count += delete_recursive(&mut arr[actual as usize], tail);
          }
        }
      }
      PathSegment::Wildcard => {
        if let Some(obj) = node.as_object_mut() {
          for (_, child) in obj.iter_mut() {
            count += delete_recursive(child, tail);
          }
        } else if let Some(arr) = node.as_array_mut() {
          for child in arr.iter_mut() {
            count += delete_recursive(child, tail);
          }
        }
      }
      PathSegment::Filter(filter) => {
        if let Some(arr) = node.as_array_mut() {
          for item in arr.iter_mut() {
            if eval_filter_expr(item, filter) {
              count += delete_recursive(item, tail);
            }
          }
        }
      }
      _ => {}
    }
  }

  if let Some(obj) = node.as_object_mut() {
    for (_, child) in obj.iter_mut() {
      count += delete_recursive_descent(child, target, tail);
    }
  } else if let Some(arr) = node.as_array_mut() {
    for child in arr.iter_mut() {
      count += delete_recursive_descent(child, target, tail);
    }
  }

  count
}
