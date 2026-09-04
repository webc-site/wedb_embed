use core::slice::from_raw_parts_mut;

use crate::{
  bitpack::{bitunpack_slice, bitunpack_slice_div, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
  params::AlpParams,
};

/// Decodes a standard Frame-of-Reference (FOR) ALP compressed block into `dst` slice.
/// 解压标准基准值对齐（FOR）ALP 数据块至 `dst` 切片 (src 为头部之后的有效载荷，零堆分配)
pub fn decode_standard_slice<F: AlpFloat>(
  src: &[u8],
  count: usize,
  params: AlpParams,
  dst: &mut [F],
) -> Result<()> {
  if dst.len() < count {
    return Err(Error::BufferTooSmall {
      needed: count,
      available: dst.len(),
    });
  }
  let dst = &mut dst[..count];
  let mut cursor = 0;

  if src.len() < cursor + F::BASE_SIZE {
    return Err(Error::UnexpectedEof {
      needed: cursor + F::BASE_SIZE,
      available: src.len(),
    });
  }
  let base = F::read_base(&src[cursor..cursor + F::BASE_SIZE]);
  cursor += F::BASE_SIZE;

  let (exp_factor, fac_int, frac_flt) = params.factors::<F>();

  if params.bit_width == 0 {
    let val = if params.use_div {
      F::decode_from_int_div(base, exp_factor)
    } else {
      F::decode_from_int(base, fac_int, frac_flt)
    };
    dst.fill(val);
  } else {
    let packed_len = packed_byte_size(count, params.bit_width);
    if src.len() < cursor + packed_len {
      return Err(Error::UnexpectedEof {
        needed: cursor + packed_len,
        available: src.len(),
      });
    }

    if params.use_div {
      bitunpack_slice_div(
        &src[cursor..cursor + packed_len],
        count,
        params.bit_width,
        base,
        exp_factor,
        dst,
      )?;
    } else {
      bitunpack_slice(
        &src[cursor..cursor + packed_len],
        count,
        params.bit_width,
        base,
        fac_int,
        frac_flt,
        dst,
      )?;
    }
    cursor += packed_len;
  }

  // 恢复异常值（Patch 字典）
  super::patch_exceptions(&src[cursor..], count, dst)?;

  Ok(())
}

/// Decodes a standard Frame-of-Reference (FOR) ALP compressed block into `dst`.
/// 解压标准基准值对齐（FOR）ALP 数据块至 `dst` 缓冲区 (src 为头部之后的有效载荷)
pub fn decode_standard<F: AlpFloat>(
  src: &[u8],
  count: usize,
  params: AlpParams,
  dst: &mut Vec<F>,
) -> Result<()> {
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst 已预留 count 个空间，from_raw_parts_mut 构造切片作为输出缓冲区供解码内核写入；
  // 解码成功后严格安全更新有效长度。
  let slice = unsafe { from_raw_parts_mut(dst.as_mut_ptr().add(old_len), count) };
  decode_standard_slice(src, count, params, slice)?;
  unsafe {
    dst.set_len(old_len + count);
  }
  Ok(())
}
