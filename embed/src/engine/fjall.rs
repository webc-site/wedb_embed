use std::{
  fs::create_dir_all, ops::Bound, path::Path, result::Result as StdResult,
  thread::available_parallelism,
};

pub use fjall::{
  CompressionType as FjallCompressionType, Database, DatabaseBuilder, Keyspace,
  KeyspaceCreateOptions as KeyspaceCreateOpt, KvSeparationOptions as KvSeparationOpt,
  PersistMode as FjallPersistMode,
  config::{
    BlockSizePolicy, BloomConstructionPolicy, CompressionPolicy, FilterPolicy, FilterPolicyEntry,
    HashRatioPolicy, PartitioningPolicy, PinningPolicy, RestartIntervalPolicy,
  },
};
use wedb_embed_engine::{Batch, Engine, KvEntry, Partition};

use crate::{
  error::{Error, Result},
  wedb::META,
};

/// Key-value entry wrapper for Fjall items.
/// Fjall 条目包装
pub struct FjallEntry {
  pub key: fjall::Slice,
  pub value: fjall::Slice,
}

impl KvEntry for FjallEntry {
  type Key = fjall::Slice;
  type Value = fjall::Slice;

  #[inline(always)]
  fn key(&self) -> &Self::Key {
    &self.key
  }

  #[inline(always)]
  fn value(&self) -> &Self::Value {
    &self.value
  }
}

/// Bidirectional iterator wrapper for Fjall keyspaces.
/// Fjall 迭代器包装
pub struct FjallIter {
  pub iter: fjall::Iter,
}

impl Iterator for FjallIter {
  type Item = StdResult<FjallEntry, fjall::Error>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    self.iter.next().map(|guard| {
      guard
        .into_inner()
        .map(|(key, value)| FjallEntry { key, value })
    })
  }
}

impl DoubleEndedIterator for FjallIter {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.iter.next_back().map(|guard| {
      guard
        .into_inner()
        .map(|(key, value)| FjallEntry { key, value })
    })
  }
}

/// Partition wrapper for Fjall keyspaces.
/// Fjall Keyspace 分区包装
#[derive(Clone)]
pub struct FjallPartition {
  pub ks: Keyspace,
}

impl Partition for FjallPartition {
  type Error = fjall::Error;
  type Value = fjall::Slice;
  type Entry<'a> = FjallEntry;
  type Iter<'a> = FjallIter;

  #[inline]
  fn get(&self, key: &[u8]) -> StdResult<Option<Self::Value>, Self::Error> {
    self.ks.get(key)
  }

  #[inline]
  fn size_of(&self, key: &[u8]) -> StdResult<Option<usize>, Self::Error> {
    self.ks.size_of(key).map(|opt| opt.map(|s| s as usize))
  }

  #[inline]
  fn contains_key(&self, key: &[u8]) -> StdResult<bool, Self::Error> {
    self.ks.contains_key(key)
  }

  #[inline]
  fn is_empty(&self) -> StdResult<bool, Self::Error> {
    self.ks.is_empty()
  }

  #[inline]
  fn len(&self) -> StdResult<usize, Self::Error> {
    self.ks.len()
  }

  #[inline]
  fn approximate_len(&self) -> StdResult<usize, Self::Error> {
    Ok(self.ks.approximate_len())
  }

  #[inline]
  fn insert(&self, key: &[u8], value: &[u8]) -> StdResult<(), Self::Error> {
    self.ks.insert(key, value)
  }

  #[inline]
  fn rm(&self, key: &[u8]) -> StdResult<(), Self::Error> {
    self.ks.remove(key)
  }

  #[inline]
  fn rm_weak(&self, key: &[u8]) -> StdResult<(), Self::Error> {
    self.ks.remove_weak(key)
  }

  #[inline]
  fn clear(&self) -> StdResult<(), Self::Error> {
    self.ks.clear()
  }

  #[inline]
  fn iter(&self) -> Self::Iter<'_> {
    FjallIter {
      iter: self.ks.iter(),
    }
  }

  #[inline]
  fn prefix(&self, prefix: &[u8]) -> Self::Iter<'_> {
    FjallIter {
      iter: self.ks.prefix(prefix),
    }
  }

  #[inline]
  fn range(&self, range: (Bound<&[u8]>, Bound<&[u8]>)) -> Self::Iter<'_> {
    FjallIter {
      iter: self.ks.range::<&[u8], _>(range),
    }
  }

  #[inline]
  fn first_entry(&self) -> StdResult<Option<Self::Entry<'_>>, Self::Error> {
    self
      .ks
      .first_key_value()
      .map(|guard| {
        guard
          .into_inner()
          .map(|(key, value)| FjallEntry { key, value })
      })
      .transpose()
  }

  #[inline]
  fn last_entry(&self) -> StdResult<Option<Self::Entry<'_>>, Self::Error> {
    self
      .ks
      .last_key_value()
      .map(|guard| {
        guard
          .into_inner()
          .map(|(key, value)| FjallEntry { key, value })
      })
      .transpose()
  }

  #[inline]
  fn is_kv_separated(&self) -> bool {
    self.ks.is_kv_separated()
  }

  #[inline]
  fn fragmented_blob_bytes(&self) -> u64 {
    self.ks.fragmented_blob_bytes()
  }

  #[inline]
  fn disk_space(&self) -> StdResult<u64, Self::Error> {
    Ok(self.ks.disk_space())
  }

  #[inline]
  fn table_count(&self) -> usize {
    self.ks.table_count()
  }

  #[inline]
  fn blob_file_count(&self) -> usize {
    self.ks.blob_file_count()
  }

  #[inline]
  fn compact(&self) -> StdResult<(), Self::Error> {
    self.ks.major_compact()
  }
}

/// Atomic write batch wrapper for Fjall.
/// Fjall 批量写入包装
pub struct FjallBatch {
  pub batch: fjall::OwnedWriteBatch,
}

impl Batch for FjallBatch {
  type Error = fjall::Error;
  type Partition = FjallPartition;

  #[inline]
  fn insert(&mut self, partition: &Self::Partition, key: &[u8], value: &[u8]) {
    self.batch.insert(&partition.ks, key, value);
  }

  #[inline]
  fn insert_batch<I, K, V>(&mut self, partition: &Self::Partition, entries: I)
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
  {
    let ks = &partition.ks;
    for (k, v) in entries {
      self.batch.insert(ks, k.as_ref(), v.as_ref());
    }
  }

  #[inline]
  fn rm(&mut self, partition: &Self::Partition, key: &[u8]) {
    self.batch.remove(&partition.ks, key);
  }

  #[inline]
  fn rm_batch<I, K>(&mut self, partition: &Self::Partition, keys: I)
  where
    I: IntoIterator<Item = K>,
    K: AsRef<[u8]>,
  {
    let ks = &partition.ks;
    for k in keys {
      self.batch.remove(ks, k.as_ref());
    }
  }

  #[inline]
  fn rm_weak(&mut self, partition: &Self::Partition, key: &[u8]) {
    self.batch.remove_weak(&partition.ks, key);
  }

  #[inline]
  fn len(&self) -> usize {
    self.batch.len()
  }

  #[inline]
  fn is_empty(&self) -> bool {
    self.batch.is_empty()
  }

  #[inline]
  fn commit(self) -> StdResult<(), Self::Error> {
    self.batch.commit()
  }
}

/// Fjall LSM-Tree storage engine wrapper.
/// Fjall LSM-Tree 存储引擎封装
#[derive(Clone)]
pub struct Fjall {
  pub db: Database,
  pub data_opts: KeyspaceCreateOpt,
  pub meta_opts: KeyspaceCreateOpt,
}

impl Fjall {
  /// Default optimized Keyspace options for data partitions (KV separation, 8KB blocks, LZ4 compression).
  /// 默认最优化 Data 分区配置（包含 KV 分离存储，8KB 块大小，Lz4 压缩）
  pub fn default_data_partition_options() -> KeyspaceCreateOpt {
    KeyspaceCreateOpt::default()
      .data_block_size_policy(BlockSizePolicy::all(8 * 1024))
      .data_block_compression_policy(CompressionPolicy::all(FjallCompressionType::Lz4))
      .data_block_hash_ratio_policy(HashRatioPolicy::all(0.75))
      .data_block_restart_interval_policy(RestartIntervalPolicy::new([8, 16]))
      .index_block_pinning_policy(PinningPolicy::all(true))
      .filter_block_pinning_policy(PinningPolicy::all(true))
      .expect_point_read_hits(true)
      .max_memtable_size(64 * 1024 * 1024)
      .manual_journal_persist(true)
      .with_kv_separation(Some(
        KvSeparationOpt::default()
          .separation_threshold(4096)
          .compression(FjallCompressionType::Lz4),
      ))
  }

  /// Default optimized Keyspace options for metadata partitions (4KB blocks, 100% in-memory hash indexing, no KV separation overhead).
  /// 默认最优化 Meta 分区配置（4KB 块大小，100% 内存哈希索引，无 KV 分离开销）
  pub fn default_meta_partition_options() -> KeyspaceCreateOpt {
    KeyspaceCreateOpt::default()
      .data_block_size_policy(BlockSizePolicy::all(4 * 1024))
      .data_block_compression_policy(CompressionPolicy::all(FjallCompressionType::Lz4))
      .data_block_hash_ratio_policy(HashRatioPolicy::all(1.0))
      .data_block_restart_interval_policy(RestartIntervalPolicy::new([4, 8]))
      .index_block_pinning_policy(PinningPolicy::all(true))
      .filter_block_pinning_policy(PinningPolicy::all(true))
      .expect_point_read_hits(true)
      .max_memtable_size(32 * 1024 * 1024)
      .manual_journal_persist(true)
      .with_kv_separation(None)
  }

  /// Default optimized DatabaseBuilder configuration.
  /// 默认最优化 Database 构造器
  pub fn default_database_builder(path: impl AsRef<Path>) -> DatabaseBuilder<Database> {
    let worker_threads = available_parallelism()
      .map(usize::from)
      .unwrap_or(2)
      .clamp(2, 8);

    Database::builder(path.as_ref())
      .cache_size(512 * 1024 * 1024) // 512MB 统一块缓存
      .manual_journal_persist(true)   // 开启异步微批刷盘，避免单次写入同步 fsync 阻塞
      .journal_compression(FjallCompressionType::Lz4) // WAL 日志 LZ4 压缩
      .max_journaling_size(512 * 1024 * 1024) // 512MB WAL 最大容量
      .max_cached_files(Some(1024))   // 缓存 1024 个文件句柄
      .worker_threads(worker_threads) // 2~8 线程自适应 CPU
  }

  /// Opens the Fjall storage engine with custom DatabaseBuilder and Keyspace configurations.
  /// 基于底层 Fjall 原生 DatabaseBuilder 与 KeyspaceCreateOpt 打开存储引擎
  pub fn open_with_cfg(
    builder: DatabaseBuilder<Database>,
    data_opts: KeyspaceCreateOpt,
    meta_opts: KeyspaceCreateOpt,
  ) -> Result<Self> {
    let db = builder.open().map_err(Error::from)?;
    Ok(Self {
      db,
      data_opts,
      meta_opts,
    })
  }

  /// Opens a Fjall storage engine instance with optimized default configurations.
  /// 打开具有最优化默认配置的 Fjall 存储引擎实例
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
      create_dir_all(parent)?;
    }
    Self::open_with_cfg(
      Self::default_database_builder(path),
      Self::default_data_partition_options(),
      Self::default_meta_partition_options(),
    )
  }
}

impl Engine for Fjall {
  type Error = fjall::Error;
  type Partition = FjallPartition;
  type Batch = FjallBatch;

  #[inline]
  fn partition(&self, name: &str) -> StdResult<Self::Partition, Self::Error> {
    let ks = if name == META {
      self.db.keyspace(name, || self.meta_opts.clone())?
    } else {
      self.db.keyspace(name, || self.data_opts.clone())?
    };
    Ok(FjallPartition { ks })
  }

  #[inline]
  fn partition_exists(&self, name: &str) -> bool {
    self.db.keyspace_exists(name)
  }

  #[inline]
  fn list_partitions(&self) -> StdResult<Vec<String>, Self::Error> {
    Ok(
      self
        .db
        .list_keyspace_names()
        .into_iter()
        .map(|k| k.to_string())
        .collect(),
    )
  }

  #[inline]
  fn rm_partition(&self, partition: &Self::Partition) -> StdResult<(), Self::Error> {
    self.db.delete_keyspace(partition.ks.clone())
  }

  #[inline]
  fn write_buffer_size(&self) -> u64 {
    self.db.write_buffer_size()
  }

  #[inline]
  fn cache_size(&self) -> u64 {
    self.db.cache_size()
  }

  #[inline]
  fn cache_capacity(&self) -> u64 {
    self.db.cache_capacity()
  }

  #[inline]
  fn outstanding_flushes(&self) -> usize {
    self.db.outstanding_flushes()
  }

  #[inline]
  fn active_compactions(&self) -> usize {
    self.db.active_compactions()
  }

  #[inline]
  fn compactions_completed(&self) -> usize {
    self.db.compactions_completed()
  }

  #[inline]
  fn journal_count(&self) -> usize {
    self.db.journal_count()
  }

  #[inline]
  fn journal_disk_space(&self) -> StdResult<u64, Self::Error> {
    self.db.journal_disk_space()
  }

  #[inline]
  fn batch(&self) -> Self::Batch {
    FjallBatch {
      batch: self.db.batch(),
    }
  }

  #[inline]
  fn batch_with_capacity(&self, capacity: usize) -> Self::Batch {
    FjallBatch {
      batch: fjall::OwnedWriteBatch::with_capacity(self.db.clone(), capacity),
    }
  }

  #[inline]
  fn persist(&self) -> StdResult<(), Self::Error> {
    self.db.persist(FjallPersistMode::SyncAll)
  }

  #[inline]
  fn disk_space(&self) -> StdResult<u64, Self::Error> {
    self.db.disk_space()
  }

  #[inline]
  fn compact(&self) -> StdResult<(), Self::Error> {
    for name in self.db.list_keyspace_names() {
      self.partition(&name)?.compact()?;
    }
    Ok(())
  }
}
