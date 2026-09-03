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
    if values.is_empty() {
      return Ok(());
    }
    if values.len() == 1 {
      return self.tdigest_add_one(key, values[0]);
    }
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
