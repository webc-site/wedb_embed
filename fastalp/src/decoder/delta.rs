use crate::{
  bitpack::{bitunpack_u64_slice, packed_byte_size},
  constants::{EXC_COUNT_LEN, HEADER_LEN},
  error::{Error, Result},
  float::AlpFloat,
};

/// Decodes an ALP Delta differential compressed block into `dst`.
/// 解压 ALP Delta 一阶差分压缩数据块至 `dst` 缓冲区
pub fn decode_delta<F: AlpFloat>(
  src: &[u8],
  count: usize,
  exp: u8,
  fac: u8,
  delta_bit_width: u8,
  use_div: bool,
  dst: &mut Vec<F>,
) -> Result<()> {
  let mut cursor = HEADER_LEN;

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

  let start_idx = dst.len();
  let exp_factor = F::exp_factor(exp, fac);
  let fac_int = F::fac_int(fac);
  let frac_flt = F::frac_exp(exp);

  if count == 1 {
    let val = if use_div {
      F::decode_from_int_div(first, exp_factor)
    } else {
      F::decode_from_int(first, fac_int, frac_flt)
    };
    dst.push(val);
  } else if delta_bit_width == 0 {
    dst.reserve(count);
    // SAFETY: dst 已预留 count 个空间，使用底层指针切片单遍写入并更新有效长度，彻底消除 resize 的双重写零开销
    unsafe {
      let ptr = dst.as_mut_ptr().add(start_idx);
      if use_div {
        *ptr = F::decode_from_int_div(first, exp_factor);
        let mut curr = first;
        for i in 1..count {
          curr = F::int_add(curr, min_delta);
          *ptr.add(i) = F::decode_from_int_div(curr, exp_factor);
        }
      } else {
        *ptr = F::decode_from_int(first, fac_int, frac_flt);
        let mut curr = first;
        for i in 1..count {
          curr = F::int_add(curr, min_delta);
          *ptr.add(i) = F::decode_from_int(curr, fac_int, frac_flt);
        }
      }
      dst.set_len(start_idx + count);
    }
  } else {
    let rest_count = count - 1;
    let packed_len = packed_byte_size(rest_count, delta_bit_width);
    if src.len() < cursor + packed_len {
      return Err(Error::UnexpectedEof {
        needed: cursor + packed_len,
        available: src.len(),
      });
    }

    let mut stack_offsets = [0u64; 1024];
    let mut heap_offsets;
    let offsets_slice: &mut [u64] = if rest_count <= 1024 {
      &mut stack_offsets[..rest_count]
    } else {
      heap_offsets = vec![0u64; rest_count];
      &mut heap_offsets[..]
    };

    bitunpack_u64_slice(
      &src[cursor..cursor + packed_len],
      rest_count,
      delta_bit_width,
      offsets_slice,
    )?;
    cursor += packed_len;

    dst.reserve(count);
    // SAFETY: dst 已 reserve(count)，循环按前缀和快速递推写满 count 个元素，最后 set_len 更新长度
    unsafe {
      let ptr = dst.as_mut_ptr().add(start_idx);
      if use_div {
        *ptr = F::decode_from_int_div(first, exp_factor);
        let mut curr = first;
        let (chunks, rem) = offsets_slice.as_chunks::<4>();
        let mut idx = 1;
        for chunk in chunks {
          let d0 = F::u64_to_int_add(chunk[0], min_delta);
          let d1 = F::u64_to_int_add(chunk[1], min_delta);
          let d2 = F::u64_to_int_add(chunk[2], min_delta);
          let d3 = F::u64_to_int_add(chunk[3], min_delta);

          let c0 = F::int_add(curr, d0);
          let c1 = F::int_add(c0, d1);
          let c2 = F::int_add(c1, d2);
          let c3 = F::int_add(c2, d3);
          curr = c3;

          *ptr.add(idx) = F::decode_from_int_div(c0, exp_factor);
          *ptr.add(idx + 1) = F::decode_from_int_div(c1, exp_factor);
          *ptr.add(idx + 2) = F::decode_from_int_div(c2, exp_factor);
          *ptr.add(idx + 3) = F::decode_from_int_div(c3, exp_factor);
          idx += 4;
        }
        for &offset in rem {
          let delta = F::u64_to_int_add(offset, min_delta);
          curr = F::int_add(curr, delta);
          *ptr.add(idx) = F::decode_from_int_div(curr, exp_factor);
          idx += 1;
        }
      } else {
        *ptr = F::decode_from_int(first, fac_int, frac_flt);
        let mut curr = first;
        if fac_int == 1 {
          let (chunks, rem) = offsets_slice.as_chunks::<4>();
          let mut idx = 1;
          for chunk in chunks {
            let d0 = F::u64_to_int_add(chunk[0], min_delta);
            let d1 = F::u64_to_int_add(chunk[1], min_delta);
            let d2 = F::u64_to_int_add(chunk[2], min_delta);
            let d3 = F::u64_to_int_add(chunk[3], min_delta);

            let c0 = F::int_add(curr, d0);
            let c1 = F::int_add(c0, d1);
            let c2 = F::int_add(c1, d2);
            let c3 = F::int_add(c2, d3);
            curr = c3;

            *ptr.add(idx) = F::decode_from_int(c0, 1, frac_flt);
            *ptr.add(idx + 1) = F::decode_from_int(c1, 1, frac_flt);
            *ptr.add(idx + 2) = F::decode_from_int(c2, 1, frac_flt);
            *ptr.add(idx + 3) = F::decode_from_int(c3, 1, frac_flt);
            idx += 4;
          }
          for &offset in rem {
            let delta = F::u64_to_int_add(offset, min_delta);
            curr = F::int_add(curr, delta);
            *ptr.add(idx) = F::decode_from_int(curr, 1, frac_flt);
            idx += 1;
          }
        } else {
          let (chunks, rem) = offsets_slice.as_chunks::<4>();
          let mut idx = 1;
          for chunk in chunks {
            let d0 = F::u64_to_int_add(chunk[0], min_delta);
            let d1 = F::u64_to_int_add(chunk[1], min_delta);
            let d2 = F::u64_to_int_add(chunk[2], min_delta);
            let d3 = F::u64_to_int_add(chunk[3], min_delta);

            let c0 = F::int_add(curr, d0);
            let c1 = F::int_add(c0, d1);
            let c2 = F::int_add(c1, d2);
            let c3 = F::int_add(c2, d3);
            curr = c3;

            *ptr.add(idx) = F::decode_from_int(c0, fac_int, frac_flt);
            *ptr.add(idx + 1) = F::decode_from_int(c1, fac_int, frac_flt);
            *ptr.add(idx + 2) = F::decode_from_int(c2, fac_int, frac_flt);
            *ptr.add(idx + 3) = F::decode_from_int(c3, fac_int, frac_flt);
            idx += 4;
          }
          for &offset in rem {
            let delta = F::u64_to_int_add(offset, min_delta);
            curr = F::int_add(curr, delta);
            *ptr.add(idx) = F::decode_from_int(curr, fac_int, frac_flt);
            idx += 1;
          }
        }
      }
      dst.set_len(start_idx + count);
    }
  }

  // 恢复异常值（Patch 字典）
  if cursor < src.len() {
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
      // SAFETY: 上方已写入 count 个元素，pos < count 保证 start_idx + pos 不越界
      unsafe {
        *dst.get_unchecked_mut(start_idx + pos) = val;
      }
    }
  }

  Ok(())
}
