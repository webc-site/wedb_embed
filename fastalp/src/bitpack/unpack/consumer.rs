use core::{
  marker::PhantomData,
  ptr::{copy_nonoverlapping, write_bytes},
};

use super::decoder::AlpDecoder;
use crate::{
  constants::{BYTES_U64, LUT_SIZE_1BIT, LUT_SIZE_2BIT, LUT_SIZE_4BIT},
  float::AlpFloat,
};

/// Abstraction for consuming bit-unpacked integers into typed memory destinations.
/// 位解包整数消费抽象（支持原始 u64 写入、FOR 浮点重构与 Delta 寄存器融合前缀和）
pub trait AlpConsumer<T: Copy> {
  /// Consumes 8 unpacked offsets and writes 8 reconstructed values to `dst_ptr`.
  /// 消费 8 个解包偏移量，将重构后的 8 个值写入 `dst_ptr`。
  ///
  /// # Safety
  ///
  /// Caller must ensure `dst_ptr` points to valid memory for at least 8 continuous writable `T` elements.
  unsafe fn consume_8(&mut self, offs: [u64; 8], dst_ptr: *mut T);

  /// Consumes 1 unpacked offset and writes 1 reconstructed value to `dst_ptr`.
  /// 消费 1 个解包偏移量，将重构后的 1 个值写入 `dst_ptr`。
  ///
  /// # Safety
  ///
  /// Caller must ensure `dst_ptr` points to valid writable memory for 1 `T` element.
  unsafe fn consume_1(&mut self, off: u64, dst_ptr: *mut T);

  /// Bulk consumption for constant 0 bit width.
  /// 0 位宽常数块快速填充。
  ///
  /// # Safety
  ///
  /// Caller must ensure `dst_ptr` points to valid memory for at least `count` continuous writable `T` elements.
  #[inline(always)]
  unsafe fn consume_zeros(&mut self, count: usize, dst_ptr: *mut T) {
    let mut i = 0;
    while i + 8 <= count {
      unsafe {
        self.consume_8([0; 8], dst_ptr.add(i));
      }
      i += 8;
    }
    while i < count {
      unsafe {
        self.consume_1(0, dst_ptr.add(i));
      }
      i += 1;
    }
  }

  /// Bulk byte-copy optimization for 64-bit width.
  /// 64 位宽字节直拷优化。
  ///
  /// # Safety
  ///
  /// Caller must ensure `src_ptr` is readable for `count * 8` bytes, and `dst_ptr` is writable for `count` `T` elements.
  #[inline(always)]
  unsafe fn consume_bulk_64(
    &mut self,
    _src_ptr: *const u8,
    _count: usize,
    _dst_ptr: *mut T,
  ) -> bool {
    false
  }

  /// Optional LUT lookup table for 1-bit width.
  #[inline(always)]
  fn use_lut_1(&self) -> Option<[T; LUT_SIZE_1BIT]> {
    None
  }

  /// Optional LUT lookup table for 2-bit width.
  #[inline(always)]
  fn use_lut_2(&self) -> Option<[T; LUT_SIZE_2BIT]> {
    None
  }

  /// Optional LUT lookup table for 4-bit width.
  #[inline(always)]
  fn use_lut_4(&self) -> Option<[T; LUT_SIZE_4BIT]> {
    None
  }
}

/// Raw u64 offset consumer: directly stores unpacked integers to destination buffer.
/// 裸 u64 偏移量消费者：将解包出的整数原样存入目标缓冲区
#[derive(Copy, Clone, Debug, Default)]
pub struct RawU64Consumer;

impl AlpConsumer<u64> for RawU64Consumer {
  #[inline(always)]
  unsafe fn consume_8(&mut self, offs: [u64; 8], dst_ptr: *mut u64) {
    unsafe {
      copy_nonoverlapping(offs.as_ptr(), dst_ptr, 8);
    }
  }

  #[inline(always)]
  unsafe fn consume_1(&mut self, off: u64, dst_ptr: *mut u64) {
    unsafe {
      *dst_ptr = off;
    }
  }

  #[inline(always)]
  unsafe fn consume_zeros(&mut self, count: usize, dst_ptr: *mut u64) {
    unsafe {
      write_bytes(dst_ptr, 0, count);
    }
  }

  #[inline(always)]
  unsafe fn consume_bulk_64(
    &mut self,
    src_ptr: *const u8,
    count: usize,
    dst_ptr: *mut u64,
  ) -> bool {
    if cfg!(target_endian = "little") {
      unsafe {
        copy_nonoverlapping(src_ptr, dst_ptr.cast::<u8>(), count * BYTES_U64);
      }
      true
    } else {
      false
    }
  }
}

/// Frame-of-Reference (FOR) float consumer: reconstructs floats from offsets via `AlpDecoder`.
/// 基准值对齐 (FOR) 浮点消费者：借助 `AlpDecoder` 从偏移量直接重构浮点数
#[derive(Copy, Clone)]
pub struct ForConsumer<F: AlpFloat, D: AlpDecoder<F>> {
  pub decoder: D,
  _phantom: PhantomData<F>,
}

impl<F: AlpFloat, D: AlpDecoder<F>> ForConsumer<F, D> {
  #[inline(always)]
  pub const fn new(decoder: D) -> Self {
    Self {
      decoder,
      _phantom: PhantomData,
    }
  }
}

impl<F: AlpFloat, D: AlpDecoder<F>> AlpConsumer<F> for ForConsumer<F, D> {
  #[inline(always)]
  unsafe fn consume_8(&mut self, offs: [u64; 8], dst_ptr: *mut F) {
    unsafe {
      write_8!(dst_ptr, k => self.decoder.decode_offset(offs[k]));
    }
  }

  #[inline(always)]
  unsafe fn consume_1(&mut self, off: u64, dst_ptr: *mut F) {
    unsafe {
      *dst_ptr = self.decoder.decode_offset(off);
    }
  }

  #[inline(always)]
  unsafe fn consume_zeros(&mut self, count: usize, dst_ptr: *mut F) {
    let val = self.decoder.decode_offset(0);
    let mut i = 0;
    while i + 8 <= count {
      unsafe {
        write_8!(dst_ptr.add(i), _k => val);
      }
      i += 8;
    }
    while i < count {
      unsafe {
        dst_ptr.add(i).write(val);
      }
      i += 1;
    }
  }

  #[inline(always)]
  fn use_lut_1(&self) -> Option<[F; LUT_SIZE_1BIT]> {
    Some(self.decoder.build_lut_1())
  }

  #[inline(always)]
  fn use_lut_2(&self) -> Option<[F; LUT_SIZE_2BIT]> {
    Some(self.decoder.build_lut_2())
  }

  #[inline(always)]
  fn use_lut_4(&self) -> Option<[F; LUT_SIZE_4BIT]> {
    Some(self.decoder.build_lut_4())
  }
}

/// ALP Delta fused consumer: executes associative prefix-sum tree in registers and reconstructs floats directly.
/// ALP Delta 融合单趟消费者：在寄存器内部完成树状结合律前缀和累加，并直接重构浮点数（零栈分配、零中间内存回读）
pub struct AlpDeltaConsumer<F: AlpFloat, D: AlpDecoder<F>> {
  pub curr: F::Int,
  pub min_delta: F::Int,
  pub decoder: D,
  pub _phantom: PhantomData<F>,
}

impl<F: AlpFloat, D: AlpDecoder<F>> AlpDeltaConsumer<F, D> {
  #[inline(always)]
  pub const fn new(curr: F::Int, min_delta: F::Int, decoder: D) -> Self {
    Self {
      curr,
      min_delta,
      decoder,
      _phantom: PhantomData,
    }
  }
}

impl<F: AlpFloat, D: AlpDecoder<F>> AlpConsumer<F> for AlpDeltaConsumer<F, D> {
  #[inline(always)]
  unsafe fn consume_8(&mut self, chunk: [u64; 8], dst_ptr: *mut F) {
    let d0 = F::u64_to_int_add(chunk[0], self.min_delta);
    let d1 = F::u64_to_int_add(chunk[1], self.min_delta);
    let d2 = F::u64_to_int_add(chunk[2], self.min_delta);
    let d3 = F::u64_to_int_add(chunk[3], self.min_delta);
    let d4 = F::u64_to_int_add(chunk[4], self.min_delta);
    let d5 = F::u64_to_int_add(chunk[5], self.min_delta);
    let d6 = F::u64_to_int_add(chunk[6], self.min_delta);
    let d7 = F::u64_to_int_add(chunk[7], self.min_delta);

    // 结合律前缀和加法树：第一层与第二层与 curr 完全解耦并行计算
    let s01 = F::int_add(d0, d1);
    let s23 = F::int_add(d2, d3);
    let s45 = F::int_add(d4, d5);
    let s67 = F::int_add(d6, d7);

    let s0123 = F::int_add(s01, s23);
    let s4567 = F::int_add(s45, s67);
    let delta_total = F::int_add(s0123, s4567);

    let curr = self.curr;
    // 跨 8 元素块循环携带依赖链仅需 1 次单周期加法！
    self.curr = F::int_add(curr, delta_total);

    let c0 = F::int_add(curr, d0);
    let c1 = F::int_add(curr, s01);
    let c2 = F::int_add(c1, d2);
    let c3 = F::int_add(curr, s0123);
    let c4 = F::int_add(c3, d4);
    let c5 = F::int_add(c3, s45);
    let c6 = F::int_add(c5, d6);
    let c7 = self.curr;

    let c = [c0, c1, c2, c3, c4, c5, c6, c7];
    unsafe {
      write_8!(dst_ptr, k => self.decoder.decode_int(c[k]));
    }
  }

  #[inline(always)]
  unsafe fn consume_1(&mut self, off: u64, dst_ptr: *mut F) {
    let delta = F::u64_to_int_add(off, self.min_delta);
    let next = F::int_add(self.curr, delta);
    self.curr = next;
    unsafe {
      *dst_ptr = self.decoder.decode_int(next);
    }
  }
}
