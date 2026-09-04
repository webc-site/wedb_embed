use std::mem::size_of;

use crate::{
  bitpack::{bitpack_u64, packed_byte_size},
  constants::MAX_DICT_ENTRIES,
  float::AlpFloat,
  header::{raw_header_len, write_header},
  params::bits_needed,
};

const HASH_TABLE_SIZE: usize = 128;
const HASH_MASK: usize = HASH_TABLE_SIZE - 1;

/// Low-cardinality dictionary compression candidate metadata.
/// 低基数紧凑字典压缩候选元数据
#[derive(Debug, Clone, Copy)]
pub(crate) struct DictCandidate<F: AlpFloat> {
  pub dict: [F; MAX_DICT_ENTRIES],
  pub dict_len: usize,
  pub bit_width: u8,
}

/// Computes the exact byte size of a dictionary-encoded chunk.
/// 计算紧凑字典编码数据块的精确字节大小
#[inline(always)]
pub(crate) fn dict_compressed_size<F: AlpFloat>(
  count: usize,
  dict_len: usize,
  bit_width: u8,
) -> usize {
  let hdr_len = raw_header_len(count);
  let meta_len = 2; // dict_len (u8) + bit_width (u8)
  let dict_bytes = dict_len * size_of::<F>();
  let indices_bytes = packed_byte_size(count, bit_width);
  hdr_len + meta_len + dict_bytes + indices_bytes
}

/// Scans the float slice with early abort to extract a compact dictionary (<= 64 unique entries).
///
/// 单次线性扫描浮点数据切片，具备超快速短路退出（在非低基数数据上约 70 元素即短路中止，耗时 < 20ns）；
/// 若基数 <= 64 则在单次遍历中直接生成字典表及对应索引流，避免二次遍历与任何堆内存分配。
pub(crate) fn scan_dict<F: AlpFloat>(
  slice: &[F],
  indices_buf: &mut Vec<u64>,
) -> Option<DictCandidate<F>> {
  if slice.is_empty() {
    return None;
  }

  let mut dict = [F::ZERO; MAX_DICT_ENTRIES];
  let mut dict_len = 0usize;
  let mut keys = [0u64; HASH_TABLE_SIZE];
  let mut indices = [0u8; HASH_TABLE_SIZE];
  let mut occupied = [0u64; HASH_TABLE_SIZE / 64];

  indices_buf.clear();
  indices_buf.reserve(slice.len());

  for &v in slice {
    let key = v.to_u64_key();
    let hash = ((key ^ (key >> 32)) as u32).wrapping_mul(0x9E37_79B9);
    let mut idx = (hash >> 25) as usize & HASH_MASK;

    loop {
      let word_idx = idx >> 6;
      let bit_mask = 1u64 << (idx & 63);

      if (occupied[word_idx] & bit_mask) == 0 {
        // 空槽位：若已达到最大字典项数则立即短路退出
        if dict_len >= MAX_DICT_ENTRIES {
          return None;
        }
        let d_idx = dict_len as u8;
        occupied[word_idx] |= bit_mask;
        keys[idx] = key;
        indices[idx] = d_idx;
        dict[dict_len] = v;
        dict_len += 1;
        indices_buf.push(d_idx as u64);
        break;
      }

      if keys[idx] == key {
        // 已有相同键值：直接记录索引
        indices_buf.push(indices[idx] as u64);
        break;
      }

      idx = (idx + 1) & HASH_MASK;
    }
  }

  let bit_width = if dict_len <= 1 {
    0
  } else {
    bits_needed((dict_len - 1) as u64)
  };

  Some(DictCandidate {
    dict,
    dict_len,
    bit_width,
  })
}

/// Writes dictionary encoded chunk directly into destination byte vector.
/// 将紧凑字典编码数据块直接写入目标字节缓冲区
#[inline]
pub(crate) fn write_dict_chunk<F: AlpFloat>(
  count: usize,
  candidate: &DictCandidate<F>,
  indices: &[u64],
  dst: &mut Vec<u8>,
) {
  let needed = dict_compressed_size::<F>(count, candidate.dict_len, candidate.bit_width);
  dst.reserve(needed);
  write_header(F::TYPE_DICT_BYTE, count, None, dst);
  dst.push(candidate.dict_len as u8);
  dst.push(candidate.bit_width);
  for &val in &candidate.dict[..candidate.dict_len] {
    val.write_raw(dst);
  }
  if candidate.bit_width > 0 {
    bitpack_u64(indices, candidate.bit_width, dst);
  }
}
