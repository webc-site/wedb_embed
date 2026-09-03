use std::ops::Bound;

use crate::{
  api::list::{
    ListMeta,
    r#const::ERR_RANK_ZERO,
    key::{
      ItemKeyComposer as ListItemKeyComposer, item as compose_list_item,
      meta as compose_list_meta_key,
    },
    opt::LPos,
  },
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

/// List element positioning and search operations (LPOS).
/// 列表元素位置定位与检索实现（对标 Redis 6.0.6+ / Kvrocks LPOS）
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn lpos_one<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    elem: V,
    opt_li: impl IntoIterator<Item = LPos>,
  ) -> Result<Option<i64>> {
    let res = self.lpos(key, elem, opt_li)?;
    Ok(res.into_iter().next())
  }

  #[inline]
  pub fn lpos<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    elem: V,
    opt_li: impl IntoIterator<Item = LPos>,
  ) -> Result<Vec<i64>> {
    let mut rank = 1i64;
    let mut count = None;
    let mut max_len = None;
    for opt in opt_li {
      match opt {
        LPos::Rank(r) => rank = r,
        LPos::Count(c) => count = Some(c),
        LPos::MaxLen(m) => max_len = Some(m),
      }
    }
    if rank == 0 {
      return Err(Error::invalid_data(ERR_RANK_ZERO));
    }

    let key_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_list_meta_key(&kc, key_bytes);
    let now_ms = current_now_ms();

    let meta = match get_meta_checked::<ListMeta, _>(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(Vec::new()),
    };

    if meta.base.size == 0 {
      return Ok(Vec::new());
    }

    let len = meta.base.size as usize;
    let reversed = rank < 0;
    let target_rank = rank.unsigned_abs() as usize;
    let limit = max_len
      .map(|m| if m == 0 { len } else { m.min(len) })
      .unwrap_or(len);

    if target_rank > limit || target_rank > len {
      return Ok(Vec::new());
    }

    let elem_bytes = elem.as_ref();
    let count_limit = match count {
      Some(0) => usize::MAX,
      Some(c) => c,
      None => 1,
    };
    let is_multi_count = count.is_some();
    let mut matches = match count {
      Some(c) if c > 0 => Vec::with_capacity(c.min(limit)),
      Some(_) => Vec::with_capacity(limit.min(16)),
      None => Vec::with_capacity(1),
    };

    let data_ks = self.data();
    let actual_start = meta.head;
    let actual_end = meta.head.wrapping_add((len - 1) as u64);
    let mut rank_count = 0usize;

    if actual_start <= actual_end {
      let start_k = compose_list_item(&kc, key_bytes, actual_start);
      let end_k = compose_list_item(&kc, key_bytes, actual_end);
      if !reversed {
        for (offset, g) in data_ks
          .range((
            Bound::Included(start_k.as_slice()),
            Bound::Included(end_k.as_slice()),
          ))
          .enumerate()
        {
          if offset >= limit {
            break;
          }
          let entry = g?;
          if entry.value().as_ref() == elem_bytes {
            rank_count += 1;
            if rank_count >= target_rank {
              matches.push(offset as i64);
              if (is_multi_count && matches.len() >= count_limit) || !is_multi_count {
                break;
              }
            }
          }
        }
      } else {
        let start_offset = len - 1;
        let end_offset = len.saturating_sub(limit);
        for (i, g) in data_ks
          .range((
            Bound::Included(start_k.as_slice()),
            Bound::Included(end_k.as_slice()),
          ))
          .rev()
          .enumerate()
        {
          if i > start_offset {
            break;
          }
          let offset = start_offset - i;
          if offset < end_offset {
            break;
          }
          let entry = g?;
          if entry.value().as_ref() == elem_bytes {
            rank_count += 1;
            if rank_count >= target_rank {
              matches.push(offset as i64);
              if (is_multi_count && matches.len() >= count_limit) || !is_multi_count {
                break;
              }
            }
          }
        }
      }
    } else {
      let mut composer = ListItemKeyComposer::new(&kc, key_bytes);
      let mut check_offset = |offset: usize| -> Result<bool> {
        let idx = meta.head.wrapping_add(offset as u64);
        let item_k = composer.key_for_idx(idx);
        if let Some(val) = data_ks.get(item_k)?
          && val.as_ref() == elem_bytes
        {
          rank_count += 1;
          if rank_count >= target_rank {
            matches.push(offset as i64);
            if (is_multi_count && matches.len() >= count_limit) || !is_multi_count {
              return Ok(false);
            }
          }
        }
        Ok(true)
      };

      if !reversed {
        for offset in 0..limit {
          if !check_offset(offset)? {
            break;
          }
        }
      } else {
        let start_offset = len - 1;
        let end_offset = len.saturating_sub(limit);
        for offset in (end_offset..=start_offset).rev() {
          if !check_offset(offset)? {
            break;
          }
        }
      }
    }

    Ok(matches)
  }
}
