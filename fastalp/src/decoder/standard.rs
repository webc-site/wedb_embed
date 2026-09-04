use crate::{
  bitpack::{bitunpack_core, bitunpack_core_div, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
  params::AlpParams,
};

/// Decodes a standard Frame-of-Reference (FOR) ALP compressed block directly to raw pointer.
/// 解压标准基准值对齐（FOR）ALP 数据块至裸指针内存 (src 为头部之后的有效载荷，零堆分配)
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
pub unsafe fn decode_standard_raw<F: AlpFloat>(
  src: &[u8],
  count: usize,
  params: AlpParams,
  dst_ptr: *mut F,
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
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
    for i in 0..count {
      unsafe {
        *dst_ptr.add(i) = val;
      }
    }
  } else {
    let packed_len = packed_byte_size(count, params.bit_width);
    if src.len() < cursor + packed_len {
      return Err(Error::UnexpectedEof {
        needed: cursor + packed_len,
        available: src.len(),
      });
    }

    if params.use_div {
      unsafe {
        bitunpack_core_div(
          &src[cursor..cursor + packed_len],
          count,
          params.bit_width,
          base,
          exp_factor,
          dst_ptr,
        );
      }
    } else {
      unsafe {
        bitunpack_core(
          &src[cursor..cursor + packed_len],
          count,
          params.bit_width,
          base,
          fac_int,
          frac_flt,
          dst_ptr,
        );
      }
    }
    cursor += packed_len;
  }

  // 恢复异常值（Patch 字典）
  unsafe {
    super::patch_exceptions(&src[cursor..], count, dst_ptr)?;
  }

  Ok(())
}

/// Decodes a standard Frame-of-Reference (FOR) ALP compressed block into `dst` slice.
/// 解压标准基准值对齐（FOR）ALP 数据块至 `dst` 切片 (src 为头部之后的有效载荷，零堆分配)
#[inline(always)]
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
  unsafe { decode_standard_raw(src, count, params, dst.as_mut_ptr()) }
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
  unsafe {
    decode_standard_raw(src, count, params, dst.as_mut_ptr().add(old_len))?;
    dst.set_len(old_len + count);
  }
  Ok(())
}
