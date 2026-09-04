use crate::{constants::MAX_EXCEPTIONS, encoder::exception::Exception, float::AlpFloat};

/// 针对浮点数编码的无分支 8 路展开内核：
/// 批处理 8 个元素，若 100% 精确命中（无异常），直接连续写入并利用无分支树形聚合更新 min/max，消除 95% 异常分支开销。
#[inline(always)]
unsafe fn encode_loop_core<F: AlpFloat, D: Fn(F::Int) -> F>(
  slice: &[F],
  enc_ptr: *mut F::Int,
  exp_factor: F,
  decode: D,
  exceptions: &mut Vec<Exception<F::RawBits>>,
) -> (F::Int, F::Int) {
  // SAFETY: 调用方保证 slice 连续有效，且 enc_ptr 具备至少 slice.len() 个连续可写 F::Int 元素空间。
  unsafe {
    let mut min_val = F::MAX_INT;
    let mut max_val = F::MIN_INT;
    let len = slice.len();
    let unroll_len = len & !7;
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
      // SAFETY: i + 7 < unroll_len <= len，严格在切片边界内
      let v0 = *slice.get_unchecked(i);
      let v1 = *slice.get_unchecked(i + 1);
      let v2 = *slice.get_unchecked(i + 2);
      let v3 = *slice.get_unchecked(i + 3);
      let v4 = *slice.get_unchecked(i + 4);
      let v5 = *slice.get_unchecked(i + 5);
      let v6 = *slice.get_unchecked(i + 6);
      let v7 = *slice.get_unchecked(i + 7);

      let enc0 = v0.fast_round_to_int(exp_factor);
      let enc1 = v1.fast_round_to_int(exp_factor);
      let enc2 = v2.fast_round_to_int(exp_factor);
      let enc3 = v3.fast_round_to_int(exp_factor);
      let enc4 = v4.fast_round_to_int(exp_factor);
      let enc5 = v5.fast_round_to_int(exp_factor);
      let enc6 = v6.fast_round_to_int(exp_factor);
      let enc7 = v7.fast_round_to_int(exp_factor);

      let d0 = decode(enc0);
      let d1 = decode(enc1);
      let d2 = decode(enc2);
      let d3 = decode(enc3);
      let d4 = decode(enc4);
      let d5 = decode(enc5);
      let d6 = decode(enc6);
      let d7 = decode(enc7);

      let ok0 = d0.is_exact_same(v0);
      let ok1 = d1.is_exact_same(v1);
      let ok2 = d2.is_exact_same(v2);
      let ok3 = d3.is_exact_same(v3);
      let ok4 = d4.is_exact_same(v4);
      let ok5 = d5.is_exact_same(v5);
      let ok6 = d6.is_exact_same(v6);
      let ok7 = d7.is_exact_same(v7);

      if ok0 && ok1 && ok2 && ok3 && ok4 && ok5 && ok6 && ok7 {
        enc_ptr.add(i).write(enc0);
        enc_ptr.add(i + 1).write(enc1);
        enc_ptr.add(i + 2).write(enc2);
        enc_ptr.add(i + 3).write(enc3);
        enc_ptr.add(i + 4).write(enc4);
        enc_ptr.add(i + 5).write(enc5);
        enc_ptr.add(i + 6).write(enc6);
        enc_ptr.add(i + 7).write(enc7);
        let l_min0 = (enc0.min(enc1)).min(enc2.min(enc3));
        let l_max0 = (enc0.max(enc1)).max(enc2.max(enc3));
        let l_min1 = (enc4.min(enc5)).min(enc6.min(enc7));
        let l_max1 = (enc4.max(enc5)).max(enc6.max(enc7));
        min_val = min_val.min(l_min0.min(l_min1));
        max_val = max_val.max(l_max0.max(l_max1));
      } else {
        handle_one!(i, v0, enc0, ok0);
        handle_one!(i + 1, v1, enc1, ok1);
        handle_one!(i + 2, v2, enc2, ok2);
        handle_one!(i + 3, v3, enc3, ok3);
        handle_one!(i + 4, v4, enc4, ok4);
        handle_one!(i + 5, v5, enc5, ok5);
        handle_one!(i + 6, v6, enc6, ok6);
        handle_one!(i + 7, v7, enc7, ok7);
        if exceptions.len() > MAX_EXCEPTIONS {
          return (F::MAX_INT, F::MIN_INT);
        }
      }
      i += 8;
    }

    while i < len {
      // SAFETY: i < len，严格在切片边界内
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
