use crate::{
  bitpack::{bitunpack_u64_slice, packed_byte_size},
  error::{Error, Result},
  float::AlpFloat,
  params::AlpParams,
};

/// Decodes an ALP Delta differential compressed block directly to raw pointer.
/// 解压 ALP Delta 一阶差分压缩数据块至裸指针内存 (src 为头部之后的有效载荷，零堆分配)
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
pub unsafe fn decode_delta_raw<F: AlpFloat>(
  src: &[u8],
  count: usize,
  params: AlpParams,
  dst_ptr: *mut F,
) -> Result<()> {
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

  let (exp_factor, fac_int, frac_flt) = params.factors::<F>();

  if count == 1 {
    let val = if params.use_div {
      F::decode_from_int_div(first, exp_factor)
    } else if fac_int == 1 {
      F::decode_from_int_fac1(first, frac_flt)
    } else {
      F::decode_from_int(first, fac_int, frac_flt)
    };
    unsafe {
      *dst_ptr = val;
    }
  } else if params.bit_width == 0 {
    // SAFETY: dst 已具备 count 个空间，使用底层指针单遍写入，消除双重写零开销
    unsafe {
      let ptr = dst_ptr;
      let mut curr = first;
      macro_rules! reconstruct_unrolled {
        ($dec_expr:expr) => {{
          let d2 = F::int_add(min_delta, min_delta);
          let d3 = F::int_add(d2, min_delta);
          let d4 = F::int_add(d2, d2);
          let unroll_end = 1 + ((count - 1) & !3);
          let mut i = 1;
          while i < unroll_end {
            *ptr.add(i) = $dec_expr(F::int_add(curr, min_delta));
            *ptr.add(i + 1) = $dec_expr(F::int_add(curr, d2));
            *ptr.add(i + 2) = $dec_expr(F::int_add(curr, d3));
            *ptr.add(i + 3) = $dec_expr(F::int_add(curr, d4));
            curr = F::int_add(curr, d4);
            i += 4;
          }
          while i < count {
            curr = F::int_add(curr, min_delta);
            *ptr.add(i) = $dec_expr(curr);
            i += 1;
          }
        }};
      }

      if params.use_div {
        *ptr = F::decode_from_int_div(first, exp_factor);
        reconstruct_unrolled!(|c| F::decode_from_int_div(c, exp_factor));
      } else if fac_int == 1 {
        *ptr = F::decode_from_int_fac1(first, frac_flt);
        reconstruct_unrolled!(|c| F::decode_from_int_fac1(c, frac_flt));
      } else {
        *ptr = F::decode_from_int(first, fac_int, frac_flt);
        reconstruct_unrolled!(|c| F::decode_from_int(c, fac_int, frac_flt));
      }
    }
  } else {
    let rest_count = count - 1;
    let packed_len = packed_byte_size(rest_count, params.bit_width);
    if src.len() < cursor + packed_len {
      return Err(Error::UnexpectedEof {
        needed: cursor + packed_len,
        available: src.len(),
      });
    }

    // SAFETY: dst 已具备 count 个空间，按 1024 分批流式解包写入 ptr
    unsafe {
      let ptr = dst_ptr;
      let mut curr = first;
      let packed_slice = &src[cursor..cursor + packed_len];

      if params.use_div {
        *ptr = F::decode_from_int_div(first, exp_factor);
        decode_delta_stream(
          packed_slice,
          rest_count,
          params.bit_width,
          min_delta,
          &mut curr,
          ptr.add(1),
          |c| F::decode_from_int_div(c, exp_factor),
        )?;
      } else if fac_int == 1 {
        *ptr = F::decode_from_int_fac1(first, frac_flt);
        decode_delta_stream(
          packed_slice,
          rest_count,
          params.bit_width,
          min_delta,
          &mut curr,
          ptr.add(1),
          |c| F::decode_from_int_fac1(c, frac_flt),
        )?;
      } else {
        *ptr = F::decode_from_int(first, fac_int, frac_flt);
        decode_delta_stream(
          packed_slice,
          rest_count,
          params.bit_width,
          min_delta,
          &mut curr,
          ptr.add(1),
          |c| F::decode_from_int(c, fac_int, frac_flt),
        )?;
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

/// Decodes an ALP Delta differential compressed block into `dst` slice.
/// 解压 ALP Delta 一阶差分压缩数据块至 `dst` 切片 (src 为头部之后的有效载荷，零堆分配)
#[inline(always)]
pub fn decode_delta_slice<F: AlpFloat>(
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
  unsafe { decode_delta_raw(src, count, params, dst.as_mut_ptr()) }
}

/// Decodes an ALP Delta differential compressed block into `dst`.
/// 解压 ALP Delta 一阶差分压缩数据块至 `dst` 缓冲区 (src 为头部之后的有效载荷)
pub fn decode_delta<F: AlpFloat>(
  src: &[u8],
  count: usize,
  params: AlpParams,
  dst: &mut Vec<F>,
) -> Result<()> {
  let old_len = dst.len();
  dst.reserve(count);
  unsafe {
    decode_delta_raw(src, count, params, dst.as_mut_ptr().add(old_len))?;
    dst.set_len(old_len + count);
  }
  Ok(())
}

/// Helper decoding a batch of delta offsets with 8-way unrolling into destination float pointer.
/// 8路循环展开解码单批 Delta 偏移量至浮点目标指针（采用树状前缀和降低依赖延迟）
///
/// # Safety
///
/// 调用方必须确保 `out_ptr` 具备至少 `offsets.len()` 个元素的连续可写内存空间。
#[inline(always)]
unsafe fn decode_delta_offsets<F: AlpFloat, D: Fn(F::Int) -> F>(
  offsets: &[u64],
  min_delta: F::Int,
  curr: &mut F::Int,
  out_ptr: *mut F,
  decode_fn: &D,
) {
  let (chunks, rem) = offsets.as_chunks::<8>();
  let mut idx = 0;
  // SAFETY: Caller guarantees out_ptr has at least offsets.len() space
  unsafe {
    for chunk in chunks {
      let d0 = F::u64_to_int_add(chunk[0], min_delta);
      let d1 = F::u64_to_int_add(chunk[1], min_delta);
      let d2 = F::u64_to_int_add(chunk[2], min_delta);
      let d3 = F::u64_to_int_add(chunk[3], min_delta);
      let d4 = F::u64_to_int_add(chunk[4], min_delta);
      let d5 = F::u64_to_int_add(chunk[5], min_delta);
      let d6 = F::u64_to_int_add(chunk[6], min_delta);
      let d7 = F::u64_to_int_add(chunk[7], min_delta);

      let s01 = F::int_add(d0, d1);
      let s23 = F::int_add(d2, d3);
      let s45 = F::int_add(d4, d5);
      let s67 = F::int_add(d6, d7);

      let s0123 = F::int_add(s01, s23);
      let s4567 = F::int_add(s45, s67);

      let c0 = F::int_add(*curr, d0);
      let c1 = F::int_add(*curr, s01);
      let c2 = F::int_add(c1, d2);
      let c3 = F::int_add(*curr, s0123);
      let c4 = F::int_add(c3, d4);
      let c5 = F::int_add(c3, s45);
      let c6 = F::int_add(c5, d6);
      let c7 = F::int_add(c3, s4567);
      *curr = c7;

      *out_ptr.add(idx) = decode_fn(c0);
      *out_ptr.add(idx + 1) = decode_fn(c1);
      *out_ptr.add(idx + 2) = decode_fn(c2);
      *out_ptr.add(idx + 3) = decode_fn(c3);
      *out_ptr.add(idx + 4) = decode_fn(c4);
      *out_ptr.add(idx + 5) = decode_fn(c5);
      *out_ptr.add(idx + 6) = decode_fn(c6);
      *out_ptr.add(idx + 7) = decode_fn(c7);
      idx += 8;
    }
    for &offset in rem {
      let delta = F::u64_to_int_add(offset, min_delta);
      *curr = F::int_add(*curr, delta);
      *out_ptr.add(idx) = decode_fn(*curr);
      idx += 1;
    }
  }
}

/// Decodes bitpacked delta stream in 1024-element stack batches (O(1) 空间复杂度，零堆分配，L1 缓存高度友好)
///
/// # Safety
///
/// 调用方必须确保 `out_ptr` 具备至少 `rest_count` 个元素的连续可写内存空间。
#[inline(always)]
unsafe fn decode_delta_stream<F: AlpFloat, D: Fn(F::Int) -> F>(
  packed_slice: &[u8],
  rest_count: usize,
  delta_bit_width: u8,
  min_delta: F::Int,
  curr: &mut F::Int,
  out_ptr: *mut F,
  decode_fn: D,
) -> Result<()> {
  let mut stack_offsets = [0u64; 1024];
  let mut processed = 0;
  let mut packed_offset = 0;
  while processed < rest_count {
    let batch = (rest_count - processed).min(1024);
    let batch_bytes = packed_byte_size(batch, delta_bit_width);
    bitunpack_u64_slice(
      &packed_slice[packed_offset..packed_offset + batch_bytes],
      batch,
      delta_bit_width,
      &mut stack_offsets[..batch],
    )?;
    // SAFETY: Caller guarantees out_ptr has rest_count space, processed + batch <= rest_count
    unsafe {
      decode_delta_offsets::<F, D>(
        &stack_offsets[..batch],
        min_delta,
        curr,
        out_ptr.add(processed),
        &decode_fn,
      );
    }
    packed_offset += batch_bytes;
    processed += batch;
  }
  Ok(())
}
