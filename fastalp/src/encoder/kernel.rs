use fearless_simd::{Level, Simd, dispatch, prelude::*};

use crate::{constants::MAX_EXCEPTIONS, encoder::exception::Exception, float::AlpFloat};

#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn encode_fearless_f64_kernel<S: Simd>(
  simd: S,
  slice: &[f64],
  enc_ptr: *mut i64,
  exp_factor: f64,
  fac_int: i64,
  frac_exp: f64,
  use_div: bool,
  exceptions: &mut Vec<Exception<u64>>,
) -> (i64, i64) {
  let n = S::f64s::N;
  let stride = n * 4;
  let exp_v = S::f64s::splat(simd, exp_factor);
  let frac_v = S::f64s::splat(simd, frac_exp);
  let fac_v = S::i64s::splat(simd, fac_int);

  let mut min_v0 = S::f64s::splat(simd, f64::INFINITY);
  let mut min_v1 = S::f64s::splat(simd, f64::INFINITY);
  let mut max_v0 = S::f64s::splat(simd, f64::NEG_INFINITY);
  let mut max_v1 = S::f64s::splat(simd, f64::NEG_INFINITY);
  let mut any_diff_v = S::u64s::splat(simd, 0);

  let full_chunks_len = slice.len() / stride * stride;
  let (full_slice, rem_slice) = slice.split_at(full_chunks_len);

  macro_rules! run_loop {
    ($decode:expr) => {
      for (chunk_idx, c) in full_slice.chunks_exact(stride).enumerate() {
        let base_out = chunk_idx * stride;
        let v0 = S::f64s::from_slice(simd, &c[0..n]);
        let v1 = S::f64s::from_slice(simd, &c[n..n * 2]);
        let v2 = S::f64s::from_slice(simd, &c[n * 2..n * 3]);
        let v3 = S::f64s::from_slice(simd, &c[n * 3..n * 4]);

        let r0 = (v0 * exp_v).round_ties_even();
        let r1 = (v1 * exp_v).round_ties_even();
        let r2 = (v2 * exp_v).round_ties_even();
        let r3 = (v3 * exp_v).round_ties_even();

        let i0 = r0.to_int::<S::i64s>();
        let i1 = r1.to_int::<S::i64s>();
        let i2 = r2.to_int::<S::i64s>();
        let i3 = r3.to_int::<S::i64s>();

        // SAFETY: enc_ptr has space for at least slice.len() elements, storing directly to initialized chunk slots
        unsafe {
          i0.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out), n));
          i1.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out + n), n));
          i2.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out + n * 2), n));
          i3.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out + n * 3), n));
        }

        let d0 = $decode(i0);
        let d1 = $decode(i1);
        let d2 = $decode(i2);
        let d3 = $decode(i3);

        let diff0 = d0.bitcast::<S::u64s>() ^ v0.bitcast::<S::u64s>();
        let diff1 = d1.bitcast::<S::u64s>() ^ v1.bitcast::<S::u64s>();
        let diff2 = d2.bitcast::<S::u64s>() ^ v2.bitcast::<S::u64s>();
        let diff3 = d3.bitcast::<S::u64s>() ^ v3.bitcast::<S::u64s>();

        any_diff_v |= ((diff0 | diff1) | (diff2 | diff3));

        min_v0 = min_v0.min(r0);
        min_v1 = min_v1.min(r1);
        min_v0 = min_v0.min(r2);
        min_v1 = min_v1.min(r3);

        max_v0 = max_v0.max(r0);
        max_v1 = max_v1.max(r1);
        max_v0 = max_v0.max(r2);
        max_v1 = max_v1.max(r3);
      }
    };
  }

  if use_div {
    run_loop!(|i: S::i64s| i.to_float::<S::f64s>() / exp_v);
  } else if fac_int == 1 {
    run_loop!(|i: S::i64s| i.to_float::<S::f64s>() * frac_v);
  } else {
    run_loop!(|i: S::i64s| (i * fac_v).to_float::<S::f64s>() * frac_v);
  }

  let min_v = min_v0.min(min_v1);
  let max_v = max_v0.max(max_v1);

  let mut min_arr = [0.0f64; 8];
  let mut max_arr = [0.0f64; 8];
  let mut diff_arr = [0u64; 8];
  min_v.store_slice(&mut min_arr[..n]);
  max_v.store_slice(&mut max_arr[..n]);
  any_diff_v.store_slice(&mut diff_arr[..n]);

  let mut any_diff = 0u64;
  let (mut min_int, mut max_int) = if full_chunks_len > 0 {
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for i in 0..n {
      min_val = min_val.min(min_arr[i]);
      max_val = max_val.max(max_arr[i]);
      any_diff |= diff_arr[i];
    }
    (min_val as i64, max_val as i64)
  } else {
    (i64::MAX, i64::MIN)
  };

  if !rem_slice.is_empty() {
    let rem_start = full_chunks_len;
    for (j, &v) in rem_slice.iter().enumerate() {
      let idx = rem_start + j;
      let enc = (v * exp_factor).round_ties_even() as i64;
      // SAFETY: idx < slice.len()
      unsafe { *enc_ptr.add(idx) = enc };
      let d = if use_div {
        (enc as f64) / exp_factor
      } else if fac_int == 1 {
        (enc as f64) * frac_exp
      } else {
        ((enc.wrapping_mul(fac_int)) as f64) * frac_exp
      };
      if d.to_bits() == v.to_bits() {
        min_int = min_int.min(enc);
        max_int = max_int.max(enc);
      } else {
        any_diff |= 1;
      }
    }
  }

  if any_diff == 0 {
    return (min_int, max_int);
  }

  // Slow path: exceptions exist. Find them and patch enc_ptr
  let mut min_int_rescanned = i64::MAX;
  let mut max_int_rescanned = i64::MIN;

  macro_rules! run_rescan {
    ($decode:expr) => {
      for (idx, &v) in slice.iter().enumerate() {
        // SAFETY: idx < slice.len()
        let enc = unsafe { *enc_ptr.add(idx) };
        let d = $decode(enc);
        if d.to_bits() == v.to_bits() {
          min_int_rescanned = min_int_rescanned.min(enc);
          max_int_rescanned = max_int_rescanned.max(enc);
        } else {
          // SAFETY: idx < slice.len()
          unsafe { *enc_ptr.add(idx) = 0 };
          exceptions.push(Exception {
            pos: idx,
            bits: v.to_bits(),
          });
          if exceptions.len() > MAX_EXCEPTIONS {
            return (i64::MAX, i64::MIN);
          }
        }
      }
    };
  }

  if use_div {
    run_rescan!(|enc: i64| (enc as f64) / exp_factor);
  } else if fac_int == 1 {
    run_rescan!(|enc: i64| (enc as f64) * frac_exp);
  } else {
    run_rescan!(|enc: i64| (enc.wrapping_mul(fac_int) as f64) * frac_exp);
  }

  (min_int_rescanned, max_int_rescanned)
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
unsafe fn encode_fearless_f32_kernel<S: Simd>(
  simd: S,
  slice: &[f32],
  enc_ptr: *mut i32,
  exp_factor: f32,
  fac_int: i64,
  frac_exp: f32,
  use_div: bool,
  exceptions: &mut Vec<Exception<u32>>,
) -> (i32, i32) {
  let n = S::f32s::N;
  let stride = n * 4;
  let exp_v = S::f32s::splat(simd, exp_factor);
  let frac_v = S::f32s::splat(simd, frac_exp);
  let fac_v = S::i32s::splat(simd, fac_int as i32);

  let mut min_v0 = S::f32s::splat(simd, f32::INFINITY);
  let mut min_v1 = S::f32s::splat(simd, f32::INFINITY);
  let mut max_v0 = S::f32s::splat(simd, f32::NEG_INFINITY);
  let mut max_v1 = S::f32s::splat(simd, f32::NEG_INFINITY);
  let mut any_diff_v = S::u32s::splat(simd, 0);

  let full_chunks_len = slice.len() / stride * stride;
  let (full_slice, rem_slice) = slice.split_at(full_chunks_len);

  macro_rules! run_loop {
    ($decode:expr) => {
      for (chunk_idx, c) in full_slice.chunks_exact(stride).enumerate() {
        let base_out = chunk_idx * stride;
        let v0 = S::f32s::from_slice(simd, &c[0..n]);
        let v1 = S::f32s::from_slice(simd, &c[n..n * 2]);
        let v2 = S::f32s::from_slice(simd, &c[n * 2..n * 3]);
        let v3 = S::f32s::from_slice(simd, &c[n * 3..n * 4]);

        let r0 = (v0 * exp_v).round_ties_even();
        let r1 = (v1 * exp_v).round_ties_even();
        let r2 = (v2 * exp_v).round_ties_even();
        let r3 = (v3 * exp_v).round_ties_even();

        let i0 = r0.to_int::<S::i32s>();
        let i1 = r1.to_int::<S::i32s>();
        let i2 = r2.to_int::<S::i32s>();
        let i3 = r3.to_int::<S::i32s>();

        // SAFETY: enc_ptr has space for at least slice.len() elements, storing directly to initialized chunk slots
        unsafe {
          i0.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out), n));
          i1.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out + n), n));
          i2.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out + n * 2), n));
          i3.store_slice(core::slice::from_raw_parts_mut(enc_ptr.add(base_out + n * 3), n));
        }

        let d0 = $decode(i0);
        let d1 = $decode(i1);
        let d2 = $decode(i2);
        let d3 = $decode(i3);

        let diff0 = d0.bitcast::<S::u32s>() ^ v0.bitcast::<S::u32s>();
        let diff1 = d1.bitcast::<S::u32s>() ^ v1.bitcast::<S::u32s>();
        let diff2 = d2.bitcast::<S::u32s>() ^ v2.bitcast::<S::u32s>();
        let diff3 = d3.bitcast::<S::u32s>() ^ v3.bitcast::<S::u32s>();

        any_diff_v |= ((diff0 | diff1) | (diff2 | diff3));

        min_v0 = min_v0.min(r0);
        min_v1 = min_v1.min(r1);
        min_v0 = min_v0.min(r2);
        min_v1 = min_v1.min(r3);

        max_v0 = max_v0.max(r0);
        max_v1 = max_v1.max(r1);
        max_v0 = max_v0.max(r2);
        max_v1 = max_v1.max(r3);
      }
    };
  }

  if use_div {
    run_loop!(|i: S::i32s| i.to_float::<S::f32s>() / exp_v);
  } else if fac_int == 1 {
    run_loop!(|i: S::i32s| i.to_float::<S::f32s>() * frac_v);
  } else {
    run_loop!(|i: S::i32s| (i * fac_v).to_float::<S::f32s>() * frac_v);
  }

  let min_v = min_v0.min(min_v1);
  let max_v = max_v0.max(max_v1);

  let mut min_arr = [0.0f32; 16];
  let mut max_arr = [0.0f32; 16];
  let mut diff_arr = [0u32; 16];
  min_v.store_slice(&mut min_arr[..n]);
  max_v.store_slice(&mut max_arr[..n]);
  any_diff_v.store_slice(&mut diff_arr[..n]);

  let mut any_diff = 0u32;
  let (mut min_int, mut max_int) = if full_chunks_len > 0 {
    let mut min_val = f32::INFINITY;
    let mut max_val = f32::NEG_INFINITY;
    for i in 0..n {
      min_val = min_val.min(min_arr[i]);
      max_val = max_val.max(max_arr[i]);
      any_diff |= diff_arr[i];
    }
    (min_val as i32, max_val as i32)
  } else {
    (i32::MAX, i32::MIN)
  };

  if !rem_slice.is_empty() {
    let rem_start = full_chunks_len;
    for (j, &v) in rem_slice.iter().enumerate() {
      let idx = rem_start + j;
      let enc = (v * exp_factor).round_ties_even() as i32;
      // SAFETY: idx < slice.len()
      unsafe { *enc_ptr.add(idx) = enc };
      let d = if use_div {
        (enc as f32) / exp_factor
      } else if fac_int == 1 {
        (enc as f32) * frac_exp
      } else {
        ((enc.wrapping_mul(fac_int as i32)) as f32) * frac_exp
      };
      if d.to_bits() == v.to_bits() {
        min_int = min_int.min(enc);
        max_int = max_int.max(enc);
      } else {
        any_diff |= 1;
      }
    }
  }

  if any_diff == 0 {
    return (min_int, max_int);
  }

  // Slow path: exceptions exist. Find them and patch enc_ptr
  let mut min_int_rescanned = i32::MAX;
  let mut max_int_rescanned = i32::MIN;

  macro_rules! run_rescan {
    ($decode:expr) => {
      for (idx, &v) in slice.iter().enumerate() {
        // SAFETY: idx < slice.len()
        let enc = unsafe { *enc_ptr.add(idx) };
        let d = $decode(enc);
        if d.to_bits() == v.to_bits() {
          min_int_rescanned = min_int_rescanned.min(enc);
          max_int_rescanned = max_int_rescanned.max(enc);
        } else {
          // SAFETY: idx < slice.len()
          unsafe { *enc_ptr.add(idx) = 0 };
          exceptions.push(Exception {
            pos: idx,
            bits: v.to_bits(),
          });
          if exceptions.len() > MAX_EXCEPTIONS {
            return (i32::MAX, i32::MIN);
          }
        }
      }
    };
  }

  if use_div {
    run_rescan!(|enc: i32| (enc as f32) / exp_factor);
  } else if fac_int == 1 {
    run_rescan!(|enc: i32| (enc as f32) * frac_exp);
  } else {
    run_rescan!(|enc: i32| (enc.wrapping_mul(fac_int as i32) as f32) * frac_exp);
  }

  (min_int_rescanned, max_int_rescanned)
}

pub(crate) unsafe fn encode_simd_f64(
  slice: &[f64],
  enc_ptr: *mut i64,
  exp_factor: f64,
  fac_int: i64,
  frac_exp: f64,
  use_div: bool,
  exceptions: &mut Vec<Exception<u64>>,
) -> (i64, i64) {
  let level = Level::new();
  dispatch!(level, simd => {
    // SAFETY: enc_ptr points to valid buffer of length >= slice.len()
    unsafe {
      encode_fearless_f64_kernel(simd, slice, enc_ptr, exp_factor, fac_int, frac_exp, use_div, exceptions)
    }
  })
}

pub(crate) unsafe fn encode_simd_f32(
  slice: &[f32],
  enc_ptr: *mut i32,
  exp_factor: f32,
  fac_int: i64,
  frac_exp: f32,
  use_div: bool,
  exceptions: &mut Vec<Exception<u32>>,
) -> (i32, i32) {
  let level = Level::new();
  dispatch!(level, simd => {
    // SAFETY: enc_ptr points to valid buffer of length >= slice.len()
    unsafe {
      encode_fearless_f32_kernel(simd, slice, enc_ptr, exp_factor, fac_int, frac_exp, use_div, exceptions)
    }
  })
}

/// Dispatches to optimal vectorized encoding kernel based on float type.
/// 统一分发至最优的向量化编码内核
#[inline(always)]
pub(crate) unsafe fn encode_slice<F: AlpFloat>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  exp_factor: F,
  fac_int: i64,
  frac_exp: F,
  use_div: bool,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  // SAFETY: Caller guarantees slice is continuous and enc_ptr has space for at least slice.len() elements.
  unsafe {
    F::encode_simd(
      slice, enc_ptr, exp_factor, fac_int, frac_exp, use_div, exceptions,
    )
  }
}
