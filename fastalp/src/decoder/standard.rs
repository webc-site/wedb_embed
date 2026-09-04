use crate::{
  bitpack::{AlpDecoder, bitunpack_core_generic, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
  params::AlpParams,
};

/// Decodes standard Frame-of-Reference (FOR) ALP block into raw pointer memory (src is payload after header, zero-heap allocation).
/// 解压标准基准值对齐（FOR）ALP 数据块至裸指针内存 (src 为头部之后的有效载荷，零堆分配)
///
/// # Safety
///
/// `dst_ptr` must point to valid memory for at least `count` continuous writable `F` elements.
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline]
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

  let payload = &src[cursor..];
  dispatch_decoder!(params, base, F, decoder => {
    // SAFETY: Valid byte length verified above and caller guarantees sufficient buffer space
    // SAFETY: 上方已校验 cursor + F::BASE_SIZE 字节，且调用方保证 dst_ptr 具有 count 空间
    unsafe {
      decode_standard_inner(payload, count, params, decoder, dst_ptr)?;
    }
  });

  if params.bit_width > 0 {
    let packed_len = packed_byte_size(count, params.bit_width);
    cursor += packed_len;
  }

  // Restore exceptions (patch dictionary)
  // 恢复异常值（Patch 字典）
  unsafe {
    super::patch_exceptions(&src[cursor..], count, dst_ptr)?;
  }

  Ok(())
}

#[inline(always)]
unsafe fn decode_standard_inner<F: AlpFloat, D: AlpDecoder<F>>(
  payload: &[u8],
  count: usize,
  params: AlpParams,
  decoder: D,
  dst_ptr: *mut F,
) -> Result<()> {
  let packed_len = packed_byte_size(count, params.bit_width);
  if payload.len() < packed_len {
    return Err(Error::UnexpectedEof {
      needed: packed_len,
      available: payload.len(),
    });
  }

  // SAFETY: Caller guarantees sufficient buffer and valid pointers.
  // bitunpack_core_generic 自动无缝处理 bit_width == 0 与 1..=64 位宽的寄存器级融合解码
  unsafe {
    bitunpack_core_generic(
      &payload[..packed_len],
      count,
      params.bit_width,
      decoder,
      dst_ptr,
    );
  }
  Ok(())
}
