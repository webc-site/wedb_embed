use std::{fmt, ops::Bound, sync::Arc};

use parking_lot::Mutex;

use crate::{
  Db,
  api::key::{DBScanInfo, cleanup_composite_data_raw},
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key_composer::{
    CATALOG_PREFIX, KeyComposer, KeyTag, NS_NEXT_ID_KEY, decode_oppv_u64,
    encode_catalog_db_key_fixed, encode_catalog_ns_prefix_fixed, encode_db_next_id_key_fixed,
    encode_oppv_u64_fixed,
  },
  meta::{KeyMeta, current_now_ms, init_version_counter},
  string::{decode_string_value, is_string_expired},
  wedb::{DbBatch, IntoOptId, Namespace, Namespaces},
};

/// Default data partition name.
/// 默认数据分区名称。
pub const DATA: &str = "data";

/// Default metadata partition name.
/// 默认元数据分区名称。
pub const META: &str = "meta";

/// Scans and queues deletion of all keys starting with the given prefix in the partition.
/// 扫描指定分区中所有匹配前缀的键并排队批量删除。
#[inline]
pub fn clear_ks_prefix<E: Engine>(
  partition: &E::Partition,
  prefix: &[u8],
  batch: &mut DbBatch<E>,
  count: &mut u64,
) -> Result<()>
where
  Error: From<E::Error>,
{
  for item in partition.prefix(prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(prefix) {
      break;
    }
    batch.rm(partition, k);
    *count += 1;
  }
  Ok(())
}

#[inline]
fn sweep_ks<P: Partition, F>(
  ks: &P,
  cursor: &mut Option<Vec<u8>>,
  sample_limit: usize,
  mut on_entry: F,
) -> Result<()>
where
  Error: From<P::Error>,
  F: FnMut(&[u8], &[u8]) -> Result<()>,
{
  let mut count = 0;
  let mut last_key = None;

  let start_bound = cursor.as_deref().map_or(Bound::Unbounded, Bound::Excluded);

  for guard in ks.range((start_bound, Bound::Unbounded)).take(sample_limit) {
    let entry = guard?;
    on_entry(entry.key(), entry.value())?;
    count += 1;
    if count == sample_limit {
      last_key = Some(entry.key().to_vec());
    }
  }

  *cursor = last_key;

  Ok(())
}

#[derive(Default)]
pub struct ExpireCursors {
  pub(crate) data_cursor: Option<Vec<u8>>,
  pub(crate) meta_cursor: Option<Vec<u8>>,
}

/// Shared internal state for WeDb, Namespace, and Db handles.
/// WeDb、Namespace 与 Db 句柄共享的底层核心存储上下文（单 Arc 封装，消除冗余原子操作与散落堆内存分配）。
pub(crate) struct WeDbInner<E: Engine> {
  pub(crate) engine: Arc<E>,
  pub(crate) data: E::Partition,
  pub(crate) meta: E::Partition,
  pub(crate) ns_lock: Mutex<()>,
  pub(crate) expire_cursor: Mutex<ExpireCursors>,
  pub(crate) db_scan_infos: parking_lot::RwLock<rapidhash::RapidHashMap<(u64, u64), DBScanInfo>>,
}

/// Activate and persist a database in the catalog directory
/// 在 Catalog 紧凑目录中持久化激活指定命名空间的数据库
#[inline]
pub(crate) fn activate_db_impl<E: Engine>(meta: &E::Partition, ns_id: u64, db_id: u64) -> Result<()>
where
  Error: From<E::Error>,
{
  let mut buf = [0u8; 20];
  let len = encode_catalog_db_key_fixed(ns_id, db_id, &mut buf);
  meta.insert(&buf[..len], b"")?;
  Ok(())
}

/// Allocate next global auto-increment namespace ID
/// 分配下一个全局递增命名空间 ID（从 1 开始自动递增）
#[inline]
pub(crate) fn next_namespace_id_impl<E: Engine>(
  meta: &E::Partition,
  lock: &Mutex<()>,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let _guard = lock.lock();
  let current_id = meta
    .get(NS_NEXT_ID_KEY)?
    .and_then(|val| decode_oppv_u64(&val))
    .map(|(v, _)| v)
    .unwrap_or(1);
  let new_id = current_id;
  let next_val = current_id + 1;

  let mut next_val_buf = [0u8; 9];
  let next_len = encode_oppv_u64_fixed(next_val, &mut next_val_buf);
  meta.insert(NS_NEXT_ID_KEY, &next_val_buf[..next_len])?;

  Ok(new_id)
}

/// Allocate next auto-increment DB ID in specified namespace
/// 分配指定命名空间下下一个递增的 DB ID（从 1 开始自动递增，0 为默认 DB）
#[inline]
pub(crate) fn next_db_id_impl<E: Engine>(
  meta: &E::Partition,
  lock: &Mutex<()>,
  ns_id: u64,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let _guard = lock.lock();
  let mut key_buf = [0u8; 12];
  let key_len = encode_db_next_id_key_fixed(ns_id, &mut key_buf);
  let key = &key_buf[..key_len];

  let current_id = meta
    .get(key)?
    .and_then(|val| decode_oppv_u64(&val))
    .map(|(v, _)| v)
    .unwrap_or(1);
  let new_id = current_id;
  let next_val = current_id + 1;

  let mut next_val_buf = [0u8; 9];
  let next_len = encode_oppv_u64_fixed(next_val, &mut next_val_buf);
  meta.insert(key, &next_val_buf[..next_len])?;

  Ok(new_id)
}

/// Removes a database under the specified namespace, cascadingly clearing data, metadata, and catalog entries.
/// 删除指定命名空间下的指定数据库（清理业务数据与元数据，并从 Catalog 目录中注销）
#[inline]
pub(crate) fn db_rm_impl<E: Engine>(
  data: &E::Partition,
  meta: &E::Partition,
  engine: &E,
  ns_id: u64,
  db_id: u64,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let mut count = 0u64;
  let mut batch = DbBatch::<E>::new(data.clone(), meta.clone(), engine.batch());

  let kc = KeyComposer::new(ns_id, db_id);
  let mut prefix_buf = [0u8; 19];
  let prefix_len = kc.encode_scope_prefix_fixed(&mut prefix_buf);
  let prefix = &prefix_buf[..prefix_len];

  // 1. 清理数据分区对应前缀
  clear_ks_prefix(data, prefix, &mut batch, &mut count)?;

  // 2. 清理元数据分区对应前缀
  clear_ks_prefix(meta, prefix, &mut batch, &mut count)?;

  // 3. 从 Catalog 目录中注销该 DB
  let mut cat_key_buf = [0u8; 20];
  let cat_len = encode_catalog_db_key_fixed(ns_id, db_id, &mut cat_key_buf);
  batch.rm_meta(&cat_key_buf[..cat_len]);

  batch.commit()?;
  Ok(count)
}

/// Removes an entire namespace, cascadingly clearing all databases, keys, metadata, counters, and catalog entries.
/// 删除指定命名空间（清理该命名空间下的所有数据库、业务数据、元数据与发号器，并从 Catalog 目录中注销）
#[inline]
pub(crate) fn namespace_rm_impl<E: Engine>(
  data: &E::Partition,
  meta: &E::Partition,
  engine: &E,
  ns_id: u64,
) -> Result<u64>
where
  Error: From<E::Error>,
{
  let mut count = 0u64;
  let mut batch = DbBatch::<E>::new(data.clone(), meta.clone(), engine.batch());

  // 1. 清理该命名空间下的所有数据与元数据
  let mut ns_prefix_buf = [0u8; 10];
  let ns_len = KeyComposer::encode_ns_prefix_fixed(ns_id, &mut ns_prefix_buf);
  let ns_prefix = &ns_prefix_buf[..ns_len];

  clear_ks_prefix(data, ns_prefix, &mut batch, &mut count)?;
  clear_ks_prefix(meta, ns_prefix, &mut batch, &mut count)?;

  // 2. 清理该命名空间下的 Catalog 目录索引（所有 DB 条目）
  let mut cat_prefix_buf = [0u8; 11];
  let cat_len = encode_catalog_ns_prefix_fixed(ns_id, &mut cat_prefix_buf);
  let mut _dummy = 0;
  clear_ks_prefix(meta, &cat_prefix_buf[..cat_len], &mut batch, &mut _dummy)?;

  // 3. 清理该命名空间下的 DB 自增 ID 发号器
  let mut db_id_key_buf = [0u8; 12];
  let id_len = encode_db_next_id_key_fixed(ns_id, &mut db_id_key_buf);
  batch.rm_meta(&db_id_key_buf[..id_len]);

  batch.commit()?;
  Ok(count)
}

/// WeDb multi-tenant and multi-namespace storage manager
/// WeDb 多租户与多命名空间存储管理器
pub struct WeDb<E: Engine> {
  pub(crate) inner: Arc<WeDbInner<E>>,
}

impl<E: Engine> Clone for WeDb<E> {
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
    }
  }
}

impl<E: Engine> fmt::Debug for WeDb<E> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("WeDb").finish()
  }
}

impl<E: Engine> WeDb<E>
where
  Error: From<E::Error>,
{
  /// Create a new WeDb database instance wrapping the storage engine
  /// 包装已打开的存储引擎实例创建 WeDb 数据库管理器（纯内存句柄构造，无异常抛出）
  #[inline]
  pub fn new(engine: E) -> Self {
    init_version_counter();
    let data = engine.partition(DATA).expect("open data partition");
    let meta = engine.partition(META).expect("open meta partition");
    let wedb = Self {
      inner: Arc::new(WeDbInner {
        engine: Arc::new(engine),
        data,
        meta,
        ns_lock: Mutex::new(()),
        expire_cursor: Mutex::new(ExpireCursors::default()),
        db_scan_infos: parking_lot::RwLock::new(rapidhash::RapidHashMap::default()),
      }),
    };
    let _ = activate_db_impl::<E>(&wedb.inner.meta, 0, 0);
    wedb
  }

  /// Get underlying data partition reference
  /// 获取底层业务数据分区引用
  #[inline(always)]
  pub fn data(&self) -> &E::Partition {
    &self.inner.data
  }

  /// Get underlying metadata partition reference
  /// 获取底层元数据分区引用
  #[inline(always)]
  pub fn meta(&self) -> &E::Partition {
    &self.inner.meta
  }

  /// Get underlying storage engine reference
  /// 获取底层存储引擎引用
  #[inline(always)]
  pub fn engine(&self) -> &Arc<E> {
    &self.inner.engine
  }

  /// Create a batch write handle
  /// 创建批量写入句柄
  #[inline(always)]
  pub fn batch(&self) -> DbBatch<E> {
    DbBatch::new(
      self.inner.data.clone(),
      self.inner.meta.clone(),
      self.inner.engine.batch(),
    )
  }

  /// Create a batch write handle with pre-allocated capacity
  /// 创建具有预分配容量槽位的批量写入句柄
  #[inline(always)]
  pub fn batch_with_capacity(&self, capacity: usize) -> DbBatch<E> {
    DbBatch::new(
      self.inner.data.clone(),
      self.inner.meta.clone(),
      self.inner.engine.batch_with_capacity(capacity),
    )
  }

  /// Stream iterate all existing namespaces starting from begin ID
  /// 纯流式迭代所有实际存在的命名空间（支持从指定起始 begin ID 开始遍历）
  #[inline]
  pub fn iter(&self, begin: u64) -> Namespaces<'_, E> {
    let mut buf = [0u8; 11];
    let start_key: &[u8] = if begin == 0 {
      CATALOG_PREFIX
    } else {
      let len = encode_catalog_ns_prefix_fixed(begin, &mut buf);
      &buf[..len]
    };
    let iter = self
      .inner
      .meta
      .range((Bound::Included(start_key), Bound::Unbounded));
    Namespaces {
      wedb: self.clone(),
      iter,
      last_emitted_ns: None,
    }
  }

  /// Open existing namespace by numerical ID, or allocate a new auto-increment namespace if `None` is passed.
  /// 打开指定编号的租户命名空间；若传入 `None`，则自动分配下一个递增自增 ID 并持久化至 Catalog 目录。
  #[inline]
  pub fn ns(&self, id: impl IntoOptId) -> Result<Namespace<E>> {
    let ns_id = match id.into_opt_id() {
      Some(id) => id,
      None => {
        let new_id = next_namespace_id_impl::<E>(&self.inner.meta, &self.inner.ns_lock)?;
        activate_db_impl::<E>(&self.inner.meta, new_id, 0)?;
        new_id
      }
    };
    Ok(Namespace {
      id: ns_id,
      inner: self.inner.clone(),
    })
  }

  /// Open database in the default namespace (ns == 0) by numerical ID, or allocate a new auto-increment database if `None` is passed.
  /// 打开默认命名空间 (ns == 0) 下的指定数据库；若传入 `None` 则自动分配自增 DB ID。
  #[inline]
  pub fn db(&self, id: impl IntoOptId) -> Result<Db<E>> {
    self.ns(0)?.db(id)
  }

  /// Remove and clear all data and metadata across the entire database
  /// 清空并删除数据库中的全部数据与元数据（优先使用底层 LSM 极速物理截断）
  #[inline]
  pub fn rm(&self) -> Result<u64> {
    let count = self.inner.data.approximate_len()? as u64;
    self.inner.data.clear()?;
    self.inner.meta.clear()?;
    Ok(count)
  }

  /// Persist WAL journal and memtables to disk
  /// 将 WAL 日志与内存表持久化落盘
  #[inline]
  pub fn persist(&self) -> Result<()> {
    Ok(self.inner.engine.persist()?)
  }

  /// Trigger major compaction and storage garbage collection
  /// 触发底层全量压缩与空间回收 (Major Compaction / GC)
  #[inline]
  pub fn compact(&self) -> Result<()> {
    self.inner.data.compact()?;
    self.inner.meta.compact()?;
    Ok(self.inner.engine.compact()?)
  }

  /// Get total physical disk space occupied by the entire database in bytes
  /// 获取整个数据库占用的物理磁盘大小 (字节数)
  #[inline]
  pub fn disk_space(&self) -> Result<u64> {
    Ok(self.inner.engine.disk_space()?)
  }

  /// Get current total write buffer memory usage in bytes
  /// 获取当前存储引擎写入缓冲区总内存占用 (字节数)
  #[inline]
  pub fn write_buffer_size(&self) -> u64 {
    self.inner.engine.write_buffer_size()
  }

  /// Get current block cache memory usage in bytes
  /// 获取当前块缓存占用的内存字节数
  #[inline]
  pub fn cache_size(&self) -> u64 {
    self.inner.engine.cache_size()
  }

  /// Get configured block cache capacity in bytes
  /// 获取配置的块缓存容量字节数
  #[inline]
  pub fn cache_capacity(&self) -> u64 {
    self.inner.engine.cache_capacity()
  }

  /// Get number of queued memtable flush tasks
  /// 获取排队等待落盘的 Memtable 刷盘任务数
  #[inline]
  pub fn outstanding_flushes(&self) -> usize {
    self.inner.engine.outstanding_flushes()
  }

  /// Return whether data partition has large-value KV separation enabled
  /// 返回数据分区是否启用了大 Value 键值分离存储
  #[inline]
  pub fn is_kv_separated(&self) -> bool {
    self.inner.data.is_kv_separated()
  }

  /// Return disk space of unreferenced stale blobs in bytes
  /// 返回当前数据库未引用的陈旧 Blob 磁盘占用字节数
  #[inline]
  pub fn fragmented_blob_bytes(&self) -> u64 {
    self.inner.data.fragmented_blob_bytes() + self.inner.meta.fragmented_blob_bytes()
  }

  /// Get total number of SST files across data and meta partitions
  /// 获取当前数据与元数据分区的 SST 文件总数
  #[inline]
  pub fn table_count(&self) -> usize {
    self.inner.data.table_count() + self.inner.meta.table_count()
  }

  /// Get total number of Blob files across data and meta partitions
  /// 获取当前数据与元数据分区的 Blob 文件总数
  #[inline]
  pub fn blob_file_count(&self) -> usize {
    self.inner.data.blob_file_count() + self.inner.meta.blob_file_count()
  }

  /// Get number of WAL journal files on disk
  /// 获取磁盘上的 WAL 日志文件数量
  #[inline]
  pub fn journal_count(&self) -> usize {
    self.inner.engine.journal_count()
  }

  /// Get disk space occupied by WAL journals in bytes
  /// 获取 WAL 日志占用的磁盘字节数
  #[inline]
  pub fn journal_disk_space(&self) -> Result<u64> {
    Ok(self.inner.engine.journal_disk_space()?)
  }

  /// Get number of currently active background compaction tasks
  /// 获取当前正在运行的后台压缩任务数
  #[inline]
  pub fn active_compactions(&self) -> usize {
    self.inner.engine.active_compactions()
  }

  /// Get total number of completed background compaction tasks
  /// 获取已完成的后台压缩任务总数
  #[inline]
  pub fn compactions_completed(&self) -> usize {
    self.inner.engine.compactions_completed()
  }

  /// List all physical partition names in the underlying storage engine
  /// 列出底层存储引擎中的所有物理分区名称
  #[inline]
  pub fn list_partitions(&self) -> Result<Vec<String>> {
    Ok(self.inner.engine.list_partitions()?)
  }

  /// Get approximate total number of entries in storage
  /// 获取当前存储中的近似条目总数
  #[inline]
  pub fn dbsize(&self) -> Result<usize> {
    Ok(self.inner.data.approximate_len()?)
  }

  /// Actively sample and sweep expired keys
  /// 主动采样扫描清理过期键
  pub fn sweep_expired(&self, sample_limit: usize) -> Result<usize> {
    let mut expired_count = 0;
    let now_ms = current_now_ms();
    let mut batch = self.batch();
    let mut buf = Vec::with_capacity(64);

    let (mut data_cur, mut meta_cur) = {
      let guard = self.inner.expire_cursor.lock();
      (guard.data_cursor.clone(), guard.meta_cursor.clone())
    };

    sweep_ks(self.data(), &mut data_cur, sample_limit, |k, v| {
      let (expire_at, _) = decode_string_value(v);
      if is_string_expired(expire_at, now_ms) {
        batch.rm_data(k);
        expired_count += 1;
      }
      Ok(())
    })?;

    sweep_ks(self.meta(), &mut meta_cur, sample_limit, |k, v| {
      if let Some((kc, _, remain)) = KeyComposer::parse_scoped_prefix(k)
        && !remain.is_empty()
      {
        let meta_tag = remain[0];
        let k_bytes = &remain[1..];
        if let Some(tag) = KeyTag::from_u8(meta_tag)
          && tag.is_meta()
          && let Some(base_meta) = KeyMeta::decode(v)
          && base_meta.is_expired(now_ms)
        {
          batch.rm_meta(k);
          cleanup_composite_data_raw(
            self.data(),
            self.meta(),
            &kc,
            meta_tag,
            k_bytes,
            &mut batch,
            &mut buf,
          )?;
          expired_count += 1;
        }
      }
      Ok(())
    })?;

    if expired_count > 0 {
      batch.commit()?;
    }

    {
      let mut guard = self.inner.expire_cursor.lock();
      guard.data_cursor = data_cur;
      guard.meta_cursor = meta_cur;
    }

    Ok(expired_count)
  }
}
