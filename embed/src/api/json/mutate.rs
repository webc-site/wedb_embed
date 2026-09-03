use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait, Value};

use super::{
  r#const::*,
  r#impl::{read_json_meta_and_val, write_json_meta_and_val},
  meta::JsonMeta,
  opt::JsonNumberOp,
  path::{
    delete_path_values, execute_numop, get_path_values, json_merge_patch, json_set_path,
    mutate_path_values,
  },
};
use crate::{
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};

/// Extracts lengths from a list of JSON nodes using a mapping closure.
#[inline]
pub(crate) fn extract_node_lengths<F>(nodes: &[&Value], extract: F) -> Vec<Option<usize>>
where
  F: Fn(&Value) -> Option<usize>,
{
  nodes.iter().map(|n| extract(n)).collect()
}

/// JSON numeric, string, boolean, and object mutations (JSON.NUMINCRBY, JSON.STRAPPEND, JSON.TOGGLE, etc.).
/// JSON 数值、字符串、布尔及对象变异操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  fn json_numop_internal<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    num_str: &str,
    op: JsonNumberOp,
  ) -> Result<Option<String>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(None),
    };

    let res_values = execute_numop(&mut root_val, path, num_str, op)?;

    if res_values.iter().any(|v| !v.is_null()) {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    let out = sonic_rs::to_string(&res_values).unwrap_or_default();
    Ok(Some(out))
  }

  #[inline]
  pub fn json_numincrby<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    num_str: &str,
  ) -> Result<Option<String>> {
    self.json_numop_internal(key, path, num_str, JsonNumberOp::Incr)
  }

  #[inline]
  pub fn json_nummultby<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    num_str: &str,
  ) -> Result<Option<String>> {
    self.json_numop_internal(key, path, num_str, JsonNumberOp::Mul)
  }

  #[inline]
  pub fn json_strappend<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
    str_to_append: &str,
  ) -> Result<Vec<Option<usize>>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let parsed_val;
    let append_str = if let Ok(val) = sonic_rs::from_str::<Value>(str_to_append) {
      if val.is_str() {
        parsed_val = val;
        parsed_val.as_str().unwrap_or(str_to_append)
      } else {
        return Err(Error::invalid_data(ERR_STRAPPEND_NEED_STRING));
      }
    } else {
      str_to_append
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let mut lengths = Vec::new();

    mutate_path_values(&mut root_val, p, |node| {
      if let Some(s) = node.as_str() {
        let mut new_str = String::with_capacity(s.len() + append_str.len());
        new_str.push_str(s);
        new_str.push_str(append_str);
        let len = new_str.len();
        *node = sonic_rs::json!(new_str);
        lengths.push(Some(len));
      } else {
        lengths.push(None);
      }
    })?;

    if lengths.iter().any(|opt| opt.is_some()) {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    Ok(lengths)
  }

  #[inline]
  pub fn json_strlen<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
  ) -> Result<Vec<Option<usize>>> {
    let kc = self.kc();
    let (_, root_val) = match read_json_meta_and_val(self, &kc, key)? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let nodes = get_path_values(&root_val, p)?;
    Ok(extract_node_lengths(&nodes, |n| {
      n.as_str().map(|s| s.len())
    }))
  }

  #[inline]
  pub fn json_clear<K: AsRef<[u8]>>(&self, key: K, path: Option<&str>) -> Result<usize> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(0),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let mut cleared = 0;

    mutate_path_values(&mut root_val, p, |node| {
      if let Some(arr) = node.as_array_mut() {
        if !arr.is_empty() {
          arr.clear();
          cleared += 1;
        }
      } else if let Some(obj) = node.as_object_mut()
        && !obj.is_empty()
      {
        obj.clear();
        cleared += 1;
      } else if let Some(f) = node.as_f64()
        && f != 0.0
      {
        *node = sonic_rs::json!(0);
        cleared += 1;
      }
    })?;

    if cleared > 0 {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    Ok(cleared)
  }

  #[inline]
  pub fn json_toggle<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
  ) -> Result<Vec<Option<bool>>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let mut toggled = Vec::new();

    mutate_path_values(&mut root_val, p, |node| {
      if let Some(b) = node.as_bool() {
        let new_b = !b;
        *node = sonic_rs::json!(new_b);
        toggled.push(Some(new_b));
      } else {
        toggled.push(None);
      }
    })?;

    if toggled.iter().any(|opt| opt.is_some()) {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    Ok(toggled)
  }

  #[inline]
  pub fn json_merge<K: AsRef<[u8]>>(&self, key: K, path: &str, patch_json: &str) -> Result<bool> {
    let kc = self.kc();
    let patch_val: Value = sonic_rs::from_str(patch_json)
      .map_err(|e| Error::invalid_data(format!("{ERR_INVALID_JSON_PATCH}: {e}")))?;

    match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some((meta, mut root_val)) => {
        let p = path.trim();
        if p == JSON_ROOT_PATH || p == "." || p.is_empty() {
          json_merge_patch(&mut root_val, &patch_val);
        } else if patch_val.is_null() {
          delete_path_values(&mut root_val, p)?;
        } else {
          let mutated = mutate_path_values(&mut root_val, p, |node| {
            json_merge_patch(node, &patch_val);
          })?;
          if mutated == 0 {
            json_set_path(&mut root_val, p, patch_val)?;
          }
        }
        write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
        Ok(true)
      }
      None => {
        let p = path.trim();
        if p != JSON_ROOT_PATH && p != "." && !p.is_empty() {
          return Err(Error::invalid_data(ERR_NEW_OBJECTS_MUST_BE_CREATED_AT_ROOT));
        }
        let mut root = sonic_rs::json!({});
        json_merge_patch(&mut root, &patch_val);
        let meta = JsonMeta::new_with_version(0, patch_json.len() as u64);
        write_json_meta_and_val(self, &kc, key, &meta, &root)?;
        Ok(true)
      }
    }
  }

  #[inline]
  pub fn json_objkeys<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
  ) -> Result<Vec<Option<Vec<String>>>> {
    let kc = self.kc();
    let (_, root_val) = match read_json_meta_and_val(self, &kc, key)? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let nodes = get_path_values(&root_val, p)?;

    let mut keys_list = Vec::with_capacity(nodes.len());
    for n in nodes {
      if let Some(obj) = n.as_object() {
        let keys: Vec<String> = obj.iter().map(|(k, _)| k.to_string()).collect();
        keys_list.push(Some(keys));
      } else {
        keys_list.push(None);
      }
    }

    Ok(keys_list)
  }

  #[inline]
  pub fn json_objlen<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
  ) -> Result<Vec<Option<usize>>> {
    let kc = self.kc();
    let (_, root_val) = match read_json_meta_and_val(self, &kc, key)? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let nodes = get_path_values(&root_val, p)?;
    Ok(extract_node_lengths(&nodes, |n| {
      n.as_object().map(|o| o.len())
    }))
  }
}
