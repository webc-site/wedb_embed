use core::hint::unreachable_unchecked;
use std::collections::hash_map::Entry;

use memchr::memchr;
use rapidhash::{RapidHashMap as HashMap, v3::rapidhash_v3};

use crate::{
  api::bloom::{
    r#const::{
      DEFAULT_BF_EXPANSION, DEFAULT_CF_BUCKET_SIZE, DEFAULT_CF_EXPANSION, DEFAULT_CF_MAX_ITERATIONS,
    },
    key,
    meta::{BloomChainMeta, CuckooChainMeta},
    opt::{
      BfInsert, BfReserve, BloomFilterAddResult, BloomFilterInfo, BloomFilterInsert, CfInsert,
      CfReserve, CuckooFilterHelper, CuckooFilterInfo, CuckooFilterInsert,
    },
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  key_composer::KeyComposer,
  meta::current_now_ms,
  wedb::{Db, DbBatch},
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

  /// Checks whether hash value exists in block-split Bloom filter (aligned with Kvrocks FindHash).
  /// 检查哈希值是否存在于块切分布隆位图中（对标 Kvrocks FindHash，零拷贝字级切片迭代）
  #[inline]
  pub fn find_hash(data: &[u8], hash: u64) -> bool {
    if data.len() < Self::BYTES_PER_FILTER_BLOCK {
      return false;
    }
    let num_blocks = (data.len() / Self::BYTES_PER_FILTER_BLOCK) as u64;
    if num_blocks == 0 {
      return false;
    }
    let bucket_index = (((hash >> 32).wrapping_mul(num_blocks)) >> 32) as usize;
    let key = hash as u32;
    let block_start = bucket_index * Self::BYTES_PER_FILTER_BLOCK;
    if block_start + Self::BYTES_PER_FILTER_BLOCK > data.len() {
      return false;
    }
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
    if data.len() < Self::BYTES_PER_FILTER_BLOCK {
      return false;
    }
    let num_blocks = (data.len() / Self::BYTES_PER_FILTER_BLOCK) as u64;
    if num_blocks == 0 {
      return false;
    }
    let bucket_index = (((hash >> 32).wrapping_mul(num_blocks)) >> 32) as usize;
    let key = hash as u32;
    let block_start = bucket_index * Self::BYTES_PER_FILTER_BLOCK;
    if block_start + Self::BYTES_PER_FILTER_BLOCK > data.len() {
      return false;
    }
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

/// Cuckoo kickout step rollback entry.
/// 布谷鸟踢出单步修改回滚单元
#[derive(Debug, Clone, Copy)]
struct PageSlotUndo {
  filter_idx: u16,
  page_idx: u32,
  byte_offset: usize,
  old_fp: u8,
}

/// Cuckoo filter page data container with zero-copy read and copy-on-write.
/// 布谷鸟分页数据单元（支持只读零拷贝与按需写时克隆）
enum CuckooPageData {
  Clean(Box<[u8]>),
  Dirty(Vec<u8>),
  Empty(usize),
}

impl CuckooPageData {
  #[inline]
  fn get_subslice(&self, offset: usize, len: usize) -> Option<&[u8]> {
    match self {
      Self::Clean(s) => s.get(offset..offset + len),
      Self::Dirty(v) => v.get(offset..offset + len),
      Self::Empty(_) => None,
    }
  }

  #[inline]
  fn get_byte(&self, offset: usize) -> u8 {
    match self {
      Self::Clean(s) => s.get(offset).copied().unwrap_or(0),
      Self::Dirty(v) => v.get(offset).copied().unwrap_or(0),
      Self::Empty(_) => 0,
    }
  }

  #[inline]
  fn ensure_mut(&mut self, expected_size: usize) -> &mut Vec<u8> {
    match self {
      Self::Dirty(v) => {
        if v.len() < expected_size {
          v.resize(expected_size, 0);
        }
        v
      }
      Self::Clean(s) => {
        let mut vec = s.to_vec();
        if vec.len() < expected_size {
          vec.resize(expected_size, 0);
        }
        *self = Self::Dirty(vec);
        if let Self::Dirty(v) = self {
          v
        } else {
          // SAFETY: 前一行已显式执行 *self = Self::Dirty(vec)，此处 match 必为 Self::Dirty 分支，其它分支在逻辑上不可达。
          unsafe { unreachable_unchecked() }
        }
      }
      Self::Empty(exp) => {
        let size = (*exp).max(expected_size);
        *self = Self::Dirty(vec![0u8; size]);
        if let Self::Dirty(v) = self {
          v
        } else {
          // SAFETY: 前一行已显式执行 *self = Self::Dirty(...)，此处 match 必为 Self::Dirty 分支，其它分支在逻辑上不可达。
          unsafe { unreachable_unchecked() }
        }
      }
    }
  }
}

/// Cuckoo filter page cache with zero-copy read optimization (aligned with Kvrocks CuckooPageCache).
/// 布谷鸟过滤器分页读写缓存（对标 Kvrocks CuckooPageCache，零拷贝读优化）
struct CuckooPageCache<'a, P: Partition> {
  data: &'a P,
  kc: KeyComposer,
  key: &'a [u8],
  bucket_size: u8,
  buckets_per_page: u32,
  pages: HashMap<(u16, u32), CuckooPageData>,
}

impl<'a, P: Partition> CuckooPageCache<'a, P>
where
  Error: From<P::Error>,
{
  #[inline]
  fn new(data: &'a P, kc: KeyComposer, key: &'a [u8], bucket_size: u8, page_size: u32) -> Self {
    let buckets_per_page = (page_size / bucket_size as u32).max(1);
    Self {
      data,
      kc,
      key,
      bucket_size,
      buckets_per_page,
      pages: HashMap::default(),
    }
  }

  #[inline]
  fn get_bucket_location(&self, bucket_idx: u32) -> (u32, usize) {
    let page_idx = bucket_idx / self.buckets_per_page;
    let offset = ((bucket_idx % self.buckets_per_page) as usize) * (self.bucket_size as usize);
    (page_idx, offset)
  }

  #[inline]
  fn get_expected_page_size(&self, num_buckets: u32, page_idx: u32) -> usize {
    let first_bucket = page_idx * self.buckets_per_page;
    let page_bucket_count = self
      .buckets_per_page
      .min(num_buckets.saturating_sub(first_bucket));
    (page_bucket_count as usize) * (self.bucket_size as usize)
  }

  fn get_page_entry_mut(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    page_idx: u32,
  ) -> Result<&mut CuckooPageData> {
    let expected_size = self.get_expected_page_size(num_buckets, page_idx);
    let key = (filter_idx, page_idx);
    match self.pages.entry(key) {
      Entry::Occupied(entry) => Ok(entry.into_mut()),
      Entry::Vacant(entry) => {
        let page_key = key::cuckoo_page(&self.kc, self.key, filter_idx, page_idx);
        let page_data = match self.data.get(&page_key)? {
          Some(slice) => CuckooPageData::Clean(Box::from(&*slice)),
          None => CuckooPageData::Empty(expected_size),
        };
        Ok(entry.insert(page_data))
      }
    }
  }

  #[inline]
  fn try_insert_in_bucket(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    fp: u8,
  ) -> Result<bool> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let bs = self.bucket_size as usize;
    let expected_size = self.get_expected_page_size(num_buckets, page_idx);
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;

    match entry {
      CuckooPageData::Empty(_) => {
        let page_vec = entry.ensure_mut(expected_size);
        page_vec[offset] = fp;
        Ok(true)
      }
      CuckooPageData::Clean(s) => {
        if let Some(sub) = s.get(offset..offset + bs) {
          if let Some(pos) = memchr(0, sub) {
            let page_vec = entry.ensure_mut(expected_size);
            page_vec[offset + pos] = fp;
            Ok(true)
          } else {
            Ok(false)
          }
        } else {
          let page_vec = entry.ensure_mut(expected_size);
          page_vec[offset] = fp;
          Ok(true)
        }
      }
      CuckooPageData::Dirty(v) => {
        if offset + bs > v.len() {
          v.resize(offset + bs, 0);
        }
        if let Some(pos) = memchr(0, &v[offset..offset + bs]) {
          v[offset + pos] = fp;
          Ok(true)
        } else {
          Ok(false)
        }
      }
    }
  }

  #[inline]
  fn try_insert_in_bucket_recorded(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    fp: u8,
    undo_log: &mut Vec<PageSlotUndo>,
  ) -> Result<bool> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let bs = self.bucket_size as usize;
    let expected_size = self.get_expected_page_size(num_buckets, page_idx);
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;

    match entry {
      CuckooPageData::Empty(_) => {
        let page_vec = entry.ensure_mut(expected_size);
        let byte_offset = offset;
        undo_log.push(PageSlotUndo {
          filter_idx,
          page_idx,
          byte_offset,
          old_fp: 0,
        });
        page_vec[byte_offset] = fp;
        Ok(true)
      }
      CuckooPageData::Clean(s) => {
        if let Some(sub) = s.get(offset..offset + bs) {
          if let Some(pos) = memchr(0, sub) {
            let page_vec = entry.ensure_mut(expected_size);
            let byte_offset = offset + pos;
            undo_log.push(PageSlotUndo {
              filter_idx,
              page_idx,
              byte_offset,
              old_fp: 0,
            });
            page_vec[byte_offset] = fp;
            Ok(true)
          } else {
            Ok(false)
          }
        } else {
          let page_vec = entry.ensure_mut(expected_size);
          let byte_offset = offset;
          undo_log.push(PageSlotUndo {
            filter_idx,
            page_idx,
            byte_offset,
            old_fp: 0,
          });
          page_vec[byte_offset] = fp;
          Ok(true)
        }
      }
      CuckooPageData::Dirty(v) => {
        if offset + bs > v.len() {
          v.resize(offset + bs, 0);
        }
        if let Some(pos) = memchr(0, &v[offset..offset + bs]) {
          let byte_offset = offset + pos;
          undo_log.push(PageSlotUndo {
            filter_idx,
            page_idx,
            byte_offset,
            old_fp: 0,
          });
          v[byte_offset] = fp;
          Ok(true)
        } else {
          Ok(false)
        }
      }
    }
  }

  #[inline]
  fn get_bucket_slot(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    slot: usize,
  ) -> Result<u8> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;
    Ok(entry.get_byte(offset + slot))
  }

  #[inline]
  fn set_bucket_slot_recorded(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    slot: usize,
    fp: u8,
    undo_log: &mut Vec<PageSlotUndo>,
  ) -> Result<()> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let expected_size = self.get_expected_page_size(num_buckets, page_idx);
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;
    let byte_offset = offset + slot;
    let old_fp = entry.get_byte(byte_offset);
    let page_vec = entry.ensure_mut(expected_size);
    undo_log.push(PageSlotUndo {
      filter_idx,
      page_idx,
      byte_offset,
      old_fp,
    });
    page_vec[byte_offset] = fp;
    Ok(())
  }

  #[inline]
  fn apply_undo_log(&mut self, undo_log: &[PageSlotUndo]) {
    for undo in undo_log.iter().rev() {
      if let Some(CuckooPageData::Dirty(vec)) =
        self.pages.get_mut(&(undo.filter_idx, undo.page_idx))
        && undo.byte_offset < vec.len()
      {
        vec[undo.byte_offset] = undo.old_fp;
      }
    }
  }

  #[inline]
  fn contains_in_bucket(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    fp: u8,
  ) -> Result<bool> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let bs = self.bucket_size as usize;
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;
    if let Some(sub) = entry.get_subslice(offset, bs) {
      Ok(memchr(fp, sub).is_some())
    } else {
      Ok(false)
    }
  }

  #[inline]
  fn count_in_bucket(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    fp: u8,
  ) -> Result<usize> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let bs = self.bucket_size as usize;
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;
    if let Some(mut slice) = entry.get_subslice(offset, bs) {
      let mut count = 0;
      while let Some(pos) = memchr(fp, slice) {
        count += 1;
        slice = &slice[pos + 1..];
      }
      Ok(count)
    } else {
      Ok(0)
    }
  }

  #[inline]
  fn delete_from_bucket(
    &mut self,
    filter_idx: u16,
    num_buckets: u32,
    bucket_idx: u32,
    fp: u8,
  ) -> Result<bool> {
    let (page_idx, offset) = self.get_bucket_location(bucket_idx);
    let bs = self.bucket_size as usize;
    let expected_size = self.get_expected_page_size(num_buckets, page_idx);
    let entry = self.get_page_entry_mut(filter_idx, num_buckets, page_idx)?;

    match entry {
      CuckooPageData::Empty(_) => Ok(false),
      CuckooPageData::Clean(s) => {
        if let Some(sub) = s.get(offset..offset + bs) {
          if let Some(pos) = memchr(fp, sub) {
            let page_vec = entry.ensure_mut(expected_size);
            page_vec[offset + pos] = 0;
            Ok(true)
          } else {
            Ok(false)
          }
        } else {
          Ok(false)
        }
      }
      CuckooPageData::Dirty(v) => {
        if let Some(sub) = v.get(offset..offset + bs) {
          if let Some(pos) = memchr(fp, sub) {
            v[offset + pos] = 0;
            Ok(true)
          } else {
            Ok(false)
          }
        } else {
          Ok(false)
        }
      }
    }
  }

  fn commit_dirty_to_batch<E: Engine>(&self, batch: &mut DbBatch<E>) {
    for (&(filter_idx, page_idx), page_data) in &self.pages {
      if let CuckooPageData::Dirty(vec) = page_data {
        let page_key = key::cuckoo_page(&self.kc, self.key, filter_idx, page_idx);
        batch.insert_data(&page_key, vec.as_slice());
      }
    }
  }
}

/// Cuckoo kick-out insertion algorithm with undo log rollback (aligned with Kvrocks CuckooSubFilter::TryKickOutInsert).
/// 布谷鸟踢出算法（对标 Apache Kvrocks CuckooSubFilter::TryKickOutInsert，带零拷贝撤销日志回滚）
fn try_cuckoo_kickout<P: Partition>(
  page_cache: &mut CuckooPageCache<'_, P>,
  filter_idx: u16,
  num_buckets: u32,
  bucket_size: u8,
  max_iterations: u16,
  hash: u64,
  fp: u8,
) -> Result<bool>
where
  Error: From<P::Error>,
{
  let mut undo_log = Vec::with_capacity(max_iterations as usize * 2);
  let mut cur_i = (hash as u32) & (num_buckets - 1);
  let mut cur_fp = fp;
  let mut victim_slot = fastrand::usize(..bucket_size as usize);

  for _ in 0..max_iterations {
    let old_fp = match page_cache.get_bucket_slot(filter_idx, num_buckets, cur_i, victim_slot) {
      Ok(v) => v,
      Err(e) => {
        page_cache.apply_undo_log(&undo_log);
        return Err(e);
      }
    };
    if let Err(e) = page_cache.set_bucket_slot_recorded(
      filter_idx,
      num_buckets,
      cur_i,
      victim_slot,
      cur_fp,
      &mut undo_log,
    ) {
      page_cache.apply_undo_log(&undo_log);
      return Err(e);
    }
    cur_fp = old_fp;

    if cur_fp == 0 {
      return Ok(true);
    }

    let alt_bucket_idx = CuckooFilterHelper::get_alt_bucket_index(cur_i, cur_fp, num_buckets);
    let inserted_in_alt = match page_cache.try_insert_in_bucket_recorded(
      filter_idx,
      num_buckets,
      alt_bucket_idx,
      cur_fp,
      &mut undo_log,
    ) {
      Ok(v) => v,
      Err(e) => {
        page_cache.apply_undo_log(&undo_log);
        return Err(e);
      }
    };
    if inserted_in_alt {
      return Ok(true);
    }

    cur_i = alt_bucket_idx;
    victim_slot = fastrand::usize(..bucket_size as usize);
  }

  page_cache.apply_undo_log(&undo_log);
  Ok(false)
}

/// Internal helper validating and retrieving Bloom filter metadata with type and TTL checks.
/// 内部辅助：校验并获取 Bloom 元数据（含 WRONGTYPE 与 TTL 状态判定）
#[inline]
fn get_bf_meta_checked<E: Engine>(
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

/// Internal helper validating and retrieving Cuckoo filter metadata with type and TTL checks.
/// 内部辅助：校验并获取 Cuckoo 元数据（含 WRONGTYPE 与 TTL 状态判定）
#[inline]
fn get_cf_meta_checked<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
) -> Result<Option<CuckooChainMeta>>
where
  Error: From<E::Error>,
{
  get_meta_checked::<CuckooChainMeta, _>(db, k_bytes, meta_k, now_ms)
}

/// Bloom and Cuckoo filter operations interface (Bloom / Cuckoo Filters).
/// 布隆与布谷鸟过滤器结构操作接口 (Bloom / Cuckoo Filters)
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
    let _meta_ks = self.meta();
    let _data_ks = self.data();

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
    let _kc = self.kc();
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
    let _kc = self.kc();
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
    let _meta_ks = self.meta();
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
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::bloom_meta(&kc, key_bytes);
    let _meta_ks = self.meta();
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
    let _meta_ks = self.meta();

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

  #[inline]
  pub fn cf_reserve_one<K: AsRef<[u8]>>(&self, key: K, capacity: u64) -> Result<()> {
    self.cf_reserve(key, capacity, [])
  }

  #[inline]
  pub fn cf_reserve<K: AsRef<[u8]>>(
    &self,
    key: K,
    capacity: u64,
    opt_li: impl IntoIterator<Item = CfReserve>,
  ) -> Result<()> {
    let mut bucket_size = DEFAULT_CF_BUCKET_SIZE;
    let mut max_iterations = DEFAULT_CF_MAX_ITERATIONS;
    let mut expansion = DEFAULT_CF_EXPANSION;
    let mut page_size = None;
    for opt in opt_li {
      match opt {
        CfReserve::BucketSize(bs) => bucket_size = bs,
        CfReserve::MaxIterations(mi) => max_iterations = mi,
        CfReserve::Expansion(exp) => expansion = exp,
        CfReserve::PageSize(ps) => page_size = Some(ps),
      }
    }
    self.raw_cf_reserve(
      key,
      capacity,
      bucket_size,
      max_iterations,
      expansion,
      page_size,
    )
  }

  #[inline]
  pub(crate) fn raw_cf_reserve<K: AsRef<[u8]>>(
    &self,
    key: K,
    capacity: u64,
    bucket_size: u8,
    max_iterations: u16,
    expansion: u16,
    page_size: Option<u32>,
  ) -> Result<()> {
    let kc = self.kc();
    if capacity == 0 {
      return Err(Error::invalid_data("capacity must be larger than 0"));
    }
    if capacity < 2 {
      return Err(Error::invalid_data("capacity must be at least 2"));
    }
    if bucket_size == 0 {
      return Err(Error::invalid_data("bucket_size must be between 1 and 255"));
    }
    if max_iterations == 0 {
      return Err(Error::invalid_data("max_iterations must be larger than 0"));
    }
    let actual_page_size = page_size.unwrap_or(CuckooFilterHelper::DEFAULT_PAGE_SIZE);
    if actual_page_size == 0 {
      return Err(Error::invalid_data("page_size must be larger than 0"));
    }
    if actual_page_size < bucket_size as u32 {
      return Err(Error::invalid_data(
        "page_size must be at least bucket_size",
      ));
    }
    if expansion > CuckooFilterHelper::MAX_EXPANSION {
      return Err(Error::invalid_data("expansion must be between 0 and 32768"));
    }

    // 验证容量是否超出限制
    CuckooFilterHelper::calculate_required_buckets(capacity, bucket_size)?;

    let key_bytes = key.as_ref();
    let normalized_expansion = CuckooFilterHelper::normalize_expansion(expansion);
    let meta_k = key::cuckoo_meta(&kc, key_bytes);
    let meta_ks = self.meta();

    let now_ms = current_now_ms();
    if get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)?.is_some() {
      return Err(Error::invalid_data("ERR item exists"));
    }

    let meta = CuckooChainMeta::new(
      capacity,
      bucket_size,
      max_iterations,
      normalized_expansion,
      actual_page_size,
      0,
      0,
    );

    meta_ks.insert(&meta_k, &meta.encode())?;
    Ok(())
  }

  #[inline]
  pub fn cf_add<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    let _kc = self.kc();
    let res = self.cf_insert(key, &[item], [])?;
    Ok(res.first().copied().unwrap_or(false))
  }

  #[inline]
  pub fn cf_addnx<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    self.cf_insertnx_one(key, item, [])
  }

  #[inline]
  pub fn cf_insert_one<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    item: I,
    opt_li: impl IntoIterator<Item = CfInsert>,
  ) -> Result<bool> {
    let res = self.cf_insert(key, &[item], opt_li)?;
    Ok(res.first().copied().unwrap_or(false))
  }

  #[inline]
  pub fn cf_insertnx_one<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    item: I,
    opt_li: impl IntoIterator<Item = CfInsert>,
  ) -> Result<bool> {
    let res = self.cf_insertnx(key, &[item], opt_li)?;
    Ok(res.first().copied().unwrap_or(false))
  }

  #[inline]
  pub fn cf_insertnx<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
    opt_li: impl IntoIterator<Item = CfInsert>,
  ) -> Result<Vec<bool>> {
    let mut opt: CuckooFilterInsert = opt_li.into_iter().collect();
    opt.nx = true;
    self.cf_insert_internal(key, items, &opt)
  }

  #[inline]
  pub fn cf_insert<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
    opt_li: impl IntoIterator<Item = CfInsert>,
  ) -> Result<Vec<bool>> {
    let opt: CuckooFilterInsert = opt_li.into_iter().collect();
    self.cf_insert_internal(key, items, &opt)
  }

  #[inline]
  pub(crate) fn cf_insert_internal<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
    opt: &CuckooFilterInsert,
  ) -> Result<Vec<bool>> {
    let kc = self.kc();
    if items.is_empty() {
      return Ok(Vec::new());
    }

    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);
    let _meta_ks = self.meta();
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let mut meta = match get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => {
        if !opt.auto_create {
          return Err(Error::invalid_data("ERR not found"));
        }
        if opt.capacity < 2 {
          return Err(Error::invalid_data("capacity must be at least 2"));
        }
        if opt.bucket_size == 0 {
          return Err(Error::invalid_data("bucket_size must be between 1 and 255"));
        }
        if opt.max_iterations == 0 {
          return Err(Error::invalid_data("max_iterations must be larger than 0"));
        }
        if opt.page_size == 0 {
          return Err(Error::invalid_data("page_size must be larger than 0"));
        }
        if opt.page_size < opt.bucket_size as u32 {
          return Err(Error::invalid_data(
            "page_size must be at least bucket_size",
          ));
        }
        if opt.expansion > CuckooFilterHelper::MAX_EXPANSION {
          return Err(Error::invalid_data("expansion must be between 0 and 32768"));
        }
        CuckooFilterHelper::calculate_required_buckets(opt.capacity, opt.bucket_size)?;

        CuckooChainMeta::new(
          opt.capacity,
          opt.bucket_size,
          opt.max_iterations,
          opt.expansion,
          opt.page_size,
          0,
          0,
        )
      }
    };

    let mut page_cache =
      CuckooPageCache::new(data_ks, kc, key_bytes, meta.bucket_size, meta.page_size);
    let mut results = Vec::with_capacity(items.len());

    for item in items {
      let h = CuckooFilterHelper::hash(item.as_ref());
      let fp = CuckooFilterHelper::generate_fingerprint(h);

      if opt.nx {
        let mut exists = false;
        for filter_idx in (0..meta.n_filters).rev() {
          let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
          let b1 = (h % (num_buckets as u64)) as u32;
          let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);
          if page_cache.contains_in_bucket(filter_idx, num_buckets, b1, fp)?
            || (b1 != b2 && page_cache.contains_in_bucket(filter_idx, num_buckets, b2, fp)?)
          {
            exists = true;
            break;
          }
        }
        if exists {
          results.push(false);
          continue;
        }
      }

      let mut inserted = false;

      // 1. 优先尝试从最新子过滤器向下直接插入空槽
      for filter_idx in (0..meta.n_filters).rev() {
        let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
        let b1 = (h % (num_buckets as u64)) as u32;
        let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);

        if page_cache.try_insert_in_bucket(filter_idx, num_buckets, b1, fp)? {
          inserted = true;
          break;
        }
        if b1 != b2 && page_cache.try_insert_in_bucket(filter_idx, num_buckets, b2, fp)? {
          inserted = true;
          break;
        }
      }

      // 2. 所有过滤器均无空槽，在最后一个过滤器上执行踢出算法
      if !inserted {
        let last_filter_idx = meta.n_filters - 1;
        let num_buckets = meta.sub_filter_num_buckets(last_filter_idx)?;
        inserted = try_cuckoo_kickout(
          &mut page_cache,
          last_filter_idx,
          num_buckets,
          meta.bucket_size,
          meta.max_iterations,
          h,
          fp,
        )?;
      }

      // 3. 踢出失败且支持扩容，分配新过滤器
      if !inserted && meta.is_scaling() && meta.n_filters < u16::MAX {
        let new_filter_idx = meta.n_filters;
        if let Ok(new_buckets) = meta.sub_filter_num_buckets(new_filter_idx) {
          let b1 = (h % (new_buckets as u64)) as u32;
          let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, new_buckets);
          let ok = page_cache.try_insert_in_bucket(new_filter_idx, new_buckets, b1, fp)?
            || (b1 != b2
              && page_cache.try_insert_in_bucket(new_filter_idx, new_buckets, b2, fp)?);
          if ok {
            meta.n_filters += 1;
            inserted = true;
          }
        }
      }

      if !inserted {
        return Err(Error::invalid_data("ERR filter is full"));
      }

      meta.base.size += 1;
      results.push(true);
    }

    let mut batch = self.batch_with_capacity(page_cache.pages.len() + 1);
    batch.insert_meta(&meta_k, &meta.encode());
    page_cache.commit_dirty_to_batch(&mut batch);
    batch.commit()?;

    Ok(results)
  }

  #[inline]
  pub fn cf_exists<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let meta = match get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(false),
    };

    let h = CuckooFilterHelper::hash(item.as_ref());
    let fp = CuckooFilterHelper::generate_fingerprint(h);
    let bs = meta.bucket_size as u32;
    let buckets_per_page = (meta.page_size / bs).max(1);

    for filter_idx in (0..meta.n_filters).rev() {
      let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
      let b1 = (h as u32) & (num_buckets - 1);
      let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);

      let page_idx1 = b1 / buckets_per_page;
      let off1 = ((b1 % buckets_per_page) as usize) * (meta.bucket_size as usize);
      let page_key1 = key::cuckoo_page(&kc, key_bytes, filter_idx, page_idx1);
      let slice1 = data_ks.get(&page_key1)?;

      if let Some(ref slice) = slice1
        && let Some(sub) = slice.get(off1..off1 + meta.bucket_size as usize)
        && memchr(fp, sub).is_some()
      {
        return Ok(true);
      }

      if b1 != b2 {
        let page_idx2 = b2 / buckets_per_page;
        let off2 = ((b2 % buckets_per_page) as usize) * (meta.bucket_size as usize);
        if page_idx2 == page_idx1 {
          if let Some(ref slice) = slice1
            && let Some(sub) = slice.get(off2..off2 + meta.bucket_size as usize)
            && memchr(fp, sub).is_some()
          {
            return Ok(true);
          }
        } else {
          let page_key2 = key::cuckoo_page(&kc, key_bytes, filter_idx, page_idx2);
          if let Some(slice2) = data_ks.get(&page_key2)?
            && let Some(sub) = slice2.get(off2..off2 + meta.bucket_size as usize)
            && memchr(fp, sub).is_some()
          {
            return Ok(true);
          }
        }
      }
    }

    Ok(false)
  }

  #[inline]
  pub fn cf_mexists<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
  ) -> Result<Vec<bool>> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let meta = match get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(vec![false; items.len()]),
    };

    let mut page_cache =
      CuckooPageCache::new(data_ks, kc, key_bytes, meta.bucket_size, meta.page_size);
    let mut results = Vec::with_capacity(items.len());

    for item in items {
      let h = CuckooFilterHelper::hash(item.as_ref());
      let fp = CuckooFilterHelper::generate_fingerprint(h);
      let mut exists = false;

      for filter_idx in (0..meta.n_filters).rev() {
        let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
        let b1 = (h % (num_buckets as u64)) as u32;
        let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);

        if page_cache.contains_in_bucket(filter_idx, num_buckets, b1, fp)?
          || (b1 != b2 && page_cache.contains_in_bucket(filter_idx, num_buckets, b2, fp)?)
        {
          exists = true;
          break;
        }
      }

      results.push(exists);
    }

    Ok(results)
  }

  #[inline]
  pub fn cf_del<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);
    let _meta_ks = self.meta();
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let mut meta = match get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(false),
    };

    let h = CuckooFilterHelper::hash(item.as_ref());
    let fp = CuckooFilterHelper::generate_fingerprint(h);
    let mut page_cache =
      CuckooPageCache::new(data_ks, kc, key_bytes, meta.bucket_size, meta.page_size);

    let mut deleted = false;
    for filter_idx in (0..meta.n_filters).rev() {
      let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
      let b1 = (h % (num_buckets as u64)) as u32;
      let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);

      if page_cache.delete_from_bucket(filter_idx, num_buckets, b1, fp)? {
        deleted = true;
        break;
      }
      if b1 != b2 && page_cache.delete_from_bucket(filter_idx, num_buckets, b2, fp)? {
        deleted = true;
        break;
      }
    }

    if deleted {
      meta.base.size = meta.base.size.saturating_sub(1);
      meta.num_deleted_items = meta.num_deleted_items.saturating_add(1);

      let mut batch = self.batch();
      batch.insert_meta(&meta_k, &meta.encode());
      page_cache.commit_dirty_to_batch(&mut batch);
      batch.commit()?;
      Ok(true)
    } else {
      Ok(false)
    }
  }

  #[inline]
  pub fn cf_count<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<u64> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);
    let _meta_ks = self.meta();
    let data_ks = self.data();

    let now_ms = current_now_ms();
    let meta = match get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)? {
      Some(m) => m,
      None => return Ok(0),
    };

    let h = CuckooFilterHelper::hash(item.as_ref());
    let fp = CuckooFilterHelper::generate_fingerprint(h);
    let mut page_cache =
      CuckooPageCache::new(data_ks, kc, key_bytes, meta.bucket_size, meta.page_size);

    let mut total_count = 0u64;
    for filter_idx in 0..meta.n_filters {
      let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
      let b1 = (h % (num_buckets as u64)) as u32;
      let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);

      total_count += page_cache.count_in_bucket(filter_idx, num_buckets, b1, fp)? as u64;
      if b1 != b2 {
        total_count += page_cache.count_in_bucket(filter_idx, num_buckets, b2, fp)? as u64;
      }
    }

    Ok(total_count)
  }

  #[inline]
  pub fn cf_info<K: AsRef<[u8]>>(&self, key: K) -> Result<CuckooFilterInfo> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);

    let now_ms = current_now_ms();
    let meta = get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)?
      .ok_or_else(|| Error::invalid_data("ERR not found"))?;

    let mut total_buckets = 0u64;
    for i in 0..meta.n_filters {
      if let Ok(nb) = meta.sub_filter_num_buckets(i) {
        total_buckets += nb as u64;
      }
    }

    Ok(CuckooFilterInfo {
      size: meta.base.size,
      num_buckets: total_buckets,
      num_filters: meta.n_filters,
      num_items_inserted: meta.base.size + meta.num_deleted_items,
      num_items_deleted: meta.num_deleted_items,
      bucket_size: meta.bucket_size,
      expansion: meta.expansion,
      max_iterations: meta.max_iterations,
    })
  }

  /// Returns the current number of items stored in the Cuckoo filter aligned with Kvrocks CuckooChain::Card.
  /// 获取布谷鸟过滤器中当前存储的元素基数（对标 Kvrocks CuckooChain::Card）
  #[inline]
  pub fn cf_card<K: AsRef<[u8]>>(&self, key: K) -> Result<u64> {
    let kc = self.kc();
    let key_bytes = key.as_ref();
    let meta_k = key::cuckoo_meta(&kc, key_bytes);

    let now_ms = current_now_ms();
    Ok(
      get_cf_meta_checked(self, key_bytes, &meta_k, now_ms)?
        .map(|m| m.base.size)
        .unwrap_or(0),
    )
  }
}
