use crate::{
  error::{Error, Result},
  hll::{
    algo::{HLL_DENSE_SIZE, HLL_REGISTERS, hll_estimate_from_histo},
    dense::{hll_dense_get_register, hll_dense_set_register},
  },
};

/// Maximum allowed byte length for sparse representation (3000 bytes) aligned with Redis hll_sparse_max_bytes.
/// 稀疏表示最大允许字节数（3000 字节，对标 Redis hll_sparse_max_bytes）
pub const HLL_SPARSE_MAX_BYTES: usize = 3000;
/// Maximum register value supported by VAL opcode (32).
/// VAL 操作码支持的最大寄存器值（32）
pub const HLL_SPARSE_VAL_MAX_VALUE: u8 = 32;
/// Maximum run length supported by VAL opcode (4).
/// VAL 操作码支持的最大连乘长度（4）
pub const HLL_SPARSE_VAL_MAX_LEN: usize = 4;
/// Maximum run length supported by ZERO opcode (64).
/// ZERO 操作码支持的最大连乘长度（64）
pub const HLL_SPARSE_ZERO_MAX_LEN: usize = 64;
/// Maximum run length supported by XZERO opcode (16384).
/// XZERO 操作码支持的最大连乘长度（16384）
pub const HLL_SPARSE_XZERO_MAX_LEN: usize = 16384;

/// HyperLogLog sparse RLE opcode aligned with Redis/Valkey sparse representation.
/// HyperLogLog 稀疏 RLE 操作码（对标 Redis/Valkey Sparse Representation）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HllSparseOp {
  /// ZERO: 00xxxxxx (run length 1..=64, value 0).
  /// ZERO: 00xxxxxx (长度 1..=64，值 0)
  Zero { len: usize },
  /// XZERO: 01xxxxxx yyyyyyyy (run length 1..=16384, value 0).
  /// XZERO: 01xxxxxx yyyyyyyy (长度 1..=16384，值 0)
  XZero { len: usize },
  /// VAL: 1vvvvvxx (value 1..=32, run length 1..=4).
  /// VAL: 1vvvvvxx (值 1..=32，长度 1..=4)
  Val { val: u8, len: usize },
}

impl HllSparseOp {
  #[inline]
  pub const fn len(&self) -> usize {
    match self {
      Self::Zero { len } | Self::XZero { len } | Self::Val { len, .. } => *len,
    }
  }

  #[inline]
  pub const fn val(&self) -> u8 {
    match self {
      Self::Zero { .. } | Self::XZero { .. } => 0,
      Self::Val { val, .. } => *val,
    }
  }

  #[inline]
  pub const fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

/// Decodes data from binary format.
/// 解码单个稀疏操作码，返回操作码及消耗的字节数
#[inline]
pub const fn decode_sparse_op(bytes: &[u8]) -> Option<(HllSparseOp, usize)> {
  if bytes.is_empty() {
    return None;
  }
  let b0 = bytes[0];
  if b0 & 0x80 != 0 {
    let val = ((b0 >> 2) & 0x1F) + 1;
    let len = (b0 & 0x03) as usize + 1;
    Some((HllSparseOp::Val { val, len }, 1))
  } else if (b0 & 0xC0) == 0x40 {
    if bytes.len() < 2 {
      return None;
    }
    let b1 = bytes[1];
    let len = (((b0 & 0x3F) as usize) << 8 | (b1 as usize)) + 1;
    Some((HllSparseOp::XZero { len }, 2))
  } else if (b0 & 0xC0) == 0x00 {
    let len = (b0 & 0x3F) as usize + 1;
    Some((HllSparseOp::Zero { len }, 1))
  } else {
    None
  }
}

/// Encodes data into binary format.
/// 编码 0 寄存器游程
#[inline]
pub fn encode_sparse_zero(len: usize, out: &mut Vec<u8>) {
  let mut rem = len;
  while rem > 0 {
    if rem > HLL_SPARSE_ZERO_MAX_LEN {
      let chunk = rem.min(HLL_SPARSE_XZERO_MAX_LEN);
      let v = (chunk - 1) as u16;
      out.push(0x40 | ((v >> 8) as u8));
      out.push((v & 0xFF) as u8);
      rem -= chunk;
    } else {
      out.push((rem - 1) as u8);
      rem = 0;
    }
  }
}

/// Encodes data into binary format.
/// 编码非 0 寄存器游程（val: 1..=32）
#[inline]
pub fn encode_sparse_val(val: u8, len: usize, out: &mut Vec<u8>) {
  let mut rem = len;
  while rem > 0 {
    let chunk = rem.min(HLL_SPARSE_VAL_MAX_LEN);
    let byte = 0x80 | (((val - 1) & 0x1F) << 2) | ((chunk - 1) as u8 & 0x03);
    out.push(byte);
    rem -= chunk;
  }
}

/// Creates an initial all-zero sparse HLL byte buffer (2-byte XZERO(16384)).
/// 创建初始全 0 稀疏 HLL 字节切片（仅需 2 字节 XZERO(16384)）
#[inline]
pub fn hll_sparse_new() -> Vec<u8> {
  let mut v = Vec::with_capacity(2);
  encode_sparse_zero(HLL_REGISTERS, &mut v);
  v
}

/// Validates integrity and syntax of a sparse HLL byte buffer.
/// 校验稀疏 HLL 字节切片完整性与合法性
#[inline]
pub fn hll_sparse_is_valid(sparse: &[u8]) -> bool {
  let mut offset = 0;
  let mut total_regs = 0;
  while offset < sparse.len() {
    match decode_sparse_op(&sparse[offset..]) {
      Some((op, consumed)) => {
        total_regs += op.len();
        offset += consumed;
      }
      None => return false,
    }
  }
  offset == sparse.len() && total_regs == HLL_REGISTERS
}

/// Computes sparse HLL register histogram in a single pass without heap allocation.
/// 单次遍历计算稀疏 HLL 寄存器直方图（零堆分配，O(S) 极速扫描）
#[inline]
pub fn hll_sparse_reg_histo(sparse: &[u8], reghisto: &mut [usize; 64]) -> Result<()> {
  let mut offset = 0;
  let mut total_regs = 0;

  while offset < sparse.len() {
    let (op, consumed) = decode_sparse_op(&sparse[offset..])
      .ok_or_else(|| Error::invalid_data("invalid sparse hll opcode"))?;
    let len = op.len();
    let val = op.val() as usize;
    if val < 64 {
      reghisto[val] += len;
    }
    total_regs += len;
    offset += consumed;
  }

  if total_regs != HLL_REGISTERS {
    return Err(Error::invalid_data(format!(
      "invalid sparse hll register count: expected {HLL_REGISTERS}, got {total_regs}"
    )));
  }

  Ok(())
}

/// Encodes data into binary format.
/// 基于稀疏编码的高性能基数估算（零中间密集缓冲区分配）
#[inline]
pub fn hll_sparse_estimate(sparse: &[u8]) -> Result<u64> {
  let mut reghisto = [0usize; 64];
  hll_sparse_reg_histo(sparse, &mut reghisto)?;
  Ok(hll_estimate_from_histo(&reghisto))
}

/// Encodes data into binary format.
/// 将稀疏编码解压至密集 12288 字节缓冲区
#[inline]
pub fn hll_sparse_to_dense(sparse: &[u8], dense: &mut [u8]) -> Result<()> {
  if dense.len() < HLL_DENSE_SIZE {
    return Err(Error::invalid_data("dense buffer too small"));
  }
  dense[..HLL_DENSE_SIZE].fill(0);

  let mut offset = 0;
  let mut reg_idx = 0;

  while offset < sparse.len() {
    let (op, consumed) = decode_sparse_op(&sparse[offset..])
      .ok_or_else(|| Error::invalid_data("invalid sparse opcode"))?;
    let len = op.len();
    let val = op.val();
    if val > 0 {
      let end = (reg_idx + len).min(HLL_REGISTERS);
      for idx in reg_idx..end {
        hll_dense_set_register(dense, idx, val);
      }
    }
    reg_idx += len;
    offset += consumed;
  }

  if reg_idx != HLL_REGISTERS {
    return Err(Error::invalid_data(format!(
      "sparse total registers mismatch: expected {HLL_REGISTERS}, got {reg_idx}"
    )));
  }

  Ok(())
}

/// Encodes data into binary format.
/// 将密集编码转换为稀疏编码（零多余堆分配，若包含 > 32 值或超长则返回 None）
pub fn hll_dense_to_sparse(dense: &[u8]) -> Option<Vec<u8>> {
  let mut sparse = Vec::with_capacity(64);
  let mut i = 0;
  while i < HLL_REGISTERS {
    let v = hll_dense_get_register(dense, i);
    if v > HLL_SPARSE_VAL_MAX_VALUE {
      return None;
    }
    let mut len = 1;
    while i + len < HLL_REGISTERS && hll_dense_get_register(dense, i + len) == v {
      len += 1;
    }
    if v == 0 {
      encode_sparse_zero(len, &mut sparse);
    } else {
      encode_sparse_val(v, len, &mut sparse);
    }
    if sparse.len() > HLL_SPARSE_MAX_BYTES {
      return None;
    }
    i += len;
  }

  Some(sparse)
}

/// Encodes data into binary format.
/// 提取稀疏编码指定寄存器的值
#[inline]
pub fn hll_sparse_get_register(sparse: &[u8], index: usize) -> Result<u8> {
  if index >= HLL_REGISTERS {
    return Err(Error::invalid_data("register index out of range"));
  }
  let mut offset = 0;
  let mut cur_reg = 0;

  while offset < sparse.len() {
    let (op, consumed) = decode_sparse_op(&sparse[offset..])
      .ok_or_else(|| Error::invalid_data("invalid sparse opcode"))?;
    let len = op.len();
    if index < cur_reg + len {
      return Ok(op.val());
    }
    cur_reg += len;
    offset += consumed;
  }

  Err(Error::invalid_data("sparse register index not found"))
}

/// Encodes data into binary format.
/// 将稀疏编码就地合并到密集 12288 字节缓冲区（零堆分配极速合并）
#[inline]
pub fn hll_merge_sparse_into_dense(dest: &mut [u8], sparse: &[u8]) {
  if dest.len() < HLL_DENSE_SIZE || sparse.is_empty() {
    return;
  }
  let mut offset = 0;
  let mut reg_idx = 0;
  while offset < sparse.len() {
    if let Some((op, consumed)) = decode_sparse_op(&sparse[offset..]) {
      let val = op.val();
      let len = op.len();
      if val > 0 {
        let end = (reg_idx + len).min(HLL_REGISTERS);
        for idx in reg_idx..end {
          let cur = hll_dense_get_register(dest, idx);
          if val > cur {
            hll_dense_set_register(dest, idx, val);
          }
        }
      }
      reg_idx += len;
      offset += consumed;
    } else {
      break;
    }
  }
}

/// Encodes data into binary format.
/// 更新稀疏编码中的单个寄存器（单趟流式重编码状态机，零多余堆分配）
/// Encodes data into binary format.
/// 若值超限 (> 32) 或编码字节数超过上限 (3000 字节)，返回 Err 提示晋升为 Dense
pub fn hll_sparse_set_register(sparse: &mut Vec<u8>, index: usize, val: u8) -> Result<bool> {
  if index >= HLL_REGISTERS {
    return Err(Error::invalid_data("register index out of range"));
  }
  if val > HLL_SPARSE_VAL_MAX_VALUE {
    return Err(Error::invalid_data(
      "sparse value exceeds 32, promotion required",
    ));
  }

  #[inline]
  fn emit_run(val: u8, len: usize, out: &mut Vec<u8>) {
    if len == 0 {
      return;
    }
    if val == 0 {
      encode_sparse_zero(len, out);
    } else {
      encode_sparse_val(val, len, out);
    }
  }

  let mut offset = 0;
  let mut cur_reg = 0;

  // 单趟扫描定位目标操作码
  while offset < sparse.len() {
    let (op, consumed) = decode_sparse_op(&sparse[offset..])
      .ok_or_else(|| Error::invalid_data("invalid sparse opcode"))?;
    let op_len = op.len();
    let op_val = op.val();
    let reg_end = cur_reg + op_len;

    if index < reg_end {
      // 命中目标寄存器所在游程
      if val <= op_val {
        // 旧值已大于等于新值，无需修改，直接零分配返回
        return Ok(false);
      }

      // 新值大于旧值，启动单趟流式重编码
      let mut new_sparse = Vec::with_capacity(sparse.len() + 8);
      let mut last_val = 0u8;
      let mut last_len = 0usize;

      let mut push_run = |v: u8, l: usize, out: &mut Vec<u8>| {
        if l == 0 {
          return;
        }
        if last_len == 0 {
          last_val = v;
          last_len = l;
        } else if last_val == v {
          last_len += l;
        } else {
          emit_run(last_val, last_len, out);
          last_val = v;
          last_len = l;
        }
      };

      // 1. 重放 target 之前的所有已有操作码
      let mut pre_offset = 0;
      while pre_offset < offset {
        let (pre_op, pre_consumed) = decode_sparse_op(&sparse[pre_offset..])
          .ok_or_else(|| Error::invalid_data("invalid sparse opcode"))?;
        push_run(pre_op.val(), pre_op.len(), &mut new_sparse);
        pre_offset += pre_consumed;
      }

      // 2. 切分并更新当前操作码
      if index > cur_reg {
        push_run(op_val, index - cur_reg, &mut new_sparse);
      }
      push_run(val, 1, &mut new_sparse);
      if reg_end > index + 1 {
        push_run(op_val, reg_end - (index + 1), &mut new_sparse);
      }

      // 3. 继续流式消费后续操作码
      let mut post_offset = offset + consumed;
      let mut total_reg = reg_end;
      while post_offset < sparse.len() {
        let (post_op, post_consumed) = decode_sparse_op(&sparse[post_offset..])
          .ok_or_else(|| Error::invalid_data("invalid sparse opcode"))?;
        push_run(post_op.val(), post_op.len(), &mut new_sparse);
        total_reg += post_op.len();
        post_offset += post_consumed;
      }

      // 4. 刷新末尾悬挂游程
      emit_run(last_val, last_len, &mut new_sparse);

      if total_reg != HLL_REGISTERS {
        return Err(Error::invalid_data(format!(
          "sparse total registers mismatch: expected {HLL_REGISTERS}, got {total_reg}"
        )));
      }

      if new_sparse.len() > HLL_SPARSE_MAX_BYTES {
        return Err(Error::invalid_data(
          "sparse bytes exceeded limit, promotion required",
        ));
      }

      *sparse = new_sparse;
      return Ok(true);
    }

    cur_reg = reg_end;
    offset += consumed;
  }

  Err(Error::invalid_data("sparse register index not found"))
}
