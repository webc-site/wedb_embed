use std::ops::Bound;

use crate::{
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key_composer::{KeyTag, matches_glob_bytes},
  meta::{KeyMeta, RedisType, current_now_ms},
  string::{
    compose_string_prefix_stack as string_key_prefix_stack, decode_string_value, is_string_expired,
  },
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn keys<P: AsRef<[u8]>>(&self, pattern: P) -> Result<Vec<Vec<u8>>> {
    keys_impl(self, pattern.as_ref())
  }

  #[inline]
  pub fn scan(
    &self,
    cursor: &[u8],
    count: Option<usize>,
    pattern: Option<&[u8]>,
    rtype: Option<RedisType>,
  ) -> Result<(Vec<u8>, Vec<Vec<u8>>)> {
    scan_impl(self, cursor, count, pattern, rtype)
  }

  #[inline]
  pub fn randomkey(&self) -> Result<Option<Vec<u8>>> {
    randomkey_impl(self)
  }
}

/// Queries all user keys in current namespace matching a wildcard pattern.
/// 按通配符模式匹配查询当前命名空间下的所有用户 Key
pub fn keys_impl<E: Engine>(db: &Db<E>, pattern: impl AsRef<[u8]>) -> Result<Vec<Vec<u8>>>
where
  Error: From<E::Error>,
{
  let mut result = Vec::new();
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();
  let pat_bytes = pattern.as_ref();

  // 1. 扫描 data_ks 中的 String 键（仅匹配 String 前缀，零子键开销）
  let str_prefix = string_key_prefix_stack(&kc);
  for item in data_ks.prefix(&str_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(str_prefix.as_slice()) {
      break;
    }
    let (expire_at, _) = decode_string_value(entry.value());
    if is_string_expired(expire_at, now_ms) {
      continue;
    }
    let user_k = &k[str_prefix.len()..];
    if matches_glob_bytes(pat_bytes, user_k) {
      result.push(user_k.to_vec());
    }
  }

  // 2. 扫描 meta_ks 中的复合数据结构元数据键（单次遍历无子键）
  let meta_prefix = kc.namespace_prefix_stack();
  let scope_prefix_len = kc.scope_prefix_len();
  for item in meta_ks.prefix(&meta_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(meta_prefix.as_slice()) {
      break;
    }
    let remain = &k[scope_prefix_len..];
    if remain.is_empty() {
      continue;
    }
    let Some(tag) = KeyTag::from_u8(remain[0]) else {
      continue;
    };
    if !tag.is_meta() {
      continue;
    }
    if let Some(meta) = KeyMeta::decode(entry.value()) {
      if meta.is_expired(now_ms) {
        continue;
      }
      let user_k = &remain[1..];
      if matches_glob_bytes(pat_bytes, user_k) {
        result.push(user_k.to_vec());
      }
    }
  }

  result.sort();
  Ok(result)
}

/// Scans database keys matching pattern and type using cursor-based pagination (SCAN).
/// 数据库键游标分页遍历（对标 Redis SCAN / Kvrocks Database::Scan）
pub fn scan_impl<E: Engine>(
  db: &Db<E>,
  cursor: &[u8],
  count: Option<usize>,
  pattern: Option<&[u8]>,
  rtype: Option<RedisType>,
) -> Result<(Vec<u8>, Vec<Vec<u8>>)>
where
  Error: From<E::Error>,
{
  let limit = count.unwrap_or(10).max(1);
  let mut keys = Vec::with_capacity(limit);
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  let is_init = cursor.is_empty() || cursor == b"0";
  let in_meta_phase = !is_init && cursor.starts_with(b"m:");

  // Phase 1: Scan String keys from data_ks if rtype is compatible
  let should_scan_string = rtype.is_none() || rtype == Some(RedisType::String);
  if should_scan_string && !in_meta_phase {
    let str_prefix = string_key_prefix_stack(&kc);
    let seek_key = if !is_init && cursor.starts_with(b"s:") {
      let mut k = str_prefix.to_vec();
      k.extend_from_slice(&cursor[2..]);
      Some(k)
    } else {
      None
    };

    let start_bound = match seek_key.as_ref() {
      Some(sk) => Bound::Included(sk.as_slice()),
      None => Bound::Included(str_prefix.as_slice()),
    };
    let iter = data_ks.range((start_bound, Bound::Unbounded));

    let seek_user_k = if !is_init && cursor.starts_with(b"s:") {
      Some(&cursor[2..])
    } else {
      None
    };

    for item in iter {
      let entry = item?;
      let k = entry.key();
      if !k.starts_with(str_prefix.as_slice()) {
        break;
      }
      let user_k = &k[str_prefix.len()..];
      if let Some(sk) = seek_user_k
        && user_k == sk
      {
        continue;
      }

      let (expire_at, _) = decode_string_value(entry.value());
      if is_string_expired(expire_at, now_ms) {
        continue;
      }

      if pattern
        .map(|p| matches_glob_bytes(p, user_k))
        .unwrap_or(true)
      {
        keys.push(user_k.to_vec());
        if keys.len() >= limit {
          let mut next_cursor = Vec::with_capacity(2 + user_k.len());
          next_cursor.extend_from_slice(b"s:");
          next_cursor.extend_from_slice(user_k);
          return Ok((next_cursor, keys));
        }
      }
    }

    if rtype == Some(RedisType::String) {
      return Ok((b"0".to_vec(), keys));
    }
  }

  // Phase 2: Scan Composite keys from meta_ks
  let should_scan_meta = rtype.map(|t| t != RedisType::String).unwrap_or(true);
  if should_scan_meta {
    let meta_prefix = kc.namespace_prefix_stack();
    let scope_prefix_len = kc.scope_prefix_len();

    let seek_key = if in_meta_phase {
      let mut k = meta_prefix.to_vec();
      k.extend_from_slice(&cursor[2..]);
      Some(k)
    } else {
      None
    };

    let start_bound = match seek_key.as_ref() {
      Some(sk) => Bound::Included(sk.as_slice()),
      None => Bound::Included(meta_prefix.as_slice()),
    };
    let iter = meta_ks.range((start_bound, Bound::Unbounded));

    let seek_meta_k = if in_meta_phase {
      Some(&cursor[2..])
    } else {
      None
    };

    for item in iter {
      let entry = item?;
      let k = entry.key();
      if !k.starts_with(meta_prefix.as_slice()) {
        break;
      }
      let remain = &k[scope_prefix_len..];
      if remain.is_empty() {
        continue;
      }

      if let Some(smk) = seek_meta_k
        && remain == smk
      {
        continue;
      }

      let Some(tag) = KeyTag::from_u8(remain[0]) else {
        continue;
      };
      if !tag.is_meta() {
        continue;
      }

      let Some(meta) = KeyMeta::decode(entry.value()) else {
        continue;
      };
      if meta.is_expired(now_ms) {
        continue;
      }

      if let Some(expected_type) = rtype
        && meta.rtype != expected_type
      {
        continue;
      }

      let user_k = &remain[1..];
      if pattern
        .map(|p| matches_glob_bytes(p, user_k))
        .unwrap_or(true)
      {
        keys.push(user_k.to_vec());
        if keys.len() >= limit {
          let mut next_cursor = Vec::with_capacity(2 + remain.len());
          next_cursor.extend_from_slice(b"m:");
          next_cursor.extend_from_slice(remain);
          return Ok((next_cursor, keys));
        }
      }
    }
  }

  Ok((b"0".to_vec(), keys))
}

/// Returns a random active key from current database (RANDOMKEY, aligned with Kvrocks Database::RandomKey).
/// 返回当前数据库中的一个随机活跃键（对标 Kvrocks RANDOM_KEY_SCAN_LIMIT = 60 算法）
pub fn randomkey_impl<E: Engine>(db: &Db<E>) -> Result<Option<Vec<u8>>>
where
  Error: From<E::Error>,
{
  let (_, mut keys) = scan_impl(db, b"0", Some(60), None, None)?;
  if keys.is_empty() {
    return Ok(None);
  }
  let idx = fastrand::usize(..keys.len());
  Ok(Some(keys.swap_remove(idx)))
}
