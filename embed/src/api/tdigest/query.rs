use crate::{
  api::tdigest::{get_tdigest, save_tdigest},
  engine::Engine,
  error::{Error, Result},
  wedb::Db,
};

/// TDigest statistical query operations (CDF, QUANTILE, RANK, REVRANK, BYRANK, BYREVRANK, TRIMMED_MEAN).
/// TDigest 统计估计查询操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
    let key_bytes = key.as_ref();
    let mut td = get_tdigest(self, key_bytes)?;
    let had_unmerged = !td.unmerged_buffer.is_empty();
    let cdf_val = td.cdf(value);
    if had_unmerged {
      save_tdigest(self, key_bytes, &td)?;
    }
    if cdf_val.is_nan() {
      Ok(None)
    } else {
      Ok(Some(cdf_val))
    }
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
}
