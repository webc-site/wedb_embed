use rapidhash::{HashMapExt, HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  api::zset::{ZSetMemberScore, key as zset_key, opt::Aggregate},
  engine::{Engine, Partition},
  error::{Error, Result},
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  /// ZDIFF numkeys key [key ...] [WITHSCORES] (computes set difference).
  /// 计算多个有序集合的差集
  #[inline]
  pub fn zdiff<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<Vec<ZSetMemberScore>> {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let first_card = self.zcard(&keys[0])?;
    if first_card == 0 {
      return Ok(Vec::new());
    }
    if keys.len() == 1 {
      return self.zget_all(&keys[0]);
    }

    let mut base_items = self.zget_all(&keys[0])?;
    if base_items.is_empty() {
      return Ok(Vec::new());
    }

    for k in &keys[1..] {
      if base_items.is_empty() {
        return Ok(Vec::new());
      }
      let card = self.zcard(k)?;
      if card == 0 {
        continue;
      }
      if (base_items.len() as u64).saturating_mul(4) < card {
        base_items.retain(|(member, _)| self.zscore(k, member).unwrap_or(None).is_none());
      } else {
        let mut exclude: HashSet<Vec<u8>> = HashSet::with_capacity(card as usize);
        self.ziter(k, |m, _| {
          exclude.insert(m.to_vec());
          true
        })?;
        base_items.retain(|(member, _)| !exclude.contains(member));
      }
    }

    Ok(base_items)
  }

  /// ZDIFFSTORE destination numkeys key [key ...] (stores set difference).
  /// 计算多个有序集合的差集并将结果存储到目标键
  #[inline]
  pub fn zdiffstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(&self, dst: D, keys: &[K]) -> Result<usize> {
    let diff = self.zdiff(keys)?;
    self.overwrite_zset(dst, &diff)
  }

  /// ZUNION numkeys key [key ...] (computes union with weights and aggregation).
  /// 计算多个有序集合的并集并支持权重与聚合函数
  #[inline]
  pub fn zunion<K: AsRef<[u8]>>(
    &self,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<Vec<ZSetMemberScore>> {
    if keys_weights.is_empty() {
      return Ok(Vec::new());
    }
    if keys_weights.len() == 1 {
      let (k, weight) = &keys_weights[0];
      let mut items = self.zget_all(k)?;
      if *weight != 1.0 {
        for (_, score) in &mut items {
          *score *= weight;
          if score.is_nan() {
            *score = 0.0;
          }
        }
        if *weight <= 0.0 {
          items.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        }
      }
      return Ok(items);
    }

    let mut map: HashMap<Vec<u8>, f64> = HashMap::default();

    for (k, weight) in keys_weights {
      self.ziter(k, |member, score| {
        let mut weighted_score = score * weight;
        if weighted_score.is_nan() {
          weighted_score = 0.0;
        }
        match map.get_mut(member) {
          Some(cur_score) => {
            *cur_score = aggregate.apply(*cur_score, weighted_score);
          }
          None => {
            map.insert(member.to_vec(), weighted_score);
          }
        }
        true
      })?;
    }

    let mut results: Vec<ZSetMemberScore> = map.into_iter().collect();
    results.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(results)
  }

  /// ZUNIONSTORE destination numkeys key [key ...] (stores union result).
  /// 计算多个有序集合的并集并将结果存储到目标键
  #[inline]
  pub fn zunionstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(
    &self,
    dst: D,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<usize> {
    let union_res = self.zunion(keys_weights, aggregate)?;
    self.overwrite_zset(dst, &union_res)
  }

  /// ZINTER numkeys key [key ...] (computes intersection with weights and aggregation).
  /// 计算多个有序集合的交集并支持权重与聚合函数
  #[inline]
  pub fn zinter<K: AsRef<[u8]>>(
    &self,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<Vec<ZSetMemberScore>> {
    if keys_weights.is_empty() {
      return Ok(Vec::new());
    }

    let mut min_idx = 0;
    let mut min_card = u64::MAX;
    for (i, (k, _)) in keys_weights.iter().enumerate() {
      let card = self.zcard(k)?;
      if card == 0 {
        return Ok(Vec::new());
      }
      if card < min_card {
        min_card = card;
        min_idx = i;
      }
    }

    let (base_k, base_w) = &keys_weights[min_idx];
    let mut current_map: HashMap<Vec<u8>, f64> = HashMap::with_capacity(min_card as usize);

    self.ziter_members(base_k, |m, s| {
      let mut score = s * base_w;
      if score.is_nan() {
        score = 0.0;
      }
      current_map.insert(m.to_vec(), score);
      true
    })?;

    if current_map.is_empty() {
      return Ok(Vec::new());
    }

    for (i, (k, weight)) in keys_weights.iter().enumerate() {
      if i == min_idx {
        continue;
      }
      current_map.retain(|member, cur_score| match self.zscore(k, member) {
        Ok(Some(score)) => {
          let mut weighted = score * weight;
          if weighted.is_nan() {
            weighted = 0.0;
          }
          *cur_score = aggregate.apply(*cur_score, weighted);
          true
        }
        _ => false,
      });
      if current_map.is_empty() {
        return Ok(Vec::new());
      }
    }

    let mut results: Vec<ZSetMemberScore> = current_map.into_iter().collect();
    results.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(results)
  }

  /// ZINTERSTORE destination numkeys key [key ...] (stores intersection result).
  /// 计算多个有序集合的交集并将结果存储到目标键
  #[inline]
  pub fn zinterstore<D: AsRef<[u8]>, K: AsRef<[u8]>>(
    &self,
    dst: D,
    keys_weights: &[(K, f64)],
    aggregate: Aggregate,
  ) -> Result<usize> {
    let inter_res = self.zinter(keys_weights, aggregate)?;
    self.overwrite_zset(dst, &inter_res)
  }

  /// ZINTERCARD numkeys key [key ...] [LIMIT limit] (computes intersection cardinality with early termination).
  /// ZINTERCARD numkeys key [key ...] [LIMIT limit]（基于基数升序优先扫描与 O(1) 存在性探针，提前中断）
  #[inline]
  pub fn zintercard<K: AsRef<[u8]>>(&self, keys: &[K], limit: usize) -> Result<usize> {
    if keys.is_empty() {
      return Ok(0);
    }
    if keys.len() == 1 {
      let card = self.zcard(&keys[0])? as usize;
      return Ok(if limit == 0 { card } else { card.min(limit) });
    }

    let mut key_cards: Vec<(&K, u64)> = Vec::with_capacity(keys.len());
    for k in keys {
      let card = self.zcard(k)?;
      if card == 0 {
        return Ok(0);
      }
      key_cards.push((k, card));
    }

    key_cards.sort_by_key(|(_, card)| *card);

    let (smallest_key, _) = key_cards[0];
    let kc = self.kc();

    let other_prefixes: Vec<_> = key_cards[1..]
      .iter()
      .map(|(k, _)| zset_key::prefix_stack(&kc, k.as_ref()))
      .collect();

    let data_ks = self.data();
    let mut cardinality = 0;
    let mut probe_buf = Vec::with_capacity(64);

    self.ziter(smallest_key, |member, _| {
      let in_all = other_prefixes.iter().all(|prefix| {
        probe_buf.clear();
        probe_buf.extend_from_slice(prefix.as_slice());
        probe_buf.extend_from_slice(member);
        data_ks.contains_key(&probe_buf).unwrap_or(false)
      });

      if in_all {
        cardinality += 1;
        if limit > 0 && cardinality >= limit {
          return false;
        }
      }
      true
    })?;

    Ok(cardinality)
  }
}
