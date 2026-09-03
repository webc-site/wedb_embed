use crate::{
  api::tdigest::{
    TDigestState,
    r#const::{DEFAULT_COMPRESSION, MAX_COMPRESSION, MIN_COMPRESSION},
    get_tdigest, key,
    meta::TDigestMeta,
    opt::{TDigestInfo, TDigestMerge},
    save_tdigest,
  },
  engine::Engine,
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn tdigest_create<K: AsRef<[u8]>>(&self, key: K, compression: f64) -> Result<()> {
    let kc = self.kc();
    let comp = if compression <= 0.0 {
      DEFAULT_COMPRESSION
    } else {
      compression as u32
    };
    if !(MIN_COMPRESSION..=MAX_COMPRESSION).contains(&comp) {
      return Err(Error::invalid_data(format!(
        "ERR compression out of range [{MIN_COMPRESSION}, {MAX_COMPRESSION}]"
      )));
    }

    let key_bytes = key.as_ref();
    let meta_k = key::meta(&kc, key_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<TDigestMeta, _>(self, key_bytes, &meta_k, now_ms)?.is_some() {
      return Err(Error::invalid_data("ERR item exists"));
    }

    let td = TDigestState::new(comp as f64);
    save_tdigest(self, key_bytes, &td)
  }

  #[inline]
  pub fn tdigest_add_one<K: AsRef<[u8]>>(&self, key: K, value: f64) -> Result<()> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    td.add(value, 1.0);
    save_tdigest(self, key_bytes, &td)
  }

  #[inline]
  pub fn tdigest_add<K: AsRef<[u8]>>(&self, key: K, values: &[f64]) -> Result<()> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    td.add_batch(values);
    save_tdigest(self, key_bytes, &td)
  }

  #[inline]
  pub fn tdigest_min<K: AsRef<[u8]>>(&self, key: K) -> Result<f64> {
    let key_bytes = key.as_ref();
    let td = get_tdigest(self, key_bytes)?;
    Ok(td.min())
  }

  #[inline]
  pub fn tdigest_max<K: AsRef<[u8]>>(&self, key: K) -> Result<f64> {
    let key_bytes = key.as_ref();
    let td = get_tdigest(self, key_bytes)?;
    Ok(td.max())
  }

  #[inline]
  pub fn tdigest_quantile_one<K: AsRef<[u8]>>(&self, key: K, quantile: f64) -> Result<Option<f64>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let val = td.quantile(quantile);
    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }
    if val.is_nan() {
      Ok(None)
    } else {
      Ok(Some(val))
    }
  }

  #[inline]
  pub fn tdigest_quantile<K: AsRef<[u8]>>(
    &self,
    key: K,
    quantiles: &[f64],
  ) -> Result<Vec<Option<f64>>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let results = quantiles
      .iter()
      .map(|&q| {
        let val = td.quantile(q);
        if val.is_nan() { None } else { Some(val) }
      })
      .collect();

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    Ok(results)
  }

  #[inline]
  pub fn tdigest_cdf_one<K: AsRef<[u8]>>(&self, key: K, value: f64) -> Result<Option<f64>> {
    let res = self.tdigest_cdf(key, &[value])?;
    Ok(res.into_iter().next().flatten())
  }

  #[inline]
  pub fn tdigest_cdf<K: AsRef<[u8]>>(&self, key: K, values: &[f64]) -> Result<Vec<Option<f64>>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let results = values
      .iter()
      .map(|&val| {
        let cdf_val = td.cdf(val);
        if cdf_val.is_nan() {
          None
        } else {
          Some(cdf_val)
        }
      })
      .collect();

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    Ok(results)
  }

  #[inline]
  pub fn tdigest_rank_one<K: AsRef<[u8]>>(&self, key: K, value: f64) -> Result<i64> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let rank = td.rank(value);
    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }
    Ok(rank)
  }

  #[inline]
  pub fn tdigest_rank<K: AsRef<[u8]>>(&self, key: K, values: &[f64]) -> Result<Vec<i64>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let res: Vec<i64> = values.iter().map(|&v| td.rank(v)).collect();

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    Ok(res)
  }

  #[inline]
  pub fn tdigest_revrank_one<K: AsRef<[u8]>>(&self, key: K, value: f64) -> Result<i64> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let rank = td.revrank(value);
    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }
    Ok(rank)
  }

  #[inline]
  pub fn tdigest_revrank<K: AsRef<[u8]>>(&self, key: K, values: &[f64]) -> Result<Vec<i64>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let res: Vec<i64> = values.iter().map(|&v| td.revrank(v)).collect();

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    Ok(res)
  }

  #[inline]
  pub fn tdigest_byrank_one<K: AsRef<[u8]>>(&self, key: K, rank: u64) -> Result<Option<f64>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let val = td.byrank(rank);
    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }
    if val.is_nan() {
      Ok(None)
    } else {
      Ok(Some(val))
    }
  }

  #[inline]
  pub fn tdigest_byrank<K: AsRef<[u8]>>(&self, key: K, ranks: &[u64]) -> Result<Vec<Option<f64>>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let res: Vec<Option<f64>> = ranks
      .iter()
      .map(|&r| {
        let v = td.byrank(r);
        if v.is_nan() { None } else { Some(v) }
      })
      .collect();

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    Ok(res)
  }

  #[inline]
  pub fn tdigest_byrevrank_one<K: AsRef<[u8]>>(&self, key: K, rank: u64) -> Result<Option<f64>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let val = td.byrevrank(rank);
    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }
    if val.is_nan() {
      Ok(None)
    } else {
      Ok(Some(val))
    }
  }

  #[inline]
  pub fn tdigest_byrevrank<K: AsRef<[u8]>>(
    &self,
    key: K,
    ranks: &[u64],
  ) -> Result<Vec<Option<f64>>> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let res: Vec<Option<f64>> = ranks
      .iter()
      .map(|&r| {
        let v = td.byrevrank(r);
        if v.is_nan() { None } else { Some(v) }
      })
      .collect();

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    Ok(res)
  }

  #[inline]
  pub fn tdigest_trimmed_mean<K: AsRef<[u8]>>(
    &self,
    key: K,
    low_cut: f64,
    high_cut: f64,
  ) -> Result<Option<f64>> {
    if !low_cut.is_finite()
      || !high_cut.is_finite()
      || !(0.0..=1.0).contains(&low_cut)
      || !(0.0..=1.0).contains(&high_cut)
    {
      return Err(Error::invalid_data(
        "ERR low_cut_percentile and high_cut_percentile should be in [0,1]",
      ));
    }
    if low_cut >= high_cut {
      return Err(Error::invalid_data(
        "ERR low_cut_percentile should be lower than high_cut_percentile",
      ));
    }

    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let mean = td.trimmed_mean(low_cut, high_cut);

    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }

    if mean.is_nan() {
      Ok(None)
    } else {
      Ok(Some(mean))
    }
  }

  #[inline]
  pub fn tdigest_reset<K: AsRef<[u8]>>(&self, key: K) -> Result<()> {
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    td.reset();
    save_tdigest(self, key_bytes, &td)
  }

  #[inline]
  pub fn tdigest_merge<K: AsRef<[u8]>>(
    &self,
    dest_key: K,
    source_keys: &[K],
    opt_li: impl IntoIterator<Item = TDigestMerge>,
  ) -> Result<()> {
    let mut compression = None;
    let mut override_dest = false;
    for opt in opt_li {
      match opt {
        TDigestMerge::Compression(c) => compression = Some(c),
        TDigestMerge::Override => override_dest = true,
      }
    }
    if source_keys.is_empty() {
      return Err(Error::invalid_data(
        "ERR wrong number of arguments for 'tdigest.merge' cmd",
      ));
    }

    if let Some(comp) = compression
      && !(MIN_COMPRESSION..=MAX_COMPRESSION).contains(&comp)
    {
      return Err(Error::invalid_data(format!(
        "ERR compression out of range [{MIN_COMPRESSION}, {MAX_COMPRESSION}]"
      )));
    }

    let dst_bytes = dest_key.as_ref();
    let dest_opt = get_tdigest(self, dst_bytes).ok();

    let mut source_tds = Vec::with_capacity(source_keys.len());
    for src in source_keys {
      let src_bytes = src.as_ref();
      let src_td = get_tdigest(self, src_bytes)
        .map_err(|_| Error::invalid_data(format!("ERR key not found: {:?}", src_bytes)))?;
      source_tds.push(src_td);
    }

    let mut dest_td = match &dest_opt {
      Some(existing) if !override_dest => {
        let mut d = existing.clone();
        d.ensure_merged();
        d
      }
      Some(existing) => TDigestState::new(existing.compression),
      None => TDigestState::new(DEFAULT_COMPRESSION as f64),
    };

    let dest_existed = dest_opt.is_some();
    dest_td.merge_with_options(&mut source_tds, override_dest, compression, dest_existed);
    save_tdigest(self, dst_bytes, &dest_td)
  }

  #[inline]
  pub fn tdigest_info<K: AsRef<[u8]>>(&self, key: K) -> Result<TDigestInfo> {
    let key_bytes = key.as_ref();
    let td = get_tdigest(self, key_bytes)?;
    Ok(td.info())
  }
}
