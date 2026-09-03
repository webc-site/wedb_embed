use rapidhash::v3::rapidhash_v3;

use crate::{
  api::bloom::{
    r#const::DEFAULT_BF_EXPANSION,
    key,
    meta::BloomChainMeta,
    opt::{BfInsert, BfReserve, BloomFilterAddResult, BloomFilterInfo, BloomFilterInsert},
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

#[inline]
fn rapid_hash(bytes: &[u8]) -> u64 {
  rapidhash_v3(bytes)
}

/// Block Split Bloom Filter (aligned with Apache Kvrocks and SIMD block split algorithm).
/// 块切分布隆过滤器（Block Split Bloom Filter，对标 Apache Kvrocks 与 SIMD 块切分算法）
pub struct BlockSplitBloomFilter;

impl BlockSplitBloomFilter {
  pub const MIN_BYTES: usize = 32;
  pub const MAX_BYTES: usize = 128 * 1024 * 1024; // 128 MB
  pub const BYTES_PER_FILTER_BLOCK: usize = 32;
  pub const BITS_SET_PER_BLOCK: usize = 8;

  pub const SALT: [u32; 8] = [
    0x47b6_137b,
    0x4497_4d91,
    0x8824_ad5b,
    0xa2b7_289d,
    0x7054_95c7,
    0x2df1_424b,
    0x9efc_4947,
    0x5c6b_fb31,
  ];

  #[inline]
  pub fn hash(data: &[u8]) -> u64 {
    rapid_hash(data)
  }

  /// Calculates optimal bit count based on capacity and false positive rate.
  /// 根据预计元素个数与假阳性率计算最优位长度（对标 Kvrocks OptimalNumOfBits）
  #[inline]
  pub fn optimal_num_of_bits(ndv: u32, fpp: f64) -> usize {
    let fpp_clamped = fpp.clamp(1e-15, 0.99999);
    let m = -8.0 * (ndv as f64) / (1.0 - fpp_clamped.powf(1.0 / 8.0)).ln();
    let max_bits = Self::MAX_BYTES * 8;
    let mut num_bits = if m.is_nan() || m < 0.0 || m > (max_bits as f64) {
      max_bits
    } else {
      m as usize
    };

    let min_bits = Self::MIN_BYTES * 8;
    if num_bits < min_bits {
      num_bits = min_bits;
    }

    if !num_bits.is_power_of_two() {
      num_bits = num_bits.next_power_of_two();
    }

    if num_bits > max_bits {
      num_bits = max_bits;
    }

    num_bits
  }

  /// Calculates optimal byte count based on capacity and false positive rate.
  /// 根据预计元素个数与假阳性率计算最优字节大小（对标 Kvrocks OptimalNumOfBytes）
  #[inline]
  pub fn optimal_num_of_bytes(ndv: u32, fpp: f64) -> usize {
    Self::optimal_num_of_bits(ndv, fpp) >> 3
  }

  #[inline(always)]
  fn block_range(data_len: usize, hash: u64) -> Option<usize> {
    if data_len < Self::BYTES_PER_FILTER_BLOCK {
      return None;
    }
    let num_blocks = (data_len / Self::BYTES_PER_FILTER_BLOCK) as u64;
    if num_blocks == 0 {
      return None;
    }
    let bucket_index = (((hash >> 32).wrapping_mul(num_blocks)) >> 32) as usize;
    let block_start = bucket_index * Self::BYTES_PER_FILTER_BLOCK;
    if block_start + Self::BYTES_PER_FILTER_BLOCK > data_len {
      None
    } else {
      Some(block_start)
    }
  }

  /// Checks whether hash value exists in block-split Bloom filter (aligned with Kvrocks FindHash).
  /// 检查哈希值是否存在于块切分布隆位图中（对标 Kvrocks FindHash，零拷贝字级切片迭代）
  #[inline]
  pub fn find_hash(data: &[u8], hash: u64) -> bool {
    let block_start = match Self::block_range(data.len(), hash) {
      Some(s) => s,
      None => return false,
    };
    let key = hash as u32;
    // SAFETY: 上方已前置校验 block_start + BYTES_PER_FILTER_BLOCK <= data.len()，索引切片区间绝不越界。
    let block =
      unsafe { data.get_unchecked(block_start..block_start + Self::BYTES_PER_FILTER_BLOCK) };

    let (chunks, _) = block.as_chunks::<4>();
    for (&salt, chunk) in Self::SALT.iter().zip(chunks) {
      let bit_shift = (key.wrapping_mul(salt)) >> 27;
      let mask = 1u32 << bit_shift;
      let word = u32::from_le_bytes(*chunk);
      if (word & mask) == 0 {
        return false;
      }
    }
    true
  }

  /// Inserts hash into block-split bloom bitmap aligned with Kvrocks InsertHash.
  /// 将哈希值插入到块切分布隆位图中（对标 Kvrocks InsertHash）
  #[inline]
  pub fn insert_hash(data: &mut [u8], hash: u64) -> bool {
    let block_start = match Self::block_range(data.len(), hash) {
      Some(s) => s,
      None => return false,
    };
    let key = hash as u32;
    // SAFETY: 上方已前置校验 block_start + BYTES_PER_FILTER_BLOCK <= data.len()，可变切片区间严格在合法内存范围内。
    let block =
      unsafe { data.get_unchecked_mut(block_start..block_start + Self::BYTES_PER_FILTER_BLOCK) };

    let mut modified = false;
    let (chunks, _) = block.as_chunks_mut::<4>();
    for (&salt, chunk) in Self::SALT.iter().zip(chunks) {
      let bit_shift = (key.wrapping_mul(salt)) >> 27;
      let mask = 1u32 << bit_shift;
      let word = u32::from_le_bytes(*chunk);
      if (word & mask) == 0 {
        *chunk = (word | mask).to_le_bytes();
        modified = true;
      }
    }
    modified
  }
}

/// Internal helper validating and retrieving Bloom filter metadata with type and TTL checks.
/// 内部辅助：校验并获取 Bloom 元数据（含 WRONGTYPE 与 TTL 状态判定）
#[inline]
pub(crate) fn get_bf_meta_checked<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
) -> Result<Option<BloomChainMeta>>
where
  Error: From<E::Error>,
{
  get_meta_checked::<BloomChainMeta, _>(db, k_bytes, meta_k, now_ms)
}

/// Bloom filter operations interface (Bloom Filter Chain).
/// 布隆过滤器链操作接口 (Bloom Filter)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn bf_reserve_one<K: AsRef<[u8]>>(
    &self,
    key: K,
    error_rate: f64,
    capacity: u32,
  ) -> Result<()> {
    self.bf_reserve(key, error_rate, capacity, [])
  }

  #[inline]
  pub fn bf_reserve<K: AsRef<[u8]>>(
    &self,
    key: K,
    error_rate: f64,
    capacity: u32,
    opt_li: impl IntoIterator<Item = BfReserve>,
  ) -> Result<()> {
    let kc = self.kc();
    if capacity == 0 {
      return Err(Error::invalid_data("capacity must be larger than 0"));
    }
    if error_rate <= 0.0 || error_rate >= 1.0 {
      return Err(Error::invalid_data("error_rate must be between 0 and 1"));
    }

    let mut expansion = DEFAULT_BF_EXPANSION;
    for opt in opt_li {
      match opt {
        BfReserve::Expansion(exp) => expansion = exp,
        BfReserve::NonScaling => expansion = 0,
      }
    }

    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);

    let now_ms = current_now_ms();
    if get_bf_meta_checked(self, key_bytes, &meta_k, now_ms)?.is_some() {
      return Err(Error::invalid_data("ERR item exists"));
    }

    let bloom_bytes = BlockSplitBloomFilter::optimal_num_of_bytes(capacity, error_rate) as u32;
    let meta = BloomChainMeta::new(capacity, error_rate, expansion, 0, 0, bloom_bytes);

    let mut batch = self.batch();
    batch.insert_meta(&meta_k, &meta.encode());

    let item_k = key::bloom_item(&kc, key_bytes, 0);
    let block_data = vec![0u8; bloom_bytes as usize];
    batch.insert_data(&item_k, block_data.as_slice());

    batch.commit()?;
    Ok(())
  }

  #[inline]
  pub fn bf_add<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    let res = self.bf_insert(key, &[item], [])?;
    match res.first() {
      Some(BloomFilterAddResult::Ok) => Ok(true),
      Some(BloomFilterAddResult::Exist) => Ok(false),
      Some(BloomFilterAddResult::Full) => Err(Error::invalid_data("ERR nonscaling filter is full")),
      None => Ok(false),
    }
  }

  #[inline]
  pub fn bf_madd<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, items: &[I]) -> Result<Vec<bool>> {
    let res = self.bf_insert(key, items, [])?;
    Ok(
      res
        .into_iter()
        .map(|r| matches!(r, BloomFilterAddResult::Ok))
        .collect(),
    )
  }

  #[inline]
  pub fn bf_insert_one<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    item: I,
    opt_li: impl IntoIterator<Item = BfInsert>,
  ) -> Result<BloomFilterAddResult> {
    let res = self.bf_insert(key, &[item], opt_li)?;
    Ok(res.into_iter().next().unwrap_or(BloomFilterAddResult::Ok))
  }

  #[inline]
  pub fn bf_insert<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
    opt_li: impl IntoIterator<Item = BfInsert>,
  ) -> Result<Vec<BloomFilterAddResult>> {
    let opt: BloomFilterInsert = opt_li.into_iter().collect();
    self.bf_insert_internal(key, items, &opt)
  }

  #[inline]
  pub(crate) fn bf_insert_internal<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
    opt: &BloomFilterInsert,
  ) -> Result<Vec<BloomFilterAddResult>> {
    let kc = self.kc();
    if items.is_empty() {
      return Ok(Vec::new());
    }

    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let mut meta = match get_bf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => {
        if !opt.auto_create {
          return Err(Error::invalid_data("ERR not found"));
        }
        if opt.capacity == 0 {
          return Err(Error::invalid_data("capacity must be larger than 0"));
        }
        if opt.error_rate <= 0.0 || opt.error_rate >= 1.0 {
          return Err(Error::invalid_data("error_rate must be between 0 and 1"));
        }
        let bloom_bytes =
          BlockSplitBloomFilter::optimal_num_of_bytes(opt.capacity, opt.error_rate) as u32;
        BloomChainMeta::new(
          opt.capacity,
          opt.error_rate,
          opt.expansion,
          0,
          0,
          bloom_bytes,
        )
      }
    };

    let initial_n_filters = meta.n_filters;
    let mut blocks = Vec::with_capacity(meta.n_filters as usize);
    for idx in 0..meta.n_filters {
      let item_k = key::bloom_item(&kc, key_bytes, idx);
      let cap = meta.sub_filter_capacity(idx);
      let expected_bytes = BlockSplitBloomFilter::optimal_num_of_bytes(cap, meta.error_rate);
      if let Some(b) = data_ks.get(&item_k)? {
        let mut vec = b.to_vec();
        if vec.len() < expected_bytes {
          vec.resize(expected_bytes, 0);
        }
        blocks.push(vec);
      } else {
        blocks.push(vec![0u8; expected_bytes]);
      }
    }

    let mut results = Vec::with_capacity(items.len());
    let mut dirty = false;

    for it in items {
      let h = BlockSplitBloomFilter::hash(it.as_ref());
      let exists = blocks
        .iter()
        .rev()
        .any(|blk| BlockSplitBloomFilter::find_hash(blk.as_slice(), h));
      if exists {
        results.push(BloomFilterAddResult::Exist);
        continue;
      }

      if meta.base.size + 1 > meta.get_capacity() as u64 {
        if meta.is_scaling() && meta.n_filters < u16::MAX {
          let new_cap = meta.sub_filter_capacity(meta.n_filters);
          let new_bytes = BlockSplitBloomFilter::optimal_num_of_bytes(new_cap, meta.error_rate);
          meta.n_filters += 1;
          meta.bloom_bytes += new_bytes as u32;
          blocks.push(vec![0u8; new_bytes]);
        } else {
          results.push(BloomFilterAddResult::Full);
          continue;
        }
      }

      let cur_idx = (meta.n_filters - 1) as usize;
      BlockSplitBloomFilter::insert_hash(&mut blocks[cur_idx], h);
      meta.base.size += 1;
      dirty = true;
      results.push(BloomFilterAddResult::Ok);
    }

    if dirty {
      let write_start_idx = (initial_n_filters as usize).saturating_sub(1);
      let mut batch = self.batch_with_capacity(blocks.len() - write_start_idx + 1);
      batch.insert_meta(&meta_k, &meta.encode());
      for (idx, blk) in blocks.iter().enumerate().skip(write_start_idx) {
        let item_k = key::bloom_item(&kc, key_bytes, idx as u16);
        batch.insert_data(&item_k, blk.as_slice());
      }
      batch.commit()?;
    }

    Ok(results)
  }

  #[inline]
  pub fn bf_exists<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let meta = match get_bf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(false),
    };

    let h = BlockSplitBloomFilter::hash(item.as_ref());
    for idx in (0..meta.n_filters).rev() {
      let item_k = key::bloom_item(&kc, key_bytes, idx);
      if let Some(b) = data_ks.get(&item_k)?
        && BlockSplitBloomFilter::find_hash(b.as_ref(), h)
      {
        return Ok(true);
      }
    }
    Ok(false)
  }

  #[inline]
  pub fn bf_mexists<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
  ) -> Result<Vec<bool>> {
    if items.is_empty() {
      return Ok(Vec::new());
    }
    if items.len() == 1 {
      return Ok(vec![self.bf_exists(key, &items[0])?]);
    }
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let meta = match get_bf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![false; items.len()]),
    };

    let mut blocks = Vec::with_capacity(meta.n_filters as usize);
    for idx in 0..meta.n_filters {
      let item_k = key::bloom_item(&kc, key_bytes, idx);
      if let Some(b) = data_ks.get(&item_k)? {
        blocks.push(b);
      }
    }

    let mut results = Vec::with_capacity(items.len());
    for it in items {
      let h = BlockSplitBloomFilter::hash(it.as_ref());
      let found = blocks
        .iter()
        .rev()
        .any(|blk| BlockSplitBloomFilter::find_hash(blk.as_ref(), h));
      results.push(found);
    }

    Ok(results)
  }

  #[inline]
  pub fn bf_info<K: AsRef<[u8]>>(&self, key: K) -> Result<BloomFilterInfo> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);

    let now_ms = current_now_ms();
    let meta = get_bf_meta_checked(self, key_bytes, &meta_k, now_ms)?
      .ok_or_else(|| Error::invalid_data("ERR not found"))?;

    Ok(BloomFilterInfo {
      capacity: meta.get_capacity(),
      bloom_bytes: meta.bloom_bytes,
      n_filters: meta.n_filters,
      size: meta.base.size,
      expansion: meta.expansion,
    })
  }

  #[inline]
  pub fn bf_card<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);

    let now_ms = current_now_ms();
    Ok(
      get_bf_meta_checked(self, key_bytes, &meta_k, now_ms)?
        .map(|m| m.base.size)
        .unwrap_or(0),
    )
  }
}
