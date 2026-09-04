use std::{mem::size_of, ptr::read_unaligned};

use super::AlpFloat;
use crate::constants::{
  BITS_PER_BYTE, ENCODING_UPPER_LIMIT_F64, EXC_POS_LEN, EXC_POS_LEN_U32, EXP_ARR_F64, FACT_ARR_F64,
  FRAC_ARR_F64, MAGIC_NUMBER_F64, MAX_EXPONENT_F64, MAX_FAC_F64, TYPE_F64, TYPE_F64_DEC,
  TYPE_F64_DEC_DELTA, TYPE_F64_DELTA, TYPE_F64_RAW,
};

impl AlpFloat for f64 {
  type Int = i64;
  type RawBits = u64;

  const TYPE_BYTE: u8 = TYPE_F64;
  const TYPE_RAW_BYTE: u8 = TYPE_F64_RAW;
  const TYPE_DELTA_BYTE: u8 = TYPE_F64_DELTA;
  const TYPE_DEC_BYTE: u8 = TYPE_F64_DEC;
  const TYPE_DEC_DELTA_BYTE: u8 = TYPE_F64_DEC_DELTA;
  const MAX_EXPONENT: u8 = MAX_EXPONENT_F64;
  const MAX_FAC: u8 = MAX_FAC_F64;
  const MAX_BIT_WIDTH: u8 = u64::BITS as u8;
  const MAGIC_NUMBER: Self = MAGIC_NUMBER_F64;
  const ENCODING_UPPER_LIMIT: Self = ENCODING_UPPER_LIMIT_F64;
  const EXC_ENTRY_SIZE: usize = EXC_POS_LEN + size_of::<Self::RawBits>();
  const EXC_ENTRY_SIZE_U32: usize = EXC_POS_LEN_U32 + size_of::<Self::RawBits>();
  const EXCEPTION_PENALTY: usize = Self::EXC_ENTRY_SIZE * BITS_PER_BYTE;
  const BASE_SIZE: usize = size_of::<Self::Int>();
  const ZERO: Self = 0.0;
  const ZERO_INT: Self::Int = 0;
  const MIN_INT: Self::Int = i64::MIN;
  const MAX_INT: Self::Int = i64::MAX;

  #[inline(always)]
  fn exp_factor(exp: u8, fac: u8) -> Self {
    debug_assert!(fac <= exp && exp <= Self::MAX_EXPONENT);
    // SAFETY: 调用方已前置校验 fac <= exp <= MAX_EXPONENT_F64 (18)，且 EXP_ARR_F64 长度为 19，(exp - fac) 必然在 [0, 18] 范围内，索引绝不越界。
    unsafe { *EXP_ARR_F64.get_unchecked((exp - fac) as usize) }
  }

  #[inline(always)]
  fn fac_int(fac: u8) -> i64 {
    debug_assert!(fac <= Self::MAX_FAC);
    // SAFETY: 调用方已前置校验 fac <= MAX_FAC (8) <= 18，且 FACT_ARR_F64 长度为 19，fac 必然在 [0, 8] 范围内，索引绝不越界。
    unsafe { *FACT_ARR_F64.get_unchecked(fac as usize) }
  }

  #[inline(always)]
  fn frac_exp(exp: u8) -> Self {
    debug_assert!(exp <= Self::MAX_EXPONENT);
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
    let encoded = scaled.round_ties_even() as i64;

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
  fn try_encode_div(self, exp_factor: Self) -> Option<Self::Int> {
    if self.is_impossible() {
      return None;
    }
    let scaled = self * exp_factor;
    if scaled.is_impossible() {
      return None;
    }
    let encoded = scaled.round_ties_even() as i64;
    let decoded = (encoded as f64) / exp_factor;
    if decoded.to_bits() == self.to_bits() {
      Some(encoded)
    } else {
      None
    }
  }

  #[inline(always)]
  fn fast_round_to_int(self, exp_factor: Self) -> Self::Int {
    (self * exp_factor).round_ties_even() as i64
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
  fn decode_from_int_fac1(encoded: Self::Int, frac_exp: Self) -> Self {
    (encoded as f64) * frac_exp
  }

  #[inline(always)]
  fn decode_from_int_div(encoded: Self::Int, exp_factor: Self) -> Self {
    (encoded as f64) / exp_factor
  }

  #[inline(always)]
  fn decode_from_offset_div(offset: u64, base: Self::Int, exp_factor: Self) -> Self {
    let unscaled = (offset as i64).wrapping_add(base);
    (unscaled as f64) / exp_factor
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
    let mut buf = [0u8; EXC_POS_LEN + size_of::<Self::RawBits>()];
    buf[..EXC_POS_LEN].copy_from_slice(&pos.to_le_bytes());
    buf[EXC_POS_LEN..].copy_from_slice(&bits.to_le_bytes());
    dst.extend_from_slice(&buf);
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

  #[inline(always)]
  fn write_exception_u32(pos: u32, bits: Self::RawBits, dst: &mut Vec<u8>) {
    let mut buf = [0u8; EXC_POS_LEN_U32 + size_of::<Self::RawBits>()];
    buf[..EXC_POS_LEN_U32].copy_from_slice(&pos.to_le_bytes());
    buf[EXC_POS_LEN_U32..].copy_from_slice(&bits.to_le_bytes());
    dst.extend_from_slice(&buf);
  }

  #[inline(always)]
  fn read_exception_u32(chunk: &[u8]) -> (usize, Self) {
    // SAFETY: 调用方在进入前已校验 chunk.len() >= EXC_ENTRY_SIZE_U32 (12)，使用 read_unaligned 保证安全读取 u32 与 u64。
    unsafe {
      let pos = u32::from_le(read_unaligned(chunk.as_ptr().cast::<u32>())) as usize;
      let bits = u64::from_le(read_unaligned(
        chunk.as_ptr().add(EXC_POS_LEN_U32).cast::<u64>(),
      ));
      (pos, f64::from_bits(bits))
    }
  }
}
