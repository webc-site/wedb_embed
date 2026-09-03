use std::ops::Bound;

use crate::{
  api::set::{
    SetScanByMemberResult, SetScanResult, compose_set_key, compose_set_meta_key,
    compose_set_prefix_stack, meta::SetMeta,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  key_composer::matches_glob_bytes,
  meta::current_now_ms,
  wedb::Db,
};

/// Set iteration and cursor-based scanning (SSCAN, SSCAN_BY_MEMBER, SITER).
/// 集合遍历与游标扫描实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub(crate) fn siter_prefix<F>(&self, prefix_bytes: &[u8], mut f: F) -> Result<()>
  where
    F: FnMut(&[u8]) -> bool,
  {
    let prefix_len = prefix_bytes.len();
    for guard in self.data().prefix(prefix_bytes) {
      let entry = guard?;
      let k = entry.key();
      if !k.starts_with(prefix_bytes) {
        break;
      }
      let member = &k[prefix_len..];
      if !f(member) {
        break;
      }
    }
    Ok(())
  }

  #[inline]
  pub fn siter<K: AsRef<[u8]>, F>(&self, key: K, f: F) -> Result<()>
  where
    F: FnMut(&[u8]) -> bool,
  {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(()),
    };
    let _ = meta;

    let prefix = compose_set_prefix_stack(&kc, k_bytes);
    self.siter_prefix(prefix.as_slice(), f)
  }

  #[inline]
  pub fn sscan<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: u64,
    pattern: Option<&[u8]>,
    count: Option<usize>,
  ) -> Result<SetScanResult> {
    let limit = count.unwrap_or(10).max(1);
    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat_bytes = pattern.unwrap_or(b"*");

    let mut skipped = 0u64;
    let mut matched = Vec::with_capacity(limit);
    let mut has_more = false;

    self.siter(key, |member| {
      if is_match_all || matches_glob_bytes(pat_bytes, member) {
        if skipped < cursor {
          skipped += 1;
        } else if matched.len() < limit {
          matched.push(member.to_vec());
        } else {
          has_more = true;
          return false;
        }
      }
      true
    })?;

    let next_cursor = if has_more {
      cursor + matched.len() as u64
    } else {
      0
    };
    Ok((next_cursor, matched))
  }

  #[inline]
  pub fn sscan_by_member<K: AsRef<[u8]>>(
    &self,
    key: K,
    cursor: Option<&[u8]>,
    pattern: Option<&[u8]>,
    count: Option<usize>,
  ) -> Result<SetScanByMemberResult> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok((None, Vec::new())),
    };
    let _ = meta;

    let limit = count.unwrap_or(10).max(1);
    let is_match_all = match pattern {
      Some(p) => p == b"*",
      None => true,
    };
    let pat_bytes = pattern.unwrap_or(b"*");

    let prefix = compose_set_prefix_stack(&kc, k_bytes);
    let prefix_bytes = prefix.as_slice();
    let prefix_len = prefix_bytes.len();

    let data_ks = self.data();
    let mut matched = Vec::with_capacity(limit);
    let mut next_cursor = None;

    let start_bound = cursor
      .map(|c| compose_set_key(&kc, k_bytes, c))
      .map(|k| Bound::Excluded(k.to_vec()))
      .unwrap_or(Bound::Included(prefix_bytes.to_vec()));
    let start_ref = Bound::as_ref(&start_bound).map(|v| v.as_slice());

    for guard in data_ks.range((start_ref, Bound::Unbounded)) {
      let entry = guard?;
      let k = entry.key();
      if !k.starts_with(prefix_bytes) {
        break;
      }
      let member = &k[prefix_len..];
      if is_match_all || matches_glob_bytes(pat_bytes, member) {
        matched.push(member.to_vec());
        if matched.len() >= limit {
          next_cursor = Some(member.to_vec());
          break;
        }
      }
    }

    Ok((next_cursor, matched))
  }
}
