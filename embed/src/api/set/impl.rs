use std::ops::Bound;

use rapidhash::{HashSetExt, RapidHashSet as HashSet, v3::rapidhash_v3};

use crate::{
  api::set::{
    SetItemKeyComposer, SetScanByMemberResult, SetScanResult, compose_set_key,
    compose_set_meta_key, compose_set_prefix_stack, meta::SetMeta,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::{check_key_not_other_type, clear_prefix_in_batch, get_meta_checked},
  key_composer::{KeyTag, matches_glob_bytes},
  meta::current_now_ms,
  wedb::{Db, DbBatch},
};

/// Set structure operations interface (Sets).
/// 集合结构操作接口 (Sets)
#[inline]
pub fn prepare_set_meta_for_write<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  prefix: &[u8],
  meta_k: &[u8],
  now_ms: u64,
  batch: &mut DbBatch<E>,
) -> Result<(SetMeta, bool)>
where
  Error: From<E::Error>,
{
  match get_meta_checked::<SetMeta, _>(db, k_bytes, meta_k, now_ms)? {
    Some(meta) => Ok((meta, false)),
    None => {
      clear_prefix_in_batch(db.data(), prefix, batch)?;
      Ok((SetMeta::new_with_version(0, 0), true))
    }
  }
}

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn sadd_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<usize> {
    self.sadd(key, &[member])
  }

  #[inline]
  pub fn sadd<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, members: &[M]) -> Result<usize> {
    if members.is_empty() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let prefix = compose_set_prefix_stack(&kc, k_bytes);
    let now_ms = current_now_ms();
    let data_ks = self.data();
    let mut batch = self.batch_with_capacity(members.len() + 1);

    let (mut meta, is_new) =
      prepare_set_meta_for_write(self, k_bytes, &prefix, &meta_k, now_ms, &mut batch)?;

    let mut added = 0usize;

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      let item_k = compose_set_key(&kc, k_bytes, m_bytes);
      if is_new || !data_ks.contains_key(item_k.as_slice())? {
        batch.insert_data(item_k.as_slice(), b"");
        added = 1;
      }
    } else {
      let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
      let mut seen = HashSet::with_capacity(members.len());
      for m in members {
        let m_bytes = m.as_ref();
        if !seen.insert(m_bytes) {
          continue;
        }

        let item_k = composer.key_for_member(m_bytes);
        if is_new || !data_ks.contains_key(item_k)? {
          batch.insert_data(item_k, b"");
          added += 1;
        }
      }
    }

    if added > 0 {
      meta.base.size += added as u64;
      batch.insert_meta(&meta_k, &meta.encode());
      batch.commit()?;
    }

    Ok(added)
  }

  #[inline]
  pub fn srem_one<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<usize> {
    self.srem(key, &[member])
  }

  #[inline]
  pub fn srem<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, members: &[M]) -> Result<usize> {
    if members.is_empty() {
      return Ok(0);
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(0),
    };

    let mut removed = 0usize;
    let mut batch = self.batch_with_capacity(members.len() + 1);
    let data_ks = self.data();

    if members.len() == 1 {
      let m_bytes = members[0].as_ref();
      let item_k = compose_set_key(&kc, k_bytes, m_bytes);
      if data_ks.contains_key(item_k.as_slice())? {
        batch.rm_weak_data(item_k.as_slice());
        removed = 1;
      }
    } else {
      let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
      let mut seen = HashSet::with_capacity(members.len());
      for m in members {
        let m_bytes = m.as_ref();
        if !seen.insert(m_bytes) {
          continue;
        }

        let item_k = composer.key_for_member(m_bytes);
        if data_ks.contains_key(item_k)? {
          batch.rm_weak_data(item_k);
          removed += 1;
        }
      }
    }

    if removed > 0 {
      meta.base.size = meta.base.size.saturating_sub(removed as u64);
      if meta.base.size == 0 {
        batch.rm_meta(&meta_k);
      } else {
        batch.insert_meta(&meta_k, &meta.encode());
      }
      batch.commit()?;
    }

    Ok(removed)
  }

  #[inline]
  pub fn smembers<K: AsRef<[u8]>>(&self, key: K) -> Result<Vec<Vec<u8>>> {
    let card = self.scard(&key)? as usize;
    let mut results = Vec::with_capacity(card.min(4096));
    self.siter(key, |m| {
      results.push(m.to_vec());
      true
    })?;
    Ok(results)
  }

  #[inline]
  pub fn sismember<K: AsRef<[u8]>, M: AsRef<[u8]>>(&self, key: K, member: M) -> Result<bool> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(false),
    };

    let _ = meta;
    let item_k = compose_set_key(&kc, k_bytes, member.as_ref());
    Ok(self.data().contains_key(item_k.as_slice())?)
  }

  #[inline]
  pub fn smismember<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<Vec<bool>> {
    if members.is_empty() {
      return Ok(Vec::new());
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(vec![false; members.len()]),
    };
    let _ = meta;

    let mut results = Vec::with_capacity(members.len());
    let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
    let data_ks = self.data();

    for m in members {
      let item_k = composer.key_for_member(m.as_ref());
      results.push(data_ks.contains_key(item_k)?);
    }
    Ok(results)
  }

  #[inline]
  pub fn scard<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) => Ok(m.base.size),
      None => Ok(0),
    }
  }

  #[inline]
  pub fn spop_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let mut res = self.spop(key, 1)?;
    Ok(res.pop())
  }

  #[inline]
  pub fn spop<K: AsRef<[u8]>>(&self, key: K, count: usize) -> Result<Vec<Vec<u8>>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let mut meta = match get_meta_checked::<SetMeta, _>(self, k_bytes, &meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(Vec::new()),
    };

    if count == 1 {
      let card = meta.base.size as usize;
      let target = fastrand::usize(0..card);
      let mut current_idx = 0usize;
      let mut popped_item = None;

      self.siter(k_bytes, |m| {
        if current_idx == target {
          popped_item = Some(m.to_vec());
          return false;
        }
        current_idx += 1;
        true
      })?;

      if let Some(item) = popped_item {
        let mut batch = self.batch_with_capacity(2);
        let item_k = compose_set_key(&kc, k_bytes, &item);
        batch.rm_weak_data(item_k.as_slice());
        meta.base.size = meta.base.size.saturating_sub(1);
        if meta.base.size == 0 {
          batch.rm_meta(&meta_k);
        } else {
          batch.insert_meta(&meta_k, &meta.encode());
        }
        batch.commit()?;
        return Ok(vec![item]);
      } else {
        return Ok(Vec::new());
      }
    }

    let mut members = Vec::with_capacity((meta.base.size as usize).min(4096));
    self.siter(k_bytes, |m| {
      members.push(m.to_vec());
      true
    })?;

    if members.is_empty() {
      return Ok(Vec::new());
    }

    let pop_count = count.min(members.len());
    let mut batch = self.batch_with_capacity(pop_count + 1);

    if pop_count == members.len() {
      let prefix = compose_set_prefix_stack(&kc, k_bytes);
      clear_prefix_in_batch(self.data(), &prefix, &mut batch)?;
      batch.rm_meta(&meta_k);
      batch.commit()?;
      return Ok(members);
    }

    let popped: Vec<Vec<u8>> = if pop_count == 1 {
      let idx = fastrand::usize(0..members.len());
      vec![members.swap_remove(idx)]
    } else {
      fastrand::shuffle(&mut members);
      members.into_iter().take(pop_count).collect()
    };

    let mut composer = SetItemKeyComposer::new(&kc, k_bytes);
    for m in &popped {
      let item_k = composer.key_for_member(m);
      batch.rm_weak_data(item_k);
    }

    meta.base.size = meta.base.size.saturating_sub(pop_count as u64);
    if meta.base.size == 0 {
      batch.rm_meta(&meta_k);
    } else {
      batch.insert_meta(&meta_k, &meta.encode());
    }
    batch.commit()?;

    Ok(popped)
  }

  #[inline]
  pub fn siter<K: AsRef<[u8]>, F>(&self, key: K, mut f: F) -> Result<()>
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
    let prefix_bytes = prefix.as_slice();
    let prefix_len = prefix_bytes.len();

    for guard in self.data().prefix(prefix_bytes) {
      let entry = guard?;
      let (k, _) = (entry.key(), entry.value());
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
  pub fn srandmember_one<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
    let card = self.scard(&key)? as usize;
    if card == 0 {
      return Ok(None);
    }
    let target = fastrand::usize(0..card);
    let mut current_idx = 0usize;
    let mut result = None;

    self.siter(key, |m| {
      if current_idx == target {
        result = Some(m.to_vec());
        return false;
      }
      current_idx += 1;
      true
    })?;

    Ok(result)
  }

  #[inline]
  pub fn srandmember<K: AsRef<[u8]>>(&self, key: K, count: i64) -> Result<Vec<Vec<u8>>> {
    if count == 0 {
      return Ok(Vec::new());
    }
    if count == 1 {
      return match self.srandmember_one(&key)? {
        Some(m) => Ok(vec![m]),
        None => Ok(Vec::new()),
      };
    }
    let mut all = self.smembers(key)?;
    let total = all.len();
    if total == 0 {
      return Ok(Vec::new());
    }

    if count > 0 {
      let sample_cnt = (count as usize).min(total);
      if sample_cnt == total {
        return Ok(all);
      }
      for i in 0..sample_cnt {
        let j = fastrand::usize(i..total);
        all.swap(i, j);
      }
      all.truncate(sample_cnt);
      Ok(all)
    } else {
      let total_sample = count.unsigned_abs() as usize;
      let mut out = Vec::with_capacity(total_sample);
      for _ in 0..total_sample {
        let idx = fastrand::usize(0..total);
        out.push(all[idx].clone());
      }
      Ok(out)
    }
  }

  #[inline]
  pub fn smove<SK: AsRef<[u8]>, DK: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    src: SK,
    dst: DK,
    member: M,
  ) -> Result<bool> {
    let src_bytes = src.as_ref();
    let dst_bytes = dst.as_ref();
    let m_bytes = member.as_ref();
    let kc = self.kc();

    if src_bytes == dst_bytes {
      return self.sismember(src, member);
    }

    let src_meta_k = compose_set_meta_key(&kc, src_bytes);
    let now_ms = current_now_ms();

    let mut src_meta = match get_meta_checked::<SetMeta, _>(self, src_bytes, &src_meta_k, now_ms)? {
      Some(m) if m.base.size > 0 => m,
      _ => return Ok(false),
    };

    let src_item_k = compose_set_key(&kc, src_bytes, m_bytes);
    let data_ks = self.data();

    if !data_ks.contains_key(src_item_k.as_slice())? {
      return Ok(false);
    }

    let dst_meta_k = compose_set_meta_key(&kc, dst_bytes);
    let dst_prefix = compose_set_prefix_stack(&kc, dst_bytes);
    let mut batch = self.batch();

    let (mut dst_meta, dst_is_new) = prepare_set_meta_for_write(
      self,
      dst_bytes,
      &dst_prefix,
      &dst_meta_k,
      now_ms,
      &mut batch,
    )?;

    batch.rm_weak_data(src_item_k.as_slice());
    src_meta.base.size = src_meta.base.size.saturating_sub(1);
    if src_meta.base.size == 0 {
      batch.rm_meta(&src_meta_k);
    } else {
      batch.insert_meta(&src_meta_k, &src_meta.encode());
    }

    let dst_item_k = compose_set_key(&kc, dst_bytes, m_bytes);
    if dst_is_new || !data_ks.contains_key(dst_item_k.as_slice())? {
      batch.insert_data(dst_item_k.as_slice(), b"");
      dst_meta.base.size += 1;
      batch.insert_meta(&dst_meta_k, &dst_meta.encode());
    }

    batch.commit()?;
    Ok(true)
  }

  #[inline]
  pub fn overwrite_set<K: AsRef<[u8]>, M: AsRef<[u8]>>(
    &self,
    key: K,
    members: &[M],
  ) -> Result<usize> {
    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_set_meta_key(&kc, key_bytes);
    let prefix = compose_set_prefix_stack(&kc, key_bytes);
    let now_ms = current_now_ms();

    check_key_not_other_type(self, key_bytes, KeyTag::SetMeta.as_slice(), now_ms)?;

    let data_ks = self.data();

    let mut batch = self.batch();
    clear_prefix_in_batch(data_ks, &prefix, &mut batch)?;
    batch.rm_meta(&meta_k);

    let mut seen = HashSet::with_capacity(members.len());
    let mut count = 0u64;
    let mut composer = SetItemKeyComposer::new(&kc, key_bytes);

    for m in members {
      let m_bytes = m.as_ref();
      if !seen.insert(m_bytes) {
        continue;
      }
      let item_k = composer.key_for_member(m_bytes);
      batch.insert_data(item_k, b"");
      count += 1;
    }

    if count > 0 {
      let mut meta = SetMeta::new_with_version(0, 0);
      meta.base.size = count;
      batch.insert_meta(&meta_k, &meta.encode());
    }

    batch.commit()?;
    Ok(count as usize)
  }

  #[inline]
  fn compute_sinter_set<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Option<HashSet<Vec<u8>>>> {
    if keys.is_empty() {
      return Ok(None);
    }
    if keys.len() == 1 {
      let mut set = HashSet::new();
      self.siter(&keys[0], |m| {
        set.insert(m.to_vec());
        true
      })?;
      return Ok(Some(set));
    }

    let mut key_cards: Vec<(&K, u64)> = Vec::with_capacity(keys.len());
    for k in keys {
      let card = self.scard(k)?;
      if card == 0 {
        return Ok(None);
      }
      key_cards.push((k, card));
    }

    key_cards.sort_unstable_by_key(|&(_, card)| card);

    let smallest_key = key_cards[0].0;
    let mut current: HashSet<Vec<u8>> = HashSet::with_capacity(key_cards[0].1 as usize);
    self.siter(smallest_key, |m| {
      current.insert(m.to_vec());
      true
    })?;

    for &(k, card) in &key_cards[1..] {
      if current.is_empty() {
        return Ok(None);
      }
      if (current.len() as u64).saturating_mul(4) < card {
        current.retain(|m| self.sismember(k, m).unwrap_or(false));
      } else {
        let mut next_set = HashSet::with_capacity(current.len());
        self.siter(k, |m| {
          if let Some(owned_vec) = current.take(m) {
            next_set.insert(owned_vec);
            if current.is_empty() {
              return false;
            }
          }
          true
        })?;
        current = next_set;
      }
    }

    Ok(Some(current))
  }

  #[inline]
  pub fn sinter<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Vec<Vec<u8>>> {
    match self.compute_sinter_set(keys)? {
      Some(set) => Ok(set.into_iter().collect()),
      None => Ok(Vec::new()),
    }
  }

  #[inline]
  pub fn sinterstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(&self, dst: D, keys: &[K]) -> Result<usize> {
    let inter_res = self.sinter(keys)?;
    self.overwrite_set(dst, &inter_res)
  }

  #[inline]
  pub fn sintercard<K: AsRef<[u8]>>(&self, keys: &[K], limit: usize) -> Result<usize> {
    if keys.is_empty() {
      return Ok(0);
    }
    if keys.len() == 1 {
      let card = self.scard(&keys[0])? as usize;
      return Ok(if limit == 0 { card } else { card.min(limit) });
    }
    match self.compute_sinter_set(keys)? {
      Some(set) => Ok(if limit == 0 {
        set.len()
      } else {
        set.len().min(limit)
      }),
      None => Ok(0),
    }
  }

  #[inline]
  pub fn sunion<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Vec<Vec<u8>>> {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    if keys.len() == 1 {
      return self.smembers(&keys[0]);
    }

    let first_card = self.scard(&keys[0])? as usize;
    let mut union_set = HashSet::with_capacity(first_card);

    for k in keys {
      self.siter(k, |m| {
        if !union_set.contains(m) {
          union_set.insert(m.to_vec());
        }
        true
      })?;
    }

    Ok(union_set.into_iter().collect())
  }

  #[inline]
  pub fn sunionstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(&self, dst: D, keys: &[K]) -> Result<usize> {
    let union_res = self.sunion(keys)?;
    self.overwrite_set(dst, &union_res)
  }

  #[inline]
  pub fn sunioncard<K: AsRef<[u8]>>(&self, keys: &[K], limit: usize) -> Result<usize> {
    if keys.is_empty() {
      return Ok(0);
    }
    if keys.len() == 1 {
      let card = self.scard(&keys[0])? as usize;
      return Ok(if limit == 0 { card } else { card.min(limit) });
    }

    let mut union_set: HashSet<u64> = HashSet::default();
    if limit > 0 {
      for k in keys {
        self.siter(k, |m| {
          union_set.insert(rapidhash_v3(m));
          union_set.len() < limit
        })?;
        if union_set.len() >= limit {
          return Ok(limit);
        }
      }
      Ok(union_set.len().min(limit))
    } else {
      for k in keys {
        self.siter(k, |m| {
          union_set.insert(rapidhash_v3(m));
          true
        })?;
      }
      Ok(union_set.len())
    }
  }

  #[inline]
  fn compute_sdiff_set<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Option<HashSet<Vec<u8>>>> {
    if keys.is_empty() {
      return Ok(None);
    }
    let first_card = self.scard(&keys[0])? as usize;
    if first_card == 0 {
      return Ok(None);
    }

    let mut current: HashSet<Vec<u8>> = HashSet::with_capacity(first_card);
    self.siter(&keys[0], |m| {
      current.insert(m.to_vec());
      true
    })?;

    for k in &keys[1..] {
      if current.is_empty() {
        return Ok(None);
      }
      let card = self.scard(k)?;
      if card == 0 {
        continue;
      }
      if (current.len() as u64).saturating_mul(4) < card {
        current.retain(|m| !self.sismember(k, m).unwrap_or(false));
      } else {
        self.siter(k, |m| {
          current.remove(m);
          !current.is_empty()
        })?;
      }
    }

    Ok(Some(current))
  }

  #[inline]
  pub fn sdiff<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Vec<Vec<u8>>> {
    match self.compute_sdiff_set(keys)? {
      Some(set) => Ok(set.into_iter().collect()),
      None => Ok(Vec::new()),
    }
  }

  #[inline]
  pub fn sdiffstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(&self, dst: D, keys: &[K]) -> Result<usize> {
    let diff = self.sdiff(keys)?;
    self.overwrite_set(dst, &diff)
  }

  #[inline]
  pub fn sdiffcard<K: AsRef<[u8]>>(&self, keys: &[K], limit: usize) -> Result<usize> {
    if keys.is_empty() {
      return Ok(0);
    }
    if keys.len() == 1 {
      let card = self.scard(&keys[0])? as usize;
      return Ok(if limit == 0 { card } else { card.min(limit) });
    }
    match self.compute_sdiff_set(keys)? {
      Some(set) => Ok(if limit == 0 {
        set.len()
      } else {
        set.len().min(limit)
      }),
      None => Ok(0),
    }
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

    if let Some(cursor_bytes) = cursor {
      let start_k = compose_set_key(&kc, k_bytes, cursor_bytes);
      for guard in data_ks.range((Bound::Excluded(start_k.as_slice()), Bound::Unbounded)) {
        let entry = guard?;
        let (k, _) = (entry.key(), entry.value());
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
    } else {
      for guard in data_ks.prefix(prefix_bytes) {
        let entry = guard?;
        let (k, _) = (entry.key(), entry.value());
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
    }

    Ok((next_cursor, matched))
  }
}
