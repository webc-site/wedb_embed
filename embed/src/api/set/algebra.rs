use rapidhash::{HashSetExt, RapidHashSet as HashSet, v3::rapidhash_v3};

use crate::{
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};

/// Set algebra operations (SINTER, SUNION, SDIFF and their store/card variants).
/// 集合代数运算（交集、并集、差集及其存储与基数统计实现）
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
}
