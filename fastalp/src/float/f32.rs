use std::{mem::size_of, ptr::read_unaligned};

use super::AlpFloat;
use crate::constants::{
  BITS_PER_BYTE, ENCODING_UPPER_LIMIT_F32, EXC_POS_LEN, EXP_ARR_F32, FACT_ARR_F32, FRAC_ARR_F32,
  MAGIC_NUMBER_F32, MAX_EXPONENT_F32, MAX_FAC_F32, TYPE_F32, TYPE_F32_DEC, TYPE_F32_DEC_DELTA,
  TYPE_F32_DELTA, TYPE_F32_RAW,
};

impl AlpFloat for f32 {
  type Int = i32;
  type RawBits = u32;

  const TYPE_BYTE: u8 = TYPE_F32;
  const TYPE_RAW_BYTE: u8 = TYPE_F32_RAW;
  const TYPE_DELTA_BYTE: u8 = TYPE_F32_DELTA;
  const TYPE_DEC_BYTE: u8 = TYPE_F32_DEC;
  const TYPE_DEC_DELTA_BYTE: u8 = TYPE_F32_DEC_DELTA;
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
  fn try_encode_div(self, exp_factor: Self) -> Option<Self::Int> {
    if self.is_impossible() {
      return None;
    }
    let scaled = self * exp_factor;
    if scaled.is_impossible() {
      return None;
    }
    let rounded = (scaled + Self::MAGIC_NUMBER) - Self::MAGIC_NUMBER;
    let encoded = rounded as i32;
    let decoded = (encoded as f32) / exp_factor;
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
  fn decode_from_int_div(encoded: Self::Int, exp_factor: Self) -> Self {
    (encoded as f32) / exp_factor
  }

  #[inline(always)]
  fn decode_from_offset_div(offset: u64, base: Self::Int, exp_factor: Self) -> Self {
    let unscaled = (offset as i32).wrapping_add(base);
    (unscaled as f32) / exp_factor
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
  fn int_sub(a: Self::Int, b: Self::Int) -> Self::Int {
    a.wrapping_sub(b)
  }

  #[inline(always)]
  fn int_add(a: Self::Int, b: Self::Int) -> Self::Int {
    a.wrapping_add(b)
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
