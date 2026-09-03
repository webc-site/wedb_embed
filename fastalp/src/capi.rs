use core::{
  ptr::{copy_nonoverlapping, null_mut},
  slice::from_raw_parts,
};
use std::{
  cell::RefCell,
  panic::{AssertUnwindSafe, catch_unwind},
};

use crate::{Encoder, MAX_HEADER_LEN, compress_into, decompress_into};

thread_local! {
  static TLS_COMP_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
  static TLS_DEC_F64: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
  static TLS_DEC_F32: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
  static TLS_ENCODER_F64: RefCell<Encoder<f64>> = const { RefCell::new(Encoder::new()) };
  static TLS_ENCODER_F32: RefCell<Encoder<f32>> = const { RefCell::new(Encoder::new()) };
}

/// Computes the maximum possible compressed buffer size in bytes for `len` f64 values.
/// Callers can allocate a destination buffer of this size to guarantee no overflow.
///
/// # Arguments
/// * `len` - Number of `f64` values to compress.
///
/// # Returns
/// Maximum required destination buffer capacity in bytes.
///
/// ---
///
/// 计算 `len` 个 f64 浮点数值在最差情况下所需的最大压缩缓冲区字节大小。
/// 调用方可预先分配该大小的目标缓冲区，确保绝不发生容量不足错误。
///
/// # 参数
/// * `len` - 待压缩的 `f64` 元素个数。
///
/// # 返回值
/// 所需的最大目标缓冲区字节容量。
#[unsafe(no_mangle)]
pub extern "C" fn fastalp_max_compressed_size_f64(len: usize) -> usize {
  MAX_HEADER_LEN.saturating_add(len.saturating_mul(size_of::<f64>()))
}

/// Computes the maximum possible compressed buffer size in bytes for `len` f32 values.
/// Callers can allocate a destination buffer of this size to guarantee no overflow.
///
/// # Arguments
/// * `len` - Number of `f32` values to compress.
///
/// # Returns
/// Maximum required destination buffer capacity in bytes.
///
/// ---
///
/// 计算 `len` 个 f32 浮点数值在最差情况下所需的最大压缩缓冲区字节大小。
/// 调用方可预先分配该大小的目标缓冲区，确保绝不发生容量不足错误。
///
/// # 参数
/// * `len` - 待压缩的 `f32` 元素个数。
///
/// # 返回值
/// 所需的最大目标缓冲区字节容量。
#[unsafe(no_mangle)]
pub extern "C" fn fastalp_max_compressed_size_f32(len: usize) -> usize {
  MAX_HEADER_LEN.saturating_add(len.saturating_mul(size_of::<f32>()))
}

/// Resets the cached model parameters for the thread-local double-precision (f64) encoder.
/// Subsequent calls to `fastalp_compress_cached_f64` will re-sample parameters.
///
/// ---
///
/// 重置当前线程局部双精度 (f64) 状态化编码器的已缓存模型参数。
/// 后续调用 `fastalp_compress_cached_f64` 时将重新进行参数探测与采样。
#[unsafe(no_mangle)]
pub extern "C" fn fastalp_reset_encoder_f64() {
  let _ = catch_unwind(|| {
    TLS_ENCODER_F64.with(|enc| enc.borrow_mut().reset());
  });
}

/// Resets the cached model parameters for the thread-local single-precision (f32) encoder.
/// Subsequent calls to `fastalp_compress_cached_f32` will re-sample parameters.
///
/// ---
///
/// 重置当前线程局部单精度 (f32) 状态化编码器的已缓存模型参数。
/// 后续调用 `fastalp_compress_cached_f32` 时将重新进行参数探测与采样。
#[unsafe(no_mangle)]
pub extern "C" fn fastalp_reset_encoder_f32() {
  let _ = catch_unwind(|| {
    TLS_ENCODER_F32.with(|enc| enc.borrow_mut().reset());
  });
}

/// Compresses an array of f64 floating-point values with dynamic parameter sampling.
///
/// # Arguments
/// * `src` - Pointer to the source `f64` array.
/// * `len` - Number of `f64` elements in the source array.
/// * `dst` - Pointer to the destination byte buffer.
/// * `dst_cap` - Capacity of the destination buffer in bytes.
///
/// # Returns
/// The number of compressed bytes written to `dst`, or `0` if an error occurs or `dst_cap` is insufficient.
///
/// # Safety
/// * `src` must point to at least `len` valid, aligned `f64` values.
/// * `dst` must point to writable memory of at least `dst_cap` bytes.
/// * `src` and `dst` must not overlap or be null.
///
/// ---
///
/// 压缩 f64 浮点数组（包含动态模型参数采样探测）。
///
/// # 参数
/// * `src` - 源 `f64` 浮点数组指针。
/// * `len` - 源数组中 `f64` 元素的个数。
/// * `dst` - 目标字节缓冲区指针。
/// * `dst_cap` - 目标字节缓冲区的最大容量。
///
/// # 返回值
/// 实际写入目标缓冲区的压缩字节数；若发生错误或目标容量不足则返回 `0`。
///
/// # 安全性保证 (Safety)
/// * `src` 必须指向至少包含 `len` 个有效且内存对齐的 `f64` 元素。
/// * `dst` 必须指向至少具有 `dst_cap` 字节可写容量的有效内存区域。
/// * `src` 与 `dst` 指针不得重叠且不得为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_compress_f64(
  src: *const f64,
  len: usize,
  dst: *mut u8,
  dst_cap: usize,
) -> usize {
  if src.is_null() || dst.is_null() || len == 0 {
    return 0;
  }
  let input = unsafe { from_raw_parts(src, len) };
  catch_unwind(|| {
    TLS_COMP_BUF.with(|buf| {
      let mut b = buf.borrow_mut();
      b.clear();
      compress_into(input, &mut b);
      if b.len() > dst_cap {
        return 0;
      }
      unsafe {
        copy_nonoverlapping(b.as_ptr(), dst, b.len());
      }
      b.len()
    })
  })
  .unwrap_or(0)
}

/// Compresses an array of f64 floating-point values by reusing cached parameters from the thread-local encoder.
/// Skips sampling overhead, suitable for high-throughput streaming pipelines of stationary data.
///
/// # Arguments
/// * `src` - Pointer to the source `f64` array.
/// * `len` - Number of `f64` elements in the source array.
/// * `dst` - Pointer to the destination byte buffer.
/// * `dst_cap` - Capacity of the destination buffer in bytes.
///
/// # Returns
/// The number of compressed bytes written to `dst`, or `0` if an error occurs or `dst_cap` is insufficient.
///
/// # Safety
/// * `src` must point to at least `len` valid, aligned `f64` values.
/// * `dst` must point to writable memory of at least `dst_cap` bytes.
/// * `src` and `dst` must not overlap or be null.
///
/// ---
///
/// 复用线程局部编码器已缓存的模型参数压缩 f64 浮点数组。
/// 跳过重复采样开销，直接执行核心编码内核，适用于平稳时序数据的高吞吐流式批量压缩。
///
/// # 参数
/// * `src` - 源 `f64` 浮点数组指针。
/// * `len` - 源数组中 `f64` 元素的个数。
/// * `dst` - 目标字节缓冲区指针。
/// * `dst_cap` - 目标字节缓冲区的最大容量。
///
/// # 返回值
/// 实际写入目标缓冲区的压缩字节数；若发生错误或目标容量不足则返回 `0`。
///
/// # 安全性保证 (Safety)
/// * `src` 必须指向至少包含 `len` 个有效且内存对齐的 `f64` 元素。
/// * `dst` 必须指向至少具有 `dst_cap` 字节可写容量的有效内存区域。
/// * `src` 与 `dst` 指针不得重叠且不得为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_compress_cached_f64(
  src: *const f64,
  len: usize,
  dst: *mut u8,
  dst_cap: usize,
) -> usize {
  if src.is_null() || dst.is_null() || len == 0 {
    return 0;
  }
  let input = unsafe { from_raw_parts(src, len) };
  catch_unwind(|| {
    TLS_COMP_BUF.with(|buf| {
      let mut b = buf.borrow_mut();
      b.clear();
      TLS_ENCODER_F64.with(|enc| {
        enc.borrow_mut().compress_into(input, &mut b);
      });
      if b.len() > dst_cap {
        return 0;
      }
      unsafe {
        copy_nonoverlapping(b.as_ptr(), dst, b.len());
      }
      b.len()
    })
  })
  .unwrap_or(0)
}

/// Decompresses a byte buffer into an array of f64 floating-point values.
///
/// # Arguments
/// * `src` - Pointer to the compressed byte buffer.
/// * `src_len` - Length of the compressed byte buffer.
/// * `dst` - Pointer to the destination `f64` array.
/// * `dst_cap` - Maximum number of `f64` elements that `dst` can hold.
///
/// # Returns
/// The number of decompressed `f64` values written to `dst`, or `0` on error or insufficient capacity.
///
/// # Safety
/// * `src` must point to at least `src_len` readable bytes.
/// * `dst` must point to writable memory for at least `dst_cap` aligned `f64` elements.
/// * `src` and `dst` must not overlap or be null.
///
/// ---
///
/// 解压字节缓冲区至 f64 浮点数组。
///
/// # 参数
/// * `src` - 压缩字节缓冲区指针。
/// * `src_len` - 压缩字节缓冲区的有效长度。
/// * `dst` - 目标 `f64` 浮点数组指针。
/// * `dst_cap` - 目标数组最多可容纳的 `f64` 元素个数。
///
/// # 返回值
/// 实际解压出的 `f64` 浮点元素个数；若数据损坏、解析失败或目标容量不足则返回 `0`。
///
/// # 安全性保证 (Safety)
/// * `src` 必须指向至少 `src_len` 字节可读的有效内存。
/// * `dst` 必须指向至少可容纳 `dst_cap` 个内存对齐 `f64` 元素的可写内存。
/// * `src` 与 `dst` 指针不得重叠且不得为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_decompress_f64(
  src: *const u8,
  src_len: usize,
  dst: *mut f64,
  dst_cap: usize,
) -> usize {
  if src.is_null() || dst.is_null() || src_len == 0 {
    return 0;
  }
  let input = unsafe { from_raw_parts(src, src_len) };
  catch_unwind(|| {
    TLS_DEC_F64.with(|buf| {
      let mut b = buf.borrow_mut();
      b.clear();
      if decompress_into::<f64>(input, &mut b).is_err() || b.len() > dst_cap {
        return 0;
      }
      unsafe {
        copy_nonoverlapping(b.as_ptr(), dst, b.len());
      }
      b.len()
    })
  })
  .unwrap_or(0)
}

/// Compresses an array of f32 floating-point values with dynamic parameter sampling.
///
/// # Arguments
/// * `src` - Pointer to the source `f32` array.
/// * `len` - Number of `f32` elements in the source array.
/// * `dst` - Pointer to the destination byte buffer.
/// * `dst_cap` - Capacity of the destination buffer in bytes.
///
/// # Returns
/// The number of compressed bytes written to `dst`, or `0` if an error occurs or `dst_cap` is insufficient.
///
/// # Safety
/// * `src` must point to at least `len` valid, aligned `f32` values.
/// * `dst` must point to writable memory of at least `dst_cap` bytes.
/// * `src` and `dst` must not overlap or be null.
///
/// ---
///
/// 压缩 f32 浮点数组（包含动态模型参数采样探测）。
///
/// # 参数
/// * `src` - 源 `f32` 浮点数组指针。
/// * `len` - 源数组中 `f32` 元素的个数。
/// * `dst` - 目标字节缓冲区指针。
/// * `dst_cap` - 目标字节缓冲区的最大容量。
///
/// # 返回值
/// 实际写入目标缓冲区的压缩字节数；若发生错误或目标容量不足则返回 `0`。
///
/// # 安全性保证 (Safety)
/// * `src` 必须指向至少包含 `len` 个有效且内存对齐的 `f32` 元素。
/// * `dst` 必须指向至少具有 `dst_cap` 字节可写容量的有效内存区域。
/// * `src` 与 `dst` 指针不得重叠且不得为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_compress_f32(
  src: *const f32,
  len: usize,
  dst: *mut u8,
  dst_cap: usize,
) -> usize {
  if src.is_null() || dst.is_null() || len == 0 {
    return 0;
  }
  let input = unsafe { from_raw_parts(src, len) };
  catch_unwind(|| {
    TLS_COMP_BUF.with(|buf| {
      let mut b = buf.borrow_mut();
      b.clear();
      compress_into(input, &mut b);
      if b.len() > dst_cap {
        return 0;
      }
      unsafe {
        copy_nonoverlapping(b.as_ptr(), dst, b.len());
      }
      b.len()
    })
  })
  .unwrap_or(0)
}

/// Compresses an array of f32 floating-point values by reusing cached parameters from the thread-local encoder.
/// Skips sampling overhead, suitable for high-throughput streaming pipelines of stationary data.
///
/// # Arguments
/// * `src` - Pointer to the source `f32` array.
/// * `len` - Number of `f32` elements in the source array.
/// * `dst` - Pointer to the destination byte buffer.
/// * `dst_cap` - Capacity of the destination buffer in bytes.
///
/// # Returns
/// The number of compressed bytes written to `dst`, or `0` if an error occurs or `dst_cap` is insufficient.
///
/// # Safety
/// * `src` must point to at least `len` valid, aligned `f32` values.
/// * `dst` must point to writable memory of at least `dst_cap` bytes.
/// * `src` and `dst` must not overlap or be null.
///
/// ---
///
/// 复用线程局部编码器已缓存的模型参数压缩 f32 浮点数组。
/// 跳过重复采样开销，直接执行核心编码内核，适用于平稳时序数据的高吞吐流式批量压缩。
///
/// # 参数
/// * `src` - 源 `f32` 浮点数组指针。
/// * `len` - 源数组中 `f32` 元素的个数。
/// * `dst` - 目标字节缓冲区指针。
/// * `dst_cap` - 目标字节缓冲区的最大容量。
///
/// # 返回值
/// 实际写入目标缓冲区的压缩字节数；若发生错误或目标容量不足则返回 `0`。
///
/// # 安全性保证 (Safety)
/// * `src` 必须指向至少包含 `len` 个有效且内存对齐的 `f32` 元素。
/// * `dst` 必须指向至少具有 `dst_cap` 字节可写容量的有效内存区域。
/// * `src` 与 `dst` 指针不得重叠且不得为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_compress_cached_f32(
  src: *const f32,
  len: usize,
  dst: *mut u8,
  dst_cap: usize,
) -> usize {
  if src.is_null() || dst.is_null() || len == 0 {
    return 0;
  }
  let input = unsafe { from_raw_parts(src, len) };
  catch_unwind(|| {
    TLS_COMP_BUF.with(|buf| {
      let mut b = buf.borrow_mut();
      b.clear();
      TLS_ENCODER_F32.with(|enc| {
        enc.borrow_mut().compress_into(input, &mut b);
      });
      if b.len() > dst_cap {
        return 0;
      }
      unsafe {
        copy_nonoverlapping(b.as_ptr(), dst, b.len());
      }
      b.len()
    })
  })
  .unwrap_or(0)
}

/// Decompresses a byte buffer into an array of f32 floating-point values.
///
/// # Arguments
/// * `src` - Pointer to the compressed byte buffer.
/// * `src_len` - Length of the compressed byte buffer.
/// * `dst` - Pointer to the destination `f32` array.
/// * `dst_cap` - Maximum number of `f32` elements that `dst` can hold.
///
/// # Returns
/// The number of decompressed `f32` values written to `dst`, or `0` on error or insufficient capacity.
///
/// # Safety
/// * `src` must point to at least `src_len` readable bytes.
/// * `dst` must point to writable memory for at least `dst_cap` aligned `f32` elements.
/// * `src` and `dst` must not overlap or be null.
///
/// ---
///
/// 解压字节缓冲区至 f32 浮点数组。
///
/// # 参数
/// * `src` - 压缩字节缓冲区指针。
/// * `src_len` - 压缩字节缓冲区的有效长度。
/// * `dst` - 目标 `f32` 浮点数组指针。
/// * `dst_cap` - 目标数组最多可容纳的 `f32` 元素个数。
///
/// # 返回值
/// 实际解压出的 `f32` 浮点元素个数；若数据损坏、解析失败或目标容量不足则返回 `0`。
///
/// # 安全性保证 (Safety)
/// * `src` 必须指向至少 `src_len` 字节可读的有效内存。
/// * `dst` 必须指向至少可容纳 `dst_cap` 个内存对齐 `f32` 元素的可写内存。
/// * `src` 与 `dst` 指针不得重叠且不得为空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_decompress_f32(
  src: *const u8,
  src_len: usize,
  dst: *mut f32,
  dst_cap: usize,
) -> usize {
  if src.is_null() || dst.is_null() || src_len == 0 {
    return 0;
  }
  let input = unsafe { from_raw_parts(src, src_len) };
  catch_unwind(|| {
    TLS_DEC_F32.with(|buf| {
      let mut b = buf.borrow_mut();
      b.clear();
      if decompress_into::<f32>(input, &mut b).is_err() || b.len() > dst_cap {
        return 0;
      }
      unsafe {
        copy_nonoverlapping(b.as_ptr(), dst, b.len());
      }
      b.len()
    })
  })
  .unwrap_or(0)
}

/// Opaque handle for a stateful double-precision (f64) encoder instance.
/// Useful when multiple encoders are needed across different threads or streams without TLS.
///
/// ---
///
/// 双精度 (f64) 状态化独立编码器句柄。
/// 适用于多线程、多流并发或不依赖 TLS 的场景。
pub struct FastAlpEncoderF64 {
  inner: Encoder<f64>,
  out_buf: Vec<u8>,
}

/// Creates a new stateful f64 encoder handle on the heap.
/// Caller is responsible for releasing it using `fastalp_encoder_f64_free`.
///
/// # Returns
/// A pointer to the newly allocated encoder, or null on allocation failure.
///
/// ---
///
/// 在堆上创建新的状态化 f64 独立编码器实例句柄。
/// 调用方需负责调用 `fastalp_encoder_f64_free` 进行内存释放。
///
/// # 返回值
/// 指向新创建编码器的指针；若内存分配失败则返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn fastalp_encoder_f64_new() -> *mut FastAlpEncoderF64 {
  catch_unwind(|| {
    Box::into_raw(Box::new(FastAlpEncoderF64 {
      inner: Encoder::new(),
      out_buf: Vec::new(),
    }))
  })
  .unwrap_or(null_mut())
}

/// Frees a stateful f64 encoder instance created by `fastalp_encoder_f64_new`.
///
/// # Safety
/// `enc` must be a valid pointer returned by `fastalp_encoder_f64_new` that has not been freed.
///
/// ---
///
/// 释放由 `fastalp_encoder_f64_new` 创建的 f64 独立编码器实例。
///
/// # 安全性保证 (Safety)
/// `enc` 必须是由 `fastalp_encoder_f64_new` 返回且尚未被释放的有效指针。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_encoder_f64_free(enc: *mut FastAlpEncoderF64) {
  if !enc.is_null() {
    let _ = catch_unwind(|| unsafe {
      drop(Box::from_raw(enc));
    });
  }
}

/// Resets cached parameters in a stateful f64 encoder handle.
///
/// # Safety
/// `enc` must point to a valid `FastAlpEncoderF64` instance.
///
/// ---
///
/// 重置 f64 独立编码器句柄中的已缓存模型参数。
///
/// # 安全性保证 (Safety)
/// `enc` 必须指向有效的 `FastAlpEncoderF64` 实例。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_encoder_f64_reset(enc: *mut FastAlpEncoderF64) {
  if let Some(enc_ref) = unsafe { enc.as_mut() } {
    let _ = catch_unwind(AssertUnwindSafe(|| {
      enc_ref.inner.reset();
    }));
  }
}

/// Compresses an array of f64 values using a stateful encoder handle, reusing cached parameters.
///
/// # Arguments
/// * `enc` - Pointer to the `FastAlpEncoderF64` instance.
/// * `src` - Pointer to the source `f64` array.
/// * `len` - Number of `f64` values to compress.
/// * `dst` - Pointer to destination buffer.
/// * `dst_cap` - Capacity of destination buffer in bytes.
///
/// # Returns
/// Number of bytes written, or `0` on error or insufficient capacity.
///
/// # Safety
/// `enc`, `src`, and `dst` must be valid and non-null.
///
/// ---
///
/// 使用 f64 独立编码器句柄压缩浮点数组（复用已缓存模型参数）。
///
/// # 参数
/// * `enc` - 编码器句柄指针。
/// * `src` - 源 `f64` 浮点数组指针。
/// * `len` - 待压缩元素个数。
/// * `dst` - 目标字节缓冲区指针。
/// * `dst_cap` - 目标缓冲区字节容量。
///
/// # 返回值
/// 实际写入字节数；出错或容量不足时返回 `0`。
///
/// # 安全性保证 (Safety)
/// `enc`、`src` 与 `dst` 必须有效且非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_encoder_f64_compress(
  enc: *mut FastAlpEncoderF64,
  src: *const f64,
  len: usize,
  dst: *mut u8,
  dst_cap: usize,
) -> usize {
  if enc.is_null() || src.is_null() || dst.is_null() || len == 0 {
    return 0;
  }
  let enc_ref = unsafe { &mut *enc };
  let input = unsafe { from_raw_parts(src, len) };
  catch_unwind(AssertUnwindSafe(|| {
    enc_ref.out_buf.clear();
    enc_ref.inner.compress_into(input, &mut enc_ref.out_buf);
    if enc_ref.out_buf.len() > dst_cap {
      return 0;
    }
    unsafe {
      copy_nonoverlapping(enc_ref.out_buf.as_ptr(), dst, enc_ref.out_buf.len());
    }
    enc_ref.out_buf.len()
  }))
  .unwrap_or(0)
}

/// Opaque handle for a stateful single-precision (f32) encoder instance.
/// Useful when multiple encoders are needed across different threads or streams without TLS.
///
/// ---
///
/// 单精度 (f32) 状态化独立编码器句柄。
/// 适用于多线程、多流并发或不依赖 TLS 的场景。
pub struct FastAlpEncoderF32 {
  inner: Encoder<f32>,
  out_buf: Vec<u8>,
}

/// Creates a new stateful f32 encoder handle on the heap.
/// Caller is responsible for releasing it using `fastalp_encoder_f32_free`.
///
/// # Returns
/// A pointer to the newly allocated encoder, or null on allocation failure.
///
/// ---
///
/// 在堆上创建新的状态化 f32 独立编码器实例句柄。
/// 调用方需负责调用 `fastalp_encoder_f32_free` 进行内存释放。
///
/// # 返回值
/// 指向新创建编码器的指针；若内存分配失败则返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn fastalp_encoder_f32_new() -> *mut FastAlpEncoderF32 {
  catch_unwind(|| {
    Box::into_raw(Box::new(FastAlpEncoderF32 {
      inner: Encoder::new(),
      out_buf: Vec::new(),
    }))
  })
  .unwrap_or(null_mut())
}

/// Frees a stateful f32 encoder instance created by `fastalp_encoder_f32_new`.
///
/// # Safety
/// `enc` must be a valid pointer returned by `fastalp_encoder_f32_new` that has not been freed.
///
/// ---
///
/// 释放由 `fastalp_encoder_f32_new` 创建的 f32 独立编码器实例。
///
/// # 安全性保证 (Safety)
/// `enc` 必须是由 `fastalp_encoder_f32_new` 返回且尚未被释放的有效指针。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_encoder_f32_free(enc: *mut FastAlpEncoderF32) {
  if !enc.is_null() {
    let _ = catch_unwind(|| unsafe {
      drop(Box::from_raw(enc));
    });
  }
}

/// Resets cached parameters in a stateful f32 encoder handle.
///
/// # Safety
/// `enc` must point to a valid `FastAlpEncoderF32` instance.
///
/// ---
///
/// 重置 f32 独立编码器句柄中的已缓存模型参数。
///
/// # 安全性保证 (Safety)
/// `enc` 必须指向有效的 `FastAlpEncoderF32` 实例。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_encoder_f32_reset(enc: *mut FastAlpEncoderF32) {
  if let Some(enc_ref) = unsafe { enc.as_mut() } {
    let _ = catch_unwind(AssertUnwindSafe(|| {
      enc_ref.inner.reset();
    }));
  }
}

/// Compresses an array of f32 values using a stateful encoder handle, reusing cached parameters.
///
/// # Arguments
/// * `enc` - Pointer to the `FastAlpEncoderF32` instance.
/// * `src` - Pointer to the source `f32` array.
/// * `len` - Number of `f32` values to compress.
/// * `dst` - Pointer to destination buffer.
/// * `dst_cap` - Capacity of destination buffer in bytes.
///
/// # Returns
/// Number of bytes written, or `0` on error or insufficient capacity.
///
/// # Safety
/// `enc`, `src`, and `dst` must be valid and non-null.
///
/// ---
///
/// 使用 f32 独立编码器句柄压缩浮点数组（复用已缓存模型参数）。
///
/// # 参数
/// * `enc` - 编码器句柄指针。
/// * `src` - 源 `f32` 浮点数组指针。
/// * `len` - 待压缩元素个数。
/// * `dst` - 目标字节缓冲区指针。
/// * `dst_cap` - 目标缓冲区字节容量。
///
/// # 返回值
/// 实际写入字节数；出错或容量不足时返回 `0`。
///
/// # 安全性保证 (Safety)
/// `enc`、`src` 与 `dst` 必须有效且非空。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastalp_encoder_f32_compress(
  enc: *mut FastAlpEncoderF32,
  src: *const f32,
  len: usize,
  dst: *mut u8,
  dst_cap: usize,
) -> usize {
  if enc.is_null() || src.is_null() || dst.is_null() || len == 0 {
    return 0;
  }
  let enc_ref = unsafe { &mut *enc };
  let input = unsafe { from_raw_parts(src, len) };
  catch_unwind(AssertUnwindSafe(|| {
    enc_ref.out_buf.clear();
    enc_ref.inner.compress_into(input, &mut enc_ref.out_buf);
    if enc_ref.out_buf.len() > dst_cap {
      return 0;
    }
    unsafe {
      copy_nonoverlapping(enc_ref.out_buf.as_ptr(), dst, enc_ref.out_buf.len());
    }
    enc_ref.out_buf.len()
  }))
  .unwrap_or(0)
}
