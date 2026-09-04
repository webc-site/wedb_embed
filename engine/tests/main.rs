use std::{collections::BTreeMap, io, ops::Bound, sync::Arc, vec::IntoIter};

use aok::{OK, Void};
use log::info;
use parking_lot::RwLock;
use rapidhash::RapidHashMap as HashMap;
use wedb_embed_engine::{Batch, Engine, KvEntry, Partition};
#[cfg(feature = "sync")]
use wedb_embed_engine::{Snapshot, SyncEngine};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MemEntry {
  key: Vec<u8>,
  value: Vec<u8>,
}

impl KvEntry for MemEntry {
  type Key = Vec<u8>;
  type Value = Vec<u8>;

  #[inline]
  fn key(&self) -> &Self::Key {
    &self.key
  }

  #[inline]
  fn value(&self) -> &Self::Value {
    &self.value
  }
}

#[derive(Clone)]
struct MemPartition {
  data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl Partition for MemPartition {
  type Error = io::Error;
  type Value = Vec<u8>;
  type Entry<'a> = MemEntry;
  type Iter<'a> = MemIter;

  fn get(&self, key: &[u8]) -> Result<Option<Self::Value>, Self::Error> {
    let map = self.data.read();
    Ok(map.get(key).cloned())
  }

  fn len(&self) -> Result<usize, Self::Error> {
    let map = self.data.read();
    Ok(map.len())
  }

  fn insert(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
    let mut map = self.data.write();
    map.insert(key.to_vec(), value.to_vec());
    Ok(())
  }

  fn rm(&self, key: &[u8]) -> Result<(), Self::Error> {
    let mut map = self.data.write();
    map.remove(key);
    Ok(())
  }

  fn clear(&self) -> Result<(), Self::Error> {
    let mut map = self.data.write();
    map.clear();
    Ok(())
  }

  fn iter(&self) -> Self::Iter<'_> {
    let map = self.data.read();
    let entries: Vec<MemEntry> = map
      .iter()
      .map(|(k, v)| MemEntry {
        key: k.clone(),
        value: v.clone(),
      })
      .collect();
    MemIter {
      iter: entries.into_iter(),
    }
  }

  fn prefix(&self, prefix: &[u8]) -> Self::Iter<'_> {
    let map = self.data.read();
    let entries: Vec<MemEntry> = map
      .range(prefix.to_vec()..)
      .take_while(|(k, _)| k.starts_with(prefix))
      .map(|(k, v)| MemEntry {
        key: k.clone(),
        value: v.clone(),
      })
      .collect();
    MemIter {
      iter: entries.into_iter(),
    }
  }

  fn range(&self, range: (Bound<&[u8]>, Bound<&[u8]>)) -> Self::Iter<'_> {
    let map = self.data.read();
    let start_bound = match range.0 {
      Bound::Included(b) => Bound::Included(b.to_vec()),
      Bound::Excluded(b) => Bound::Excluded(b.to_vec()),
      Bound::Unbounded => Bound::Unbounded,
    };
    let end_bound = match range.1 {
      Bound::Included(b) => Bound::Included(b.to_vec()),
      Bound::Excluded(b) => Bound::Excluded(b.to_vec()),
      Bound::Unbounded => Bound::Unbounded,
    };
    let entries: Vec<MemEntry> = map
      .range((start_bound, end_bound))
      .map(|(k, v)| MemEntry {
        key: k.clone(),
        value: v.clone(),
      })
      .collect();
    MemIter {
      iter: entries.into_iter(),
    }
  }
}

struct MemIter {
  iter: IntoIter<MemEntry>,
}

impl Iterator for MemIter {
  type Item = Result<MemEntry, io::Error>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    self.iter.next().map(Ok)
  }
}

impl DoubleEndedIterator for MemIter {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.iter.next_back().map(Ok)
  }
}

enum BatchOp {
  Insert(MemPartition, Vec<u8>, Vec<u8>),
  Rm(MemPartition, Vec<u8>),
}

struct MemBatch {
  ops: Vec<BatchOp>,
}

impl Batch for MemBatch {
  type Error = io::Error;
  type Partition = MemPartition;

  fn insert(&mut self, partition: &Self::Partition, key: &[u8], value: &[u8]) {
    self.ops.push(BatchOp::Insert(
      partition.clone(),
      key.to_vec(),
      value.to_vec(),
    ));
  }

  fn rm(&mut self, partition: &Self::Partition, key: &[u8]) {
    self.ops.push(BatchOp::Rm(partition.clone(), key.to_vec()));
  }

  fn commit(self) -> Result<(), Self::Error> {
    for op in self.ops {
      match op {
        BatchOp::Insert(part, key, value) => {
          part.insert(&key, &value)?;
        }
        BatchOp::Rm(part, key) => {
          part.rm(&key)?;
        }
      }
    }
    Ok(())
  }
}

#[derive(Default)]
struct MemEngine {
  partitions: Arc<RwLock<HashMap<String, MemPartition>>>,
}

impl Engine for MemEngine {
  type Error = io::Error;
  type Partition = MemPartition;
  type Batch = MemBatch;

  fn partition(&self, name: &str) -> Result<Self::Partition, Self::Error> {
    let mut parts = self.partitions.write();
    let part = parts
      .entry(name.to_string())
      .or_insert_with(|| MemPartition {
        data: Arc::new(RwLock::new(BTreeMap::new())),
      })
      .clone();
    Ok(part)
  }

  fn rm_partition(&self, partition: &Self::Partition) -> Result<(), Self::Error> {
    let mut parts = self.partitions.write();
    parts.retain(|_, p| !Arc::ptr_eq(&p.data, &partition.data));
    Ok(())
  }

  fn batch(&self) -> Self::Batch {
    MemBatch { ops: Vec::new() }
  }

  fn persist(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

#[test]
fn test_kv_entry_blanket_impl() {
  let pair = (b"key1".to_vec(), b"val1".to_vec());
  assert_eq!(pair.key(), b"key1");
  assert_eq!(pair.value(), b"val1");
}

#[test]
fn test_engine_crud() -> Void {
  let engine = MemEngine::default();
  let part = engine.partition("users")?;

  assert!(part.is_empty()?);
  assert_eq!(part.len()?, 0);
  assert_eq!(part.approximate_len()?, 0);
  assert_eq!(part.first_entry()?.map(|e| e.key().clone()), None);
  assert_eq!(part.last_entry()?.map(|e| e.key().clone()), None);
  assert_eq!(part.table_count(), 0);
  assert_eq!(part.blob_file_count(), 0);

  assert!(engine.partition_exists("users"));
  assert_eq!(engine.journal_count(), 0);
  assert_eq!(engine.journal_disk_space()?, 0);
  assert_eq!(engine.active_compactions(), 0);
  assert_eq!(engine.compactions_completed(), 0);

  part.insert(b"k1", b"v1")?;
  part.insert(b"k2", b"v2")?;
  part.insert(b"k3", b"v3")?;

  assert!(!part.is_empty()?);
  assert_eq!(part.len()?, 3);
  assert_eq!(part.size_of(b"k1")?, Some(2));
  assert_eq!(part.size_of(b"nonexistent")?, None);
  assert!(part.contains_key(b"k2")?);
  assert!(!part.contains_key(b"k4")?);

  assert_eq!(part.get(b"k1")?.as_deref(), Some(&b"v1"[..]));
  assert_eq!(
    part.first_entry()?.map(|e| e.key().clone()),
    Some(b"k1".to_vec())
  );
  assert_eq!(
    part.last_entry()?.map(|e| e.key().clone()),
    Some(b"k3".to_vec())
  );

  part.rm(b"k2")?;
  assert_eq!(part.len()?, 2);
  assert!(!part.contains_key(b"k2")?);

  part.rm_weak(b"k3")?;
  assert_eq!(part.len()?, 1);

  part.compact()?;
  engine.compact()?;

  part.clear()?;
  assert!(part.is_empty()?);

  engine.rm_partition(&part)?;

  info!("test_engine_crud passed");
  OK
}

#[test]
fn test_engine_iterators() -> Void {
  let engine = MemEngine::default();
  let part = engine.partition("scan")?;

  part.insert(b"user:001", b"alice")?;
  part.insert(b"user:002", b"bob")?;
  part.insert(b"user:003", b"charlie")?;
  part.insert(b"zone:001", b"asia")?;

  // Prefix scan
  let prefix_items: Vec<_> = part
    .prefix(b"user:")
    .map(|res| {
      let entry = res.unwrap();
      (entry.key().clone(), entry.value().clone())
    })
    .collect();
  assert_eq!(prefix_items.len(), 3);
  assert_eq!(prefix_items[0].0, b"user:001");
  assert_eq!(prefix_items[2].0, b"user:003");

  // Range scan
  let range_items: Vec<_> = part
    .range((Bound::Included(b"user:002"), Bound::Included(b"zone:001")))
    .map(|res| {
      let entry = res.unwrap();
      (entry.key().clone(), entry.value().clone())
    })
    .collect();
  assert_eq!(range_items.len(), 3);
  assert_eq!(range_items[0].0, b"user:002");
  assert_eq!(range_items[2].0, b"zone:001");

  // Double ended iterator
  let mut iter = part.prefix(b"user:");
  assert_eq!(iter.next().unwrap().unwrap().key(), b"user:001");
  assert_eq!(iter.next_back().unwrap().unwrap().key(), b"user:003");
  assert_eq!(iter.next().unwrap().unwrap().key(), b"user:002");
  assert!(iter.next().is_none());

  info!("test_engine_iterators passed");
  OK
}

#[test]
fn test_engine_batch() -> Void {
  let engine = MemEngine::default();
  let part_a = engine.partition("part_a")?;
  let part_b = engine.partition("part_b")?;

  part_a.insert(b"old_key", b"old_val")?;

  let mut batch = engine.batch();
  batch.insert(&part_a, b"a1", b"val_a1");
  batch.rm(&part_a, b"old_key");
  batch.insert(&part_b, b"b1", b"val_b1");
  batch.commit()?;

  assert_eq!(part_a.get(b"a1")?.as_deref(), Some(&b"val_a1"[..]));
  assert_eq!(part_a.get(b"old_key")?, None);
  assert_eq!(part_b.get(b"b1")?.as_deref(), Some(&b"val_b1"[..]));

  info!("test_engine_batch passed");
  OK
}

#[cfg(feature = "sync")]
#[derive(Clone)]
struct MemSnapshot {
  _data: HashMap<String, BTreeMap<Vec<u8>, Vec<u8>>>,
  seqno: u64,
}

#[cfg(feature = "sync")]
impl Snapshot for MemSnapshot {
  type Error = io::Error;
  type Partition = MemPartition;
  type Value = Vec<u8>;
  type Entry<'a> = MemEntry;
  type Iter<'a> = MemIter;

  fn seqno(&self) -> u64 {
    self.seqno
  }

  fn get(
    &self,
    partition: &Self::Partition,
    key: &[u8],
  ) -> Result<Option<Self::Value>, Self::Error> {
    let map = partition.data.read();
    Ok(map.get(key).cloned())
  }

  fn iter<'a>(&'a self, partition: &'a Self::Partition) -> Self::Iter<'a> {
    partition.iter()
  }

  fn prefix<'a>(&'a self, partition: &'a Self::Partition, prefix: &[u8]) -> Self::Iter<'a> {
    partition.prefix(prefix)
  }

  fn range<'a>(
    &'a self,
    partition: &'a Self::Partition,
    range: (Bound<&[u8]>, Bound<&[u8]>),
  ) -> Self::Iter<'a> {
    partition.range(range)
  }
}

#[cfg(feature = "sync")]
impl SyncEngine for MemEngine {
  type Snapshot = MemSnapshot;

  fn snapshot(&self) -> Self::Snapshot {
    let parts = self.partitions.read();
    let mut snap_data = HashMap::default();
    for (name, part) in parts.iter() {
      snap_data.insert(name.clone(), part.data.read().clone());
    }
    MemSnapshot {
      _data: snap_data,
      seqno: 42,
    }
  }

  fn visible_seqno(&self) -> u64 {
    42
  }
}

#[cfg(feature = "sync")]
#[test]
fn test_engine_sync() -> Void {
  let engine = MemEngine::default();
  let part = engine.partition("sync_part")?;
  part.insert(b"k1", b"v1")?;
  part.insert(b"k2", b"v2")?;

  assert_eq!(engine.visible_seqno(), 42);
  assert_eq!(engine.next_seqno(), 42);

  let snap = engine.snapshot();
  assert_eq!(snap.seqno(), 42);
  assert_eq!(snap.get(&part, b"k1")?.as_deref(), Some(&b"v1"[..]));
  assert!(snap.contains_key(&part, b"k2")?);

  info!("test_engine_sync passed");
  OK
}
