use std::ptr::eq as ptr_eq;

use crate::{
  error::{Error, Result},
  hll::{
    algo::{
      HLL_DENSE_SIZE, HLL_REGISTERS, extract_dense_hll_result, hll_murmur_hash_64a, rapid_hash,
    },
    dense::{hll_dense_estimate, hll_dense_get_register, hll_dense_set_register, hll_merge_bytes},
    meta::HllEncodeType,
    sparse::{
      hll_dense_to_sparse, hll_merge_sparse_into_dense, hll_sparse_estimate,
      hll_sparse_get_register, hll_sparse_is_valid, hll_sparse_new, hll_sparse_set_register,
      hll_sparse_to_dense,
    },
  },
};

/// Encodes data into binary format.
/// HyperLogLog 独立核心结构（同时支持 Dense 密集与 Sparse 稀疏编码）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperLogLog {
  pub registers: Vec<u8>,
  pub encode_type: HllEncodeType,
}

impl Default for HyperLogLog {
  fn default() -> Self {
    Self::new()
  }
}

impl HyperLogLog {
  /// Encodes data into binary format.
  /// 创建默认密集编码的 HyperLogLog
  #[inline]
  pub fn new() -> Self {
    Self {
      registers: vec![0u8; HLL_DENSE_SIZE],
      encode_type: HllEncodeType::Dense,
    }
  }

  /// Encodes data into binary format.
  /// 创建初始稀疏编码的 HyperLogLog（仅占 2 字节）
  #[inline]
  pub fn new_sparse() -> Self {
    Self {
      registers: hll_sparse_new(),
      encode_type: HllEncodeType::Sparse,
    }
  }

  /// Composes storage key or prefix.
  /// 从字节切片智能构造（自动识别 Sparse 或 Dense 编码）
  #[inline]
  pub fn from_bytes(bytes: &[u8]) -> Self {
    if bytes.len() >= HLL_DENSE_SIZE {
      Self {
        registers: bytes[..HLL_DENSE_SIZE].to_vec(),
        encode_type: HllEncodeType::Dense,
      }
    } else if hll_sparse_is_valid(bytes) {
      Self {
        registers: bytes.to_vec(),
        encode_type: HllEncodeType::Sparse,
      }
    } else {
      let mut registers = vec![0u8; HLL_DENSE_SIZE];
      let copy_len = bytes.len().min(HLL_DENSE_SIZE);
      registers[..copy_len].copy_from_slice(&bytes[..copy_len]);
      Self {
        registers,
        encode_type: HllEncodeType::Dense,
      }
    }
  }

  /// Composes storage key or prefix.
  /// 从已校验的稀疏字节流构造
  #[inline]
  pub fn from_sparse_bytes(bytes: &[u8]) -> Result<Self> {
    if !hll_sparse_is_valid(bytes) {
      return Err(Error::invalid_data("invalid sparse hll payload"));
    }
    Ok(Self {
      registers: bytes.to_vec(),
      encode_type: HllEncodeType::Sparse,
    })
  }

  #[inline]
  pub fn to_bytes(&self) -> &[u8] {
    &self.registers
  }

  #[inline]
  pub fn as_slice(&self) -> &[u8] {
    &self.registers
  }

  #[inline]
  pub fn as_mut_slice(&mut self) -> &mut [u8] {
    &mut self.registers
  }

  #[inline]
  pub fn encode_type(&self) -> HllEncodeType {
    self.encode_type
  }

  /// Encodes data into binary format.
  /// 将当前稀疏结构晋升为 12288 字节密集编码
  pub fn promote_to_dense(&mut self) -> Result<()> {
    if self.encode_type == HllEncodeType::Dense {
      if self.registers.len() < HLL_DENSE_SIZE {
        self.registers.resize(HLL_DENSE_SIZE, 0);
      }
      return Ok(());
    }
    let mut dense_buf = vec![0u8; HLL_DENSE_SIZE];
    hll_sparse_to_dense(&self.registers, &mut dense_buf)?;
    self.registers = dense_buf;
    self.encode_type = HllEncodeType::Dense;
    Ok(())
  }

  /// Extracts count value from specified register (0..63).
  /// 提取指定寄存器的计数值（0..63）
  #[inline]
  pub fn get_register(&self, index: usize) -> u8 {
    match self.encode_type {
      HllEncodeType::Dense => hll_dense_get_register(&self.registers, index),
      HllEncodeType::Sparse => hll_sparse_get_register(&self.registers, index).unwrap_or(0),
    }
  }

  /// Sets count value for specified register.
  /// 设置指定寄存器的计数值
  #[inline]
  pub fn set_register(&mut self, index: usize, val: u8) {
    if index >= HLL_REGISTERS {
      return;
    }
    match self.encode_type {
      HllEncodeType::Dense => {
        if self.registers.len() < HLL_DENSE_SIZE {
          self.registers.resize(HLL_DENSE_SIZE, 0);
        }
        hll_dense_set_register(&mut self.registers, index, val);
      }
      HllEncodeType::Sparse => match hll_sparse_set_register(&mut self.registers, index, val) {
        Ok(_) => {}
        Err(_) => {
          if self.promote_to_dense().is_ok() {
            hll_dense_set_register(&mut self.registers, index, val);
          }
        }
      },
    }
  }

  /// Adds element data using RapidHash.
  /// 添加元素数据（RapidHash 极速哈希）
  #[inline]
  pub fn add(&mut self, data: &[u8]) -> bool {
    let h = rapid_hash(data);
    self.add_hash(h)
  }

  /// Adds element data using MurmurHash64A aligned with Kvrocks / Redis.
  /// 添加元素数据（MurmurHash64A 对标 Kvrocks / Redis）
  #[inline]
  pub fn add_murmur(&mut self, data: &[u8]) -> bool {
    let h = hll_murmur_hash_64a(data);
    self.add_hash(h)
  }

  /// Adds precomputed 64-bit hash value directly.
  /// 添加已计算好的 64 位哈希值
  #[inline]
  pub fn add_hash(&mut self, hash: u64) -> bool {
    let (idx, count) = extract_dense_hll_result(hash);
    match self.encode_type {
      HllEncodeType::Dense => {
        if self.registers.len() < HLL_DENSE_SIZE {
          self.registers.resize(HLL_DENSE_SIZE, 0);
        }
        let old = hll_dense_get_register(&self.registers, idx);
        if count > old {
          hll_dense_set_register(&mut self.registers, idx, count);
          true
        } else {
          false
        }
      }
      HllEncodeType::Sparse => match hll_sparse_set_register(&mut self.registers, idx, count) {
        Ok(updated) => updated,
        Err(_) => {
          if self.promote_to_dense().is_ok() {
            let old = hll_dense_get_register(&self.registers, idx);
            if count > old {
              hll_dense_set_register(&mut self.registers, idx, count);
              true
            } else {
              false
            }
          } else {
            false
          }
        }
      },
    }
  }

  /// Computes current approximate cardinality estimation.
  /// 计算当前近似基数估算值
  #[inline]
  pub fn count(&self) -> u64 {
    match self.encode_type {
      HllEncodeType::Dense => hll_dense_estimate(&self.registers),
      HllEncodeType::Sparse => {
        hll_sparse_estimate(&self.registers).unwrap_or_else(|_| hll_dense_estimate(&self.registers))
      }
    }
  }

  /// Merges another HyperLogLog in-place with zero heap allocation.
  /// 就地合并另一个 HyperLogLog（零堆分配极速合并）
  pub fn merge(&mut self, other: &Self) {
    if ptr_eq(self, other) || other.is_empty() {
      return;
    }

    if self.encode_type == HllEncodeType::Dense && other.encode_type == HllEncodeType::Dense {
      if self.registers.len() < HLL_DENSE_SIZE {
        self.registers.resize(HLL_DENSE_SIZE, 0);
      }
      hll_merge_bytes(&mut self.registers, &other.registers);
      return;
    }

    // 包含稀疏编码时，统一提升为密集编码并进行零分配就地合并
    self.promote_to_dense().ok();
    match other.encode_type {
      HllEncodeType::Dense => {
        hll_merge_bytes(&mut self.registers, &other.registers);
      }
      HllEncodeType::Sparse => {
        hll_merge_sparse_into_dense(&mut self.registers, &other.registers);
      }
    }
  }

  /// Merges raw byte slice directly into current HyperLogLog.
  /// 合并原始字节切片（零堆分配极速合并）
  pub fn merge_bytes(&mut self, other: &[u8]) {
    if other.is_empty() {
      return;
    }
    self.promote_to_dense().ok();
    if other.len() >= HLL_DENSE_SIZE {
      hll_merge_bytes(&mut self.registers, other);
    } else if hll_sparse_is_valid(other) {
      hll_merge_sparse_into_dense(&mut self.registers, other);
    } else {
      hll_merge_bytes(&mut self.registers, other);
    }
  }

  /// Returns whether HyperLogLog is completely empty (cardinality 0).
  /// 判断是否全空（估算为 0）
  #[inline]
  pub fn is_empty(&self) -> bool {
    match self.encode_type {
      HllEncodeType::Dense => self.registers.iter().all(|&b| b == 0),
      HllEncodeType::Sparse => self.count() == 0,
    }
  }

  /// Clears all registers to zero.
  /// 清空所有寄存器
  #[inline]
  pub fn clear(&mut self) {
    match self.encode_type {
      HllEncodeType::Dense => self.registers.fill(0),
      HllEncodeType::Sparse => self.registers = hll_sparse_new(),
    }
  }

  /// Exports as dense register byte slice.
  /// 导出为 Dense 密集切片
  pub fn to_dense(&self) -> Result<Vec<u8>> {
    match self.encode_type {
      HllEncodeType::Dense => Ok(self.registers.clone()),
      HllEncodeType::Sparse => {
        let mut buf = vec![0u8; HLL_DENSE_SIZE];
        hll_sparse_to_dense(&self.registers, &mut buf)?;
        Ok(buf)
      }
    }
  }

  /// Attempts to export as sparse byte slice (returns None if uncompressible).
  /// 尝试导出为 Sparse 稀疏切片（若不可压缩则返回 None）
  pub fn to_sparse(&self) -> Option<Vec<u8>> {
    match self.encode_type {
      HllEncodeType::Sparse => Some(self.registers.clone()),
      HllEncodeType::Dense => hll_dense_to_sparse(&self.registers),
    }
  }

  /// Validates algorithmic correctness and error bounds.
  /// 自检算法正确性与误差边界
  pub fn selftest() -> bool {
    let mut hll = Self::new();
    for i in 0..1000 {
      hll.add(format!("test_element_{i}").as_bytes());
    }
    let est = hll.count();
    (800..=1200).contains(&est)
  }
}
