use crate::{
  bitpack::{bitpack_u64, packed_byte_size},
  float::AlpFloat,
  header::{raw_header_len, write_header},
  params::bits_needed,
};

pub(crate) const MAX_RD_DICT_SIZE: usize = 8;
const RD_SAMPLE_POINTS: usize = 64;
const HASH_TABLE_SIZE: usize = 256;
const HASH_TABLE_MASK: usize = HASH_TABLE_SIZE - 1;

/// Low-overhead candidate for Real Doubles (ALP-RD) compression.
/// 真实双精度高低位解耦（ALP-RD）压缩候选元数据
pub(crate) struct RdCandidate {
  pub right_bw: u8,
  pub left_bw: u8,
  pub actual_dict_size: u8,
  pub dict: [u16; MAX_RD_DICT_SIZE],
  pub total_size: usize,
}

/// Evaluates and encodes float slice using Real Doubles (ALP-RD) bit decoupling.
///
/// 通过高低位解耦技术，将难以十进制化的浮点数拆分为紧凑高位字典索引与低位连续比特流：
/// 1. 快速多点采样评估最佳截断位宽；
/// 2. 高位（阶码及符号位）抽取至多 8 个最高频离散项进行紧凑位打包；
/// 3. 超出字典项的极少量高位记为异常补丁；
/// 4. 低位直接高吞吐位打包。
pub(crate) fn try_encode_rd<F: AlpFloat>(
  slice: &[F],
  left_indices: &mut Vec<u64>,
  right_parts: &mut Vec<u64>,
  exceptions: &mut Vec<(u16, u16)>,
) -> Option<RdCandidate> {
  let count = slice.len();
  if count == 0 || count > u16::MAX as usize {
    return None;
  }

  let total_bits = F::RD_TOTAL_BITS;
  let max_cut = F::RD_MAX_CUT;

  // 1. 快速采样探测最佳 cut 宽度
  let sample_step = (count / RD_SAMPLE_POINTS).max(1);
  let mut best_cut = max_cut;
  let mut best_est_cost = usize::MAX;

  for cut in 1..=max_cut {
    let right_bw = total_bits - cut;
    let mut freq_keys = [0u16; MAX_RD_DICT_SIZE];
    let mut freq_counts = [0u16; MAX_RD_DICT_SIZE];
    let mut num_distinct = 0usize;
    let mut exc_count = 0usize;

    for &v in slice.iter().step_by(sample_step) {
      let raw = v.to_u64_key();
      let left = (raw >> right_bw) as u16;

      if let Some(pos) = freq_keys[..num_distinct].iter().position(|&k| k == left) {
        freq_counts[pos] += 1;
      } else if num_distinct < MAX_RD_DICT_SIZE {
        freq_keys[num_distinct] = left;
        freq_counts[num_distinct] = 1;
        num_distinct += 1;
      } else {
        exc_count += 1;
      }
    }

    let left_bw = if num_distinct <= 1 {
      0
    } else {
      bits_needed((num_distinct - 1) as u64)
    };

    let est_cost = packed_byte_size(count, right_bw)
      + packed_byte_size(count, left_bw)
      + exc_count * sample_step * 4;

    if est_cost < best_est_cost {
      best_est_cost = est_cost;
      best_cut = cut;
    }
  }

  // 2. 使用确定的最佳 cut 宽度对完整切片建立字典与提取高低位
  let right_bw = total_bits - best_cut;

  // 栈上 256 项开放寻址哈希表统计频次，零堆内存分配
  let mut table_keys = [0u16; HASH_TABLE_SIZE];
  let mut table_counts = [0u16; HASH_TABLE_SIZE];
  let mut table_occupied = [0u64; HASH_TABLE_SIZE / 64];

  for &v in slice {
    let raw = v.to_u64_key();
    let left = (raw >> right_bw) as u16;
    let mut idx = ((left as usize).wrapping_mul(0x9E37) >> 8) & HASH_TABLE_MASK;
    // 严格限制探测步数至多 HASH_TABLE_SIZE，杜绝哈希表满槽时潜在的死循环
    for _ in 0..HASH_TABLE_SIZE {
      let word_idx = idx >> 6;
      let bit_mask = 1u64 << (idx & 63);
      if (table_occupied[word_idx] & bit_mask) == 0 {
        table_occupied[word_idx] |= bit_mask;
        table_keys[idx] = left;
        table_counts[idx] = 1;
        break;
      }
      if table_keys[idx] == left {
        table_counts[idx] = table_counts[idx].saturating_add(1);
        break;
      }
      idx = (idx + 1) & HASH_TABLE_MASK;
    }
  }

  // 利用 trailing_zeros 位运算快速遍历已占用槽位，筛选出频次最高的至多 8 个高位字典项
  let mut dict = [0u16; MAX_RD_DICT_SIZE];
  let mut top_counts = [0u16; MAX_RD_DICT_SIZE];
  let mut actual_dict_size = 0usize;

  for (word_idx, &word_bits) in table_occupied.iter().enumerate() {
    let mut bits = word_bits;
    while bits != 0 {
      let trailing = bits.trailing_zeros() as usize;
      let idx = (word_idx << 6) | trailing;
      bits &= bits - 1;

      let key = table_keys[idx];
      let count_val = table_counts[idx];

      if actual_dict_size < MAX_RD_DICT_SIZE {
        dict[actual_dict_size] = key;
        top_counts[actual_dict_size] = count_val;
        actual_dict_size += 1;
      } else {
        let mut min_idx = 0;
        let mut min_count = top_counts[0];
        for (j, &c) in top_counts.iter().enumerate().skip(1) {
          if c < min_count {
            min_count = c;
            min_idx = j;
          }
        }
        if count_val > min_count {
          top_counts[min_idx] = count_val;
          dict[min_idx] = key;
        }
      }
    }
  }

  encode_rd_fast(
    slice,
    right_bw,
    actual_dict_size,
    dict,
    left_indices,
    right_parts,
    exceptions,
  )
}

/// Fast encoding using pre-determined cut width and dictionary (zero parameter search overhead).
/// 使用已确定截断位宽与字典直接编码（零参数采样与探测开销）
pub(crate) fn encode_rd_fast<F: AlpFloat>(
  slice: &[F],
  right_bw: u8,
  actual_dict_size: usize,
  dict: [u16; MAX_RD_DICT_SIZE],
  left_indices: &mut Vec<u64>,
  right_parts: &mut Vec<u64>,
  exceptions: &mut Vec<(u16, u16)>,
) -> Option<RdCandidate> {
  let count = slice.len();
  let right_mask = if right_bw == 64 {
    u64::MAX
  } else {
    (1u64 << right_bw) - 1
  };
  let left_bw = if actual_dict_size <= 1 {
    0
  } else {
    bits_needed((actual_dict_size - 1) as u64)
  };

  left_indices.clear();
  if left_bw > 0 {
    if left_indices.capacity() < count {
      left_indices.reserve(count);
    }
    // SAFETY: every element in 0..count will be fully initialized below
    unsafe { left_indices.set_len(count) };
  }
  right_parts.clear();
  if right_parts.capacity() < count {
    right_parts.reserve(count);
  }
  // SAFETY: every element in 0..count will be fully initialized below
  unsafe { right_parts.set_len(count) };
  exceptions.clear();

  let left_ptr = left_indices.as_mut_ptr();
  let right_ptr = right_parts.as_mut_ptr();

  // SAFETY: left_indices and right_parts have length count, pos < count
  unsafe {
    if actual_dict_size == 1 {
      let d0 = dict[0];
      for (pos, &v) in slice.iter().enumerate() {
        let raw = v.to_u64_key();
        let left = (raw >> right_bw) as u16;
        let right = raw & right_mask;
        *right_ptr.add(pos) = right;
        if left != d0 {
          exceptions.push((pos as u16, left));
        }
      }
    } else if actual_dict_size == 2 {
      let d0 = dict[0];
      let d1 = dict[1];
      for (pos, &v) in slice.iter().enumerate() {
        let raw = v.to_u64_key();
        let left = (raw >> right_bw) as u16;
        let right = raw & right_mask;
        *right_ptr.add(pos) = right;
        if left == d0 {
          *left_ptr.add(pos) = 0;
        } else if left == d1 {
          *left_ptr.add(pos) = 1;
        } else {
          exceptions.push((pos as u16, left));
        }
      }
    } else {
      for (pos, &v) in slice.iter().enumerate() {
        let raw = v.to_u64_key();
        let left = (raw >> right_bw) as u16;
        let right = raw & right_mask;
        *right_ptr.add(pos) = right;

        let mut matched = false;
        for (idx, &entry) in dict[..actual_dict_size].iter().enumerate() {
          if entry == left {
            *left_ptr.add(pos) = idx as u64;
            matched = true;
            break;
          }
        }
        if !matched {
          exceptions.push((pos as u16, left));
        }
      }
    }
  }

  let hdr_len = raw_header_len(count);
  let meta_len = 5; // right_bw (1B) + left_bw (1B) + dict_size (1B) + exc_count (2B)
  let dict_bytes = actual_dict_size * 2;
  let left_bytes = if left_bw > 0 {
    packed_byte_size(count, left_bw)
  } else {
    0
  };
  let right_bytes = packed_byte_size(count, right_bw);
  let exc_bytes = exceptions.len() * 4;

  let total_size = hdr_len + meta_len + dict_bytes + left_bytes + right_bytes + exc_bytes;

  Some(RdCandidate {
    right_bw,
    left_bw,
    actual_dict_size: actual_dict_size as u8,
    dict,
    total_size,
  })
}

/// Writes Real Doubles (ALP-RD) encoded chunk into destination byte vector.
/// 将真实双精度高低位解耦（ALP-RD）数据块写入目标字节缓冲区
#[inline]
pub(crate) fn write_rd_chunk<F: AlpFloat>(
  count: usize,
  candidate: &RdCandidate,
  left_indices: &[u64],
  right_parts: &[u64],
  exceptions: &[(u16, u16)],
  dst: &mut Vec<u8>,
) {
  dst.reserve(candidate.total_size);
  write_header(F::TYPE_RD_BYTE, count, None, dst);
  dst.push(candidate.right_bw);
  dst.push(candidate.left_bw);
  dst.push(candidate.actual_dict_size);
  dst.extend_from_slice(&(exceptions.len() as u16).to_le_bytes());

  for &val in &candidate.dict[..candidate.actual_dict_size as usize] {
    dst.extend_from_slice(&val.to_le_bytes());
  }

  if candidate.left_bw > 0 {
    bitpack_u64(left_indices, candidate.left_bw, dst);
  }
  bitpack_u64(right_parts, candidate.right_bw, dst);

  let mut exc_buf = [0u8; 4];
  for &(pos, left_val) in exceptions {
    exc_buf[..2].copy_from_slice(&pos.to_le_bytes());
    exc_buf[2..].copy_from_slice(&left_val.to_le_bytes());
    dst.extend_from_slice(&exc_buf);
  }
}
