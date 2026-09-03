use crate::{
  api::key::opt::{DBScanInfo, KeyNumStats},
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key_composer::KeyTag,
  meta::{KeyMeta, current_now_ms},
  string::{
    compose_string_prefix_stack as string_key_prefix_stack, decode_string_value, is_string_expired,
  },
  wedb::Db,
};

impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn key_count(&self) -> Result<usize> {
    key_count_impl(self)
  }

  #[inline]
  pub fn dbsize(&self) -> Result<usize> {
    self.key_count()
  }

  /// Forces a full keyspace scan and updates the cached KeyNumStats (aligned with Kvrocks DBSIZE SCAN / AsyncScanDBSize).
  /// 显式执行键空间全量扫描并更新统计缓存（对标 Apache Kvrocks DBSIZE SCAN / AsyncScanDBSize）
  #[inline]
  pub fn dbsize_scan(&self) -> Result<KeyNumStats> {
    dbsize_scan_impl(self)
  }

  /// Retrieves the latest cached DBSIZE count without disk scanning (O(1) memory lookup, aligned with Kvrocks DBSIZE).
  /// If not scanned yet, returns 0.
  /// 获取最新缓存的 DBSIZE 键总数，零磁盘扫描（O(1) 内存查询，对标 Apache Kvrocks DBSIZE）
  #[inline]
  pub fn dbsize_cached(&self) -> usize {
    let kc = self.kc();
    self
      .inner
      .db_scan_infos
      .read()
      .get(&(kc.ns_id(), kc.db()))
      .map(|info| info.stats.n_key)
      .unwrap_or(0)
  }

  /// Retrieves the latest cached KeyNumStats (aligned with Kvrocks GetLatestKeyNumStats).
  /// If not scanned yet, returns default stats.
  /// 获取最新缓存的 KeyNumStats 统计信息（对标 Apache Kvrocks GetLatestKeyNumStats）
  #[inline]
  pub fn key_num_stats(&self) -> KeyNumStats {
    let kc = self.kc();
    self
      .inner
      .db_scan_infos
      .read()
      .get(&(kc.ns_id(), kc.db()))
      .map(|info| info.stats)
      .unwrap_or_default()
  }

  /// Retrieves the timestamp in seconds of the last DBSIZE scan (aligned with Kvrocks GetLastScanTime / last_dbsize_scan_timestamp).
  /// 获取上一次 DBSIZE 扫描的 Unix 秒级时间戳（对标 Apache Kvrocks GetLastScanTime）
  #[inline]
  pub fn last_dbsize_scan_time(&self) -> u64 {
    let kc = self.kc();
    self
      .inner
      .db_scan_infos
      .read()
      .get(&(kc.ns_id(), kc.db()))
      .map(|info| info.last_scan_time_secs)
      .unwrap_or(0)
  }

  /// Returns keyspace info string formatted like Kvrocks / Redis INFO keyspace.
  /// Example: "keys=10,expires=2,avg_ttl=3600,expired=1"
  /// 返回对标 Apache Kvrocks / Redis INFO keyspace 规范的统计格式化字符串
  #[inline]
  pub fn keyspace_info_string(&self) -> String {
    let stats = self.key_num_stats();
    format!(
      "keys={},expires={},avg_ttl={},expired={}",
      stats.n_key, stats.n_expires, stats.avg_ttl, stats.n_expired
    )
  }
}

/// Scans the keyspace and calculates KeyNumStats (aligned with Apache Kvrocks Database::GetKeyNumStats / AsyncScanDBSize).
/// 扫描键空间并计算 KeyNumStats 统计信息（对标 Apache Kvrocks Database::GetKeyNumStats / AsyncScanDBSize）
pub fn dbsize_scan_impl<E: Engine>(db: &Db<E>) -> Result<KeyNumStats>
where
  Error: From<E::Error>,
{
  let mut stats = KeyNumStats::default();
  let mut ttl_sum_ms = 0u64;
  let now_ms = current_now_ms();
  let kc = db.kc();
  let data_ks = db.data();
  let meta_ks = db.meta();

  // 1. 扫描 data_ks 中的 String 键（零分配流式扫描，统计存活、过期与 TTL）
  let str_prefix = string_key_prefix_stack(&kc);
  for item in data_ks.prefix(&str_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(str_prefix.as_slice()) {
      break;
    }
    let (expire_at, _) = decode_string_value(entry.value());
    if is_string_expired(expire_at, now_ms) {
      stats.n_expired += 1;
    } else {
      stats.n_key += 1;
      if expire_at > 0 {
        stats.n_expires += 1;
        ttl_sum_ms += expire_at.saturating_sub(now_ms);
      }
    }
  }

  // 2. 扫描 meta_ks 中的复合数据结构元数据键（单次遍历无子键，统计存活、过期与 TTL）
  let meta_prefix = kc.namespace_prefix_stack();
  let scope_prefix_len = kc.scope_prefix_len();
  for item in meta_ks.prefix(&meta_prefix) {
    let entry = item?;
    let k = entry.key();
    if !k.starts_with(meta_prefix.as_slice()) {
      break;
    }
    let remain = &k[scope_prefix_len..];
    if remain.is_empty() {
      continue;
    }
    let Some(tag) = KeyTag::from_u8(remain[0]) else {
      continue;
    };
    if !tag.is_meta() {
      continue;
    }
    if let Some(meta) = KeyMeta::decode(entry.value()) {
      if meta.is_expired(now_ms) {
        stats.n_expired += 1;
      } else {
        stats.n_key += 1;
        if meta.expire_at > 0 {
          stats.n_expires += 1;
          ttl_sum_ms += meta.expire_at.saturating_sub(now_ms);
        }
      }
    }
  }

  if stats.n_expires > 0 {
    stats.avg_ttl = (ttl_sum_ms / stats.n_expires as u64) / 1000;
  }

  let scan_info = DBScanInfo {
    stats,
    last_scan_time_secs: now_ms / 1000,
  };
  db.inner
    .db_scan_infos
    .write()
    .insert((kc.ns_id(), kc.db()), scan_info);

  Ok(stats)
}

/// Counts the total number of user keys in the current namespace.
/// 统计当前命名空间下的 Key 总数（零堆内存分配流式计数并更新缓存）
#[inline]
pub fn key_count_impl<E: Engine>(db: &Db<E>) -> Result<usize>
where
  Error: From<E::Error>,
{
  let stats = dbsize_scan_impl(db)?;
  Ok(stats.n_key)
}
