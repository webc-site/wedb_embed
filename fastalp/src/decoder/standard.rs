use crate::{
  bitpack::{bitunpack_into, bitunpack_into_div, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
};

/// Decodes a standard Frame-of-Reference (FOR) ALP compressed block into `dst`.
/// 解压标准基准值对齐（FOR）ALP 数据块至 `dst` 缓冲区 (src 为头部之后的有效载荷)
pub fn decode_standard<F: AlpFloat>(
  src: &[u8],
  count: usize,
  exp: u8,
  fac: u8,
  bit_width: u8,
  use_div: bool,
  dst: &mut Vec<F>,
) -> Result<()> {
  let mut cursor = 0;

  if src.len() < cursor + F::BASE_SIZE {
    return Err(Error::UnexpectedEof {
      needed: cursor + F::BASE_SIZE,
      available: src.len(),
    });
  }
  let base = F::read_base(&src[cursor..cursor + F::BASE_SIZE]);
  cursor += F::BASE_SIZE;

  let start_idx = dst.len();
  let exp_factor = F::exp_factor(exp, fac);
  let fac_int = F::fac_int(fac);
  let frac_flt = F::frac_exp(exp);

  if bit_width == 0 {
    let val = if use_div {
      F::decode_from_int_div(base, exp_factor)
    } else {
      F::decode_from_int(base, fac_int, frac_flt)
    };
    dst.resize(start_idx + count, val);
  } else {
    let packed_len = packed_byte_size(count, bit_width);
    if src.len() < cursor + packed_len {
      return Err(Error::UnexpectedEof {
        needed: cursor + packed_len,
        available: src.len(),
      });
    }

    if use_div {
      bitunpack_into_div(
        &src[cursor..cursor + packed_len],
        count,
        bit_width,
        base,
        exp_factor,
        dst,
      )?;
    } else {
      bitunpack_into(
        &src[cursor..cursor + packed_len],
        count,
        bit_width,
        base,
        fac_int,
        frac_flt,
        dst,
      )?;
    }
    cursor += packed_len;
  }

  // 恢复异常值（Patch 字典）
  super::patch_exceptions(&src[cursor..], count, start_idx, dst)?;

  Ok(())
}
