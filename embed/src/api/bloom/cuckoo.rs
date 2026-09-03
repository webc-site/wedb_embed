use core::hint::unreachable_unchecked;
use std::{collections::hash_map::Entry, iter::once};

use memchr::memchr;
use rapidhash::RapidHashMap as HashMap;

use crate::{
  api::bloom::{
    r#const::{DEFAULT_CF_BUCKET_SIZE, DEFAULT_CF_EXPANSION, DEFAULT_CF_MAX_ITERATIONS},
    key,
    meta::CuckooChainMeta,
    opt::{CfInsert, CfReserve, CuckooFilterHelper, CuckooFilterInfo, CuckooFilterInsert},
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  key_composer::KeyComposer,
  meta::current_now_ms,
  wedb::{Db, DbBatch},
};

/// Cuckoo kickout step rollback entry.
/// 布谷鸟踢出单步修改回滚单元
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageSlotUndo {
  filter_idx: u16,
  page_idx: u32,
  byte_offset: usize,
  old_fp: u8,
}

/// Cuckoo filter page data container with zero-copy read and copy-on-write.
/// 布谷鸟分页数据单元（支持只读零拷贝与按需写时克隆）
pub(crate) enum CuckooPageData {
  Clean(Box<[u8]>),
  Dirty(Vec<u8>),
  Empty(usize),
}

impl CuckooPageData {
  #[inline]
  pub(crate) fn get_subslice(&self, offset: usize, len: usize) -> Option<&[u8]> {
    match self {
      Self::Clean(s) => s.get(offset..offset + len),
      Self::Dirty(v) => v.get(offset..offset + len),
      Self::Empty(_) => None,
    }
  }

  #[inline]
  pub(crate) fn get_byte(&self, offset: usize) -> u8 {
    match self {
      Self::Clean(s) => s.get(offset).copied().unwrap_or(0),
      Self::Dirty(v) => v.get(offset).copied().unwrap_or(0),
      Self::Empty(_) => 0,
    }
  }

  #[inline]
  pub(crate) fn ensure_mut(&mut self, expected_size: usize) -> &mut Vec<u8> {
    match self {
      Self::Dirty(v) => {
        if v.len() < expected_size {
          v.resize(expected_size, 0);
        }
      }
      Self::Clean(s) => {
        let mut vec = s.to_vec();
        if vec.len() < expected_size {
          vec.resize(expected_size, 0);
        }
        *self = Self::Dirty(vec);
      }
      Self::Empty(exp) => {
        let size = (*exp).max(expected_size);
        *self = Self::Dirty(vec![0u8; size]);
      }
    }
    match self {
      Self::Dirty(v) => v,
      _ => unsafe { unreachable_unchecked() },
    }
  }
}

/// Cuckoo filter page cache with zero-copy read optimization (aligned with Kvrocks CuckooPageCache).
/// 布谷鸟过滤器分页读写缓存（对标 Kvrocks CuckooPageCache，零拷贝读优化）
pub(crate) struct CuckooPageCache<'a, P: Partition> {
  data: &'a P,
  kc: KeyComposer,
  key: &'a [u8],
  bucket_size: u8,
  buckets_per_page: u32,
  pub(crate) pages: HashMap<(u16, u32), CuckooPageData>,
}

impl<'a, P: Partition> CuckooPageCache<'a, P>
where
  Error: From<P::Error>,
{
  #[inline]
  pub(crate) fn new(
    data: &'a P,
    kc: KeyComposer,
    key: &'a [u8],
    bucket_size: u8,
    page_size: u32,
  ) -> Self {
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
  pub(crate) fn get_bucket_location(&self, bucket_idx: u32) -> (u32, usize) {
    let page_idx = bucket_idx / self.buckets_per_page;
    let offset = ((bucket_idx % self.buckets_per_page) as usize) * (self.bucket_size as usize);
    (page_idx, offset)
  }

  #[inline]
  pub(crate) fn get_expected_page_size(&self, num_buckets: u32, page_idx: u32) -> usize {
    let first_bucket = page_idx * self.buckets_per_page;
    let page_bucket_count = self
      .buckets_per_page
      .min(num_buckets.saturating_sub(first_bucket));
    (page_bucket_count as usize) * (self.bucket_size as usize)
  }

  pub(crate) fn get_page_entry_mut(
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
  pub(crate) fn try_insert_in_bucket(
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
  pub(crate) fn try_insert_in_bucket_recorded(
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
  pub(crate) fn get_bucket_slot(
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
  pub(crate) fn set_bucket_slot_recorded(
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
  pub(crate) fn apply_undo_log(&mut self, undo_log: &[PageSlotUndo]) {
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
  pub(crate) fn contains_in_bucket(
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
  pub(crate) fn count_in_bucket(
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
  pub(crate) fn delete_from_bucket(
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

  pub(crate) fn commit_dirty_to_batch<E: Engine>(&self, batch: &mut DbBatch<E>) {
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
pub(crate) fn try_cuckoo_kickout<P: Partition>(
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

/// Internal helper validating and retrieving Cuckoo filter metadata with type and TTL checks.
/// 内部辅助：校验并获取 Cuckoo 元数据（含 WRONGTYPE 与 TTL 状态判定）
#[inline]
pub(crate) fn get_cf_meta_checked<E: Engine>(
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

/// Cuckoo filter operations interface.
/// 布谷鸟过滤器结构操作接口 (Cuckoo Filter)
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
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
    let res = self.cf_insert(key, &[item], [])?;
    Ok(res.first().copied().unwrap_or(true))
  }

  #[inline]
  pub fn cf_addnx<K: AsRef<[u8]>, I: AsRef<[u8]>>(&self, key: K, item: I) -> Result<bool> {
    let res = self.cf_insert(key, &[item], [CfInsert::Nx])?;
    Ok(res.first().copied().unwrap_or(false))
  }

  #[inline]
  pub fn cf_insert_one<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    item: I,
    opt_li: impl IntoIterator<Item = CfInsert>,
  ) -> Result<bool> {
    let res = self.cf_insert(key, &[item], opt_li)?;
    Ok(res.first().copied().unwrap_or(true))
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
    self.cf_insert(key, items, opt_li.into_iter().chain(once(CfInsert::Nx)))
  }

  #[inline]
  pub fn cf_insert<K: AsRef<[u8]>, I: AsRef<[u8]>>(
    &self,
    key: K,
    items: &[I],
    opt_li: impl IntoIterator<Item = CfInsert>,
  ) -> Result<Vec<bool>> {
    let opt: CuckooFilterInsert = opt_li.into_iter().collect();
    self.raw_cf_insert(key, items, &opt)
  }

  #[inline]
  pub(crate) fn raw_cf_insert<K: AsRef<[u8]>, I: AsRef<[u8]>>(
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
        if opt.capacity == 0 {
          return Err(Error::invalid_data("capacity must be larger than 0"));
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
          let b1 = (h as u32) & (num_buckets - 1);
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
        let b1 = (h as u32) & (num_buckets - 1);
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
          let b1 = (h as u32) & (new_buckets - 1);
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
        let b1 = (h as u32) & (num_buckets - 1);
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
      let b1 = (h as u32) & (num_buckets - 1);
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

    let mut total = 0;
    for filter_idx in (0..meta.n_filters).rev() {
      let num_buckets = meta.sub_filter_num_buckets(filter_idx)?;
      let b1 = (h as u32) & (num_buckets - 1);
      let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);

      total += page_cache.count_in_bucket(filter_idx, num_buckets, b1, fp)?;
      if b1 != b2 {
        total += page_cache.count_in_bucket(filter_idx, num_buckets, b2, fp)?;
      }
    }

    Ok(total as u64)
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
