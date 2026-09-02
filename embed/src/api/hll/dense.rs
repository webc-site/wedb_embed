use std::ptr::eq as ptr_eq;

use crate::hll::algo::{
  HLL_DENSE_SIZE, HLL_REGISTER_BITS, HLL_REGISTER_MAX, HLL_REGISTERS, HLL_SEGMENT_BYTES,
  HLL_SEGMENT_COUNT, HLL_SEGMENT_REGISTERS, hll_estimate_from_histo,
};

/// Extracts 6-bit register value aligned with Apache Kvrocks HllDenseGetRegister.
/// 提取 6-bit 寄存器值（对标 Apache Kvrocks HllDenseGetRegister）
#[inline]
pub fn hll_dense_get_register(registers: &[u8], register_index: usize) -> u8 {
  let bit = register_index * HLL_REGISTER_BITS;
  let byte = bit / 8;
  let fb = (bit & 7) as u8;
  let fb8 = 8 - fb;
  let b0 = registers.get(byte).copied().unwrap_or(0) as u16;
  let b1 = if fb as usize > 8 - HLL_REGISTER_BITS {
    registers.get(byte + 1).copied().unwrap_or(0) as u16
  } else {
    0
  };
  (((b0 >> fb) | (b1 << fb8)) & (HLL_REGISTER_MAX as u16)) as u8
}

/// Compatibility alias.
/// 兼容别名
#[inline]
pub fn get_register(registers: &[u8], index: usize) -> u8 {
  hll_dense_get_register(registers, index)
}

/// Sets 6-bit register value aligned with Apache Kvrocks HllDenseSetRegister.
/// 设置 6-bit 寄存器值（对标 Apache Kvrocks HllDenseSetRegister）
#[inline]
pub fn hll_dense_set_register(registers: &mut [u8], register_index: usize, val: u8) {
  let bit = register_index * HLL_REGISTER_BITS;
  let byte = bit / 8;
  let fb = (bit & 7) as u8;
  let fb8 = 8 - fb;
  let v = (val & HLL_REGISTER_MAX) as u16;
  let max_mask = HLL_REGISTER_MAX as u16;

  if byte < registers.len() {
    let b0 = registers[byte] as u16;
    let b0_new = (b0 & !(max_mask << fb)) | (v << fb);
    registers[byte] = (b0_new & 0xFF) as u8;
  }

  if fb as usize > 8 - HLL_REGISTER_BITS && byte + 1 < registers.len() {
    let b1 = registers[byte + 1] as u16;
    let b1_new = (b1 & !(max_mask >> fb8)) | (v >> fb8);
    registers[byte + 1] = (b1_new & 0xFF) as u8;
  }
}

/// Compatibility alias.
/// 兼容别名
#[inline]
pub fn set_register(registers: &mut [u8], index: usize, val: u8) {
  hll_dense_set_register(registers, index, val);
}

/// 16-register unrolled histogram calculation aligned with Apache Kvrocks HllDenseRegHisto.
/// 16 寄存器循环展开的高性能直方图统计（对标 Apache Kvrocks HllDenseRegHisto）
#[inline]
pub fn hll_dense_reg_histo(registers: &[u8], reghisto: &mut [usize; 64]) {
  let (chunks, remainder) = registers.as_chunks::<12>();
  for r in chunks {
    let b0 = r[0] as usize;
    let b1 = r[1] as usize;
    let b2 = r[2] as usize;
    let b3 = r[3] as usize;
    let b4 = r[4] as usize;
    let b5 = r[5] as usize;
    let b6 = r[6] as usize;
    let b7 = r[7] as usize;
    let b8 = r[8] as usize;
    let b9 = r[9] as usize;
    let b10 = r[10] as usize;
    let b11 = r[11] as usize;

    let r0 = b0 & 0x3F;
    let r1 = (b0 >> 6) | ((b1 & 0x0F) << 2);
    let r2 = (b1 >> 4) | ((b2 & 0x03) << 4);
    let r3 = b2 >> 2;

    let r4 = b3 & 0x3F;
    let r5 = (b3 >> 6) | ((b4 & 0x0F) << 2);
    let r6 = (b4 >> 4) | ((b5 & 0x03) << 4);
    let r7 = b5 >> 2;

    let r8 = b6 & 0x3F;
    let r9 = (b6 >> 6) | ((b7 & 0x0F) << 2);
    let r10 = (b7 >> 4) | ((b8 & 0x03) << 4);
    let r11 = b8 >> 2;

    let r12 = b9 & 0x3F;
    let r13 = (b9 >> 6) | ((b10 & 0x0F) << 2);
    let r14 = (b10 >> 4) | ((b11 & 0x03) << 4);
    let r15 = b11 >> 2;

    // SAFETY: r0..r15 are guaranteed <= 63 by bitmasking.
    unsafe {
      *reghisto.get_unchecked_mut(r0) += 1;
      *reghisto.get_unchecked_mut(r1) += 1;
      *reghisto.get_unchecked_mut(r2) += 1;
      *reghisto.get_unchecked_mut(r3) += 1;
      *reghisto.get_unchecked_mut(r4) += 1;
      *reghisto.get_unchecked_mut(r5) += 1;
      *reghisto.get_unchecked_mut(r6) += 1;
      *reghisto.get_unchecked_mut(r7) += 1;
      *reghisto.get_unchecked_mut(r8) += 1;
      *reghisto.get_unchecked_mut(r9) += 1;
      *reghisto.get_unchecked_mut(r10) += 1;
      *reghisto.get_unchecked_mut(r11) += 1;
      *reghisto.get_unchecked_mut(r12) += 1;
      *reghisto.get_unchecked_mut(r13) += 1;
      *reghisto.get_unchecked_mut(r14) += 1;
      *reghisto.get_unchecked_mut(r15) += 1;
    }
  }

  if !remainder.is_empty() {
    let base_reg = chunks.len() * 16;
    let total_regs = (registers.len() * 8) / 6;
    for i in base_reg..total_regs {
      let v = hll_dense_get_register(registers, i) as usize;
      if v < 64 {
        // SAFETY: 上方已显式判定 v < 64，且 reghisto 长度为 64，索引严格合法。
        unsafe {
          *reghisto.get_unchecked_mut(v) += 1;
        }
      }
    }
  }
}

/// High-precision cardinality estimation based on dense registers aligned with Apache Kvrocks HllDenseEstimate.
/// 基于完整密集寄存器的高精度基数估算（零堆分配，对标 Apache Kvrocks HllDenseEstimate）
#[inline]
pub fn hll_dense_estimate(registers: &[u8]) -> u64 {
  let mut reghisto = [0usize; 64];

  if registers.len() >= HLL_DENSE_SIZE {
    hll_dense_reg_histo(&registers[..HLL_DENSE_SIZE], &mut reghisto);
  } else {
    hll_dense_reg_histo(registers, &mut reghisto);
    let present_regs = (registers.len() * 8) / 6;
    let missing_regs = HLL_REGISTERS.saturating_sub(present_regs);
    reghisto[0] += missing_regs;
  }

  hll_estimate_from_histo(&reghisto)
}

/// Compatibility alias.
/// 兼容别名
#[inline]
pub fn dense_estimate(registers: &[u8]) -> u64 {
  hll_dense_estimate(registers)
}

/// High-performance cardinality estimation across 16 segments aligned with Kvrocks HllDenseEstimate.
/// 基于 16 个分段（每段 768 字节 / 1024 寄存器）的高性能基数估算（对标 Apache Kvrocks HllDenseEstimate(vector<span<uint8_t>>)）
#[inline]
pub fn hll_dense_estimate_segments(segments: &[Option<&[u8]>]) -> u64 {
  let mut reghisto = [0usize; 64];

  for segment in segments {
    match segment {
      Some(seg) if !seg.is_empty() => {
        hll_dense_reg_histo(seg, &mut reghisto);
        let present_regs = (seg.len() * 8) / 6;
        let missing_regs = HLL_SEGMENT_REGISTERS.saturating_sub(present_regs);
        reghisto[0] += missing_regs;
      }
      _ => {
        reghisto[0] += HLL_SEGMENT_REGISTERS;
      }
    }
  }

  if segments.len() < HLL_SEGMENT_COUNT {
    reghisto[0] += (HLL_SEGMENT_COUNT - segments.len()) * HLL_SEGMENT_REGISTERS;
  }

  hll_estimate_from_histo(&reghisto)
}

#[inline(always)]
fn merge_3_bytes(d: &mut [u8; 3], s: &[u8; 3]) {
  if (s[0] | s[1] | s[2]) == 0 {
    return;
  }
  if (d[0] | d[1] | d[2]) == 0 {
    d.copy_from_slice(s);
    return;
  }

  let dr0 = d[0] & 0x3F;
  let dr1 = (d[0] >> 6) | ((d[1] & 0x0F) << 2);
  let dr2 = (d[1] >> 4) | ((d[2] & 0x03) << 4);
  let dr3 = d[2] >> 2;

  let sr0 = s[0] & 0x3F;
  let sr1 = (s[0] >> 6) | ((s[1] & 0x0F) << 2);
  let sr2 = (s[1] >> 4) | ((s[2] & 0x03) << 4);
  let sr3 = s[2] >> 2;

  let m0 = dr0.max(sr0);
  let m1 = dr1.max(sr1);
  let m2 = dr2.max(sr2);
  let m3 = dr3.max(sr3);

  d[0] = (m0 & 0x3F) | ((m1 & 0x03) << 6);
  d[1] = (m1 >> 2) | ((m2 & 0x0F) << 4);
  d[2] = (m2 >> 4) | ((m3 & 0x3F) << 2);
}

/// High-performance 12-byte (16 registers) unrolled MAX in-place merge aligned with Apache Kvrocks HllMerge.
/// 高性能 12 字节（16 寄存器）循环展开 MAX 就地合并（对标 Apache Kvrocks HllMerge）
#[inline]
pub fn hll_merge_bytes(dest: &mut [u8], src: &[u8]) {
  if ptr_eq(dest.as_ptr(), src.as_ptr()) || dest.is_empty() || src.is_empty() {
    return;
  }

  let limit = dest.len().min(src.len());
  let (d_chunks12, d_rem12) = dest[..limit].as_chunks_mut::<12>();
  let (s_chunks12, s_rem12) = src[..limit].as_chunks::<12>();

  for (d12, s12) in d_chunks12.iter_mut().zip(s_chunks12) {
    let (d_3s, _) = d12.as_chunks_mut::<3>();
    let (s_3s, _) = s12.as_chunks::<3>();
    merge_3_bytes(&mut d_3s[0], &s_3s[0]);
    merge_3_bytes(&mut d_3s[1], &s_3s[1]);
    merge_3_bytes(&mut d_3s[2], &s_3s[2]);
    merge_3_bytes(&mut d_3s[3], &s_3s[3]);
  }

  let (d_chunks3, d_rem) = d_rem12.as_chunks_mut::<3>();
  let (s_chunks3, s_rem) = s_rem12.as_chunks::<3>();

  for (d, s) in d_chunks3.iter_mut().zip(s_chunks3) {
    merge_3_bytes(d, s);
  }

  if !d_rem.is_empty() {
    let rem_regs = (d_rem.len() * 8) / 6;
    for i in 0..rem_regs {
      let s_val = hll_dense_get_register(s_rem, i);
      let d_val = hll_dense_get_register(d_rem, i);
      if s_val > d_val {
        hll_dense_set_register(d_rem, i, s_val);
      }
    }
  }
}

/// Segmented register merge aligned with Apache Kvrocks HllMerge.
/// 分段寄存器合并（对标 Apache Kvrocks HllMerge）
#[inline]
pub fn hll_merge_segments(dest: &mut [Vec<u8>], src: &[Option<&[u8]>]) {
  for (dest_seg, src_seg_opt) in dest.iter_mut().zip(src.iter()) {
    if let Some(src_seg) = src_seg_opt {
      if src_seg.is_empty() {
        continue;
      }
      if dest_seg.is_empty() {
        dest_seg.resize(HLL_SEGMENT_BYTES, 0);
        let copy_len = src_seg.len().min(HLL_SEGMENT_BYTES);
        dest_seg[..copy_len].copy_from_slice(&src_seg[..copy_len]);
        continue;
      }
      hll_merge_bytes(dest_seg, src_seg);
    }
  }
}
