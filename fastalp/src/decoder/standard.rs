use core::slice::from_raw_parts_mut;

use crate::{
  bitpack::{AlpDecoder, bitunpack_core_generic, bitunpack_u64_slice, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
  params::AlpParams,
};

/// Stack batch unpack scratch buffer size
/// 栈上批量位解包暂存块大小
const DECODE_BATCH_SIZE: usize = 1024;

/// Dedicated bit-widths with vectorized inlining
/// 具备针对性内联向量化优化的专用位宽表
const SPECIAL_BW: [u8; 7] = [1, 2, 4, 8, 16, 32, 64];

/// Decodes standard Frame-of-Reference (FOR) ALP block into raw pointer memory (src is payload after header, zero-heap allocation).
/// 解压标准基准值对齐（FOR）ALP 数据块至裸指针内存 (src 为头部之后的有效载荷，零堆分配)
///
/// # Safety
///
/// `dst_ptr` must point to valid memory for at least `count` continuous writable `F` elements.
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
  if params.bit_width == 0 {
    let val = decoder.decode_offset(0);
    // SAFETY: dst_ptr has sufficient capacity, perform direct SIMD broadcast fill
    // SAFETY: dst_ptr 具有至少 count 个连续元素的可写空间，直接进行 SIMD 广播填充
    unsafe {
      from_raw_parts_mut(dst_ptr, count).fill(val);
    }
  } else {
    let packed_len = packed_byte_size(count, params.bit_width);
    if payload.len() < packed_len {
      return Err(Error::UnexpectedEof {
        needed: packed_len,
        available: payload.len(),
      });
    }

    if SPECIAL_BW.contains(&params.bit_width) || params.bit_width > 32 {
      // SAFETY: Caller guarantees sufficient buffer and valid pointers
      // SAFETY: 调用方保证缓冲区与指针充足有效
      unsafe {
        bitunpack_core_generic(
          &payload[..packed_len],
          count,
          params.bit_width,
          decoder,
          dst_ptr,
        );
      }
    } else {
      let mut stack_offsets = [0u64; DECODE_BATCH_SIZE];
      let mut processed = 0;
      let mut pack_off = 0;
      while processed < count {
        let batch = (count - processed).min(DECODE_BATCH_SIZE);
        let batch_bytes = packed_byte_size(batch, params.bit_width);
        bitunpack_u64_slice(
          &payload[pack_off..pack_off + batch_bytes],
          batch,
          params.bit_width,
          &mut stack_offsets[..batch],
        )?;
        let out = unsafe { dst_ptr.add(processed) };
        // SAFETY: out pointer points to batch continuous valid writable float slots
        // SAFETY: out 指针指向 processed 开始的 batch 个有效连续可写浮点内存
        let out_slice = unsafe { from_raw_parts_mut(out, batch) };
        for (&off, dst) in stack_offsets[..batch].iter().zip(out_slice.iter_mut()) {
          *dst = decoder.decode_offset(off);
        }
        pack_off += batch_bytes;
        processed += batch;
      }
    }
  }
  Ok(())
}

/// Decodes standard FOR ALP block into `dst` slice (src is payload after header, zero-heap allocation).
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

/// Decodes standard FOR ALP block into `dst` vector (src is payload after header).
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
