use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, Value};

use super::{
  r#const::*,
  r#impl::{read_json_meta_and_val, write_json_meta_and_val},
  opt::JsonArrIndex,
  path::{get_path_values, mutate_path_values},
};
use crate::{
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};

/// JSON array operations interface (JSON.ARRAPPEND, JSON.ARRINSERT, etc.).
/// JSON 数组操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
