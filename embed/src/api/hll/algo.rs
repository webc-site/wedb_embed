use rapidhash::v3::rapidhash_v3;

pub use super::r#const::*;

/// 64-bit rapidhash function.
/// RapidHash 64 位哈希函数
#[inline]
pub fn rapid_hash(bytes: &[u8]) -> u64 {
  rapidhash_v3(bytes)
}

/// MurmurHash64A hash function (1:1 aligned with Redis / Kvrocks HllMurMurHash64A).
/// MurmurHash64A 哈希函数（1:1 对标 Redis / Apache Kvrocks HllMurMurHash64A）
#[inline]
pub fn murmur_hash_64a(data: &[u8], seed: u32) -> u64 {
  const M: u64 = 0xc6a4_a793_5bd1_e995;
  const R: u32 = 47;
  let len = data.len();
  let mut h = (seed as u64) ^ ((len as u64).wrapping_mul(M));

  let (chunks, remainder) = data.as_chunks::<8>();
  for chunk in chunks {
    let mut k = u64::from_le_bytes(*chunk);
    k = k.wrapping_mul(M);
    k ^= k >> R;
    k = k.wrapping_mul(M);
    h ^= k;
    h = h.wrapping_mul(M);
  }

  if !remainder.is_empty() {
    let mut k = 0u64;
    for (i, &b) in remainder.iter().enumerate() {
      k |= (b as u64) << (i * 8);
    }
    h ^= k;
    h = h.wrapping_mul(M);
  }

  h ^= h >> R;
  h = h.wrapping_mul(M);
  h ^= h >> R;
  h
}

/// MurmurHash64A with default seed aligned with Apache Kvrocks HyperLogLog::HllHash.
/// 兼容 Redis / Kvrocks 默认 seed 的 HLL MurmurHash64A（对标 Apache Kvrocks HyperLogLog::HllHash）
#[inline]
pub fn hll_murmur_hash_64a(data: &[u8]) -> u64 {
  murmur_hash_64a(data, HLL_HASH_SEED)
}

/// Extracts 14-bit bucket index and 50-bit trailing zero count aligned with Apache Kvrocks ExtractDenseHllResult.
/// 从 64 位哈希值中提取 14 位桶索引和 50 位尾随零计数（对标 Apache Kvrocks ExtractDenseHllResult）
#[inline]
pub const fn extract_dense_hll_result(hash: u64) -> (usize, u8) {
  let index = (hash & HLL_REGISTER_COUNT_MASK) as usize;
  let shifted = (hash >> HLL_REGISTER_COUNT_POW) | (1u64 << HLL_HASH_BIT_COUNT);
  let count = (shifted.trailing_zeros() + 1) as u8;
  (index, count)
}

/// Otmar Ertl helper function sigma (arXiv:1702.01284) aligned with Apache Kvrocks HllSigma.
/// Otmar Ertl 辅助函数 sigma (arXiv:1702.01284，对标 Apache Kvrocks HllSigma)
#[inline]
pub fn hll_sigma(x: f64) -> f64 {
  if x <= 0.0 || x.is_nan() {
    return 0.0;
  }
  if x >= 1.0 {
    return f64::INFINITY;
  }
  let mut x = x;
  let mut y = 1.0;
  let mut z = x;
  loop {
    x *= x;
    let z_prime = z;
    z += x * y;
    y += y;
    if z_prime == z {
      break;
    }
  }
  z
}

/// Otmar Ertl helper function tau (arXiv:1702.01284) aligned with Apache Kvrocks HllTau.
/// Otmar Ertl 辅助函数 tau (arXiv:1702.01284，对标 Apache Kvrocks HllTau)
#[inline]
pub fn hll_tau(x: f64) -> f64 {
  if x <= 0.0 || x >= 1.0 || x.is_nan() {
    return 0.0;
  }
  let mut x = x;
  let mut y = 1.0;
  let mut z = 1.0 - x;
  loop {
    x = x.sqrt();
    let z_prime = z;
    y *= 0.5;
    let diff = 1.0 - x;
    z -= diff * diff * y;
    if z_prime == z {
      break;
    }
  }
  z / 3.0
}

/// Calculates cardinality estimate from register histogram using Otmar Ertl LogLog-Beta algorithm.
/// 基于寄存器直方图使用 Otmar Ertl (2017) LogLog-Beta 算法计算基数估算（零堆分配极速计算）
#[inline]
pub fn hll_estimate_from_histo(reghisto: &[usize; 64]) -> u64 {
  let mut z =
    HLL_M_F64 * hll_tau((HLL_M_F64 - reghisto[HLL_HASH_BIT_COUNT + 1] as f64) * HLL_INV_M);
  for j in (1..=HLL_HASH_BIT_COUNT).rev() {
    z += reghisto[j] as f64;
    z *= 0.5;
  }
  z += HLL_M_F64 * hll_sigma(reghisto[0] as f64 * HLL_INV_M);
  if z <= 0.0 || z.is_nan() || z.is_infinite() {
    0
  } else {
    (HLL_ALPHA_M_SQ / z).round() as u64
  }
}
