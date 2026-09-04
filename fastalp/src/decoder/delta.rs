use crate::{
  bitpack::{AlpDecoder, AlpDeltaConsumer, bitunpack_core_consumer, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
  params::AlpParams,
};

/// Decodes an ALP Delta differential compressed block directly to raw pointer.
/// 解压 ALP Delta 一阶差分压缩数据块至裸指针内存 (src 为头部之后的有效载荷，零堆分配)
///
/// # Safety
///
/// `dst_ptr` must point to valid memory for at least `count` continuous writable `F` elements.
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline]
pub unsafe fn decode_delta_raw<F: AlpFloat>(
  src: &[u8],
  count: usize,
  params: AlpParams,
  dst_ptr: *mut F,
) -> Result<()> {
  if count == 0 {
    return Ok(());
  }
  let mut cursor = 0;

  if src.len() < cursor + F::BASE_SIZE * 2 {
    return Err(Error::UnexpectedEof {
      needed: cursor + F::BASE_SIZE * 2,
      available: src.len(),
    });
  }

  let first = F::read_base(&src[cursor..cursor + F::BASE_SIZE]);
  cursor += F::BASE_SIZE;

  let min_delta = F::read_base(&src[cursor..cursor + F::BASE_SIZE]);
  cursor += F::BASE_SIZE;

  let payload = &src[cursor..];
  dispatch_decoder!(params, first, F, decoder => {
    // SAFETY: Valid byte count verified above, and caller guarantees dst_ptr has count space
    // SAFETY: 上方已校验有效字节数，且调用方保证 dst_ptr 具有 count 空间
    unsafe {
      decode_delta_inner(payload, count, params, decoder, first, min_delta, dst_ptr)?;
    }
  });

  if params.bit_width > 0 && count > 1 {
    let packed_len = packed_byte_size(count - 1, params.bit_width);
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
unsafe fn decode_delta_inner<F: AlpFloat, D: AlpDecoder<F>>(
  payload: &[u8],
  count: usize,
  params: AlpParams,
  decoder: D,
  first: F::Int,
  min_delta: F::Int,
  dst_ptr: *mut F,
) -> Result<()> {
  unsafe {
    *dst_ptr = decoder.decode_int(first);
  }
  if count == 1 {
    return Ok(());
  }

  let rest_count = count - 1;
  let packed_len = packed_byte_size(rest_count, params.bit_width);
  if payload.len() < packed_len {
    return Err(Error::UnexpectedEof {
      needed: packed_len,
      available: payload.len(),
    });
  }

  // SAFETY: dst_ptr has count slots guaranteed by caller; unpack and reconstruct in a single fused pass
  // SAFETY: 调用方保证 dst_ptr 具备 count 空间；在单趟流水线中完成位解包与前缀和浮点重构
  unsafe {
    let consumer = AlpDeltaConsumer::new(first, min_delta, decoder);
    bitunpack_core_consumer(
      &payload[..packed_len],
      rest_count,
      params.bit_width,
      consumer,
      dst_ptr.add(1),
    );
  }
  Ok(())
}
