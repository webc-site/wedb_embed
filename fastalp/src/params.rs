use crate::{constants::BITS_U64, float::AlpFloat, sampler::BestParams};

/// Packed parameters bitfield masks and shift constants.
/// 参数位域掩码与位移常量
pub const EXP_MASK: u16 = 0x001F;
pub const FAC_SHIFT: u16 = 5;
pub const FAC_MASK: u16 = 0x000F;
pub const BIT_WIDTH_SHIFT: u16 = 9;
pub const BIT_WIDTH_MASK: u16 = 0x007F;

/// Packs exponent (5b), factor (4b), and bit width (7b) into a 2-byte unsigned integer.
/// 将 exp (5b), fac (4b), bit_width (7b) 打包进 2 字节 u16
#[inline(always)]
pub const fn pack_params(exp: u8, fac: u8, bit_width: u8) -> u16 {
  ((exp as u16) & EXP_MASK)
    | (((fac as u16) & FAC_MASK) << FAC_SHIFT)
    | (((bit_width as u16) & BIT_WIDTH_MASK) << BIT_WIDTH_SHIFT)
}

/// Unpacks (exponent, factor, bit width) from a 2-byte unsigned integer.
/// 从 2 字节 u16 解包 (exp, fac, bit_width)
#[inline(always)]
pub const fn unpack_params(params: u16) -> (u8, u8, u8) {
  let exp = (params & EXP_MASK) as u8;
  let fac = ((params >> FAC_SHIFT) & FAC_MASK) as u8;
  let bit_width = ((params >> BIT_WIDTH_SHIFT) & BIT_WIDTH_MASK) as u8;
  (exp, fac, bit_width)
}

/// Calculates minimum bit width required to represent an unsigned integer (0..=64).
/// 快速计算表示数值所需的最少比特位数 (0..=64，无分支实现)
#[inline(always)]
pub const fn bits_needed(max_val: u64) -> u8 {
  (u64::BITS - max_val.leading_zeros()) as u8
}

/// Computes lower bitmask for a given bit width (0..=64).
/// 快速计算 0..=64 比特宽度的低位掩码
#[inline(always)]
pub const fn bit_mask(bit_width: u8) -> u64 {
  if bit_width >= BITS_U64 as u8 {
    u64::MAX
  } else {
    (1u64 << bit_width).wrapping_sub(1)
  }
}

/// Compact ALP encoding and decoding parameters.
/// 封装 ALP 核心参数 (exp, fac, bit_width, use_div)，消除多参数冗余传递并统一头部打包与类型判断
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlpParams {
  pub exp: u8,
  pub fac: u8,
  pub bit_width: u8,
  pub use_div: bool,
}

impl AlpParams {
  #[inline(always)]
  pub const fn new(exp: u8, fac: u8, bit_width: u8, use_div: bool) -> Self {
    Self {
      exp,
      fac,
      bit_width,
      use_div,
    }
  }

  #[inline(always)]
  pub const fn from_best_params(best: BestParams, bit_width: u8) -> Self {
    Self {
      exp: best.exp,
      fac: best.fac,
      bit_width,
      use_div: best.use_div,
    }
  }

  #[inline(always)]
  pub const fn pack(&self) -> u16 {
    pack_params(self.exp, self.fac, self.bit_width)
  }

  #[inline(always)]
  pub const fn from_packed(packed: u16, use_div: bool) -> Self {
    let (exp, fac, bit_width) = unpack_params(packed);
    Self {
      exp,
      fac,
      bit_width,
      use_div,
    }
  }

  #[inline(always)]
  pub const fn standard_type<F: AlpFloat>(&self) -> u8 {
    if self.use_div {
      F::TYPE_DEC_BYTE
    } else {
      F::TYPE_BYTE
    }
  }

  #[inline(always)]
  pub const fn delta_type<F: AlpFloat>(&self) -> u8 {
    if self.use_div {
      F::TYPE_DEC_DELTA_BYTE
    } else {
      F::TYPE_DELTA_BYTE
    }
  }

  #[inline(always)]
  pub const fn validate<F: AlpFloat>(&self) -> bool {
    self.exp <= F::MAX_EXPONENT
      && self.fac <= F::MAX_FAC
      && self.fac <= self.exp
      && self.bit_width <= F::MAX_BIT_WIDTH
  }

  /// Computes decoding float factors (exp_factor, fac_int, frac_flt) for floating-point reconstruction.
  /// 计算浮点重构所需的放大因子、整型因子与负幂小数因子
  #[inline(always)]
  pub fn factors<F: AlpFloat>(&self) -> (F, i64, F) {
    (
      F::exp_factor(self.exp, self.fac),
      F::fac_int(self.fac),
      F::frac_exp(self.exp),
    )
  }
}
