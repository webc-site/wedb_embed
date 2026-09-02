use crate::{
  bitpack::{bitunpack_into, bitunpack_into_div, packed_byte_size},
  constants::{EXC_COUNT_LEN, HEADER_LEN},
  error::{Error, Result},
  float::AlpFloat,
};

/// Decodes a standard Frame-of-Reference (FOR) ALP compressed block into `dst`.
/// 解压标准基准值对齐（FOR）ALP 数据块至 `dst` 缓冲区
pub fn decode_standard<F: AlpFloat>(
  src: &[u8],
  count: usize,
  exp: u8,
  fac: u8,
  bit_width: u8,
  use_div: bool,
  dst: &mut Vec<F>,
) -> Result<()> {
  let mut cursor = HEADER_LEN;

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
    dst.reserve(count);
    // SAFETY: dst 已 reserve(count)，循环快速填充常数值并更新有效长度
    unsafe {
      let ptr = dst.as_mut_ptr().add(start_idx);
      for i in 0..count {
        *ptr.add(i) = val;
      }
      dst.set_len(start_idx + count);
    }
    if cursor == src.len() {
      return Ok(());
    }
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

    if cursor == src.len() {
      return Ok(());
    }
  }

  if src.len() < cursor + EXC_COUNT_LEN {
    return Err(Error::UnexpectedEof {
      needed: cursor + EXC_COUNT_LEN,
      available: src.len(),
    });
  }

  let exc_count = u16::from_le_bytes([src[cursor], src[cursor + 1]]) as usize;
  cursor += EXC_COUNT_LEN;

  let exc_bytes_needed = exc_count * F::EXC_ENTRY_SIZE;
  if src.len() < cursor + exc_bytes_needed {
    return Err(Error::UnexpectedEof {
      needed: cursor + exc_bytes_needed,
      available: src.len(),
    });
  }

  for _ in 0..exc_count {
    let (pos, val) = F::read_exception(&src[cursor..cursor + F::EXC_ENTRY_SIZE]);
    cursor += F::EXC_ENTRY_SIZE;

    if pos >= count {
      return Err(Error::CorruptedData { index: pos, count });
    }
    // SAFETY: bitunpack_into 已经向 dst 写入了 count 个元素，因此 dst.len() 此时等于 start_idx + count。上方已校验 pos < count，因此 start_idx + pos 严格小于 dst.len()。
    unsafe {
      *dst.get_unchecked_mut(start_idx + pos) = val;
    }
  }

  Ok(())
}
