use core::{array::from_fn, marker::PhantomData};

use crate::{
  constants::{LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT, MAX_DICT_ENTRIES},
  float::AlpFloat,
};

/// Generic trait for ALP floating-point reconstruction decoders.
/// ALP 浮点重构解码器通用抽象 Trait
pub trait AlpDecoder<F: AlpFloat>: Copy {
  /// Reconstructs float from unsigned integer offset
  /// 根据无符号整型偏移量还原浮点数
  fn decode_offset(&self, off: u64) -> F;

  /// Reconstructs float from encoded integer value
  /// 根据已编码整型原值还原浮点数
  #[inline(always)]
  fn decode_int(&self, _val: F::Int) -> F {
    F::ZERO
  }

  /// Builds 1-bit decoding lookup table
  /// 构建 1-bit 解码查找表
  #[inline(always)]
  fn build_lut_1(&self) -> [F; LUT_SIZE_1BIT] {
    [self.decode_offset(0), self.decode_offset(1)]
  }

  /// Builds 2-bit decoding lookup table
  /// 构建 2-bit 解码查找表
  #[inline(always)]
  fn build_lut_2(&self) -> [F; LUT_SIZE_2BIT] {
    [
      self.decode_offset(0),
      self.decode_offset(1),
      self.decode_offset(2),
      self.decode_offset(3),
    ]
  }

  /// Builds 4-bit decoding lookup table
  /// 构建 4-bit 解码查找表
  #[inline(always)]
  fn build_lut_4(&self) -> [F; LUT_SIZE_4BIT] {
    from_fn(|i| self.decode_offset(i as u64))
  }
}

/// High-efficiency decoder for factor == 1 (pure multiplication).
/// 针对纯乘法且因子为 1 (fac_int == 1) 的高效解码器
#[derive(Copy, Clone)]
pub struct AlpFac1Decoder<F: AlpFloat> {
  pub base: F::Int,
  pub frac_flt: F,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpFac1Decoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::decode_from_offset_fac1(off, self.base, self.frac_flt)
  }

  #[inline(always)]
  fn decode_int(&self, val: F::Int) -> F {
    F::decode_from_int_fac1(val, self.frac_flt)
  }
}

/// General multiplier decoder for fac_int != 1.
/// 针对带因子乘法 (fac_int != 1) 的通用乘法解码器
#[derive(Copy, Clone)]
pub struct AlpMulDecoder<F: AlpFloat> {
  pub base: F::Int,
  pub fac_int: i64,
  pub frac_flt: F,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpMulDecoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::decode_from_offset(off, self.base, self.fac_int, self.frac_flt)
  }

  #[inline(always)]
  fn decode_int(&self, val: F::Int) -> F {
    F::decode_from_int(val, self.fac_int, self.frac_flt)
  }
}

/// Decimal division decoder for division mode (use_div == true).
/// 针对除法模式 (use_div == true) 的十进制除法解码器
#[derive(Copy, Clone)]
pub struct AlpDivDecoder<F: AlpFloat> {
  pub base: F::Int,
  pub exp_factor: F,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpDivDecoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::decode_from_offset_div(off, self.base, self.exp_factor)
  }

  #[inline(always)]
  fn decode_int(&self, val: F::Int) -> F {
    F::decode_from_int_div(val, self.exp_factor)
  }
}

/// Real Doubles (ALP-RD) constant high-bits fused decoder.
/// 针对高位阶码恒定的 ALP-RD 融合单趟解码器
#[derive(Copy, Clone)]
pub struct AlpRdConstantDecoder<F: AlpFloat> {
  pub high_bits: u64,
  pub _phantom: PhantomData<F>,
}

impl<F: AlpFloat> AlpDecoder<F> for AlpRdConstantDecoder<F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    F::from_u64_raw(self.high_bits | off)
  }
}

/// Compact low-cardinality dictionary fused decoder.
/// 低基数紧凑字典融合单趟解码器（持有轻量 8 字节引用，消除 512B 栈复制）
#[derive(Copy, Clone)]
pub struct AlpDictDecoder<'a, F: AlpFloat> {
  pub dict: &'a [F; MAX_DICT_ENTRIES],
}

impl<'a, F: AlpFloat> AlpDecoder<F> for AlpDictDecoder<'a, F> {
  #[inline(always)]
  fn decode_offset(&self, off: u64) -> F {
    // SAFETY: off & (MAX_DICT_ENTRIES - 1) 严格限制在 [0, 63] 内，dict 具备 64 项有效元素
    unsafe {
      *self
        .dict
        .get_unchecked((off as usize) & (MAX_DICT_ENTRIES - 1))
    }
  }
}
