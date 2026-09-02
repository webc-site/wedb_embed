use std::{mem::size_of, ptr::read_unaligned};

use crate::{
  constants::{
    BITS_PER_BYTE, ENCODING_UPPER_LIMIT_F32, ENCODING_UPPER_LIMIT_F64, EXC_POS_LEN, EXP_ARR_F32,
    EXP_ARR_F64, FACT_ARR_F32, FACT_ARR_F64, FRAC_ARR_F32, FRAC_ARR_F64, MAGIC_NUMBER_F32,
    MAGIC_NUMBER_F64, MAX_EXPONENT_F32, MAX_EXPONENT_F64, MAX_FAC_F32, MAX_FAC_F64, TYPE_F32,
    TYPE_F32_RAW, TYPE_F64, TYPE_F64_RAW,
  },
  params::bits_needed,
};

/// ALP floating-point abstraction trait (unifies zero-cost f32 and f64 compression).
/// ALP 浮点数抽象特征（统一 f32 / f64 零成本编解码）
pub trait AlpFloat: Copy + Default + PartialEq + PartialOrd + Send + Sync + 'static {
  type Int: Copy + Default + PartialEq + Eq + PartialOrd + Ord + Send + Sync + 'static;
  type RawBits: Copy + Default + PartialEq + Eq + Send + Sync + 'static;

  const TYPE_BYTE: u8;
  const TYPE_RAW_BYTE: u8;
  const MAX_EXPONENT: u8;
  const MAX_FAC: u8;
  const MAX_BIT_WIDTH: u8;
  const MAGIC_NUMBER: Self;
  const ENCODING_UPPER_LIMIT: Self;
  const EXCEPTION_PENALTY: usize;
  const EXC_ENTRY_SIZE: usize;
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
  fn fast_round_to_int(self, exp_factor: Self) -> Self::Int;
  fn decode_from_int(encoded: Self::Int, fac_int: i64, frac_exp: Self) -> Self;
  fn decode_from_offset(offset: u64, base: Self::Int, fac_int: i64, frac_exp: Self) -> Self;

  fn int_diff_to_u64(val: Self::Int, base: Self::Int) -> u64;
  fn u64_to_int_add(offset: u64, base: Self::Int) -> Self::Int;
  fn calc_range(min_val: Self::Int, max_val: Self::Int) -> u64;

  #[inline(always)]
  fn bits_needed(max_offset: u64) -> u8 {
    bits_needed(max_offset)
  }

  fn to_raw_bits(self) -> Self::RawBits;
  fn from_raw_bits(bits: Self::RawBits) -> Self;

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

  /// Decodes a floating-point value from offset and base when factor is 1 (scale = 10^-exp).
  /// 当因子为 1 时根据基准值与逆缩放因子快速解码浮点数
  fn decode_from_offset_fac1(offset: u64, base: Self::Int, frac_exp: Self) -> Self;

  #[inline(always)]
  fn build_lut<const N: usize>(base: Self::Int, fac_int: i64, frac_exp: Self) -> [Self; N] {
    let mut lut = [Self::ZERO; N];
    for (i, slot) in lut.iter_mut().enumerate() {
      *slot = Self::decode_from_offset(i as u64, base, fac_int, frac_exp);
    }
    lut
  }
}

impl AlpFloat for f64 {
  type Int = i64;
  type RawBits = u64;

  const TYPE_BYTE: u8 = TYPE_F64;
  const TYPE_RAW_BYTE: u8 = TYPE_F64_RAW;
  const MAX_EXPONENT: u8 = MAX_EXPONENT_F64;
  const MAX_FAC: u8 = MAX_FAC_F64;
  const MAX_BIT_WIDTH: u8 = u64::BITS as u8;
  const MAGIC_NUMBER: Self = MAGIC_NUMBER_F64;
  const ENCODING_UPPER_LIMIT: Self = ENCODING_UPPER_LIMIT_F64;
  const EXC_ENTRY_SIZE: usize = EXC_POS_LEN + size_of::<Self::RawBits>();
  const EXCEPTION_PENALTY: usize = Self::EXC_ENTRY_SIZE * BITS_PER_BYTE;
  const BASE_SIZE: usize = size_of::<Self::Int>();
  const ZERO: Self = 0.0;
  const ZERO_INT: Self::Int = 0;
  const MIN_INT: Self::Int = i64::MIN;
  const MAX_INT: Self::Int = i64::MAX;

  #[inline(always)]
  fn exp_factor(exp: u8, fac: u8) -> Self {
    // SAFETY: 调用方已前置校验 fac <= exp <= MAX_EXPONENT_F64 (18)，且 EXP_ARR_F64 长度为 19，(exp - fac) 必然在 [0, 18] 范围内，索引绝不越界。
    unsafe { *EXP_ARR_F64.get_unchecked((exp - fac) as usize) }
  }

  #[inline(always)]
  fn fac_int(fac: u8) -> i64 {
    // SAFETY: 调用方已前置校验 fac <= MAX_FAC (8) <= 18，且 FACT_ARR_F64 长度为 19，fac 必然在 [0, 8] 范围内，索引绝不越界。
    unsafe { *FACT_ARR_F64.get_unchecked(fac as usize) }
  }

  #[inline(always)]
  fn frac_exp(exp: u8) -> Self {
    // SAFETY: 调用方已前置校验 exp <= MAX_EXPONENT_F64 (18)，且 FRAC_ARR_F64 长度为 19，exp 必然在 [0, 18] 范围内，索引绝不越界。
    unsafe { *FRAC_ARR_F64.get_unchecked(exp as usize) }
  }

  #[inline(always)]
  fn is_impossible(self) -> bool {
    !self.is_finite()
      || self.abs() > Self::ENCODING_UPPER_LIMIT
      || (self == Self::ZERO && self.is_sign_negative())
  }

  #[inline(always)]
  fn try_encode_fast(self, exp_factor: Self, fac_int: i64, frac_exp: Self) -> Option<Self::Int> {
    if self.is_impossible() {
      return None;
    }
    let scaled = self * exp_factor;
    if scaled.is_impossible() {
      return None;
    }
    let rounded = (scaled + Self::MAGIC_NUMBER) - Self::MAGIC_NUMBER;
    let encoded = rounded as i64;

    let int_with_fac = if fac_int == 1 {
      encoded
    } else {
      encoded.checked_mul(fac_int)?
    };
    let decoded = (int_with_fac as f64) * frac_exp;
    if decoded.to_bits() == self.to_bits() {
      Some(encoded)
    } else {
      None
    }
  }

  #[inline(always)]
  fn fast_round_to_int(self, exp_factor: Self) -> Self::Int {
    let scaled = self * exp_factor;
    let rounded = (scaled + Self::MAGIC_NUMBER) - Self::MAGIC_NUMBER;
    rounded as i64
  }

  #[inline(always)]
  fn decode_from_int(encoded: Self::Int, fac_int: i64, frac_exp: Self) -> Self {
    let int_with_fac = if fac_int == 1 {
      encoded
    } else {
      encoded.wrapping_mul(fac_int)
    };
    (int_with_fac as f64) * frac_exp
  }

  #[inline(always)]
  fn decode_from_offset(offset: u64, base: Self::Int, fac_int: i64, frac_exp: Self) -> Self {
    let unscaled = (offset as i64).wrapping_add(base);
    let int_with_fac = if fac_int == 1 {
      unscaled
    } else {
      unscaled.wrapping_mul(fac_int)
    };
    (int_with_fac as f64) * frac_exp
  }

  #[inline(always)]
  fn decode_from_offset_fac1(offset: u64, base: Self::Int, frac_exp: Self) -> Self {
    let unscaled = (offset as i64).wrapping_add(base);
    (unscaled as f64) * frac_exp
  }

  #[inline(always)]
  fn int_diff_to_u64(val: Self::Int, base: Self::Int) -> u64 {
    val.wrapping_sub(base) as u64
  }

  #[inline(always)]
  fn u64_to_int_add(offset: u64, base: Self::Int) -> Self::Int {
    (offset as i64).wrapping_add(base)
  }

  #[inline(always)]
  fn calc_range(min_val: Self::Int, max_val: Self::Int) -> u64 {
    max_val.wrapping_sub(min_val) as u64
  }

  #[inline(always)]
  fn to_raw_bits(self) -> Self::RawBits {
    self.to_bits()
  }

  #[inline(always)]
  fn from_raw_bits(bits: Self::RawBits) -> Self {
    f64::from_bits(bits)
  }

  #[inline(always)]
  fn write_base(base: Self::Int, dst: &mut Vec<u8>) {
    dst.extend_from_slice(&base.to_le_bytes());
  }

  #[inline(always)]
  fn read_base(src: &[u8]) -> Self::Int {
    // SAFETY: 调用方在进入前已校验 src.len() >= BASE_SIZE (8)，使用 read_unaligned 保证任何内存对齐下的安全读取。
    unsafe { i64::from_le(read_unaligned(src.as_ptr().cast::<i64>())) }
  }

  #[inline(always)]
  fn write_exception(pos: u16, bits: Self::RawBits, dst: &mut Vec<u8>) {
    dst.extend_from_slice(&pos.to_le_bytes());
    dst.extend_from_slice(&bits.to_le_bytes());
  }

  #[inline(always)]
  fn read_exception(chunk: &[u8]) -> (usize, Self) {
    // SAFETY: 调用方在进入前已校验 chunk.len() >= EXC_ENTRY_SIZE (10)，使用 read_unaligned 保证安全读取 u16 与 u64。
    unsafe {
      let pos = u16::from_le(read_unaligned(chunk.as_ptr().cast::<u16>())) as usize;
      let bits = u64::from_le(read_unaligned(
        chunk.as_ptr().add(EXC_POS_LEN).cast::<u64>(),
      ));
      (pos, f64::from_bits(bits))
    }
  }
}

impl AlpFloat for f32 {
  type Int = i32;
  type RawBits = u32;

  const TYPE_BYTE: u8 = TYPE_F32;
  const TYPE_RAW_BYTE: u8 = TYPE_F32_RAW;
  const MAX_EXPONENT: u8 = MAX_EXPONENT_F32;
  const MAX_FAC: u8 = MAX_FAC_F32;
  const MAX_BIT_WIDTH: u8 = u32::BITS as u8;
  const MAGIC_NUMBER: Self = MAGIC_NUMBER_F32;
  const ENCODING_UPPER_LIMIT: Self = ENCODING_UPPER_LIMIT_F32;
  const EXC_ENTRY_SIZE: usize = EXC_POS_LEN + size_of::<Self::RawBits>();
  const EXCEPTION_PENALTY: usize = Self::EXC_ENTRY_SIZE * BITS_PER_BYTE;
  const BASE_SIZE: usize = size_of::<Self::Int>();
  const ZERO: Self = 0.0;
  const ZERO_INT: Self::Int = 0;
  const MIN_INT: Self::Int = i32::MIN;
  const MAX_INT: Self::Int = i32::MAX;

  #[inline(always)]
  fn exp_factor(exp: u8, fac: u8) -> Self {
    // SAFETY: 调用方已前置校验 fac <= exp <= MAX_EXPONENT_F32 (10)，且 EXP_ARR_F32 长度为 11，(exp - fac) 必然在 [0, 10] 范围内，索引绝不越界。
    unsafe { *EXP_ARR_F32.get_unchecked((exp - fac) as usize) }
  }

  #[inline(always)]
  fn fac_int(fac: u8) -> i64 {
    // SAFETY: 调用方已前置校验 fac <= MAX_FAC (4) <= 10，且 FACT_ARR_F32 长度为 11，fac 必然在 [0, 4] 范围内，索引绝不越界。
    unsafe { *FACT_ARR_F32.get_unchecked(fac as usize) }
  }

  #[inline(always)]
  fn frac_exp(exp: u8) -> Self {
    // SAFETY: 调用方已前置校验 exp <= MAX_EXPONENT_F32 (10)，且 FRAC_ARR_F32 长度为 11，exp 必然在 [0, 10] 范围内，索引绝不越界。
    unsafe { *FRAC_ARR_F32.get_unchecked(exp as usize) }
  }

  #[inline(always)]
  fn is_impossible(self) -> bool {
    !self.is_finite()
      || self.abs() > Self::ENCODING_UPPER_LIMIT
      || (self == Self::ZERO && self.is_sign_negative())
  }

  #[inline(always)]
  fn try_encode_fast(self, exp_factor: Self, fac_int: i64, frac_exp: Self) -> Option<Self::Int> {
    if self.is_impossible() {
      return None;
    }
    let scaled = self * exp_factor;
    if scaled.is_impossible() {
      return None;
    }
    let rounded = (scaled + Self::MAGIC_NUMBER) - Self::MAGIC_NUMBER;
    let encoded = rounded as i32;

    let int_with_fac = if fac_int == 1 {
      encoded as i64
    } else {
      (encoded as i64).checked_mul(fac_int)?
    };
    let decoded = (int_with_fac as f32) * frac_exp;
    if decoded.to_bits() == self.to_bits() {
      Some(encoded)
    } else {
      None
    }
  }

  #[inline(always)]
  fn fast_round_to_int(self, exp_factor: Self) -> Self::Int {
    let scaled = self * exp_factor;
    let rounded = (scaled + Self::MAGIC_NUMBER) - Self::MAGIC_NUMBER;
    rounded as i32
  }

  #[inline(always)]
  fn decode_from_int(encoded: Self::Int, fac_int: i64, frac_exp: Self) -> Self {
    let int_with_fac = if fac_int == 1 {
      encoded as i64
    } else {
      (encoded as i64).wrapping_mul(fac_int)
    };
    (int_with_fac as f32) * frac_exp
  }

  #[inline(always)]
  fn decode_from_offset(offset: u64, base: Self::Int, fac_int: i64, frac_exp: Self) -> Self {
    let unscaled = (offset as i32).wrapping_add(base);
    let int_with_fac = if fac_int == 1 {
      unscaled as i64
    } else {
      (unscaled as i64).wrapping_mul(fac_int)
    };
    (int_with_fac as f32) * frac_exp
  }

  #[inline(always)]
  fn decode_from_offset_fac1(offset: u64, base: Self::Int, frac_exp: Self) -> Self {
    let unscaled = (offset as i32).wrapping_add(base);
    (unscaled as f32) * frac_exp
  }

  #[inline(always)]
  fn int_diff_to_u64(val: Self::Int, base: Self::Int) -> u64 {
    val.wrapping_sub(base) as u32 as u64
  }

  #[inline(always)]
  fn u64_to_int_add(offset: u64, base: Self::Int) -> Self::Int {
    (offset as i32).wrapping_add(base)
  }

  #[inline(always)]
  fn calc_range(min_val: Self::Int, max_val: Self::Int) -> u64 {
    max_val.wrapping_sub(min_val) as u32 as u64
  }

  #[inline(always)]
  fn to_raw_bits(self) -> Self::RawBits {
    self.to_bits()
  }

  #[inline(always)]
  fn from_raw_bits(bits: Self::RawBits) -> Self {
    f32::from_bits(bits)
  }

  #[inline(always)]
  fn write_base(base: Self::Int, dst: &mut Vec<u8>) {
    dst.extend_from_slice(&base.to_le_bytes());
  }

  #[inline(always)]
  fn read_base(src: &[u8]) -> Self::Int {
    // SAFETY: 调用方在进入前已校验 src.len() >= BASE_SIZE (4)，使用 read_unaligned 保证任何内存对齐下的安全读取。
    unsafe { i32::from_le(read_unaligned(src.as_ptr().cast::<i32>())) }
  }

  #[inline(always)]
  fn write_exception(pos: u16, bits: Self::RawBits, dst: &mut Vec<u8>) {
    dst.extend_from_slice(&pos.to_le_bytes());
    dst.extend_from_slice(&bits.to_le_bytes());
  }

  #[inline(always)]
  fn read_exception(chunk: &[u8]) -> (usize, Self) {
    // SAFETY: 调用方在进入前已校验 chunk.len() >= EXC_ENTRY_SIZE (6)，使用 read_unaligned 保证安全读取 u16 与 u32。
    unsafe {
      let pos = u16::from_le(read_unaligned(chunk.as_ptr().cast::<u16>())) as usize;
      let bits = u32::from_le(read_unaligned(
        chunk.as_ptr().add(EXC_POS_LEN).cast::<u32>(),
      ));
      (pos, f32::from_bits(bits))
    }
  }
}
