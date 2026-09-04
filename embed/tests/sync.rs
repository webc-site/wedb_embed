#![cfg(feature = "sync")]

use std::ops::Bound;

use aok::{OK, Void};
use log::info;
use tempfile::tempdir;
use wedb_embed::engine::{Engine, Fjall, KvEntry, Partition, Snapshot, SyncEngine};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_fjall_sync_snapshot_and_isolation() -> Void {
  let dir = tempdir()?;
  let fjall = Fjall::open(dir.path())?;

  let part_data = fjall.partition("data")?;
  let part_meta = fjall.partition("meta")?;

  // 1. Initial write
  part_data.insert(b"user:1", b"alice")?;
  part_data.insert(b"user:2", b"bob")?;
  part_meta.insert(b"meta:version", b"1.0")?;

  let initial_seqno = fjall.visible_seqno();
  let next_seq = fjall.next_seqno();
  info!("Initial visible seqno: {initial_seqno}, next seqno: {next_seq}");
  assert!(next_seq >= initial_seqno);

  // 2. Take a consistent cross-partition snapshot
  let snapshot = fjall.snapshot();
  let snap_seqno = snapshot.seqno();
  assert_eq!(snap_seqno, initial_seqno);

  assert_eq!(
    snapshot.get(&part_data, b"user:1")?.as_deref(),
    Some(&b"alice"[..])
  );
  assert_eq!(
    snapshot.get(&part_meta, b"meta:version")?.as_deref(),
    Some(&b"1.0"[..])
  );
  assert!(snapshot.contains_key(&part_data, b"user:1")?);
  assert!(!snapshot.is_empty(&part_data)?);
  assert_eq!(snapshot.len(&part_data)?, 2);
  assert_eq!(snapshot.size_of(&part_data, b"user:1")?, Some(5));
  assert_eq!(snapshot.iter(&part_data).count(), 2);

  // 3. Concurrent writes to live partitions
  part_data.insert(b"user:1", b"alice_modified")?;
  part_data.insert(b"user:3", b"charlie")?;
  part_meta.insert(b"meta:version", b"2.0")?;

  let after_seqno = fjall.visible_seqno();
  assert!(after_seqno > initial_seqno);

  // 4. Verify snapshot remains immutable and completely consistent
  assert_eq!(
    snapshot.get(&part_data, b"user:1")?.as_deref(),
    Some(&b"alice"[..])
  );
  assert_eq!(snapshot.get(&part_data, b"user:3")?, None);
  assert!(!snapshot.contains_key(&part_data, b"user:3")?);
  assert_eq!(
    snapshot.get(&part_meta, b"meta:version")?.as_deref(),
    Some(&b"1.0"[..])
  );
  assert_eq!(snapshot.len(&part_data)?, 2);
  assert_eq!(snapshot.iter(&part_data).count(), 2);

  // Verify live partitions see new writes
  assert_eq!(
    part_data.get(b"user:1")?.as_deref(),
    Some(&b"alice_modified"[..])
  );
  assert_eq!(part_data.get(b"user:3")?.as_deref(), Some(&b"charlie"[..]));
  assert_eq!(
    part_meta.get(b"meta:version")?.as_deref(),
    Some(&b"2.0"[..])
  );
  assert_eq!(part_data.len()?, 3);

  // 5. Test snapshot scan & bounds
  let prefix_items: Vec<_> = snapshot
    .prefix(&part_data, b"user:")
    .map(|res| res.map(|entry| (entry.key().to_vec(), entry.value().to_vec())))
    .collect::<Result<Vec<_>, _>>()?;
  assert_eq!(prefix_items.len(), 2);
  assert_eq!(prefix_items[0].0, b"user:1");
  assert_eq!(prefix_items[0].1, b"alice");
  assert_eq!(prefix_items[1].0, b"user:2");
  assert_eq!(prefix_items[1].1, b"bob");

  let range_items: Vec<_> = snapshot
    .range(
      &part_data,
      (Bound::Included(b"user:1"), Bound::Included(b"user:1")),
    )
    .map(|res| res.map(|entry| (entry.key().to_vec(), entry.value().to_vec())))
    .collect::<Result<Vec<_>, _>>()?;
  assert_eq!(range_items.len(), 1);
  assert_eq!(range_items[0].0, b"user:1");

  let first = snapshot
    .first_entry(&part_data)?
    .expect("snapshot first entry must exist");
  assert_eq!(first.key_ref(), b"user:1");
  assert_eq!(first.value_ref(), b"alice");

  let last = snapshot
    .last_entry(&part_data)?
    .expect("snapshot last entry must exist");
  assert_eq!(last.key_ref(), b"user:2");
  assert_eq!(last.value_ref(), b"bob");

  // 6. Test flush
  part_data.flush()?;
  part_meta.flush()?;

  info!("test_fjall_sync_snapshot_and_isolation passed");
  OK
}
