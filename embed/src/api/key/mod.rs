pub mod r#impl;
pub mod opt;

pub use r#impl::{
  copy_impl, dbsize_scan_impl, del_impl, exists_impl, get_key_expire_at_impl, key_count_impl,
  key_type_impl, keys_impl, randomkey_impl, scan_impl, set_key_expire_at_impl,
  set_key_expire_at_impl_with_condition, sort_impl,
};
pub use opt::{DBScanInfo, ExpireCondition, KeyNumStats, SortArgs};

pub use crate::key_composer::ALL_COMPOSITE_META_TAGS;
use crate::{
  engine::{Engine, KvEntry, Partition},
  error::{ERR_WRONG_TYPE, Error, Result},
  key_composer::{KeyComposer, KeyTag},
  meta::{KeyMeta, MetaOps},
  string::{compose_string_key as raw, decode_string_value, is_string_expired},
  wedb::{Db, DbBatch},
};

/// Clears all keys matching the prefix in batch write.
/// 批量清除前缀匹配的所有键
#[inline]
pub fn clear_prefix_in_batch<E: Engine>(
  partition: &E::Partition,
  prefix: &[u8],
  batch: &mut DbBatch<E>,
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
  }
  Ok(())
}

/// Cleans up composite subkey data using single reusable buffer.
/// 清理特定复合数据结构的子键数据（底层原始方法，单缓冲区零堆分配，二进制安全）
#[inline]
pub fn cleanup_composite_data_raw<E: Engine>(
  data_ks: &E::Partition,
  meta_ks: &E::Partition,
  kc: &KeyComposer,
  meta_tag: u8,
  k_bytes: &[u8],
  batch: &mut DbBatch<E>,
  buf: &mut Vec<u8>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  if let Some(tag) = KeyTag::from_u8(meta_tag) {
    match tag {
      KeyTag::HashMeta => {
        kc.compose_prefix_into(KeyTag::HashData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::ListMeta => {
        kc.compose_prefix_into(KeyTag::ListData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::SetMeta => {
        kc.compose_prefix_into(KeyTag::SetData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::ZSetMeta => {
        kc.compose_prefix_into(KeyTag::ZSetData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
        kc.compose_prefix_into(KeyTag::ZSetScore.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::BitmapMeta => {
        kc.compose_prefix_into(KeyTag::BitmapData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::BloomMeta => {
        kc.compose_prefix_into(KeyTag::BloomData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::CuckooMeta => {
        kc.compose_prefix_into(KeyTag::CuckooData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::SortedIntMeta => {
        kc.compose_prefix_into(KeyTag::SortedIntData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::TimeSeriesMeta => {
        kc.compose_prefix_into(KeyTag::TimeSeriesData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
        clear_prefix_in_batch(meta_ks, buf, batch)?;
      }
      KeyTag::StreamMeta => {
        for prefix_tag in [
          KeyTag::StreamData.as_slice(),
          KeyTag::StreamGroup.as_slice(),
          KeyTag::StreamConsumer.as_slice(),
          KeyTag::StreamPel.as_slice(),
        ] {
          kc.compose_prefix_into(prefix_tag, k_bytes, buf);
          clear_prefix_in_batch(data_ks, buf, batch)?;
        }
      }
      KeyTag::HllMeta => {
        kc.compose_meta_key_into(KeyTag::HllRaw.as_slice(), k_bytes, buf);
        batch.rm_data(buf.as_slice());
      }
      KeyTag::JsonMeta => {
        kc.compose_prefix_into(KeyTag::JsonData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::TDigestMeta => {
        kc.compose_prefix_into(KeyTag::TDigestData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
      }
      KeyTag::FtSchema | KeyTag::FtAlias => {
        kc.compose_prefix_into(KeyTag::FtIndex.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
        clear_prefix_in_batch(meta_ks, buf, batch)?;
        kc.compose_prefix_into(KeyTag::FtData.as_slice(), k_bytes, buf);
        clear_prefix_in_batch(data_ks, buf, batch)?;
        clear_prefix_in_batch(meta_ks, buf, batch)?;
      }
      _ => {}
    }
  }
  Ok(())
}

/// Cleans up subkey data for specific composite data structure.
/// 清理特定复合数据结构的子键数据（按需精准清理，单缓冲区零堆分配，二进制安全）
#[inline]
pub fn cleanup_composite_data<E: Engine>(
  db: &Db<E>,
  meta_tag: &[u8],
  k_bytes: &[u8],
  batch: &mut DbBatch<E>,
  buf: &mut Vec<u8>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  if let Some(&tag_u8) = meta_tag.first() {
    let kc = db.kc();
    let data_ks = db.data();
    let meta_ks = db.meta();
    cleanup_composite_data_raw(data_ks, meta_ks, &kc, tag_u8, k_bytes, batch, buf)?;
  }
  Ok(())
}

/// Metadata key.
/// 清理特定 Key 下所有复合数据结构的元数据与子键数据（支持复用缓冲区，避免高频循环内存分配）
#[inline]
pub fn cleanup_all_composite_data_with_buf<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  batch: &mut DbBatch<E>,
  buf: &mut Vec<u8>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let meta_ks = db.meta();
  if meta_ks.is_empty()? {
    return Ok(());
  }
  let kc = db.kc();
  for &meta_tag in ALL_COMPOSITE_META_TAGS {
    kc.compose_meta_key_into(meta_tag, k_bytes, buf);
    if meta_ks.contains_key(buf.as_slice())? {
      batch.rm_meta(buf.as_slice());
      cleanup_composite_data(db, meta_tag, k_bytes, batch, buf)?;
    }
  }
  Ok(())
}

/// Metadata key.
/// 清理特定 Key 下所有复合数据结构的元数据与子键数据（用于 String 覆盖写入场景）
#[inline]
pub fn cleanup_all_composite_data<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  batch: &mut DbBatch<E>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let mut buf = Vec::with_capacity(32 + k_bytes.len());
  cleanup_all_composite_data_with_buf(db, k_bytes, batch, &mut buf)
}

/// Checks for active metadata of other complex data types with external buffer reuse.
/// 检查是否存在其他复杂数据类型的活跃元数据（支持外部复用缓冲区与底层分区直传）
#[inline]
pub fn check_composite_meta_not_other_type_with_buf<E: Engine>(
  meta_ks: &E::Partition,
  kc: &KeyComposer,
  k_bytes: &[u8],
  current_meta_tag: &[u8],
  now_ms: u64,
  buf: &mut Vec<u8>,
) -> Result<()>
where
  Error: From<E::Error>,
{
  for &tag in ALL_COMPOSITE_META_TAGS {
    if tag == current_meta_tag {
      continue;
    }
    kc.compose_meta_key_into(tag, k_bytes, buf);

    if let Some(m_bytes) = meta_ks.get(buf.as_slice())?
      && let Some(base_meta) = KeyMeta::decode(&m_bytes)
      && !base_meta.is_expired(now_ms)
    {
      return Err(Error::wrong_type(ERR_WRONG_TYPE));
    }
  }

  Ok(())
}

/// Core internal check for active metadata of other complex data types.
/// 检查是否存在其他复杂数据类型的活跃元数据（内部核心方法，单缓冲区零冗余分配）
#[inline]
pub fn check_composite_meta_not_other_type<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  current_meta_tag: &[u8],
  now_ms: u64,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let meta_ks = db.meta();
  if meta_ks.is_empty()? {
    return Ok(());
  }
  let kc = db.kc();
  let mut buf = Vec::with_capacity(32 + k_bytes.len());
  check_composite_meta_not_other_type_with_buf::<E>(
    meta_ks,
    &kc,
    k_bytes,
    current_meta_tag,
    now_ms,
    &mut buf,
  )
}

/// Generic WRONGTYPE cross-type collision validator (aligned with Kvrocks Database::GetMetadata).
/// 通用 WRONGTYPE 跨类型占用冲突校验（对标 Kvrocks Database::GetMetadata，单缓冲区零冗余分配）
#[inline]
pub fn check_key_not_other_type<E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  current_meta_tag: &[u8],
  now_ms: u64,
) -> Result<()>
where
  Error: From<E::Error>,
{
  let kc = db.kc();

  // 1. 检查是否存在未过期的原生 String
  let raw_k = raw(&kc, k_bytes);
  if let Some(raw) = db.data().get(&raw_k)? {
    let (expire_at, _) = decode_string_value(&raw);
    if !is_string_expired(expire_at, now_ms) {
      return Err(Error::wrong_type(ERR_WRONG_TYPE));
    }
  }

  // 2. 检查其他复杂数据类型元数据
  check_composite_meta_not_other_type(db, k_bytes, current_meta_tag, now_ms)
}

/// Active composite metadata entry tuple: (meta_tag, base_meta, raw_value_guard).
/// 活跃复合元数据项三元组：(meta_tag, base_meta, raw_value_guard)
pub type ActiveCompositeMeta<V> = (u8, KeyMeta, V);

/// Finds live unexpired metadata across composite metadata tables with single buffer reuse.
/// 在复合元数据表中查找未过期的元数据（单缓冲区复用，零堆分配）
#[inline]
pub fn find_active_composite_meta<E: Engine>(
  db: &Db<E>,
  key: &[u8],
  now_ms: u64,
  buf: &mut Vec<u8>,
) -> Result<Option<ActiveCompositeMeta<<E::Partition as Partition>::Value>>>
where
  Error: From<E::Error>,
{
  let meta_ks = db.meta();
  if meta_ks.is_empty()? {
    return Ok(None);
  }
  let kc = db.kc();
  for &tag in ALL_COMPOSITE_META_TAGS {
    kc.compose_meta_key_into(tag, key, buf);
    if let Some(guard) = meta_ks.get(&*buf)?
      && let Some(base_meta) = KeyMeta::decode(&guard)
      && !base_meta.is_expired(now_ms)
    {
      return Ok(Some((tag[0], base_meta, guard)));
    }
  }
  Ok(None)
}

/// Generic metadata retrieval and type validation in a single pass.
/// 泛型 meta 获取 + 类型校验（单次判断，零冗余双重检索）
#[inline]
pub fn get_meta_checked<M: MetaOps, E: Engine>(
  db: &Db<E>,
  k_bytes: &[u8],
  meta_k: &[u8],
  now_ms: u64,
) -> Result<Option<M>>
where
  Error: From<E::Error>,
{
  let data_ks = db.data();
  let meta_ks = db.meta();
  let kc = db.kc();

  // 1. 检查是否存在未过期的原生 String（处理 String 覆盖复杂结构场景）
  let raw_k = raw(&kc, k_bytes);
  if let Some(raw) = data_ks.get(&raw_k)? {
    let (expire_at, _) = decode_string_value(&raw);
    if !is_string_expired(expire_at, now_ms) {
      return Err(Error::wrong_type(ERR_WRONG_TYPE));
    }
  }

  // 2. 检查当前复合结构的元数据
  if let Some(m_bytes) = meta_ks.get(meta_k)?
    && let Some(meta) = M::decode(&m_bytes)
    && !meta.is_expired(now_ms)
  {
    return Ok(Some(meta));
  }

  // 3. 检查是否存在其他复杂数据类型的活跃元数据
  check_composite_meta_not_other_type(db, k_bytes, M::TAG, now_ms)?;

  Ok(None)
}
