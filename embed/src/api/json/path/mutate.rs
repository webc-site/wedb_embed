use sonic_rs::{JsonValueMutTrait, JsonValueTrait, Value};

use crate::{
  api::json::{
    r#const::{
      ERR_INPUT_SHOULD_BE_NUMBER, ERR_PARENT_PATH_DOES_NOT_EXIST, ERR_RESULT_IS_INFINITE,
      ERR_TARGET_PARENT_NOT_OBJECT, JSON_ROOT_PATH,
    },
    opt::JsonNumberOp,
    path::{
      ast::PathSegment,
      eval::{extract_simple_field, get_path_values, mutate_path_values},
      parser::parse_json_path,
    },
  },
  error::{Error, Result},
};

/// Recursively locates and mutates JSON value (aligned with Kvrocks Json::Set).
/// 内部递归定位并修改 JSON 值（对标 Kvrocks Json::Set / jsoncons::replace）
pub fn json_set_path(root: &mut Value, path_str: &str, new_val: Value) -> Result<bool> {
  let path_str = path_str.trim();
  if path_str == JSON_ROOT_PATH || path_str == "." || path_str.is_empty() {
    *root = new_val;
    return Ok(true);
  }

  let mutated = mutate_path_values(root, path_str, |node| {
    *node = new_val.clone();
  })?;

  if mutated > 0 {
    return Ok(true);
  }

  // 单层简单字段快速插入（父级为根对象且尚未包含该字段）
  if let Some(field) = extract_simple_field(path_str) {
    if let Some(obj) = root.as_object_mut() {
      obj.insert(field, new_val);
      return Ok(true);
    } else {
      return Err(Error::invalid_data(ERR_TARGET_PARENT_NOT_OBJECT));
    }
  }

  // 路径不存在时，尝试为规范的单目标路径定位父级并插入
  let segments = parse_json_path(path_str)?;
  let meaningful_segs: Vec<&PathSegment<'_>> = segments
    .iter()
    .filter(|s| !matches!(s, PathSegment::Root))
    .collect();

  if meaningful_segs.is_empty() {
    *root = new_val;
    return Ok(true);
  }

  let (parent_segs, last_seg) = meaningful_segs.split_at(meaningful_segs.len() - 1);
  let mut cur = root;

  for seg in parent_segs {
    match seg {
      PathSegment::Field(name) => {
        if let Some(obj) = cur.as_object_mut() {
          if let Some(next) = obj.get_mut(name) {
            cur = next;
          } else {
            return Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST));
          }
        } else {
          return Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST));
        }
      }
      PathSegment::Index(idx) => {
        if let Some(arr) = cur.as_array_mut() {
          let len = arr.len() as isize;
          let actual = if *idx < 0 { len + *idx } else { *idx };
          if actual >= 0 && (actual as usize) < arr.len() {
            cur = &mut arr[actual as usize];
          } else {
            return Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST));
          }
        } else {
          return Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST));
        }
      }
      _ => return Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST)),
    }
  }

  match last_seg[0] {
    PathSegment::Field(name) => {
      if let Some(obj) = cur.as_object_mut() {
        obj.insert(name.as_ref(), new_val);
        Ok(true)
      } else {
        Err(Error::invalid_data(ERR_TARGET_PARENT_NOT_OBJECT))
      }
    }
    PathSegment::Index(idx) => {
      if let Some(arr) = cur.as_array_mut() {
        let len = arr.len() as isize;
        if *idx == len {
          arr.push(new_val);
          Ok(true)
        } else if *idx >= 0 && (*idx as usize) < arr.len() {
          arr[*idx as usize] = new_val;
          Ok(true)
        } else if *idx < 0 && (len + *idx) >= 0 && ((len + *idx) as usize) < arr.len() {
          arr[(len + *idx) as usize] = new_val;
          Ok(true)
        } else {
          Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST))
        }
      } else {
        Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST))
      }
    }
    _ => Err(Error::invalid_data(ERR_PARENT_PATH_DOES_NOT_EXIST)),
  }
}

/// Internal generic numeric operation engine aligned with Kvrocks Json::numop.
/// 内部通用数值计算引擎（对标 Kvrocks Json::numop 与 NumOpEnum）
pub fn execute_numop(
  root: &mut Value,
  path: &str,
  num_str: &str,
  op: JsonNumberOp,
) -> Result<Vec<Value>> {
  // 严格校验操作数是否为有效数值（对标 Kvrocks: !number_res || !number.is_number() || number.is_string()）
  let delta = if let Ok(val) = sonic_rs::from_str::<Value>(num_str) {
    if val.is_number() && !val.is_str() {
      val.as_f64().unwrap_or(f64::NAN)
    } else {
      return Err(Error::invalid_data(ERR_INPUT_SHOULD_BE_NUMBER));
    }
  } else if let Ok(f) = num_str.parse::<f64>() {
    f
  } else {
    return Err(Error::invalid_data(ERR_INPUT_SHOULD_BE_NUMBER));
  };

  if delta.is_nan() || delta.is_infinite() {
    return Err(Error::invalid_data(ERR_RESULT_IS_INFINITE));
  }

  let mut res_values = Vec::new();
  let mut is_infinite_err = false;

  mutate_path_values(root, path, |node| {
    if is_infinite_err {
      return;
    }

    // 非数值目标返回 null 且不修改原值（对标 Kvrocks: !origin.is_number() || origin.is_string()）
    if !node.is_number() || node.is_str() {
      res_values.push(sonic_rs::json!(null));
      return;
    }

    let origin_f64 = node.as_f64().unwrap_or(0.0);
    let result_f64 = match op {
      JsonNumberOp::Incr => origin_f64 + delta,
      JsonNumberOp::Mul => origin_f64 * delta,
    };

    if result_f64.is_infinite() || result_f64.is_nan() {
      is_infinite_err = true;
      return;
    }

    // 整数精度保持（对标 Kvrocks: modf == 0 && min < v < max）
    if result_f64.fract() == 0.0 && result_f64 > (i64::MIN as f64) && result_f64 < (i64::MAX as f64)
    {
      let int_val = result_f64 as i64;
      *node = sonic_rs::json!(int_val);
      res_values.push(sonic_rs::json!(int_val));
    } else {
      *node = sonic_rs::json!(result_f64);
      res_values.push(sonic_rs::json!(result_f64));
    }
  })?;

  if is_infinite_err {
    return Err(Error::invalid_data(ERR_RESULT_IS_INFINITE));
  }

  Ok(res_values)
}

/// Recursively locates and retrieves first matching JSON value node.
/// 内部递归定位并获取首个匹配的 JSON 节点
pub fn json_get_path<'a>(root: &'a Value, path_str: &str) -> Option<&'a Value> {
  let values = get_path_values(root, path_str).ok()?;
  values.into_iter().next()
}

/// Helper utility for evaluating JSONPath queries.
/// JSON 路径查询辅助
pub fn json_path_query<'a>(root: &'a Value, path: &str) -> Vec<&'a Value> {
  get_path_values(root, path).unwrap_or_default()
}
