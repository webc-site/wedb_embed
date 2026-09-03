use std::str;

use rapidhash::RapidHashMap;
use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait, Value};

use crate::{
  api::json::{
    r#const::{
      ERR_CORRUPTED_JSON, ERR_INVALID_JSON, ERR_INVALID_JSON_NEEDLE, ERR_INVALID_JSON_PATCH,
      ERR_INVALID_JSON_VALUE, ERR_NEW_OBJECTS_MUST_BE_CREATED_AT_ROOT, ERR_STRAPPEND_NEED_STRING,
      JSON_ROOT_PATH,
    },
    key,
    meta::{JsonMeta, JsonStorageFormat},
    opt::{JsonArrIndex, JsonGet, JsonNumberOp, JsonSet},
    path::{
      delete_path_values, execute_numop, format_json, get_path_values, json_merge_patch,
      json_set_path, mutate_path_values,
    },
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  key_composer::KeyComposer,
  meta::current_now_ms,
  wedb::Db,
};

/// Internal helper reading JSON metadata and parsed value for a given key.
/// 内部辅助：读取指定 JSON 键的元数据与解析后的 Value
#[inline]
fn read_json_meta_and_val<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  kc: &KeyComposer,
  key: K,
) -> Result<Option<(JsonMeta, Value)>>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let meta_k = key::meta(kc, key_bytes);
  let now_ms = current_now_ms();

  let meta = match get_meta_checked::<JsonMeta, _>(db, key_bytes, &meta_k, now_ms)? {
    Some(m) => m,
    None => return Ok(None),
  };

  let data_k = key::prefix_stack(kc, key_bytes);
  let data_ks = db.data();

  let payload = match data_ks.get(&data_k)? {
    Some(v) => v,
    None => return Ok(None),
  };

  let val: Value = sonic_rs::from_slice(&payload)
    .map_err(|e| Error::invalid_data(format!("{ERR_CORRUPTED_JSON}: {e}")))?;

  Ok(Some((meta, val)))
}

/// Metadata key.
/// 内部辅助：序列化并写回 JSON 键与元数据
#[inline]
fn write_json_meta_and_val<E: Engine, K: AsRef<[u8]>>(
  db: &Db<E>,
  kc: &KeyComposer,
  key: K,
  meta: &JsonMeta,
  val: &Value,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let key_bytes = key.as_ref();
  let meta_k = key::meta(kc, key_bytes);
  let data_k = key::prefix_stack(kc, key_bytes);

  let payload =
    sonic_rs::to_vec(val).map_err(|e| Error::invalid_data(format!("ERR JSON serialize: {e}")))?;

  let mut updated_meta = *meta;
  updated_meta.base.size = payload.len() as u64;

  let mut batch = db.batch();
  batch.insert_meta(&meta_k, &updated_meta.encode());
  batch.insert_data(&data_k, &payload);
  batch.commit()?;
  Ok(())
}

/// JSON data structure operations interface (JSON).
/// JSON 数据结构操作接口 (JSON)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn json_set<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    json_str: &str,
    opt_li: impl IntoIterator<Item = JsonSet>,
  ) -> Result<bool> {
    let kc = self.kc();
    let parsed_new_val: Value = sonic_rs::from_str(json_str)
      .map_err(|e| Error::invalid_data(format!("{ERR_INVALID_JSON}: {e}")))?;

    let existing = read_json_meta_and_val(self, &kc, key.as_ref())?;
    let key_exists = existing.is_some();

    for option in opt_li {
      match option {
        JsonSet::Nx if key_exists => return Ok(false),
        JsonSet::Xx if !key_exists => return Ok(false),
        _ => {}
      }
    }

    match existing {
      Some((meta, mut root_val)) => {
        json_set_path(&mut root_val, path, parsed_new_val)?;
        write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
        Ok(true)
      }
      None => {
        let p = path.trim();
        if p != JSON_ROOT_PATH && p != "." && !p.is_empty() {
          return Err(Error::invalid_data(ERR_NEW_OBJECTS_MUST_BE_CREATED_AT_ROOT));
        }
        let meta = JsonMeta::new_with_version(0, json_str.len() as u64);
        write_json_meta_and_val(self, &kc, key, &meta, &parsed_new_val)?;
        Ok(true)
      }
    }
  }

  #[inline]
  pub fn json_set_one<K: AsRef<[u8]>>(&self, key: K, path: &str, json_str: &str) -> Result<bool> {
    self.json_set(key, path, json_str, [])
  }

  #[inline]
  pub fn json_get_one<K: AsRef<[u8]>>(&self, key: K, path: &str) -> Result<Option<String>> {
    self.json_get(key, &[path], [])
  }

  #[inline]
  pub fn json_get<K: AsRef<[u8]>>(
    &self,
    key: K,
    paths: &[&str],
    opt_li: impl IntoIterator<Item = JsonGet>,
  ) -> Result<Option<String>> {
    let mut indent = None;
    let mut newline = None;
    let mut space = None;
    for o in opt_li {
      match o {
        JsonGet::Indent(i) => indent = Some(i),
        JsonGet::Newline(n) => newline = Some(n),
        JsonGet::Space(s) => space = Some(s),
      }
    }
    self.json_get_formatted(
      key,
      paths,
      indent.as_deref(),
      newline.as_deref(),
      space.as_deref(),
    )
  }

  #[inline]
  pub fn json_get_formatted<K: AsRef<[u8]>>(
    &self,
    key: K,
    paths: &[&str],
    indent: Option<&str>,
    newline: Option<&str>,
    space: Option<&str>,
  ) -> Result<Option<String>> {
    let kc = self.kc();
    let (_, root_val) = match read_json_meta_and_val(self, &kc, key)? {
      Some(pair) => pair,
      None => return Ok(None),
    };

    if paths.is_empty() {
      let out = format_json(&root_val, indent, newline, space);
      return Ok(Some(out));
    }

    if paths.len() == 1 {
      let path_str = paths[0];
      let nodes = get_path_values(&root_val, path_str)?;
      let arr_val: Value = nodes.into_iter().cloned().collect();
      let out = format_json(&arr_val, indent, newline, space);
      return Ok(Some(out));
    }

    // 多路径：返回各路径为 key 的映射对象（对标 Kvrocks MPath 匹配数组映射）
    let mut result_obj = sonic_rs::json!({});
    if let Some(obj_mut) = result_obj.as_object_mut() {
      for &p in paths {
        let nodes = get_path_values(&root_val, p)?;
        let arr_val: Value = nodes.into_iter().cloned().collect();
        obj_mut.insert(p, arr_val);
      }
    }

    let out = format_json(&result_obj, indent, newline, space);
    Ok(Some(out))
  }

  #[inline]
  pub fn json_mget<K: AsRef<[u8]>>(&self, keys: &[K], path: &str) -> Result<Vec<Option<String>>> {
    let kc = self.kc();
    let mut results = Vec::with_capacity(keys.len());
    for k in keys {
      let res = match read_json_meta_and_val(self, &kc, k)? {
        Some((_, root_val)) => {
          let nodes = get_path_values(&root_val, path)?;
          let arr_val: Value = nodes.into_iter().cloned().collect();
          Some(sonic_rs::to_string(&arr_val).unwrap_or_default())
        }
        None => None,
      };
      results.push(res);
    }
    Ok(results)
  }

  #[inline]
  pub fn json_mset_one<K: AsRef<[u8]>>(&self, key: K, path: &str, val_str: &str) -> Result<()> {
    self.json_mset(&[(key, path, val_str)])
  }

  #[inline]
  pub fn json_mset<K: AsRef<[u8]>>(&self, triplets: &[(K, &str, &str)]) -> Result<()> {
    let kc = self.kc();
    let mut dirty_keys: RapidHashMap<Vec<u8>, (Value, JsonMeta)> = RapidHashMap::default();

    for (k, path, val_str) in triplets {
      let key_bytes = k.as_ref();
      let parsed_new_val: Value = sonic_rs::from_str(val_str)
        .map_err(|e| Error::invalid_data(format!("{ERR_INVALID_JSON}: {e}")))?;

      if let Some((existing_val, _)) = dirty_keys.get_mut(key_bytes) {
        json_set_path(existing_val, path, parsed_new_val)?;
      } else {
        match read_json_meta_and_val(self, &kc, key_bytes)? {
          Some((meta, mut val)) => {
            json_set_path(&mut val, path, parsed_new_val)?;
            dirty_keys.insert(key_bytes.to_vec(), (val, meta));
          }
          None => {
            let p = path.trim();
            if p != JSON_ROOT_PATH && p != "." && !p.is_empty() {
              return Err(Error::invalid_data(ERR_NEW_OBJECTS_MUST_BE_CREATED_AT_ROOT));
            }
            let meta = JsonMeta::new_with_version(0, val_str.len() as u64);
            dirty_keys.insert(key_bytes.to_vec(), (parsed_new_val, meta));
          }
        }
      }
    }

    for (k_bytes, (val, meta)) in dirty_keys {
      write_json_meta_and_val(self, &kc, &k_bytes, &meta, &val)?;
    }

    Ok(())
  }

  #[inline]
  pub fn json_del<K: AsRef<[u8]>>(&self, key: K, path: Option<&str>) -> Result<usize> {
    let kc = self.kc();
    let key_bytes = key.as_ref();

    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key_bytes)? {
      Some(pair) => pair,
      None => return Ok(0),
    };

    let Some(p) = path else {
      let meta_k = key::meta(&kc, key_bytes);
      let data_k = key::prefix_stack(&kc, key_bytes);
      let mut batch = self.batch();
      batch.rm_meta(&meta_k);
      batch.rm_data(&data_k);
      batch.commit()?;
      return Ok(1);
    };
    let p = p.trim();
    if p == JSON_ROOT_PATH || p == "." {
      let meta_k = key::meta(&kc, key_bytes);
      let data_k = key::prefix_stack(&kc, key_bytes);
      let mut batch = self.batch();
      batch.rm_meta(&meta_k);
      batch.rm_data(&data_k);
      batch.commit()?;
      return Ok(1);
    }

    let deleted = delete_path_values(&mut root_val, p)?;

    if deleted > 0 {
      write_json_meta_and_val(self, &kc, key_bytes, &meta, &root_val)?;
    }

    Ok(deleted)
  }

  #[inline]
  pub fn json_type<K: AsRef<[u8]>>(&self, key: K, path: Option<&str>) -> Result<Vec<String>> {
    let kc = self.kc();
    let (_, root_val) = match read_json_meta_and_val(self, &kc, key)? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let nodes = get_path_values(&root_val, p)?;

    let mut types = Vec::with_capacity(nodes.len());
    for n in nodes {
      let t = if n.is_null() {
        "null"
      } else if n.is_boolean() {
        "boolean"
      } else if n.is_i64() || n.is_u64() {
        "integer"
      } else if n.is_f64() {
        "number"
      } else if n.is_str() {
        "string"
      } else if n.is_array() {
        "array"
      } else if n.is_object() {
        "object"
      } else {
        "unknown"
      };
      types.push(t.to_string());
    }

    Ok(types)
  }

  #[inline]
  pub fn json_numincrby<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    num_str: &str,
  ) -> Result<Option<String>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(None),
    };

    let res_values = execute_numop(&mut root_val, path, num_str, JsonNumberOp::Incr)?;

    if res_values.iter().any(|v| !v.is_null()) {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    let out = sonic_rs::to_string(&res_values).unwrap_or_default();
    Ok(Some(out))
  }

  #[inline]
  pub fn json_nummultby<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    num_str: &str,
  ) -> Result<Option<String>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(None),
    };

    let res_values = execute_numop(&mut root_val, path, num_str, JsonNumberOp::Mul)?;

    if res_values.iter().any(|v| !v.is_null()) {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    let out = sonic_rs::to_string(&res_values).unwrap_or_default();
    Ok(Some(out))
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

    // 校验并提取追加字符串（对标 Kvrocks: STRAPPEND need input a string to append）
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

    let mut lengths = Vec::with_capacity(nodes.len());
    for n in nodes {
      if let Some(s) = n.as_str() {
        lengths.push(Some(s.len()));
      } else {
        lengths.push(None);
      }
    }

    Ok(lengths)
  }

  #[inline]
  pub fn json_arrappend<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    values_json: &[&str],
  ) -> Result<Vec<Option<usize>>> {
    let kc = self.kc();
    let mut parsed_vals = Vec::with_capacity(values_json.len());
    for &s in values_json {
      let v: Value = sonic_rs::from_str(s)
        .map_err(|e| Error::invalid_data(format!("{ERR_INVALID_JSON_VALUE}: {e}")))?;
      parsed_vals.push(v);
    }

    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let mut lengths = Vec::new();
    mutate_path_values(&mut root_val, path, |node| {
      if let Some(arr) = node.as_array_mut() {
        for item in &parsed_vals {
          arr.push(item.clone());
        }
        lengths.push(Some(arr.len()));
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
  pub fn json_arrinsert<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    index: isize,
    values_json: &[&str],
  ) -> Result<Vec<Option<usize>>> {
    let kc = self.kc();
    let mut parsed_vals = Vec::with_capacity(values_json.len());
    for &s in values_json {
      let v: Value = sonic_rs::from_str(s)
        .map_err(|e| Error::invalid_data(format!("{ERR_INVALID_JSON_VALUE}: {e}")))?;
      parsed_vals.push(v);
    }

    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let mut lengths = Vec::new();
    mutate_path_values(&mut root_val, path, |node| {
      if let Some(arr) = node.as_array_mut() {
        let len = arr.len() as isize;
        // 当 index >= 0 时需满足 index <= len；负数时需满足 index >= -len
        if index > len || index < -len {
          lengths.push(None);
          return;
        }
        let pos = if index >= 0 {
          index as usize
        } else {
          (len + index) as usize
        };
        for (offset, item) in parsed_vals.iter().enumerate() {
          arr.insert(pos + offset, item.clone());
        }
        lengths.push(Some(arr.len()));
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
  pub fn json_arrindex<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    needle_json: &str,
    opt_li: impl IntoIterator<Item = JsonArrIndex>,
  ) -> Result<Vec<Option<isize>>> {
    let mut start = 0isize;
    let mut stop = None;
    for opt in opt_li {
      match opt {
        JsonArrIndex::Start(s) => start = s,
        JsonArrIndex::Stop(e) => stop = Some(e),
        JsonArrIndex::Range(s, e) => {
          start = s;
          stop = Some(e);
        }
      }
    }
    self.json_arrindex_internal(key, path, needle_json, start, stop)
  }

  #[inline]
  pub(crate) fn json_arrindex_internal<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    needle_json: &str,
    start: isize,
    stop: Option<isize>,
  ) -> Result<Vec<Option<isize>>> {
    let kc = self.kc();
    let needle: Value = sonic_rs::from_str(needle_json)
      .map_err(|e| Error::invalid_data(format!("{ERR_INVALID_JSON_NEEDLE}: {e}")))?;

    let (_, root_val) = match read_json_meta_and_val(self, &kc, key)? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let nodes = get_path_values(&root_val, path)?;
    let mut results = Vec::with_capacity(nodes.len());

    for n in nodes {
      if let Some(arr) = n.as_array() {
        let len = arr.len() as isize;
        if len == 0 {
          results.push(Some(-1));
          continue;
        }

        let s = if start < 0 {
          (len + start).max(0)
        } else {
          start.min(len.saturating_sub(1))
        };
        let e = match stop {
          Some(0) | None => len,
          Some(v) if v < 0 => (len + v).max(0).min(len),
          Some(v) => v.min(len),
        };

        if s >= e {
          results.push(Some(-1));
          continue;
        }

        let mut found = -1;
        for i in s..e {
          if (i as usize) < arr.len() && arr[i as usize] == needle {
            found = i;
            break;
          }
        }
        results.push(Some(found));
      } else {
        results.push(None);
      }
    }

    Ok(results)
  }

  #[inline]
  pub fn json_arrlen<K: AsRef<[u8]>>(
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

    let mut lengths = Vec::with_capacity(nodes.len());
    for n in nodes {
      if let Some(arr) = n.as_array() {
        lengths.push(Some(arr.len()));
      } else {
        lengths.push(None);
      }
    }

    Ok(lengths)
  }

  #[inline]
  pub fn json_arrpop<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
    index: Option<isize>,
  ) -> Result<Vec<Option<String>>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let idx = index.unwrap_or(-1);

    let mut popped = Vec::new();
    mutate_path_values(&mut root_val, p, |node| {
      if let Some(arr) = node.as_array_mut() {
        if arr.is_empty() {
          popped.push(None);
        } else {
          let len = arr.len() as isize;
          let target_i = if idx < 0 {
            len - len.min(-idx)
          } else {
            (len - 1).min(idx)
          } as usize;

          if target_i < arr.len() {
            let removed = arr[target_i].clone();
            arr.remove(target_i);
            popped.push(Some(sonic_rs::to_string(&removed).unwrap_or_default()));
          } else {
            popped.push(None);
          }
        }
      } else {
        popped.push(None);
      }
    })?;

    if popped.iter().any(|opt| opt.is_some()) {
      write_json_meta_and_val(self, &kc, key, &meta, &root_val)?;
    }

    Ok(popped)
  }

  #[inline]
  pub fn json_arrtrim<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: &str,
    start: isize,
    stop: isize,
  ) -> Result<Vec<Option<usize>>> {
    let kc = self.kc();
    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let mut lengths = Vec::new();
    mutate_path_values(&mut root_val, path, |node| {
      if let Some(arr) = node.as_array_mut() {
        let len = arr.len() as isize;
        let begin_index = if start < 0 {
          (len + start).max(0)
        } else {
          start
        };
        let end_index = if stop < 0 {
          (len + stop).max(0)
        } else {
          stop.min(len - 1)
        };

        if begin_index >= len || begin_index > end_index {
          arr.clear();
          lengths.push(Some(0));
        } else {
          let b = begin_index as usize;
          let e = ((end_index + 1) as usize).min(arr.len());
          arr.truncate(e);
          if b > 0 {
            arr.drain(0..b);
          }
          lengths.push(Some(arr.len()));
        }
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

    let mut lengths = Vec::with_capacity(nodes.len());
    for n in nodes {
      if let Some(obj) = n.as_object() {
        lengths.push(Some(obj.len()));
      } else {
        lengths.push(None);
      }
    }

    Ok(lengths)
  }

  #[inline]
  pub fn json_debug_memory<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
  ) -> Result<Vec<usize>> {
    let kc = self.kc();
    let (meta, root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    if p == JSON_ROOT_PATH || p == "." {
      return Ok(vec![meta.base.size as usize]);
    }

    let nodes = get_path_values(&root_val, p)?;
    let mut sizes = Vec::with_capacity(nodes.len());
    for n in nodes {
      let s = sonic_rs::to_vec(n).unwrap_or_default();
      sizes.push(s.len());
    }
    Ok(sizes)
  }

  #[inline]
  pub fn json_info<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<(JsonStorageFormat, usize)>> {
    let kc = self.kc();
    if let Some((meta, _)) = read_json_meta_and_val(self, &kc, key)? {
      Ok(Some((meta.format, meta.base.size as usize)))
    } else {
      Ok(None)
    }
  }
}
