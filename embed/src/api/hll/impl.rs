use rapidhash::RapidHashSet as HashSet;

use crate::{
  api::hll::{
    algo::{HLL_DENSE_SIZE, extract_dense_hll_result, hll_murmur_hash_64a, rapid_hash},
    compose_hll_data_key, compose_hll_meta_key,
    core::HyperLogLog,
    dense::{hll_dense_estimate, hll_dense_get_register, hll_dense_set_register, hll_merge_bytes},
    meta::{HllEncodeType, HyperLogLogMeta},
    sparse::{
      hll_merge_sparse_into_dense, hll_sparse_estimate, hll_sparse_set_register,
      hll_sparse_to_dense,
    },
  },
  engine::{Engine, Partition},
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
  pub fn pfadd_one<K: AsRef<[u8]>, EL: AsRef<[u8]>>(&self, key: K, element: EL) -> Result<bool> {
    self.pfadd(key, &[element])
  }

  #[inline]
  pub fn pfadd<K: AsRef<[u8]>, EL: AsRef<[u8]>>(&self, key: K, elements: &[EL]) -> Result<bool> {
    if elements.is_empty() {
      return Ok(false);
    }
    self.pfadd_with_hashes(key, elements.iter().map(|el| rapid_hash(el.as_ref())))
  }

  /// Adds elements using MurmurHash64A with seed 0xadc83b19 (100% binary-compatible with Redis / Apache Kvrocks).
  /// 添加元素数据（MurmurHash64A 对标 Redis / Apache Kvrocks）
  #[inline]
  pub fn pfadd_murmur<K: AsRef<[u8]>, EL: AsRef<[u8]>>(
    &self,
    key: K,
    elements: &[EL],
  ) -> Result<bool> {
    if elements.is_empty() {
      return Ok(false);
    }
    self.pfadd_with_hashes(
      key,
      elements.iter().map(|el| hll_murmur_hash_64a(el.as_ref())),
    )
  }

  /// Adds precomputed 64-bit element hashes aligned with Apache Kvrocks HyperLogLog::Add.
  /// 添加预先计算好的 64 位元素哈希值（对标 Apache Kvrocks HyperLogLog::Add）
  #[inline]
  pub fn pfadd_hashes<K: AsRef<[u8]>>(&self, key: K, hashes: &[u64]) -> Result<bool> {
    if hashes.is_empty() {
      return Ok(false);
    }
    self.pfadd_with_hashes(key, hashes.iter().copied())
  }

  #[inline]
  fn pfadd_with_hashes<K: AsRef<[u8]>, I: IntoIterator<Item = u64>>(
    &self,
    key: K,
    hashes: I,
  ) -> Result<bool> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_hll_meta_key(&kc, k_bytes);
    let data_k = compose_hll_data_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    let (mut meta, exists_valid) = match get_meta_checked(self, k_bytes, meta_k.as_slice(), now_ms)?
    {
      Some(meta) => (meta, true),
      None => (HyperLogLogMeta::new_with_version(0), false),
    };

    let data_ks = self.data();

    let mut registers = if exists_valid {
      data_ks
        .get(data_k.as_slice())?
        .map(|v| v.to_vec())
        .unwrap_or_else(|| vec![0u8; HLL_DENSE_SIZE])
    } else {
      vec![0u8; HLL_DENSE_SIZE]
    };

    if meta.encode_type == HllEncodeType::Dense && registers.len() < HLL_DENSE_SIZE {
      registers.resize(HLL_DENSE_SIZE, 0);
    }

    let mut updated = false;
    for hash in hashes {
      let (reg_idx, count) = extract_dense_hll_result(hash);
      match meta.encode_type {
        HllEncodeType::Dense => {
          let cur = hll_dense_get_register(&registers, reg_idx);
          if count > cur {
            hll_dense_set_register(&mut registers, reg_idx, count);
            updated = true;
          }
        }
        HllEncodeType::Sparse => match hll_sparse_set_register(&mut registers, reg_idx, count) {
          Ok(changed) => {
            if changed {
              updated = true;
            }
          }
          Err(_) => {
            let mut dense_buf = vec![0u8; HLL_DENSE_SIZE];
            hll_sparse_to_dense(&registers, &mut dense_buf)?;
            let cur = hll_dense_get_register(&dense_buf, reg_idx);
            if count > cur {
              hll_dense_set_register(&mut dense_buf, reg_idx, count);
              updated = true;
            }
            registers = dense_buf;
            meta.encode_type = HllEncodeType::Dense;
          }
        },
      }
    }

    if updated {
      meta.base.size = registers.len() as u64;
      let mut batch = self.batch();
      batch.insert_data(data_k.as_slice(), &registers);
      batch.insert_meta(meta_k.as_slice(), &meta.encode());
      batch.commit()?;
    }

    Ok(updated)
  }

  #[inline]
  pub fn pfcount_one<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    self.pfcount(&[key])
  }

  /// Calculates cardinality for multiple keys aligned with Apache Kvrocks HyperLogLog::CountMultiple.
  /// 统计多键联合基数（对标 Apache Kvrocks HyperLogLog::CountMultiple）
  #[inline]
  pub fn pfcount_multiple<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<u64> {
    self.pfcount(keys)
  }

  #[inline]
  pub fn pfcount<K: AsRef<[u8]>>(&self, keys: &[K]) -> Result<u64> {
    if keys.is_empty() {
      return Ok(0);
    }

    let kc = self.kc();
    let now_ms = current_now_ms();
    let data_ks = self.data();

    if keys.len() == 1 {
      let k_bytes = keys[0].as_ref();
      let meta_k = compose_hll_meta_key(&kc, k_bytes);
      let meta =
        match get_meta_checked::<HyperLogLogMeta, _>(self, k_bytes, meta_k.as_slice(), now_ms)? {
          Some(meta) => meta,
          None => return Ok(0),
        };
      let data_k = compose_hll_data_key(&kc, k_bytes);
      let registers = match data_ks.get(data_k.as_slice())? {
        Some(v) => v,
        None => return Ok(0),
      };
      return match meta.encode_type {
        HllEncodeType::Dense => Ok(hll_dense_estimate(&registers)),
        HllEncodeType::Sparse => {
          Ok(hll_sparse_estimate(&registers).unwrap_or_else(|_| hll_dense_estimate(&registers)))
        }
      };
    }

    let mut seen = HashSet::default();
    let mut merged = vec![0u8; HLL_DENSE_SIZE];
    let mut has_any = false;

    for k in keys {
      let k_bytes = k.as_ref();
      if !seen.insert(k_bytes) {
        continue;
      }
      let meta_k = compose_hll_meta_key(&kc, k_bytes);
      let meta =
        match get_meta_checked::<HyperLogLogMeta, _>(self, k_bytes, meta_k.as_slice(), now_ms)? {
          Some(meta) => meta,
          None => continue,
        };
      let data_k = compose_hll_data_key(&kc, k_bytes);
      if let Some(reg) = data_ks.get(data_k.as_slice())? {
        match meta.encode_type {
          HllEncodeType::Dense => {
            hll_merge_bytes(&mut merged, &reg);
          }
          HllEncodeType::Sparse => {
            hll_merge_sparse_into_dense(&mut merged, &reg);
          }
        }
        has_any = true;
      }
    }

    if !has_any {
      return Ok(0);
    }

    Ok(hll_dense_estimate(&merged))
  }

  #[inline]
  pub fn pfmerge<K: AsRef<[u8]>>(&self, dest: K, sources: &[K]) -> Result<()> {
    if sources.is_empty() {
      return Ok(());
    }

    let kc = self.kc();
    let dest_bytes = dest.as_ref();
    let dest_meta_k = compose_hll_meta_key(&kc, dest_bytes);
    let dest_data_k = compose_hll_data_key(&kc, dest_bytes);
    let now_ms = current_now_ms();

    let data_ks = self.data();

    let (mut dest_meta, dest_valid) =
      match get_meta_checked(self, dest_bytes, dest_meta_k.as_slice(), now_ms)? {
        Some(meta) => (meta, true),
        None => (HyperLogLogMeta::new_with_version(0), false),
      };

    let mut merged = if dest_valid {
      let data = data_ks
        .get(dest_data_k.as_slice())?
        .map(|v| v.to_vec())
        .unwrap_or_else(|| vec![0u8; HLL_DENSE_SIZE]);
      if dest_meta.encode_type == HllEncodeType::Sparse {
        let mut dense_buf = vec![0u8; HLL_DENSE_SIZE];
        if hll_sparse_to_dense(&data, &mut dense_buf).is_ok() {
          dense_buf
        } else {
          data
        }
      } else {
        data
      }
    } else {
      vec![0u8; HLL_DENSE_SIZE]
    };

    if merged.len() < HLL_DENSE_SIZE {
      merged.resize(HLL_DENSE_SIZE, 0);
    }

    let mut seen = HashSet::default();
    seen.insert(dest_bytes);

    for k in sources {
      let k_bytes = k.as_ref();
      if !seen.insert(k_bytes) {
        continue;
      }
      let meta_k = compose_hll_meta_key(&kc, k_bytes);
      let meta =
        match get_meta_checked::<HyperLogLogMeta, _>(self, k_bytes, meta_k.as_slice(), now_ms)? {
          Some(meta) => meta,
          None => continue,
        };
      let data_k = compose_hll_data_key(&kc, k_bytes);
      if let Some(reg) = data_ks.get(data_k.as_slice())? {
        match meta.encode_type {
          HllEncodeType::Dense => {
            hll_merge_bytes(&mut merged, &reg);
          }
          HllEncodeType::Sparse => {
            hll_merge_sparse_into_dense(&mut merged, &reg);
          }
        }
      }
    }

    dest_meta.base.size = HLL_DENSE_SIZE as u64;
    dest_meta.encode_type = HllEncodeType::Dense;
    let mut batch = self.batch();
    batch.insert_data(dest_data_k.as_slice(), &merged);
    batch.insert_meta(dest_meta_k.as_slice(), &dest_meta.encode());
    batch.commit()?;

    Ok(())
  }
  #[inline]
  pub fn pfselftest(&self) -> bool {
    HyperLogLog::selftest()
  }
}
