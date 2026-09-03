/// Expiration condition options for EXPIRE/PEXPIRE/EXPIREAT/PEXPIREAT (aligned with Redis 7.0+ / Kvrocks).
/// 过期时间设置条件选项（对标 Redis 7.0+ 与 Apache Kvrocks）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpireCondition {
  /// Always update expiration (default).
  /// 无条件更新（默认行为）
  #[default]
  None,
  /// Set expiry only when the key has no expiry.
  /// 仅当键当前未设置过期时间时才设置
  NX,
  /// Set expiry only when the key has an existing expiry.
  /// 仅当键当前已有过期时间时才更新
  XX,
  /// Set expiry only when the new expiry is greater than current expiry.
  /// 仅当新过期时间大于当前过期时间时才更新（当前无过期时间则失败）
  GT,
  /// Set expiry only when the new expiry is less than current expiry.
  /// 仅当新过期时间小于当前过期时间时才更新（当前无过期时间视为无限大，允许设置）
  LT,
}

impl ExpireCondition {
  #[inline(always)]
  pub const fn should_update(self, current_exp: u64, new_exp: u64) -> bool {
    match self {
      Self::None => true,
      Self::NX => current_exp == 0,
      Self::XX => current_exp > 0,
      Self::GT => current_exp > 0 && new_exp > current_exp,
      Self::LT => current_exp == 0 || new_exp < current_exp,
    }
  }
}

/// Arguments for SORT and SORT_RO commands (aligned with Redis 7.0+ / Apache Kvrocks).
/// SORT / SORT_RO 排序参数选项（对标 Redis 7.0+ 与 Apache Kvrocks）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortArgs {
  /// BY pattern (e.g. `weight_*` or `weight_*->field`).
  pub by: Option<Vec<u8>>,
  /// LIMIT offset (default: 0).
  pub offset: usize,
  /// LIMIT count (None means all remaining elements).
  pub count: Option<usize>,
  /// GET patterns (e.g. `object_*` or `object_*->field`, `#` returns the element itself).
  pub get: Vec<Vec<u8>>,
  /// Sort in descending order (DESC).
  pub desc: bool,
  /// Sort lexicographically (ALPHA). Default is false (sorts numerically as f64).
  pub alpha: bool,
  /// STORE key: stores the sorted result into a List instead of returning it.
  pub store: Option<Vec<u8>>,
  /// Don't sort, just apply LIMIT and GET (DONT_SORT).
  pub dont_sort: bool,
}

impl SortArgs {
  #[inline]
  pub fn new() -> Self {
    Self::default()
  }

  #[inline]
  pub fn by(mut self, pattern: impl Into<Vec<u8>>) -> Self {
    self.by = Some(pattern.into());
    self
  }

  #[inline]
  pub fn limit(mut self, offset: usize, count: Option<usize>) -> Self {
    self.offset = offset;
    self.count = count;
    self
  }

  #[inline]
  pub fn get(mut self, pattern: impl Into<Vec<u8>>) -> Self {
    self.get.push(pattern.into());
    self
  }

  #[inline]
  pub fn desc(mut self) -> Self {
    self.desc = true;
    self
  }

  #[inline]
  pub fn asc(mut self) -> Self {
    self.desc = false;
    self
  }

  #[inline]
  pub fn alpha(mut self) -> Self {
    self.alpha = true;
    self
  }

  #[inline]
  pub fn store(mut self, store_key: impl Into<Vec<u8>>) -> Self {
    self.store = Some(store_key.into());
    self
  }

  #[inline]
  pub fn dont_sort(mut self) -> Self {
    self.dont_sort = true;
    self
  }
}

/// Key and keyspace statistics aligned with Apache Kvrocks KeyNumStats.
/// 键空间统计信息（对标 Apache Kvrocks KeyNumStats）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyNumStats {
  /// Total number of active (non-expired) keys.
  pub n_key: usize,
  /// Total number of active keys with an expiration set (TTL).
  pub n_expires: usize,
  /// Total number of expired keys encountered during scanning.
  pub n_expired: usize,
  /// Average remaining TTL of keys with expiration (in seconds, aligned with Kvrocks avg_ttl).
  pub avg_ttl: u64,
}

/// Cached scan information for keyspace statistics aligned with Apache Kvrocks DBScanInfo.
/// 键空间扫描缓存信息（对标 Apache Kvrocks DBScanInfo）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DBScanInfo {
  /// Calculated key numbers and TTL statistics.
  pub stats: KeyNumStats,
  /// Timestamp of the last scan in seconds (aligned with Kvrocks last_scan_time_secs).
  pub last_scan_time_secs: u64,
}
