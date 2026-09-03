use crate::{
  api::key::{key_type_impl, opt::SortArgs},
  engine::Engine,
  error::{ERR_WRONG_TYPE, Error, Result},
  string::parse_redis_float,
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn sort<K: AsRef<[u8]>>(&self, key: K, args: &SortArgs) -> Result<Vec<Option<Vec<u8>>>> {
    sort_impl(self, key.as_ref(), args)
  }

  #[inline]
  pub fn sort_ro<K: AsRef<[u8]>>(&self, key: K, args: &SortArgs) -> Result<Vec<Option<Vec<u8>>>> {
    if args.store.is_some() {
      return Err(Error::redis(
        "ERR SORT_RO is read-only and does not support the STORE parameter",
      ));
    }
    sort_impl(self, key.as_ref(), args)
  }

  #[inline]
  pub fn sort_store<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(
    &self,
    key: K1,
    store_key: K2,
    mut args: SortArgs,
  ) -> Result<usize> {
    args.store = Some(store_key.as_ref().to_vec());
    let res = sort_impl(self, key.as_ref(), &args)?;
    Ok(res.len())
  }
}

/// Helper for pattern substitution and value lookup for SORT command (aligned with Kvrocks lookupKeyByPattern).
/// SORT 模式字符串替换与键/字段值解析查找（对标 Kvrocks lookupKeyByPattern）
fn lookup_sort_pattern<E: Engine>(
  db: &Db<E>,
  pattern: &[u8],
  member: &[u8],
) -> Result<Option<Vec<u8>>>
where
  Error: From<E::Error>,
{
  if pattern == b"#" {
    return Ok(Some(member.to_vec()));
  }
  let mut expanded = Vec::with_capacity(pattern.len() + member.len());
  if let Some(pos) = pattern.iter().position(|&b| b == b'*') {
    expanded.extend_from_slice(&pattern[..pos]);
    expanded.extend_from_slice(member);
    expanded.extend_from_slice(&pattern[pos + 1..]);
  } else {
    expanded.extend_from_slice(pattern);
  }

  if let Some(arrow_pos) = expanded.windows(2).position(|w| w == b"->") {
    let hash_key = &expanded[..arrow_pos];
    let field = &expanded[arrow_pos + 2..];
    db.hget(hash_key, field)
  } else {
    db.get(&expanded)
  }
}

/// Internal element wrapper for sorting.
struct SortItem {
  member: Vec<u8>,
  num_val: f64,
  str_val: Option<Vec<u8>>,
}

/// Executes SORT / SORT_RO on a list, set, or zset (aligned with Kvrocks Database::Sort).
/// 列表/集合/有序集合通用排序执行（对标 Kvrocks Database::Sort）
pub fn sort_impl<E: Engine>(db: &Db<E>, key: &[u8], args: &SortArgs) -> Result<Vec<Option<Vec<u8>>>>
where
  Error: From<E::Error>,
{
  let ktype = key_type_impl(db, key)?;
  if ktype == "none" {
    return Ok(Vec::new());
  }

  let raw_elements: Vec<Vec<u8>> = match ktype {
    "list" => db.lrange(key, (0, -1))?,
    "set" => db.smembers(key)?,
    "zset" => db
      .zrange(key, b"0", b"-1", [])?
      .into_iter()
      .map(|(m, _)| m)
      .collect(),
    _ => return Err(Error::wrong_type(ERR_WRONG_TYPE)),
  };

  let mut items = Vec::with_capacity(raw_elements.len());
  let by_pat = args.by.as_deref();

  for member in raw_elements {
    let val = if let Some(by) = by_pat {
      lookup_sort_pattern(db, by, &member)?
    } else {
      Some(member.clone())
    };

    let (num_val, str_val) = if args.alpha {
      (0.0, val)
    } else if let Some(ref v) = val {
      if v.is_empty() {
        (0.0, None)
      } else {
        let num = parse_redis_float(v)
          .map_err(|_| Error::redis("One or more scores can't be converted into double"))?;
        (num, None)
      }
    } else {
      (0.0, None)
    };

    items.push(SortItem {
      member,
      num_val,
      str_val,
    });
  }

  if !args.dont_sort {
    if args.alpha {
      items.sort_by(|a, b| {
        let a_str = a.str_val.as_deref().unwrap_or(b"");
        let b_str = b.str_val.as_deref().unwrap_or(b"");
        let cmp = a_str.cmp(b_str);
        if args.desc { cmp.reverse() } else { cmp }
      });
    } else {
      items.sort_by(|a, b| {
        let cmp = a.num_val.total_cmp(&b.num_val);
        if args.desc { cmp.reverse() } else { cmp }
      });
    }
  }

  // LIMIT pagination
  let total_len = items.len();
  let start = args.offset.min(total_len);
  let end = match args.count {
    Some(cnt) => (start + cnt).min(total_len),
    None => total_len,
  };
  let paginated = &items[start..end];

  // Result projection (GET patterns or self)
  let mut result = Vec::new();
  if args.get.is_empty() {
    result.reserve(paginated.len());
    for item in paginated {
      result.push(Some(item.member.clone()));
    }
  } else {
    result.reserve(paginated.len() * args.get.len());
    for item in paginated {
      for pattern in &args.get {
        let val = lookup_sort_pattern(db, pattern, &item.member)?;
        result.push(val);
      }
    }
  }

  // Handle STORE
  if let Some(ref store_key) = args.store {
    db.del_one(store_key)?;
    let push_items: Vec<&[u8]> = result
      .iter()
      .map(|opt| opt.as_deref().unwrap_or(b""))
      .collect();
    if !push_items.is_empty() {
      db.rpush(store_key, &push_items)?;
    }
  }

  Ok(result)
}
