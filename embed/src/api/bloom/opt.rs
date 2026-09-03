use crate::{
  api::bloom::r#const::{
    DEFAULT_BF_CAPACITY, DEFAULT_BF_ERROR_RATE, DEFAULT_BF_EXPANSION, DEFAULT_CF_BUCKET_SIZE,
    DEFAULT_CF_CAPACITY, DEFAULT_CF_EXPANSION, DEFAULT_CF_MAX_ITERATIONS, DEFAULT_CF_PAGE_SIZE,
    MAX_CF_EXPANSION,
  },
  error::{Error, Result},
  hll::rapid_hash,
};

/// BF.INSERT command options.
/// BF.INSERT 命令选项
#[derive(Debug, Clone, Copy, PartialEq, bitcode::Encode, bitcode::Decode)]
pub enum BfInsert {
  Capacity(u32),
  ErrorRate(f64),
  Expansion(u16),
  NoCreate,
  NonScaling,
}

/// BF.RESERVE command options.
/// BF.RESERVE 命令选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum BfReserve {
  Expansion(u16),
  NonScaling,
}

/// Bloom filter single item addition result aligned with Kvrocks BloomFilterAddResult.
/// 布隆过滤器单项添加结果（对标 Kvrocks BloomFilterAddResult）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum BloomFilterAddResult {
  Ok,
  Exist,
  Full,
}

/// Bloom filter insertion configuration options aligned with Kvrocks BloomFilterInsertOpt.
/// 布隆过滤器插入配置选项（对标 Kvrocks BloomFilterInsertOpt）
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct BloomFilterInsert {
  pub capacity: u32,
  pub error_rate: f64,
  pub expansion: u16,
  pub auto_create: bool,
}

impl FromIterator<BfInsert> for BloomFilterInsert {
  fn from_iter<I: IntoIterator<Item = BfInsert>>(iter: I) -> Self {
    let mut opt = Self::default();
    for o in iter {
      match o {
        BfInsert::Capacity(c) => opt.capacity = c,
        BfInsert::ErrorRate(e) => opt.error_rate = e,
        BfInsert::Expansion(exp) => opt.expansion = exp,
        BfInsert::NoCreate => opt.auto_create = false,
        BfInsert::NonScaling => opt.expansion = 0,
      }
    }
    opt
  }
}

impl BloomFilterInsert {
  #[inline]
  pub fn from_options(options: impl IntoIterator<Item = BfInsert>) -> Self {
    options.into_iter().collect()
  }
}

impl Default for BloomFilterInsert {
  #[inline]
  fn default() -> Self {
    Self {
      capacity: DEFAULT_BF_CAPACITY,
      error_rate: DEFAULT_BF_ERROR_RATE,
      expansion: DEFAULT_BF_EXPANSION,
      auto_create: true,
    }
  }
}

/// Bloom filter information snapshot aligned with Kvrocks BloomFilterInfo.
/// 布隆过滤器信息快照（对标 Kvrocks BloomFilterInfo）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct BloomFilterInfo {
  pub capacity: u32,
  pub bloom_bytes: u32,
  pub n_filters: u16,
  pub size: u64,
  pub expansion: u16,
}

/// CF.INSERT command options.
/// CF.INSERT 命令选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum CfInsert {
  Capacity(u64),
  BucketSize(u8),
  MaxIterations(u16),
  Expansion(u16),
  PageSize(u32),
  NoCreate,
  Nx,
}

/// CF.RESERVE command options.
/// CF.RESERVE 命令选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum CfReserve {
  BucketSize(u8),
  MaxIterations(u16),
  Expansion(u16),
  PageSize(u32),
}

/// Cuckoo filter insertion configuration options aligned with Kvrocks CuckooFilterInsertOpt.
/// 布谷鸟过滤器插入配置选项（对标 Kvrocks CuckooFilterInsertOpt）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct CuckooFilterInsert {
  pub capacity: u64,
  pub bucket_size: u8,
  pub max_iterations: u16,
  pub expansion: u16,
  pub page_size: u32,
  pub auto_create: bool,
  pub nx: bool,
}

impl FromIterator<CfInsert> for CuckooFilterInsert {
  fn from_iter<I: IntoIterator<Item = CfInsert>>(iter: I) -> Self {
    let mut opt = Self::default();
    for o in iter {
      match o {
        CfInsert::Capacity(c) => opt.capacity = c,
        CfInsert::BucketSize(bs) => opt.bucket_size = bs,
        CfInsert::MaxIterations(mi) => opt.max_iterations = mi,
        CfInsert::Expansion(exp) => opt.expansion = exp,
        CfInsert::PageSize(ps) => opt.page_size = ps,
        CfInsert::NoCreate => opt.auto_create = false,
        CfInsert::Nx => opt.nx = true,
      }
    }
    opt
  }
}

impl CuckooFilterInsert {
  #[inline]
  pub fn from_options(options: impl IntoIterator<Item = CfInsert>) -> Self {
    options.into_iter().collect()
  }
}

impl Default for CuckooFilterInsert {
  #[inline]
  fn default() -> Self {
    Self {
      capacity: DEFAULT_CF_CAPACITY,
      bucket_size: DEFAULT_CF_BUCKET_SIZE,
      max_iterations: DEFAULT_CF_MAX_ITERATIONS,
      expansion: DEFAULT_CF_EXPANSION,
      page_size: DEFAULT_CF_PAGE_SIZE,
      auto_create: true,
      nx: false,
    }
  }
}

/// Cuckoo filter information snapshot aligned with Kvrocks CuckooFilterInfo.
/// 布谷鸟过滤器信息快照（对标 Kvrocks CuckooFilterInfo）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct CuckooFilterInfo {
  pub size: u64,
  pub num_buckets: u64,
  pub num_filters: u16,
  pub num_items_inserted: u64,
  pub num_items_deleted: u64,
  pub bucket_size: u8,
  pub expansion: u16,
  pub max_iterations: u16,
}

/// Cuckoo filter helper utilities aligned with Apache Kvrocks CuckooFilterHelper.
/// 布谷鸟过滤器辅助工具（对标 Apache Kvrocks CuckooFilterHelper）
pub struct CuckooFilterHelper;

impl CuckooFilterHelper {
  pub const LOAD_FACTOR: f64 = 0.955;
  pub const ALT_HASH_MULTIPLIER: u64 = 0x5bd1_e995;
  pub const FINGERPRINT_MODULUS: u64 = 255;
  pub const DEFAULT_PAGE_SIZE: u32 = DEFAULT_CF_PAGE_SIZE;
  pub const DEFAULT_CAPACITY: u64 = DEFAULT_CF_CAPACITY;
  pub const DEFAULT_BUCKET_SIZE: u8 = DEFAULT_CF_BUCKET_SIZE;
  pub const DEFAULT_MAX_ITERATIONS: u16 = DEFAULT_CF_MAX_ITERATIONS;
  pub const DEFAULT_EXPANSION: u16 = DEFAULT_CF_EXPANSION;
  pub const MAX_EXPANSION: u16 = MAX_CF_EXPANSION;

  #[inline]
  pub fn hash(data: &[u8]) -> u64 {
    rapid_hash(data)
  }

  /// Generates non-zero 1-byte fingerprint aligned with RedisBloom / Kvrocks.
  /// 生成非零 1 字节指纹（对标 RedisBloom / Kvrocks GenerateFingerprint: hash % 255 + 1）
  #[inline]
  pub fn generate_fingerprint(hash: u64) -> u8 {
    ((hash % Self::FINGERPRINT_MODULUS) + 1) as u8
  }

  /// Symmetrically calculates alternate hash using XOR aligned with Kvrocks GetAltHash.
  /// 对称计算异或候选哈希（对标 Kvrocks GetAltHash: h2 = h1 ^ (fp * 0x5bd1e995)）
  #[inline]
  pub fn get_alt_hash(fingerprint: u8, hash: u64) -> u64 {
    hash ^ ((fingerprint as u64).wrapping_mul(Self::ALT_HASH_MULTIPLIER))
  }

  /// Computes alternate bucket index from current index and fingerprint aligned with Kvrocks.
  /// 从桶索引和指纹计算备选桶索引（对标 Kvrocks GetAltBucketIndex）
  #[inline]
  pub fn get_alt_bucket_index(bucket_idx: u32, fingerprint: u8, num_buckets: u32) -> u32 {
    let alt_hash = Self::get_alt_hash(fingerprint, bucket_idx as u64);
    (alt_hash as u32) & (num_buckets - 1)
  }

  /// Normalizes expansion factor to power-of-two aligned with Kvrocks NormalizeExpansion.
  /// 规范化扩容因子为 2 的幂次（对标 Kvrocks NormalizeExpansion）
  #[inline]
  pub fn normalize_expansion(expansion: u16) -> u16 {
    if expansion <= 1 {
      expansion
    } else {
      (expansion as u32).next_power_of_two().min(32768) as u16
    }
  }

  /// Computes required number of buckets from capacity and bucket size aligned with Kvrocks.
  /// 根据容量和桶大小计算所需桶数量（对标 Kvrocks CalculateRequiredBuckets）
  pub fn calculate_required_buckets(capacity: u64, bucket_size: u8) -> Result<u32> {
    if bucket_size == 0 {
      return Err(Error::invalid_data("bucket_size must be larger than 0"));
    }
    let max_supported_capacity =
      ((1u64 << 31) as f64 * (bucket_size as f64) * Self::LOAD_FACTOR) as u64;
    if capacity > max_supported_capacity {
      return Err(Error::invalid_data("capacity is too large"));
    }
    let exact_buckets = (capacity as f64) / (bucket_size as f64) / Self::LOAD_FACTOR;
    let mut req_buckets = exact_buckets.ceil() as u64;
    if req_buckets == 0 {
      req_buckets = 1;
    }
    let num_buckets = req_buckets.next_power_of_two();
    if num_buckets > (1u64 << 31) {
      return Err(Error::invalid_data("capacity is too large"));
    }
    Ok(num_buckets as u32)
  }
}
