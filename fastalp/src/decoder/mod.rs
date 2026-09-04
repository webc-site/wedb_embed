/// Dynamically dispatches concrete decoder type according to AlpParams (zero virtual function overhead, monomorphized).
/// 根据 AlpParams 参数动态派发具体的解码器类型（零运行时虚函数开销，单次单态化展开）
macro_rules! dispatch_decoder {
  ($params:expr, $base:expr, $F:ty, $decoder:ident => $body:expr) => {{
    let (exp_factor, fac_int, frac_flt) = $params.factors::<$F>();
    if $params.use_div {
      let $decoder = $crate::bitpack::AlpDivDecoder {
        base: $base,
        exp_factor,
      };
      $body
    } else if fac_int == 1 {
      let $decoder = $crate::bitpack::AlpFac1Decoder {
        base: $base,
        frac_flt,
      };
      $body
    } else {
      let $decoder = $crate::bitpack::AlpMulDecoder {
        base: $base,
        fac_int,
        frac_flt,
      };
      $body
    }
  }};
}

mod delta;
mod standard;

use core::{
  mem::{MaybeUninit, size_of},
  ptr::copy_nonoverlapping,
};
use std::ptr::read_unaligned;

use delta::decode_delta_raw;
use standard::decode_standard_raw;

use crate::{
  bitpack::{bitunpack_u64_slice, packed_byte_size},
  constants::{EXC_COUNT_LEN, EXC_COUNT_LEN_U32, MAX_DICT_ENTRIES},
  error::{Error, Result},
  float::AlpFloat,
  header::{ParsedHeader, read_count, read_header},
  params::AlpParams,
};

#[inline(always)]
fn count_bitmap_ones(bitmap: &[u8], count: usize) -> usize {
  let full_words = count / 64;
  let mut ones = 0usize;
  let ptr = bitmap.as_ptr().cast::<u64>();
  for i in 0..full_words {
    // SAFETY: bitmap has (count + 7) / 8 bytes, which has at least full_words * 8 bytes
    let word = unsafe { ptr.add(i).read_unaligned() };
    ones += word.count_ones() as usize;
  }
  let rem = count % 64;
  if rem > 0 {
    for i in (full_words * 64)..count {
      let b = bitmap[i / 8];
      if (b & (1 << (i % 8))) != 0 {
        ones += 1;
      }
    }
  }
  ones
}

/// Expands repeat run-length bitmap into output buffer.
/// 将时序重复游程位图展开还原至输出缓冲区
///
/// # Safety
///
/// - `non_repeats` 必须指向包含全部非重复元素的连续有效内存。
/// - `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
/// - `bitmap` 必须至少包含 `(count + 7) / 8` 个字节。
#[inline]
pub(crate) unsafe fn expand_repeats<F: AlpFloat>(
  bitmap: &[u8],
  count: usize,
  non_repeats: *const F,
  dst_ptr: *mut F,
) {
  if count == 0 {
    return;
  }
  // SAFETY: 调用方保证 non_repeats 包含足够元素，dst_ptr 在 count 范围内连续可写，bitmap 长度充足
  unsafe {
    let mut src_idx = 0usize;
    let mut prev = *non_repeats;
    *dst_ptr = prev;

    let full_words = count / 64;
    let ptr_u64 = bitmap.as_ptr().cast::<u64>();

    for w in 0..full_words {
      let word = ptr_u64.add(w).read_unaligned();
      let base_out = w * 64;

      if w == 0 {
        for j in 1..64 {
          if (word & (1u64 << j)) != 0 {
            *dst_ptr.add(j) = prev;
          } else {
            src_idx += 1;
            prev = *non_repeats.add(src_idx);
            *dst_ptr.add(j) = prev;
          }
        }
      } else if word == 0 {
        copy_nonoverlapping(non_repeats.add(src_idx + 1), dst_ptr.add(base_out), 64);
        src_idx += 64;
        prev = *dst_ptr.add(base_out + 63);
      } else if word == u64::MAX {
        for j in 0..64 {
          *dst_ptr.add(base_out + j) = prev;
        }
      } else {
        for j in 0..64 {
          if (word & (1u64 << j)) != 0 {
            *dst_ptr.add(base_out + j) = prev;
          } else {
            src_idx += 1;
            prev = *non_repeats.add(src_idx);
            *dst_ptr.add(base_out + j) = prev;
          }
        }
      }
    }

    let rem_start = full_words * 64;
    let start_j = if full_words == 0 { 1 } else { 0 };
    for i in (rem_start + start_j)..count {
      let byte_idx = i / 8;
      let bit_idx = i % 8;
      if (bitmap[byte_idx] & (1 << bit_idx)) != 0 {
        *dst_ptr.add(i) = prev;
      } else {
        src_idx += 1;
        prev = *non_repeats.add(src_idx);
        *dst_ptr.add(i) = prev;
      }
    }
  }
}

/// Decodes dictionary compressed chunk into destination buffer.
/// 解码紧凑字典压缩数据块至目标指针缓冲区
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
unsafe fn decode_dict_raw<F: AlpFloat>(
  payload: &[u8],
  count: usize,
  dst_ptr: *mut F,
) -> Result<()> {
  if payload.len() < 2 {
    return Err(Error::UnexpectedEof {
      needed: 2,
      available: payload.len(),
    });
  }

  let dict_len = payload[0] as usize;
  let bit_width = payload[1];

  if dict_len == 0 || dict_len > MAX_DICT_ENTRIES || bit_width > 6 {
    return Err(Error::InvalidHeader);
  }

  let elem_size = size_of::<F>();
  let dict_bytes = dict_len * elem_size;
  if payload.len() < 2 + dict_bytes {
    return Err(Error::UnexpectedEof {
      needed: 2 + dict_bytes,
      available: payload.len(),
    });
  }

  let mut dict = [F::ZERO; MAX_DICT_ENTRIES];
  let dict_slice = &payload[2..2 + dict_bytes];
  for (entry, chunk) in dict.iter_mut().zip(dict_slice.chunks_exact(elem_size)) {
    *entry = F::read_raw(chunk);
  }

  if bit_width == 0 {
    let single_val = dict[0];
    // SAFETY: 调用方保证 dst_ptr 具有至少 count 个连续有效可写槽位
    unsafe {
      for i in 0..count {
        *dst_ptr.add(i) = single_val;
      }
    }
    return Ok(());
  }

  let indices_offset = 2 + dict_bytes;
  let packed_bytes = packed_byte_size(count, bit_width);
  if payload.len() < indices_offset + packed_bytes {
    return Err(Error::UnexpectedEof {
      needed: indices_offset + packed_bytes,
      available: payload.len(),
    });
  }

  const CHUNK: usize = 1024;
  let mut idx_buf = [0u64; CHUNK];
  let mut out_offset = 0;
  let mut in_offset = indices_offset;

  while out_offset < count {
    let cur_count = (count - out_offset).min(CHUNK);
    let cur_bytes = packed_byte_size(cur_count, bit_width);
    bitunpack_u64_slice(
      &payload[in_offset..in_offset + cur_bytes],
      cur_count,
      bit_width,
      &mut idx_buf[..cur_count],
    )?;

    // SAFETY: cur_count <= CHUNK，i < cur_count，查表前校验 idx < dict_len <= MAX_DICT_ENTRIES，
    // out_offset + i < count 且 dst_ptr 严格在有效范围内。
    unsafe {
      for i in 0..cur_count {
        let idx = *idx_buf.get_unchecked(i) as usize;
        if idx >= dict_len {
          return Err(Error::InvalidHeader);
        }
        *dst_ptr.add(out_offset + i) = *dict.get_unchecked(idx);
      }
    }

    out_offset += cur_count;
    in_offset += cur_bytes;
  }

  Ok(())
}

/// Decodes Real Doubles (ALP-RD) compressed chunk into destination buffer.
/// 解码真实双精度高低位解耦（ALP-RD）压缩数据块至目标指针缓冲区
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
unsafe fn decode_rd_raw<F: AlpFloat>(payload: &[u8], count: usize, dst_ptr: *mut F) -> Result<()> {
  if payload.len() < 5 {
    return Err(Error::UnexpectedEof {
      needed: 5,
      available: payload.len(),
    });
  }

  let right_bw = payload[0];
  let left_bw = payload[1];
  let actual_dict_size = payload[2] as usize;
  if actual_dict_size > 8 || right_bw == 0 || right_bw >= F::RD_TOTAL_BITS || left_bw > 3 {
    return Err(Error::InvalidHeader);
  }

  // SAFETY: 上方已校验 payload.len() >= 5，add(3) 安全读取小端 u16 异常数
  let exc_count =
    unsafe { u16::from_le(read_unaligned(payload.as_ptr().add(3).cast::<u16>())) } as usize;

  let dict_bytes = actual_dict_size * 2;
  let mut cursor = 5;
  if payload.len() < cursor + dict_bytes {
    return Err(Error::UnexpectedEof {
      needed: cursor + dict_bytes,
      available: payload.len(),
    });
  }

  let mut dict = [0u16; 8];
  // SAFETY: 已校验 payload.len() >= cursor + dict_bytes 且 actual_dict_size <= 8，读取字典项不越界
  unsafe {
    let dict_ptr = payload.as_ptr().add(cursor).cast::<u16>();
    for (i, entry) in dict.iter_mut().take(actual_dict_size).enumerate() {
      *entry = u16::from_le(read_unaligned(dict_ptr.add(i)));
    }
  }
  cursor += dict_bytes;

  let left_bytes = if left_bw > 0 {
    packed_byte_size(count, left_bw)
  } else {
    0
  };
  let right_bytes = packed_byte_size(count, right_bw);
  let exc_bytes = exc_count * 4;

  if payload.len() < cursor + left_bytes + right_bytes + exc_bytes {
    return Err(Error::UnexpectedEof {
      needed: cursor + left_bytes + right_bytes + exc_bytes,
      available: payload.len(),
    });
  }

  const CHUNK: usize = 1024;
  let mut left_idx_buf = [0u64; CHUNK];
  let mut right_val_buf = [0u64; CHUNK];

  let mut left_cursor = cursor;
  let mut right_cursor = cursor + left_bytes;
  let exc_cursor = right_cursor + right_bytes;

  let mut out_offset = 0;
  let shift = right_bw as u64;

  let mut shifted_dict = [0u64; 8];
  for (i, &entry) in dict.iter().take(actual_dict_size).enumerate() {
    shifted_dict[i] = (entry as u64) << shift;
  }

  while out_offset < count {
    let cur_count = (count - out_offset).min(CHUNK);
    let cur_left_bytes = if left_bw > 0 {
      packed_byte_size(cur_count, left_bw)
    } else {
      0
    };
    let cur_right_bytes = packed_byte_size(cur_count, right_bw);

    if left_bw > 0 {
      bitunpack_u64_slice(
        &payload[left_cursor..left_cursor + cur_left_bytes],
        cur_count,
        left_bw,
        &mut left_idx_buf[..cur_count],
      )?;
      left_cursor += cur_left_bytes;
    }

    bitunpack_u64_slice(
      &payload[right_cursor..right_cursor + cur_right_bytes],
      cur_count,
      right_bw,
      &mut right_val_buf[..cur_count],
    )?;
    right_cursor += cur_right_bytes;

    // SAFETY: cur_count <= CHUNK，i < cur_count，out_offset + i < count，
    // left_idx & 7 < 8 索引受控于 shifted_dict 容量，dst_ptr 具有充足连续空间。
    unsafe {
      if left_bw == 0 {
        let left = shifted_dict[0];
        for i in 0..cur_count {
          let right = *right_val_buf.get_unchecked(i);
          *dst_ptr.add(out_offset + i) = F::from_u64_raw(left | right);
        }
      } else {
        for i in 0..cur_count {
          let left_idx = *left_idx_buf.get_unchecked(i) as usize;
          let left = *shifted_dict.get_unchecked(left_idx & 7);
          let right = *right_val_buf.get_unchecked(i);
          *dst_ptr.add(out_offset + i) = F::from_u64_raw(left | right);
        }
      }
    }

    out_offset += cur_count;
  }

  // SAFETY: 已校验 payload 长度覆盖至 exc_cursor + exc_bytes，read_unaligned 安全读取，
  // pos < count 严格校验确保 dst_ptr 写入不越界。
  unsafe {
    let exc_ptr = payload.as_ptr().add(exc_cursor);
    for i in 0..exc_count {
      let pos = u16::from_le(read_unaligned(exc_ptr.add(i * 4).cast::<u16>())) as usize;
      let left_val = u16::from_le(read_unaligned(exc_ptr.add(i * 4 + 2).cast::<u16>())) as u64;
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      let cur = (*dst_ptr.add(pos)).to_u64_key();
      let right = cur & ((1u64 << right_bw) - 1);
      let raw = (left_val << shift) | right;
      *dst_ptr.add(pos) = F::from_u64_raw(raw);
    }
  }

  Ok(())
}

/// Decompresses concrete chunk type directly into raw pointer memory without repeat expansion.
/// 解码具体块类型至裸指针内存（未展开时序重复行程）
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个连续可写 `F` 元素的有效内存。
#[inline(always)]
unsafe fn decompress_into_raw_direct<F: AlpFloat>(
  src: &[u8],
  cursor: usize,
  type_byte: u8,
  count: usize,
  params: Option<AlpParams>,
  dst_ptr: *mut F,
) -> Result<()> {
  if type_byte == F::TYPE_RAW_BYTE {
    let raw_bytes_needed = count
      .checked_mul(size_of::<F>())
      .ok_or(Error::InvalidHeader)?;
    if src.len() < cursor + raw_bytes_needed {
      return Err(Error::UnexpectedEof {
        needed: cursor + raw_bytes_needed,
        available: src.len(),
      });
    }
    // SAFETY: 上方已校验可用字节充足，dst_ptr 具备 count 个有效 F 空间，字节大小匹配
    unsafe {
      copy_nonoverlapping(
        src.as_ptr().add(cursor),
        dst_ptr.cast::<u8>(),
        raw_bytes_needed,
      );
    }
    return Ok(());
  }

  if type_byte == F::TYPE_DICT_BYTE {
    // SAFETY: 调用方保证 dst_ptr 具有至少 count 个连续可写槽位
    unsafe {
      decode_dict_raw::<F>(&src[cursor..], count, dst_ptr)?;
    }
    return Ok(());
  }

  if type_byte == F::TYPE_RD_BYTE {
    // SAFETY: 调用方保证 dst_ptr 具有至少 count 个连续可写槽位
    unsafe {
      decode_rd_raw::<F>(&src[cursor..], count, dst_ptr)?;
    }
    return Ok(());
  }

  let is_delta = type_byte == F::TYPE_DELTA_BYTE || type_byte == F::TYPE_DEC_DELTA_BYTE;
  let is_standard = type_byte == F::TYPE_BYTE || type_byte == F::TYPE_DEC_BYTE;

  if !is_standard && !is_delta {
    return Err(Error::InvalidHeader);
  }

  let alp_params = match params {
    Some(p) => p,
    None => return Err(Error::InvalidHeader),
  };

  if !alp_params.validate::<F>() {
    return Err(Error::UnsupportedParams {
      exp: alp_params.exp,
      fac: alp_params.fac,
      bit_width: alp_params.bit_width,
    });
  }

  let payload = &src[cursor..];
  if is_delta {
    // SAFETY: 调用方保证 dst_ptr 具有至少 count 个连续可写槽位
    unsafe {
      decode_delta_raw::<F>(payload, count, alp_params, dst_ptr)?;
    }
  } else {
    // SAFETY: 调用方保证 dst_ptr 具有至少 count 个连续可写槽位
    unsafe {
      decode_standard_raw::<F>(payload, count, alp_params, dst_ptr)?;
    }
  }
  Ok(())
}

/// Generic floating-point decompression directly into raw pointer memory.
/// 通用解压浮点数组至裸指针内存（零堆分配、零内存拷贝，避免未初始化切片构造）
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `dst_cap` 个连续可写 `F` 元素的有效内存。
pub unsafe fn decompress_into_raw<F: AlpFloat>(
  src: &[u8],
  dst_ptr: *mut F,
  dst_cap: usize,
) -> Result<usize> {
  let ParsedHeader {
    type_byte,
    count,
    params,
    mut cursor,
    has_repeat,
    ..
  } = read_header(src)?;

  if count == 0 {
    return Ok(0);
  }

  if dst_cap < count {
    return Err(Error::BufferTooSmall {
      needed: count,
      available: dst_cap,
    });
  }

  if !has_repeat {
    // SAFETY: dst_cap >= count 已校验，dst_ptr 在 count 范围内连续有效
    unsafe {
      decompress_into_raw_direct::<F>(src, cursor, type_byte, count, params, dst_ptr)?;
    }
    return Ok(count);
  }

  let bitmap_len = count.div_ceil(8);
  if src.len() < cursor + bitmap_len {
    return Err(Error::UnexpectedEof {
      needed: cursor + bitmap_len,
      available: src.len(),
    });
  }
  let bitmap = &src[cursor..cursor + bitmap_len];
  cursor += bitmap_len;

  let repeats = count_bitmap_ones(bitmap, count);
  let non_repeat_count = count - repeats;
  if non_repeat_count == 0 {
    return Err(Error::InvalidHeader);
  }

  let mut stack_buf = MaybeUninit::<[F; 1024]>::uninit();
  let mut heap_buf: Vec<F> = Vec::new();
  let tmp_ptr: *mut F = if non_repeat_count <= 1024 {
    stack_buf.as_mut_ptr().cast::<F>()
  } else {
    heap_buf.reserve(non_repeat_count);
    heap_buf.as_mut_ptr()
  };

  // SAFETY: tmp_ptr 指向至少 non_repeat_count 个连续有效 F 空间，dst_ptr 在 count 范围内有效，bitmap 已校验
  unsafe {
    decompress_into_raw_direct::<F>(src, cursor, type_byte, non_repeat_count, params, tmp_ptr)?;
    expand_repeats::<F>(bitmap, count, tmp_ptr, dst_ptr);
  }

  Ok(count)
}

/// Generic floating-point decompression into destination slice.
/// 通用解压浮点数组至目标切片（零堆分配、零内存拷贝）
#[inline(always)]
pub fn decompress_into_slice<F: AlpFloat>(src: &[u8], dst: &mut [F]) -> Result<usize> {
  // SAFETY: dst.as_mut_ptr() 指向 dst.len() 个有效连续元素，契约完全满足
  unsafe { decompress_into_raw(src, dst.as_mut_ptr(), dst.len()) }
}

/// Generic floating-point decompression into `dst` buffer.
/// 通用解压浮点数组至 `dst` 缓冲区（自动分发 RAW、标准 FOR 与 Delta 差分块）
pub fn decompress_into<F: AlpFloat>(src: &[u8], dst: &mut Vec<F>) -> Result<()> {
  let count = read_count(src)?;
  if count == 0 {
    return Ok(());
  }
  let old_len = dst.len();
  dst.reserve(count);
  // SAFETY: dst has reserved count slots, decompress_into_raw writes directly to pointer,
  // initializes count elements before updating Vec len. Never constructs uninitialized slice.
  // SAFETY: dst 已预留 count 个空间，decompress_into_raw 直接写入裸指针，
  // 严格初始化 count 个元素后安全更新 Vec 长度。绝不构造未初始化内存的切片引用。
  unsafe {
    let written = decompress_into_raw(src, dst.as_mut_ptr().add(old_len), count)?;
    dst.set_len(old_len + written);
  }
  Ok(())
}

/// Generic floating-point slice decompression.
/// 通用解压浮点数切片
#[inline]
pub fn decompress<F: AlpFloat>(src: &[u8]) -> Result<Vec<F>> {
  let mut dst = Vec::new();
  decompress_into(src, &mut dst)?;
  Ok(dst)
}

/// Patches exceptions into decoded buffer directly using raw pointer.
/// 将异常值字典打补丁至解码缓冲区（统一处理普通 u16 与超大数组 u32 格式，严格校验内存边界）
///
/// # Safety
///
/// `dst_ptr` 必须指向至少具备 `count` 个有效 `F` 元素的内存空间。
#[inline]
pub(crate) unsafe fn patch_exceptions<F: AlpFloat>(
  src: &[u8],
  count: usize,
  dst_ptr: *mut F,
) -> Result<()> {
  if src.is_empty() {
    return Ok(());
  }

  let mut cursor = 0;
  let is_large = count > u16::MAX as usize;
  let (exc_count, exc_count_len) = if is_large {
    if src.len() < cursor + EXC_COUNT_LEN_U32 {
      return Err(Error::UnexpectedEof {
        needed: cursor + EXC_COUNT_LEN_U32,
        available: src.len(),
      });
    }
    // SAFETY: Verified src.len() >= cursor + 4 above, read_unaligned safely reads little-endian u32
    // SAFETY: 上方已校验 src.len() >= cursor + 4，read_unaligned 安全读取小端 u32
    let c =
      unsafe { u32::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u32>())) } as usize;
    (c, EXC_COUNT_LEN_U32)
  } else {
    if src.len() < cursor + EXC_COUNT_LEN {
      return Err(Error::UnexpectedEof {
        needed: cursor + EXC_COUNT_LEN,
        available: src.len(),
      });
    }
    // SAFETY: Verified src.len() >= cursor + 2 above, read_unaligned safely reads little-endian u16
    // SAFETY: 上方已校验 src.len() >= cursor + 2，read_unaligned 安全读取小端 u16
    let c =
      unsafe { u16::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u16>())) } as usize;
    (c, EXC_COUNT_LEN)
  };
  cursor += exc_count_len;

  let entry_size = if is_large {
    F::EXC_ENTRY_SIZE_U32
  } else {
    F::EXC_ENTRY_SIZE
  };
  let exc_bytes_needed = exc_count
    .checked_mul(entry_size)
    .ok_or(Error::InvalidHeader)?;
  if src.len() < cursor + exc_bytes_needed {
    return Err(Error::UnexpectedEof {
      needed: cursor + exc_bytes_needed,
      available: src.len(),
    });
  }

  let exc_slice = &src[cursor..cursor + exc_bytes_needed];
  if is_large {
    for chunk in exc_slice.chunks_exact(entry_size) {
      let (pos, val) = F::read_exception_u32(chunk);
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      // SAFETY: pos < count verified above, and caller guarantees dst_ptr is valid for count elements
      // SAFETY: 上方已校验 pos < count，且调用方保证 count 范围内指针有效
      unsafe {
        *dst_ptr.add(pos) = val;
      }
    }
  } else {
    for chunk in exc_slice.chunks_exact(entry_size) {
      let (pos, val) = F::read_exception(chunk);
      if pos >= count {
        return Err(Error::CorruptedData { index: pos, count });
      }
      // SAFETY: pos < count verified above, and caller guarantees dst_ptr is valid for count elements
      // SAFETY: 上方已校验 pos < count，且调用方保证 count 范围内指针有效
      unsafe {
        *dst_ptr.add(pos) = val;
      }
    }
  }

  Ok(())
}
