use crate::{constants::MAX_EXCEPTIONS, encoder::exception::Exception, float::AlpFloat};

/// Unified branchless unrolled encoding loop for 4 elements per step.
/// 针对浮点数编码的无分支 4-way 展开内核：
/// 批处理 4 个元素，若 100% 精确命中（无异常），直接 SIMD/向量化写入并利用无分支指令更新 min/max，消除 95% 异常分支开销。
#[inline(always)]
unsafe fn encode_loop_core<F: AlpFloat, D: Fn(F::Int) -> F>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  exp_factor: F,
  decode: D,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  unsafe {
    let mut min_val = F::MAX_INT;
    let mut max_val = F::MIN_INT;
    let len = slice.len();
    let unroll_len = len & !3;
    let mut i = 0;

    macro_rules! handle_one {
      ($idx:expr, $val:expr, $enc:expr, $ok:expr) => {
        if $ok {
          enc_ptr.add($idx).write($enc);
          min_val = min_val.min($enc);
          max_val = max_val.max($enc);
        } else {
          enc_ptr.add($idx).write(F::ZERO_INT);
          exceptions.push(Exception {
            pos: $idx,
            bits: $val.to_raw_bits(),
          });
        }
      };
    }

    while i < unroll_len {
      // SAFETY: i + 3 < unroll_len <= len, within slice bounds
      let v0 = *slice.get_unchecked(i);
      let v1 = *slice.get_unchecked(i + 1);
      let v2 = *slice.get_unchecked(i + 2);
      let v3 = *slice.get_unchecked(i + 3);

      let enc0 = v0.fast_round_to_int(exp_factor);
      let enc1 = v1.fast_round_to_int(exp_factor);
      let enc2 = v2.fast_round_to_int(exp_factor);
      let enc3 = v3.fast_round_to_int(exp_factor);

      let d0 = decode(enc0);
      let d1 = decode(enc1);
      let d2 = decode(enc2);
      let d3 = decode(enc3);

      let ok0 = d0.is_exact_same(v0);
      let ok1 = d1.is_exact_same(v1);
      let ok2 = d2.is_exact_same(v2);
      let ok3 = d3.is_exact_same(v3);

      if ok0 && ok1 && ok2 && ok3 {
        enc_ptr.add(i).write(enc0);
        enc_ptr.add(i + 1).write(enc1);
        enc_ptr.add(i + 2).write(enc2);
        enc_ptr.add(i + 3).write(enc3);
        let l_min = (enc0.min(enc1)).min(enc2.min(enc3));
        let l_max = (enc0.max(enc1)).max(enc2.max(enc3));
        min_val = min_val.min(l_min);
        max_val = max_val.max(l_max);
      } else {
        handle_one!(i, v0, enc0, ok0);
        handle_one!(i + 1, v1, enc1, ok1);
        handle_one!(i + 2, v2, enc2, ok2);
        handle_one!(i + 3, v3, enc3, ok3);
        if exceptions.len() > MAX_EXCEPTIONS {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 4;
    }

    while i < len {
      // SAFETY: i < len, within bounds
      let val = *slice.get_unchecked(i);
      let enc = val.fast_round_to_int(exp_factor);
      let d = decode(enc);
      handle_one!(i, val, enc, d.is_exact_same(val));
      if exceptions.len() > MAX_EXCEPTIONS {
        return (F::MAX_INT, F::MIN_INT);
      }
      i += 1;
    }

    (min_val, max_val)
  }
}

/// Dispatches to the optimal unrolled encoding loop based on parameters.
/// 统一根据参数特征分发至最优的展开编码内核（除法模式、fac_int==1 无因子模式、常规双因子模式）
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
  unsafe {
    if use_div {
      encode_loop_core(
        slice,
        enc_ptr,
        exp_factor,
        #[inline(always)]
        |enc| F::decode_from_int_div(enc, exp_factor),
        exceptions,
      )
    } else if fac_int == 1 {
      encode_loop_core(
        slice,
        enc_ptr,
        exp_factor,
        #[inline(always)]
        |enc| F::decode_from_int_fac1(enc, frac_exp),
        exceptions,
      )
    } else {
      encode_loop_core(
        slice,
        enc_ptr,
        exp_factor,
        #[inline(always)]
        |enc| F::decode_from_int(enc, fac_int, frac_exp),
        exceptions,
      )
    }
  }
}
