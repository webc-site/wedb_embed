use std::ops::Bound;

use crate::{
  api::hash::{
    HashFieldPair, HashRandField, HashScanByFieldResult, HashScanResult,
    meta::{HashItemKeyComposer, HashMeta, compose_hash_meta_key, compose_hash_prefix_stack},
    opt::RangeLex,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{get_meta_checked, prefix_upper_bound},
  key_composer::matches_glob_bytes,
  meta::current_now_ms,
  wedb::Db,
};

#[inline]
fn hash_lex_range_bounds(prefix: &[u8], spec: &RangeLex) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
  let start = if spec.min_infinite {
    Bound::Included(prefix.to_vec())
  } else if spec.minex {
    let mut k = Vec::with_capacity(prefix.len() + spec.min.len() + 1);
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.min);
    k.push(0x00);
    Bound::Included(k)
  } else {
    let mut k = Vec::with_capacity(prefix.len() + spec.min.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.min);
    Bound::Included(k)
  };

  let end = if spec.max_infinite {
    prefix_upper_bound(prefix)
  } else if spec.maxex {
    let mut k = Vec::with_capacity(prefix.len() + spec.max.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.max);
    Bound::Excluded(k)
  } else {
    let mut k = Vec::with_capacity(prefix.len() + spec.max.len());
    k.extend_from_slice(prefix);
    k.extend_from_slice(&spec.max);
    prefix_upper_bound(&k)
  };

  (start, end)
}

/// Hash scanning, iteration, and lexicographical range queries.
/// 哈希结构遍历、随机抽样与字段字典序范围检索 (HSCAN, HRANDFIELD, HRANGEBYLEX)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn hiter<K: AsRef<[u8]>, F>(&self, key: K, f: F) -> Result<()>
  where
    F: FnMut(&[u8], &[u8]) -> bool,
  {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(()),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(());
    }

    self.hiter_with_meta(&kc, key_bytes, &meta, now_ms, f)
  }

  #[inline]
  pub fn hrandfield<K: AsRef<[u8]>>(
    &self,
    key: K,
    count: i64,
    with_values: bool,
  ) -> Result<Vec<HashRandField>> {
    if count == 0 {
      return Ok(Vec::new());
    }

    if with_values {
      let mut all = self.hgetall(key)?;
      let total = all.len();
      if total == 0 {
        return Ok(Vec::new());
      }

      if count > 0 {
        let sample_cnt = (count as usize).min(total);
        for i in 0..sample_cnt {
          let j = fastrand::usize(i..total);
          all.swap(i, j);
        }
        all.truncate(sample_cnt);
        let out = all.into_iter().map(|(f, v)| (f, Some(v))).collect();
        Ok(out)
      } else {
        let total_sample = count.unsigned_abs() as usize;
        let mut out = Vec::with_capacity(total_sample);
        for _ in 0..total_sample {
          let idx = fastrand::usize(0..total);
          let (f, v) = &all[idx];
          out.push((f.clone(), Some(v.clone())));
        }
        Ok(out)
      }
    } else {
      let mut all_keys = self.hkeys(key)?;
      let total = all_keys.len();
      if total == 0 {
        return Ok(Vec::new());
      }

      if count > 0 {
        let sample_cnt = (count as usize).min(total);
        for i in 0..sample_cnt {
          let j = fastrand::usize(i..total);
          all_keys.swap(i, j);
        }
        all_keys.truncate(sample_cnt);
        let out = all_keys.into_iter().map(|f| (f, None)).collect();
        Ok(out)
      } else {
        let total_sample = count.unsigned_abs() as usize;
        let mut out = Vec::with_capacity(total_sample);
        for _ in 0..total_sample {
          let idx = fastrand::usize(0..total);
          out.push((all_keys[idx].clone(), None));
        }
        Ok(out)
      }
    }
  }

  #[inline]
  pub fn hrandfield_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let res = self.hrandfield(key, 1, false)?;
    Ok(res.into_iter().next().map(|(f, _)| f))
  }

  #[inline]
  pub fn hscan<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: usize,
    limit: usize,
    pattern: Option<&[u8]>,
  ) -> Result<HashScanResult> {
    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat = pattern.unwrap_or(b"*");

    let mut skipped = 0;
    let mut matched = Vec::with_capacity(limit);
    let mut has_more = false;

    self.hiter(key, |field, value| {
      if is_match_all || matches_glob_bytes(pat, field) {
        if skipped < cursor {
          skipped += 1;
        } else if matched.len() < limit {
          matched.push((field.to_vec(), value.to_vec()));
        } else {
          has_more = true;
          return false;
        }
      }
      true
    })?;

    let next_cursor = if has_more { cursor + matched.len() } else { 0 };
    Ok((next_cursor, matched))
  }

  #[inline]
  pub fn hscan_by_field<K: AsRef<[u8]>, C: AsRef<[u8]>>(
    &self,
    key: K,
    cursor_field: C,
    limit: usize,
    pattern: Option<&[u8]>,
  ) -> Result<HashScanByFieldResult> {
    if limit == 0 {
      return Ok((None, Vec::new()));
    }

    let key_bytes = key.as_ref();
    let cursor_bytes = cursor_field.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok((None, Vec::new())),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok((None, Vec::new()));
    }

    let prefix_buf = compose_hash_prefix_stack(&kc, key_bytes);
    let prefix = prefix_buf.as_slice();
    let prefix_len = prefix.len();
    let end_bound = prefix_upper_bound(prefix);
    let end_ref = match &end_bound {
      Bound::Excluded(b) => Bound::Excluded(b.as_slice()),
      _ => Bound::Unbounded,
    };

    let mut composer = HashItemKeyComposer::new(&kc, key_bytes);
    let start_ref = if cursor_bytes.is_empty() {
      Bound::Included(prefix)
    } else {
      Bound::Excluded(composer.key_for_field(cursor_bytes))
    };

    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat = pattern.unwrap_or(b"*");

    let mut matched = Vec::with_capacity(limit);

    for g in self.data().range((start_ref, end_ref)) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(prefix) {
        break;
      }
      let field_bytes = &k[prefix_len..];

      if let Some((_, payload)) = meta.decode_live_subkey_value(entry.value(), now_ms)
        && (is_match_all || matches_glob_bytes(pat, field_bytes))
      {
        matched.push((field_bytes.to_vec(), payload.to_vec()));
        if matched.len() >= limit {
          break;
        }
      }
    }

    let next_cursor = if matched.len() == limit {
      matched.last().map(|(f, _)| f.clone())
    } else {
      None
    };

    Ok((next_cursor, matched))
  }

  #[inline]
  pub fn hrangebylex<K: AsRef<[u8]>>(&self, key: K, spec: RangeLex) -> Result<Vec<HashFieldPair>> {
    self.hrange_by_lex(key, spec)
  }

  #[inline]
  pub fn hrange_by_lex<K: AsRef<[u8]>>(
    &self,
    key: K,
    spec: RangeLex,
  ) -> Result<Vec<HashFieldPair>> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hash_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<HashMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if meta.upper != 0 && now_ms > meta.upper && meta.persist == 0 {
      return Ok(Vec::new());
    }

    let prefix = compose_hash_prefix_stack(&kc, key_bytes);
    let prefix_len = prefix.len();
    let (start_bound, end_bound) = hash_lex_range_bounds(prefix.as_slice(), &spec);

    let start_ref = match &start_bound {
      Bound::Included(b) => Bound::Included(b.as_slice()),
      Bound::Excluded(b) => Bound::Excluded(b.as_slice()),
      Bound::Unbounded => Bound::Unbounded,
    };
    let end_ref = match &end_bound {
      Bound::Included(b) => Bound::Included(b.as_slice()),
      Bound::Excluded(b) => Bound::Excluded(b.as_slice()),
      Bound::Unbounded => Bound::Unbounded,
    };

    let data_ks = self.data();
    let limit = spec.count.unwrap_or(usize::MAX);
    if limit == 0 {
      return Ok(Vec::new());
    }
    let mut matching = Vec::new();
    let mut skipped = 0;

    let mut process_entry = |k: &[u8], v: &[u8]| -> bool {
      if !k.starts_with(prefix.as_slice()) {
        return false;
      }
      let field_bytes = &k[prefix_len..];
      if let Some((_, payload)) = meta.decode_live_subkey_value(v, now_ms) {
        if skipped < spec.offset {
          skipped += 1;
          return true;
        }
        matching.push((field_bytes.to_vec(), payload.to_vec()));
        if matching.len() >= limit {
          return false;
        }
      }
      true
    };

    if !spec.reversed {
      for g in data_ks.range((start_ref, end_ref)) {
        let entry = g?;
        if !process_entry(entry.key(), entry.value()) {
          break;
        }
      }
    } else {
      for g in data_ks.range((start_ref, end_ref)).rev() {
        let entry = g?;
        if !process_entry(entry.key(), entry.value()) {
          break;
        }
      }
    }

    Ok(matching)
  }
}
