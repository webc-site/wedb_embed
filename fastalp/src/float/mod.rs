mod f32;
mod f64;

use crate::params::bits_needed;

/// ALP floating-point abstraction trait (unifies zero-cost f32 and f64 compression).
/// ALP 浮点数抽象特征（统一 f32 与 f64 零成本编解码）
pub trait AlpFloat: Copy + Default + PartialEq + PartialOrd + Send + Sync + 'static {
  type Int: Copy + Default + PartialEq + Eq + PartialOrd + Ord + Send + Sync + 'static;
  type RawBits: Copy + Default + PartialEq + Eq + Send + Sync + 'static;

  const TYPE_BYTE: u8;
  const TYPE_RAW_BYTE: u8;
  const TYPE_DELTA_BYTE: u8;
  const TYPE_DEC_BYTE: u8;
  const TYPE_DEC_DELTA_BYTE: u8;
  const TYPE_DICT_BYTE: u8;
  const TYPE_RD_BYTE: u8;
  const RD_TOTAL_BITS: u8;
  const RD_MAX_CUT: u8;
  const MAX_EXPONENT: u8;
  const MAX_FAC: u8;
  const MAX_BIT_WIDTH: u8;
  const ENCODING_UPPER_LIMIT: Self;
  const EXCEPTION_PENALTY: usize;
  const EXC_ENTRY_SIZE: usize;
  const EXC_ENTRY_SIZE_U32: usize;
  const BASE_SIZE: usize;
  const ZERO: Self;
  const ZERO_INT: Self::Int;
  const MIN_INT: Self::Int;
  const MAX_INT: Self::Int;

  fn exp_factor(exp: u8, fac: u8) -> Self;
  fn fac_int(fac: u8) -> i64;
  fn frac_exp(exp: u8) -> Self;

  fn is_impossible(self) -> bool;
  fn try_encode_fast(self, exp_factor: Self, fac_int: i64, frac_exp: Self) -> Option<Self::Int>;
  fn try_encode_div(self, exp_factor: Self) -> Option<Self::Int>;
  fn fast_round_to_int(self, exp_factor: Self) -> Self::Int;
  fn decode_from_int(encoded: Self::Int, fac_int: i64, frac_exp: Self) -> Self;
  fn decode_from_offset(offset: u64, base: Self::Int, fac_int: i64, frac_exp: Self) -> Self;
  fn decode_from_int_div(encoded: Self::Int, exp_factor: Self) -> Self;
  fn decode_from_offset_div(offset: u64, base: Self::Int, exp_factor: Self) -> Self;

  fn int_diff_to_u64(val: Self::Int, base: Self::Int) -> u64;
  fn u64_to_int_add(offset: u64, base: Self::Int) -> Self::Int;
  fn calc_range(min_val: Self::Int, max_val: Self::Int) -> u64;

  fn int_sub(a: Self::Int, b: Self::Int) -> Self::Int;
  fn int_add(a: Self::Int, b: Self::Int) -> Self::Int;

  #[inline(always)]
  fn bits_needed(max_offset: u64) -> u8 {
    bits_needed(max_offset)
  }

  fn to_raw_bits(self) -> Self::RawBits;
  fn from_raw_bits(bits: Self::RawBits) -> Self;

  /// Returns 64-bit unsigned integer representation for hash table key.
  /// 返回用于哈希表键的 64 位无符号整数表示
  fn to_u64_key(self) -> u64;

  /// Appends raw little-endian bytes of float into destination vector.
  /// 将浮点数的原始小端字节追加至目标向量
  fn write_raw(self, dst: &mut Vec<u8>);

  /// Reads float directly from raw little-endian bytes.
  /// 从原始小端字节直接读取浮点数
  fn read_raw(src: &[u8]) -> Self;

  /// Reconstructs float from 64-bit unsigned integer raw bit representation.
  /// 从 64 位无符号整数原始二进制位重建浮点数
  fn from_u64_raw(raw: u64) -> Self;

  /// Strictly checks whether two floating-point values are bitwise identical (distinguishes +0.0 and -0.0).
  /// 基于底层二进制比特位严格判断两浮点数是否完全相同（可区分 +0.0 与 -0.0）
  #[inline(always)]
  fn is_exact_same(self, other: Self) -> bool {
    self.to_raw_bits() == other.to_raw_bits()
  }

  fn write_base(base: Self::Int, dst: &mut Vec<u8>);
  fn read_base(src: &[u8]) -> Self::Int;

  fn write_exception(pos: u16, bits: Self::RawBits, dst: &mut Vec<u8>);
  fn read_exception(chunk: &[u8]) -> (usize, Self);

  fn write_exception_u32(pos: u32, bits: Self::RawBits, dst: &mut Vec<u8>);
  fn read_exception_u32(chunk: &[u8]) -> (usize, Self);

  /// Decodes a floating-point value from offset and base when factor is 1 (scale = 10^-exp).
  /// 当因子为 1 时根据基准值与逆缩放因子快速解码浮点数
  fn decode_from_offset_fac1(offset: u64, base: Self::Int, frac_exp: Self) -> Self;

  /// Decodes a floating-point value directly from integer when factor is 1 (scale = 10^-exp).
  /// 当因子为 1 时根据整数与逆缩放因子快速解码浮点数
  #[inline(always)]
  fn decode_from_int_fac1(encoded: Self::Int, frac_exp: Self) -> Self {
    Self::decode_from_int(encoded, 1, frac_exp)
  }

  #[inline(always)]
  fn build_lut<const N: usize>(base: Self::Int, fac_int: i64, frac_exp: Self) -> [Self; N] {
    let mut lut = [Self::ZERO; N];
    for (i, slot) in lut.iter_mut().enumerate() {
      *slot = Self::decode_from_offset(i as u64, base, fac_int, frac_exp);
    }
    lut
  }

  #[inline(always)]
  fn build_lut_div<const N: usize>(base: Self::Int, exp_factor: Self) -> [Self; N] {
    let mut lut = [Self::ZERO; N];
    for (i, slot) in lut.iter_mut().enumerate() {
      *slot = Self::decode_from_offset_div(i as u64, base, exp_factor);
    }
    lut
  }
}
