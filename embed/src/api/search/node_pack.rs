use std::{mem::align_of, slice};

use crate::{
  error::{Error, Result},
  key_composer::{decode_oppv_u64, encode_oppv_u64_fixed},
};

/// 8-bit scalar quantized vector structure compressing f64 to 1 byte per dimension.
/// 8-bit 标量量化向量结构（将 f64 压缩至 1 字节/维，在线线性映射无需训练）
#[derive(Debug, Clone, PartialEq)]
pub struct Sq8Vector {
  pub scale: f32,
  pub offset: f32,
  pub data: Vec<i8>,
}

impl Sq8Vector {
  /// Encodes data into binary format.
  /// 对 f64 向量进行在线标量量化编码并写入传入的缓冲区（零新堆内存分配）
  #[inline]
  pub fn encode_into(v: &[f64], out_data: &mut Vec<i8>) -> (f32, f32) {
    if v.is_empty() {
      out_data.clear();
      return (1.0, 0.0);
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &x in v {
      if x < min {
        min = x;
      }
      if x > max {
        max = x;
      }
    }
    let range = (max - min).max(1e-9);
    let scale = (range / 255.0) as f32;
    let offset = min as f32;
    let inv_scale = 255.0 / range;
    out_data.clear();
    out_data.reserve(v.len());
    out_data.extend(v.iter().map(|&x| {
      let normalized = (x - min) * inv_scale - 128.0;
      normalized.round().clamp(-128.0, 127.0) as i8
    }));
    (scale, offset)
  }

  /// Encodes data into binary format.
  /// 对 f64 向量进行在线标量量化编码
  #[inline]
  pub fn encode(v: &[f64]) -> Self {
    let mut data = Vec::with_capacity(v.len());
    let (scale, offset) = Self::encode_into(v, &mut data);
    Self {
      scale,
      offset,
      data,
    }
  }

  /// Dequantizes SQ8 vector back into f64 vector.
  /// 反量化还原为 f64 向量
  #[inline]
  pub fn decode(&self) -> Vec<f64> {
    let mut out = Vec::with_capacity(self.data.len());
    self.decode_into(&mut out);
    out
  }

  /// Dequantizes SQ8 vector into provided buffer with zero heap allocation.
  /// 将 SQ8 向量反量化填充至传入的复用缓冲区（零新堆内存分配）
  #[inline]
  pub fn decode_into(&self, out: &mut Vec<f64>) {
    out.clear();
    out.reserve(self.data.len());
    let scale = self.scale as f64;
    let offset = self.offset as f64;
    out.extend(self.data.iter().map(|&q| {
      let normalized = (q as f64) + 128.0;
      offset + normalized * scale
    }));
  }
}

/// Inline storage encoding tag for graph nodes.
/// 节点在 Fjall 中的内联存储格式标签
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodePackFormat {
  RawF64 = 0x00,
  Sq8 = 0x01,
}

/// Encodes data into binary format.
/// 节点在 Fjall / LSM-Tree 中的内联紧凑视图（Node-Centric Pack + SQ8 标量量化 + OP-PV 变长编码）
///
/// 物理内存布局（SQ8 格式）：
/// ```text
/// ┌──────────┬──────────────┬──────────────┬────────────────────────┬──────────────┬───────────────────────────────┐
/// Node byte layout: Flag(1B) | Scale(4B) | Offset(4B) | Quantized Vector(dim) | Degree(u16) | Delta IDs.
/// │ Flag(1B) │ Scale(f32 4B)│ Offset(f32 4B)│ Quantized Vector(dim*1B)│ Degree (u16) │ OP-PV Delta 压缩邻居 ID 字节流  │
/// └──────────┴──────────────┴──────────────┴────────────────────────┴──────────────┴───────────────────────────────┘
/// ```
#[derive(Debug, Clone)]
pub struct NodePackRef<'a> {
  pub format: NodePackFormat,
  pub raw_f64_vector: Option<&'a [f64]>,
  pub sq8_scale: f32,
  pub sq8_offset: f32,
  pub sq8_vector: Option<&'a [i8]>,
  pub raw_neighbors: &'a [u8],
  pub degree: usize,
}

impl<'a> NodePackRef<'a> {
  /// Deserializes node data with zero copy.
  /// 零拷贝反序列化节点数据
  #[inline]
  pub fn decode(payload: &'a [u8], dim: usize) -> Result<Self> {
    if payload.is_empty() {
      return Err(Error::invalid_data("empty node pack payload"));
    }

    // 1. SQ8 格式 (Flag 0x01)
    if payload[0] == NodePackFormat::Sq8 as u8 {
      let header_len = 1 + 8 + dim + 2;
      if payload.len() < header_len {
        return Err(Error::invalid_data(
          "invalid sq8 node pack payload: too short",
        ));
      }
      let scale = f32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
      let offset = f32::from_be_bytes([payload[5], payload[6], payload[7], payload[8]]);
      let vec_slice = &payload[9..9 + dim];
      // SAFETY: i8 与 u8 内存布局完全一致
      let sq8_vector: &'a [i8] =
        unsafe { slice::from_raw_parts(vec_slice.as_ptr().cast::<i8>(), dim) };

      let deg_bytes = &payload[9 + dim..11 + dim];
      let degree = u16::from_be_bytes([deg_bytes[0], deg_bytes[1]]) as usize;
      let raw_neighbors = &payload[11 + dim..];

      return Ok(Self {
        format: NodePackFormat::Sq8,
        raw_f64_vector: None,
        sq8_scale: scale,
        sq8_offset: offset,
        sq8_vector: Some(sq8_vector),
        raw_neighbors,
        degree,
      });
    }

    // 2. 带 Flag 0x00 的 RawF64 格式
    if payload[0] == NodePackFormat::RawF64 as u8 {
      let vec_bytes_len = dim * 8;
      if payload.len() < 1 + vec_bytes_len + 2 {
        return Err(Error::invalid_data(
          "invalid raw f64 node pack payload: too short",
        ));
      }
      let (vec_bytes, rest) = payload[1..].split_at(vec_bytes_len);
      if !(vec_bytes.as_ptr() as usize).is_multiple_of(align_of::<f64>()) {
        return Err(Error::invalid_data(
          "invalid node pack payload: unaligned f64 vector pointer",
        ));
      }
      let vector: &'a [f64] =
        unsafe { slice::from_raw_parts(vec_bytes.as_ptr().cast::<f64>(), dim) };
      let degree = u16::from_be_bytes([rest[0], rest[1]]) as usize;
      let raw_neighbors = &rest[2..];
      return Ok(Self {
        format: NodePackFormat::RawF64,
        raw_f64_vector: Some(vector),
        sq8_scale: 1.0,
        sq8_offset: 0.0,
        sq8_vector: None,
        raw_neighbors,
        degree,
      });
    }

    // 3. 兼容不带 Flag 的旧 RawF64 格式
    let vec_bytes_len = dim * 8;
    if payload.len() >= vec_bytes_len + 2 {
      let (vec_bytes, rest) = payload.split_at(vec_bytes_len);
      if (vec_bytes.as_ptr() as usize).is_multiple_of(align_of::<f64>()) {
        let vector: &'a [f64] =
          unsafe { slice::from_raw_parts(vec_bytes.as_ptr().cast::<f64>(), dim) };
        let degree = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        let raw_neighbors = &rest[2..];
        return Ok(Self {
          format: NodePackFormat::RawF64,
          raw_f64_vector: Some(vector),
          sq8_scale: 1.0,
          sq8_offset: 0.0,
          sq8_vector: None,
          raw_neighbors,
          degree,
        });
      }
    }

    Err(Error::invalid_data("unsupported node pack format"))
  }

  /// Retrieves dequantized f64 vector.
  /// 获取还原后的 f64 向量
  #[inline]
  pub fn to_f64_vec(&self) -> Vec<f64> {
    if let Some(v) = self.raw_f64_vector {
      v.to_vec()
    } else if let Some(q) = self.sq8_vector {
      let scale = self.sq8_scale as f64;
      let offset = self.sq8_offset as f64;
      q.iter()
        .map(|&val| {
          let normalized = (val as f64) + 128.0;
          offset + normalized * scale
        })
        .collect()
    } else {
      Vec::new()
    }
  }

  /// Iterates over all neighbor node IDs with zero heap allocation.
  /// 迭代获取所有邻居 Node ID（零堆分配，还原绝对 u64 ID）
  #[inline]
  pub fn iter_neighbors(&self) -> OppvDeltaNeighborIter<'a> {
    OppvDeltaNeighborIter {
      rem: self.raw_neighbors,
      remaining_count: self.degree,
      prev: 0,
      is_first: true,
    }
  }

  /// Collects all neighbor node IDs into Vec<u64>.
  /// 收集所有邻居为 Vec<u64>
  #[inline]
  pub fn to_neighbor_vec(&self) -> Vec<u64> {
    self.iter_neighbors().collect()
  }

  /// Decodes data from binary format.
  /// 将邻居直接解码填充至传入的复用 Vec 缓冲区（零新内存分配）
  #[inline]
  pub fn collect_neighbors_into(&self, out: &mut Vec<u64>) {
    out.clear();
    out.extend(self.iter_neighbors());
  }

  /// Encodes data into binary format.
  /// 编码 SQ8 标量量化节点数据到字节缓冲区（写入 Fjall LSM-Tree）
  #[inline]
  pub fn encode_sq8(scale: f32, offset: f32, vector: &[i8], neighbors: &[u64], out: &mut Vec<u8>) {
    let dim = vector.len();
    out.clear();
    out.reserve(1 + 8 + dim + 2 + neighbors.len() * 2);

    // 1. 写入 Flag 0x01 (SQ8)
    out.push(NodePackFormat::Sq8 as u8);

    // 2. 写入 Scale 与 Offset
    out.extend_from_slice(&scale.to_be_bytes());
    out.extend_from_slice(&offset.to_be_bytes());

    // 3. 写入 SQ8 向量数据
    // SAFETY: i8 与 u8 大小对齐一致
    let slice_u8 = unsafe { slice::from_raw_parts(vector.as_ptr().cast::<u8>(), dim) };
    out.extend_from_slice(slice_u8);

    // 4. 写入邻居度数
    let deg = neighbors.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&deg.to_be_bytes());

    // 5. 排序并写入 Delta 差分
    if deg == 0 {
      return;
    }
    let valid_neighbors = &neighbors[..deg as usize];
    let is_sorted = valid_neighbors.windows(2).all(|w| w[0] <= w[1]);

    let mut fixed = [0u8; 9];
    let mut prev = 0u64;
    let mut sorted_buf;
    let slice: &[u64] = if is_sorted {
      valid_neighbors
    } else {
      sorted_buf = valid_neighbors.to_vec();
      sorted_buf.sort_unstable();
      &sorted_buf
    };
    for (i, &n) in slice.iter().enumerate() {
      let delta = if i == 0 { n } else { n.saturating_sub(prev) };
      prev = n;
      let len = encode_oppv_u64_fixed(delta, &mut fixed);
      out.extend_from_slice(&fixed[..len]);
    }
  }

  /// Encodes data into binary format.
  /// 编码浮点原始向量（默认直接在线做 SQ8 标量量化压缩写入）
  #[inline]
  pub fn encode(vector: &[f64], neighbors: &[u64], out: &mut Vec<u8>) {
    let sq8 = Sq8Vector::encode(vector);
    Self::encode_sq8(sq8.scale, sq8.offset, &sq8.data, neighbors, out);
  }
}

/// OP-PV Delta neighbor ID iterator with zero heap allocation.
/// OP-PV Delta 邻居迭代器（零分配扫描）
#[derive(Debug, Clone)]
pub struct OppvDeltaNeighborIter<'a> {
  rem: &'a [u8],
  remaining_count: usize,
  prev: u64,
  is_first: bool,
}

impl<'a> Iterator for OppvDeltaNeighborIter<'a> {
  type Item = u64;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    if self.remaining_count == 0 || self.rem.is_empty() {
      return None;
    }
    let (delta, len) = match decode_oppv_u64(self.rem) {
      Some(res) => res,
      None => {
        self.remaining_count = 0;
        return None;
      }
    };
    self.rem = &self.rem[len..];
    let val = if self.is_first {
      self.is_first = false;
      delta
    } else {
      self.prev.saturating_add(delta)
    };
    self.prev = val;
    self.remaining_count -= 1;
    Some(val)
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.remaining_count, Some(self.remaining_count))
  }
}

impl<'a> ExactSizeIterator for OppvDeltaNeighborIter<'a> {}
