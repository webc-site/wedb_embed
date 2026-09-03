use rapidhash::RapidHashMap;
use sonic_rs::{JsonValueMutTrait, JsonValueTrait, Value};

use crate::{
  api::json::{
    r#const::{
      ERR_CORRUPTED_JSON, ERR_INVALID_JSON, ERR_NEW_OBJECTS_MUST_BE_CREATED_AT_ROOT, JSON_ROOT_PATH,
    },
    key,
    meta::{JsonMeta, JsonStorageFormat},
    opt::{JsonGet, JsonSet},
    path::{delete_path_values, format_json, get_path_values, json_set_path, json_transform_resp},
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
pub(crate) fn read_json_meta_and_val<E: Engine, K: AsRef<[u8]>>(
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
pub(crate) fn write_json_meta_and_val<E: Engine, K: AsRef<[u8]>>(
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
    let is_root_del = match path {
      None => true,
      Some(p) => {
        let trimmed = p.trim();
        trimmed == JSON_ROOT_PATH || trimmed == "."
      }
    };

    if is_root_del {
      let meta_k = key::meta(&kc, key_bytes);
      let now_ms = current_now_ms();
      if get_meta_checked::<JsonMeta, _>(self, key_bytes, &meta_k, now_ms)?.is_none() {
        return Ok(0);
      }
      let data_k = key::prefix_stack(&kc, key_bytes);
      let mut batch = self.batch();
      batch.rm_meta(&meta_k);
      batch.rm_data(&data_k);
      batch.commit()?;
      return Ok(1);
    }

    let (meta, mut root_val) = match read_json_meta_and_val(self, &kc, key_bytes)? {
      Some(pair) => pair,
      None => return Ok(0),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH).trim();
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
    let key_bytes = key.as_ref();
    let meta_k = key::meta(&self.kc(), key_bytes);
    let now_ms = current_now_ms();
    if let Some(meta) = get_meta_checked::<JsonMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Ok(Some((meta.format, meta.base.size as usize)))
    } else {
      Ok(None)
    }
  }

  #[inline]
  pub fn json_resp_one<K: AsRef<[u8]>>(
    &self,
    key: K,
    path: Option<&str>,
  ) -> Result<Option<String>> {
    let res = self.json_resp(key, path)?;
    Ok(res.into_iter().next())
  }

  #[inline]
  pub fn json_resp<K: AsRef<[u8]>>(&self, key: K, path: Option<&str>) -> Result<Vec<String>> {
    let kc = self.kc();
    let (_, root_val) = match read_json_meta_and_val(self, &kc, key.as_ref())? {
      Some(pair) => pair,
      None => return Ok(Vec::new()),
    };

    let p = path.unwrap_or(JSON_ROOT_PATH);
    let matched_nodes = get_path_values(&root_val, p)?;
    let mut results = Vec::with_capacity(matched_nodes.len());
    for node in matched_nodes {
      let mut resp_str = String::with_capacity(64);
      json_transform_resp(node, &mut resp_str);
      results.push(resp_str);
    }

    Ok(results)
  }
}
