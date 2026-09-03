use std::str;

use simsimd::SpatialSimilarity;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};

use crate::{
  error::{Error, Result},
  key_composer::{decode_oppv_u64, encode_oppv_u64, oppv_len_u64},
  meta::{decode_hex_u64, u64_to_hex_16},
  search::meta::{
    DistanceMetric, IndexFieldType, IndexOnDataType, VectorAlgorithm, VectorFieldMetadata,
    VectorType,
  },
};

/// Search subkey type enumeration aligned with Apache Kvrocks SearchSubkeyType.
/// 检索子键类型枚举（对标 Apache Kvrocks SearchSubkeyType）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SearchSubkeyType {
  IndexMeta = 0,
  Prefixes = 1,
  FieldMeta = 2,
  Field = 3,
  FieldAlias = 4,
}

impl SearchSubkeyType {
  #[inline]
  pub const fn from_u8(val: u8) -> Option<Self> {
    match val {
      0 => Some(Self::IndexMeta),
      1 => Some(Self::Prefixes),
      2 => Some(Self::FieldMeta),
      3 => Some(Self::Field),
      4 => Some(Self::FieldAlias),
      _ => None,
    }
  }
}

/// HNSW graph layer data type aligned with Apache Kvrocks HnswLevelType.
/// HNSW 图层级数据类型（对标 Apache Kvrocks HnswLevelType）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HnswLevelType {
  Node = 1,
  Edge = 2,
}

impl HnswLevelType {
  #[inline]
  pub const fn from_u8(val: u8) -> Option<Self> {
    match val {
      1 => Some(Self::Node),
      2 => Some(Self::Edge),
      _ => None,
    }
  }
}

/// Composes storage key or prefix.
/// 检索键构造器（对标 Apache Kvrocks SearchKey，全面采用 OPPV 与数值命名空间）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchKey<'a> {
  pub ns_id: u64,
  pub index: &'a str,
  pub field: Option<&'a str>,
}

impl<'a> SearchKey<'a> {
  #[inline]
  pub const fn new(ns_id: u64, index: &'a str) -> Self {
    Self {
      ns_id,
      index,
      field: None,
    }
  }

  #[inline]
  pub const fn with_field(ns_id: u64, index: &'a str, field: &'a str) -> Self {
    Self {
      ns_id,
      index,
      field: Some(field),
    }
  }

  #[inline]
  pub fn put_namespace(dst: &mut Vec<u8>, ns_id: u64) {
    dst.push(0);
    encode_oppv_u64(ns_id, dst);
  }

  #[inline]
  pub fn put_type(dst: &mut Vec<u8>, subkey_type: SearchSubkeyType) {
    dst.push(subkey_type as u8);
  }

  #[inline]
  pub fn put_sized_string(dst: &mut Vec<u8>, s: &str) {
    encode_oppv_u64(s.len() as u64, dst);
    dst.extend_from_slice(s.as_bytes());
  }

  #[inline]
  pub fn get_sized_string<'b>(input: &mut &'b [u8]) -> Option<&'b str> {
    let (len, consumed) = decode_oppv_u64(input)?;
    let len = len as usize;
    *input = &input[consumed..];
    if input.len() < len {
      return None;
    }
    let str_bytes = &input[..len];
    *input = &input[len..];
    str::from_utf8(str_bytes).ok()
  }

  #[inline]
  pub fn put_hnsw_level_prefix(
    dst: &mut Vec<u8>,
    ns_id: u64,
    index: &str,
    field: &str,
    level: u16,
  ) {
    Self::put_namespace(dst, ns_id);
    Self::put_type(dst, SearchSubkeyType::Field);
    Self::put_sized_string(dst, index);
    Self::put_sized_string(dst, field);
    dst.extend_from_slice(&level.to_be_bytes());
  }

  #[inline]
  pub fn put_hnsw_level_node_prefix(
    dst: &mut Vec<u8>,
    ns_id: u64,
    index: &str,
    field: &str,
    level: u16,
  ) {
    Self::put_hnsw_level_prefix(dst, ns_id, index, field, level);
    dst.push(HnswLevelType::Node as u8);
  }

  #[inline]
  pub fn put_hnsw_level_edge_prefix(
    dst: &mut Vec<u8>,
    ns_id: u64,
    index: &str,
    field: &str,
    level: u16,
  ) {
    Self::put_hnsw_level_prefix(dst, ns_id, index, field, level);
    dst.push(HnswLevelType::Edge as u8);
  }

  #[inline]
  pub fn construct_index_meta(&self) -> Vec<u8> {
    let mut dst = Vec::with_capacity(1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len());
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::IndexMeta);
    Self::put_sized_string(&mut dst, self.index);
    dst
  }

  #[inline]
  pub fn construct_index_prefixes(&self) -> Vec<u8> {
    let mut dst = Vec::with_capacity(1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len());
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::Prefixes);
    Self::put_sized_string(&mut dst, self.index);
    dst
  }

  #[inline]
  pub fn construct_field_meta(&self) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len() + 9 + field_name.len(),
    );
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::FieldMeta);
    Self::put_sized_string(&mut dst, self.index);
    Self::put_sized_string(&mut dst, field_name);
    dst
  }

  #[inline]
  pub fn construct_all_field_meta_begin(&self) -> Vec<u8> {
    let mut dst = Vec::with_capacity(1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len() + 1);
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::FieldMeta);
    Self::put_sized_string(&mut dst, self.index);
    dst.push(0);
    dst
  }

  #[inline]
  pub fn construct_all_field_meta_end(&self) -> Vec<u8> {
    let mut dst = Vec::with_capacity(1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len() + 1);
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::FieldMeta);
    Self::put_sized_string(&mut dst, self.index);
    dst.push(0xFF);
    dst
  }

  #[inline]
  pub fn construct_all_field_data_begin(&self) -> Vec<u8> {
    let mut dst = Vec::with_capacity(1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len() + 1);
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::Field);
    Self::put_sized_string(&mut dst, self.index);
    dst.push(0);
    dst
  }

  #[inline]
  pub fn construct_all_field_data_end(&self) -> Vec<u8> {
    let mut dst = Vec::with_capacity(1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len() + 1);
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::Field);
    Self::put_sized_string(&mut dst, self.index);
    dst.push(0xFF);
    dst
  }

  #[inline]
  pub fn construct_tag_field_data(&self, tag: &str, key: &str) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id)
        + 1
        + 9
        + self.index.len()
        + 9
        + field_name.len()
        + 9
        + tag.len()
        + 9
        + key.len(),
    );
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::Field);
    Self::put_sized_string(&mut dst, self.index);
    Self::put_sized_string(&mut dst, field_name);
    Self::put_sized_string(&mut dst, tag);
    Self::put_sized_string(&mut dst, key);
    dst
  }

  #[inline]
  pub fn construct_numeric_field_data(&self, num: f64, key: &str) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id)
        + 1
        + 9
        + self.index.len()
        + 9
        + field_name.len()
        + 8
        + 9
        + key.len(),
    );
    Self::put_namespace(&mut dst, self.ns_id);
    Self::put_type(&mut dst, SearchSubkeyType::Field);
    Self::put_sized_string(&mut dst, self.index);
    Self::put_sized_string(&mut dst, field_name);
    dst.extend_from_slice(&encode_sortable_f64_u64(num).to_be_bytes());
    Self::put_sized_string(&mut dst, key);
    dst
  }

  #[inline]
  pub fn construct_hnsw_level_node_prefix(&self, level: u16) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id) + 1 + 9 + self.index.len() + 9 + field_name.len() + 2 + 1,
    );
    Self::put_hnsw_level_node_prefix(&mut dst, self.ns_id, self.index, field_name, level);
    dst
  }

  #[inline]
  pub fn construct_hnsw_node(&self, level: u16, key: &str) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id)
        + 1
        + 9
        + self.index.len()
        + 9
        + field_name.len()
        + 2
        + 1
        + 9
        + key.len(),
    );
    Self::put_hnsw_level_node_prefix(&mut dst, self.ns_id, self.index, field_name, level);
    Self::put_sized_string(&mut dst, key);
    dst
  }

  #[inline]
  pub fn construct_hnsw_edge_with_single_end(&self, level: u16, key: &str) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id)
        + 1
        + 9
        + self.index.len()
        + 9
        + field_name.len()
        + 2
        + 1
        + 9
        + key.len(),
    );
    Self::put_hnsw_level_edge_prefix(&mut dst, self.ns_id, self.index, field_name, level);
    Self::put_sized_string(&mut dst, key);
    dst
  }

  #[inline]
  pub fn construct_hnsw_edge(&self, level: u16, key1: &str, key2: &str) -> Vec<u8> {
    let field_name = self.field.unwrap_or("");
    let mut dst = Vec::with_capacity(
      1 + oppv_len_u64(self.ns_id)
        + 1
        + 9
        + self.index.len()
        + 9
        + field_name.len()
        + 2
        + 1
        + 9
        + key1.len()
        + 9
        + key2.len(),
    );
    Self::put_hnsw_level_edge_prefix(&mut dst, self.ns_id, self.index, field_name, level);
    Self::put_sized_string(&mut dst, key1);
    Self::put_sized_string(&mut dst, key2);
    dst
  }
}

/// Encodes data into binary format.
/// 索引元数据二进制编码（对标 Apache Kvrocks IndexMetadata::Encode）
#[inline]
pub fn encode_index_meta(on_data_type: IndexOnDataType) -> Vec<u8> {
  vec![0u8, on_data_type as u8]
}

/// Decodes data from binary format.
/// 索引元数据二进制解码（对标 Apache Kvrocks IndexMetadata::Decode）
#[inline]
pub fn decode_index_meta(slice: &[u8]) -> Result<IndexOnDataType> {
  if slice.len() < 2 {
    return Err(Error::invalid_data(
      "insufficient length while decoding metadata",
    ));
  }
  match slice[1] {
    2 => Ok(IndexOnDataType::Hash),
    10 => Ok(IndexOnDataType::Json),
    other => Err(Error::invalid_data(format!(
      "unknown on_data_type: {other}"
    ))),
  }
}

/// Encodes data into binary format with prefix-free OPPV string length encoding.
/// 索引前缀列表二进制编码（OPPV 变长保序编码）
pub fn encode_index_prefixes(prefixes: &[&str]) -> Vec<u8> {
  let mut dst = Vec::new();
  for prefix in prefixes {
    encode_oppv_u64(prefix.len() as u64, &mut dst);
    dst.extend_from_slice(prefix.as_bytes());
  }
  dst
}

/// Decodes data from binary format with prefix-free OPPV string length decoding.
/// 索引前缀列表二进制解码（OPPV 变长保序解码）
pub fn decode_index_prefixes(mut slice: &[u8]) -> Result<Vec<String>> {
  let mut prefixes = Vec::new();
  while !slice.is_empty() {
    let (len, consumed) = decode_oppv_u64(slice)
      .ok_or_else(|| Error::invalid_data("insufficient length while decoding index prefixes"))?;
    let len = len as usize;
    slice = &slice[consumed..];
    if slice.len() < len {
      return Err(Error::invalid_data(
        "insufficient length while decoding index prefixes",
      ));
    }
    let prefix_str = str::from_utf8(&slice[..len])
      .map_err(|_| Error::invalid_data("invalid utf-8 string in index prefixes"))?;
    prefixes.push(prefix_str.to_string());
    slice = &slice[len..];
  }
  Ok(prefixes)
}

/// Encodes data into binary format.
/// 标签字段元数据二进制编码（对标 Apache Kvrocks TagFieldMetadata::Encode）
#[inline]
pub fn encode_tag_field_meta(separator: char, case_sensitive: bool, noindex: bool) -> Vec<u8> {
  let flag = (noindex as u8) | ((IndexFieldType::Tag as u8) << 1);
  vec![flag, separator as u8, case_sensitive as u8]
}

/// Decodes data from binary format.
/// 标签字段元数据二进制解码（对标 Apache Kvrocks TagFieldMetadata::Decode）
#[inline]
pub fn decode_tag_field_meta(slice: &[u8]) -> Result<(char, bool, bool)> {
  if slice.len() < 3 {
    return Err(Error::invalid_data(
      "insufficient length while decoding tag field metadata",
    ));
  }
  let flag = slice[0];
  let noindex = (flag & 1) != 0;
  let separator = slice[1] as char;
  let case_sensitive = slice[2] != 0;
  Ok((separator, case_sensitive, noindex))
}

/// Encodes data into binary format.
/// 数值字段元数据二进制编码（对标 Apache Kvrocks NumericFieldMetadata::Encode）
#[inline]
pub fn encode_numeric_field_meta(noindex: bool) -> Vec<u8> {
  let flag = (noindex as u8) | ((IndexFieldType::Numeric as u8) << 1);
  vec![flag]
}

/// Decodes data from binary format.
/// 数值字段元数据二进制解码（对标 Apache Kvrocks NumericFieldMetadata::Decode）
#[inline]
pub fn decode_numeric_field_meta(slice: &[u8]) -> Result<bool> {
  if slice.is_empty() {
    return Err(Error::invalid_data(
      "insufficient length while decoding numeric field metadata",
    ));
  }
  let flag = slice[0];
  let noindex = (flag & 1) != 0;
  Ok(noindex)
}

/// Encodes data into binary format.
/// HNSW 向量字段元数据编码字节长度 (1 flag + 1 type + 2 dim + 1 metric + 4 cap + 2 m + 4 ef_c + 4 ef_r + 8 epsilon + 2 num_levels)
pub use super::r#const::HNSW_VECTOR_FIELD_META_LEN;

/// Encodes data into binary format.
/// HNSW 向量字段元数据二进制编码（对标 Apache Kvrocks HnswVectorFieldMetadata::Encode）
pub fn encode_hnsw_vector_field_meta(meta: &VectorFieldMetadata, noindex: bool) -> Vec<u8> {
  let flag = (noindex as u8) | ((IndexFieldType::Vector as u8) << 1);
  let mut dst = Vec::with_capacity(HNSW_VECTOR_FIELD_META_LEN);
  dst.push(flag);
  dst.push(meta.vector_type as u8);
  dst.extend_from_slice(&(meta.dim as u16).to_be_bytes());
  dst.push(meta.distance_metric as u8);
  dst.extend_from_slice(&(meta.initial_cap as u32).to_be_bytes());
  dst.extend_from_slice(&(meta.m as u16).to_be_bytes());
  dst.extend_from_slice(&(meta.ef_construction as u32).to_be_bytes());
  dst.extend_from_slice(&(meta.ef_runtime as u32).to_be_bytes());
  dst.extend_from_slice(&encode_sortable_f64_u64(meta.epsilon).to_be_bytes());
  dst.extend_from_slice(&meta.num_levels.to_be_bytes());
  dst
}

/// Decodes data from binary format.
/// HNSW 向量字段元数据二进制解码（对标 Apache Kvrocks HnswVectorFieldMetadata::Decode）
pub fn decode_hnsw_vector_field_meta(slice: &[u8]) -> Result<(VectorFieldMetadata, bool)> {
  if slice.len() < HNSW_VECTOR_FIELD_META_LEN {
    return Err(Error::invalid_data(
      "insufficient length while decoding hnsw vector field metadata",
    ));
  }
  let flag = slice[0];
  let noindex = (flag & 1) != 0;

  let vector_type = match slice[1] {
    1 => VectorType::Float64,
    2 => VectorType::Float32,
    _ => VectorType::Float64,
  };
  let dim = u16::from_be_bytes([slice[2], slice[3]]) as usize;
  let distance_metric = match slice[4] {
    0 => DistanceMetric::L2,
    1 => DistanceMetric::IP,
    2 => DistanceMetric::Cosine,
    _ => DistanceMetric::Cosine,
  };
  let initial_cap = u32::from_be_bytes([slice[5], slice[6], slice[7], slice[8]]) as usize;
  let m = u16::from_be_bytes([slice[9], slice[10]]) as usize;
  let ef_construction = u32::from_be_bytes([slice[11], slice[12], slice[13], slice[14]]) as usize;
  let ef_runtime = u32::from_be_bytes([slice[15], slice[16], slice[17], slice[18]]) as usize;
  let epsilon_u64 = u64::from_be_bytes([
    slice[19], slice[20], slice[21], slice[22], slice[23], slice[24], slice[25], slice[26],
  ]);
  let epsilon = decode_sortable_f64_u64(epsilon_u64);
  let num_levels = u16::from_be_bytes([slice[27], slice[28]]);

  let meta = VectorFieldMetadata {
    vector_type,
    dim,
    distance_metric,
    algorithm: VectorAlgorithm::Hnsw,
    initial_cap,
    m,
    ef_construction,
    ef_runtime,
    epsilon,
    num_levels,
  };
  Ok((meta, noindex))
}

/// Encodes data into binary format.
/// HNSW 节点元数据二进制编码（对标 Apache Kvrocks HnswNodeFieldMetadata::Encode）
pub fn encode_hnsw_node_meta(num_neighbours: u16, vector: &[f64]) -> Vec<u8> {
  let mut dst = Vec::with_capacity(4 + vector.len() * 8);
  dst.extend_from_slice(&num_neighbours.to_be_bytes());
  dst.extend_from_slice(&(vector.len() as u16).to_be_bytes());
  for &element in vector {
    dst.extend_from_slice(&encode_sortable_f64_u64(element).to_be_bytes());
  }
  dst
}

/// Decodes data from binary format.
/// HNSW 节点元数据二进制解码（对标 Apache Kvrocks HnswNodeFieldMetadata::Decode）
pub fn decode_hnsw_node_meta(slice: &[u8]) -> Result<(u16, Vec<f64>)> {
  if slice.len() < 4 {
    return Err(Error::invalid_data(
      "insufficient length while decoding hnsw node metadata",
    ));
  }
  let num_neighbours = u16::from_be_bytes([slice[0], slice[1]]);
  let dim = u16::from_be_bytes([slice[2], slice[3]]) as usize;
  if slice.len() != 4 + dim * 8 {
    return Err(Error::invalid_data(
      "length is too short or too long to be parsed as a vector",
    ));
  }
  let mut vec = Vec::with_capacity(dim);
  for chunk in slice[4..].as_chunks::<8>().0 {
    let u = u64::from_be_bytes(*chunk);
    vec.push(decode_sortable_f64_u64(u));
  }
  Ok((num_neighbours, vec))
}

/// Encodes float as sortable 64-bit integer preserving lexicographical order for range scans.
/// 浮点数可排序 64 位整数转换（按位变换保证前缀扫描有序，IEEE 754 标准映射）
pub use crate::meta::{decode_sortable_f64_u64, encode_sortable_f64_u64};

/// Encodes data into binary format.
/// 浮点数可排序十六进制字符串编码
#[inline]
pub fn encode_sortable_f64(val: f64) -> String {
  let encoded = encode_sortable_f64_u64(val);
  let bytes = u64_to_hex_16(encoded);
  unsafe { str::from_utf8_unchecked(&bytes) }.to_string()
}

/// Decodes data from binary format.
/// 浮点数可排序十六进制字符串解码
#[inline]
pub fn decode_sortable_f64(hex_str: &str) -> Option<f64> {
  let sortable = decode_hex_u64(hex_str.as_bytes())?;
  Some(decode_sortable_f64_u64(sortable))
}

/// Encodes data into binary format.
/// 有符号 64 位整数可排序十六进制编码
#[inline]
pub fn encode_sortable_i64(val: i64) -> String {
  let unsigned = (val as u64) ^ (1 << 63);
  let bytes = u64_to_hex_16(unsigned);
  unsafe { str::from_utf8_unchecked(&bytes) }.to_string()
}

/// Decodes data from binary format.
/// 有符号 64 位整数可排序十六进制解码
#[inline]
pub fn decode_sortable_i64(hex_str: &str) -> Option<i64> {
  let unsigned = decode_hex_u64(hex_str.as_bytes())?;
  Some((unsigned ^ (1 << 63)) as i64)
}

/// Computes distance between SQ8 scalar quantized vectors using SIMD acceleration.
/// SQ8 标量量化向量距离计算（调用 simsimd AVX-512 / NEON 硬件级 Int8 SIMD 算子，4x~8x 吞吐加速）
#[inline]
pub fn compute_sq8_distance(q: &[i8], v: &[i8], metric: DistanceMetric) -> Result<f64> {
  if q.len() != v.len() {
    let len1 = q.len();
    let len2 = v.len();
    return Err(Error::invalid_data(format!(
      "sq8 vector dimension mismatch: {len1} vs {len2}"
    )));
  }
  if q.is_empty() {
    return Err(Error::invalid_data("empty vector is invalid"));
  }

  match metric {
    DistanceMetric::L2 => {
      if let Some(sq) = <i8 as SpatialSimilarity>::sqeuclidean(q, v) {
        Ok(sq.max(0.0).sqrt())
      } else {
        let mut sum = 0u64;
        for (&a, &b) in q.iter().zip(v.iter()) {
          let diff = (a as i32) - (b as i32);
          sum += (diff * diff) as u64;
        }
        Ok((sum as f64).sqrt())
      }
    }
    DistanceMetric::IP => {
      if let Some(dot) = <i8 as SpatialSimilarity>::dot(q, v) {
        Ok(-dot)
      } else {
        let mut dot = 0i64;
        for (&a, &b) in q.iter().zip(v.iter()) {
          dot += (a as i64) * (b as i64);
        }
        Ok(-(dot as f64))
      }
    }
    DistanceMetric::Cosine => {
      if let Some(cos) = <i8 as SpatialSimilarity>::cosine(q, v) {
        Ok(cos)
      } else {
        let mut dot = 0i64;
        let mut sum_sq1 = 0i64;
        let mut sum_sq2 = 0i64;
        for (&a, &b) in q.iter().zip(v.iter()) {
          let a = a as i64;
          let b = b as i64;
          dot += a * b;
          sum_sq1 += a * a;
          sum_sq2 += b * b;
        }
        let denom = ((sum_sq1 * sum_sq2) as f64).sqrt();
        if denom == 0.0 || !denom.is_finite() {
          return Ok(1.0);
        }
        let cos = ((dot as f64) / denom).clamp(-1.0, 1.0);
        Ok(1.0 - cos)
      }
    }
  }
}

/// Computes distance and similarity metrics with hardware acceleration and unrolled fallback.
/// 向量距离与相似度度量计算（优先调用 simsimd 硬件加速内核，带 8 路循环展开 fallback）
#[inline]
pub fn compute_vector_distance(v1: &[f64], v2: &[f64], metric: DistanceMetric) -> Result<f64> {
  if v1.len() != v2.len() {
    let len1 = v1.len();
    let len2 = v2.len();
    return Err(Error::invalid_data(format!(
      "vector dimension mismatch: {len1} vs {len2}"
    )));
  }
  if v1.is_empty() {
    return Err(Error::invalid_data("empty vector is invalid"));
  }

  match metric {
    DistanceMetric::L2 => {
      if let Some(sq) = <f64 as SpatialSimilarity>::sqeuclidean(v1, v2) {
        Ok(sq.max(0.0).sqrt())
      } else {
        let (c1, r1) = v1.as_chunks::<8>();
        let (c2, r2) = v2.as_chunks::<8>();
        let mut acc0 = 0.0;
        let mut acc1 = 0.0;
        let mut acc2 = 0.0;
        let mut acc3 = 0.0;
        let mut acc4 = 0.0;
        let mut acc5 = 0.0;
        let mut acc6 = 0.0;
        let mut acc7 = 0.0;
        for (a, b) in c1.iter().zip(c2.iter()) {
          let d0 = a[0] - b[0];
          let d1 = a[1] - b[1];
          let d2 = a[2] - b[2];
          let d3 = a[3] - b[3];
          let d4 = a[4] - b[4];
          let d5 = a[5] - b[5];
          let d6 = a[6] - b[6];
          let d7 = a[7] - b[7];
          acc0 += d0 * d0;
          acc1 += d1 * d1;
          acc2 += d2 * d2;
          acc3 += d3 * d3;
          acc4 += d4 * d4;
          acc5 += d5 * d5;
          acc6 += d6 * d6;
          acc7 += d7 * d7;
        }
        let mut sum = ((acc0 + acc1) + (acc2 + acc3)) + ((acc4 + acc5) + (acc6 + acc7));
        for (&a, &b) in r1.iter().zip(r2.iter()) {
          let diff = a - b;
          sum += diff * diff;
        }
        Ok(sum.sqrt())
      }
    }
    DistanceMetric::IP => {
      if let Some(dot) = <f64 as SpatialSimilarity>::dot(v1, v2) {
        Ok(-dot)
      } else {
        let (c1, r1) = v1.as_chunks::<8>();
        let (c2, r2) = v2.as_chunks::<8>();
        let mut dot0 = 0.0;
        let mut dot1 = 0.0;
        let mut dot2 = 0.0;
        let mut dot3 = 0.0;
        let mut dot4 = 0.0;
        let mut dot5 = 0.0;
        let mut dot6 = 0.0;
        let mut dot7 = 0.0;
        for (a, b) in c1.iter().zip(c2.iter()) {
          dot0 += a[0] * b[0];
          dot1 += a[1] * b[1];
          dot2 += a[2] * b[2];
          dot3 += a[3] * b[3];
          dot4 += a[4] * b[4];
          dot5 += a[5] * b[5];
          dot6 += a[6] * b[6];
          dot7 += a[7] * b[7];
        }
        let mut dot = ((dot0 + dot1) + (dot2 + dot3)) + ((dot4 + dot5) + (dot6 + dot7));
        for (&a, &b) in r1.iter().zip(r2.iter()) {
          dot += a * b;
        }
        Ok(-dot)
      }
    }
    DistanceMetric::Cosine => {
      if let Some(cos) = <f64 as SpatialSimilarity>::cosine(v1, v2) {
        Ok(cos)
      } else {
        let (c1, r1) = v1.as_chunks::<8>();
        let (c2, r2) = v2.as_chunks::<8>();
        let mut dot0 = 0.0;
        let mut dot1 = 0.0;
        let mut dot2 = 0.0;
        let mut dot3 = 0.0;
        let mut n1_0 = 0.0;
        let mut n1_1 = 0.0;
        let mut n1_2 = 0.0;
        let mut n1_3 = 0.0;
        let mut n2_0 = 0.0;
        let mut n2_1 = 0.0;
        let mut n2_2 = 0.0;
        let mut n2_3 = 0.0;
        for (a, b) in c1.iter().zip(c2.iter()) {
          dot0 += a[0] * b[0] + a[1] * b[1];
          dot1 += a[2] * b[2] + a[3] * b[3];
          dot2 += a[4] * b[4] + a[5] * b[5];
          dot3 += a[6] * b[6] + a[7] * b[7];
          n1_0 += a[0] * a[0] + a[1] * a[1];
          n1_1 += a[2] * a[2] + a[3] * a[3];
          n1_2 += a[4] * a[4] + a[5] * a[5];
          n1_3 += a[6] * a[6] + a[7] * a[7];
          n2_0 += b[0] * b[0] + b[1] * b[1];
          n2_1 += b[2] * b[2] + b[3] * b[3];
          n2_2 += b[4] * b[4] + b[5] * b[5];
          n2_3 += b[6] * b[6] + b[7] * b[7];
        }
        let mut dot = (dot0 + dot1) + (dot2 + dot3);
        let mut sum_sq1 = (n1_0 + n1_1) + (n1_2 + n1_3);
        let mut sum_sq2 = (n2_0 + n2_1) + (n2_2 + n2_3);
        for (&a, &b) in r1.iter().zip(r2.iter()) {
          dot += a * b;
          sum_sq1 += a * a;
          sum_sq2 += b * b;
        }
        let denom = (sum_sq1 * sum_sq2).sqrt();
        if denom == 0.0 || !denom.is_finite() {
          return Ok(1.0);
        }
        let cos = (dot / denom).clamp(-1.0, 1.0);
        Ok(1.0 - cos)
      }
    }
  }
}

/// Parses float vector from raw binary byte slice or JSON array.
/// 从二进制字节数组或 JSON 数组中解析浮点向量（零冗余单次解析）
pub fn parse_vector_from_slice(bytes: &[u8], vector_type: VectorType) -> Result<Vec<f64>> {
  let mut vec = Vec::new();
  parse_vector_from_slice_into(bytes, vector_type, &mut vec)?;
  Ok(vec)
}

/// Parses float vector into reusable buffer without heap allocations.
/// 从二进制字节数组或 JSON 数组中解析浮点向量至复用缓冲区（零新堆内存分配）
pub fn parse_vector_from_slice_into(
  bytes: &[u8],
  vector_type: VectorType,
  out: &mut Vec<f64>,
) -> Result<()> {
  if bytes.is_empty() {
    return Err(Error::invalid_data("empty vector byte format"));
  }

  // 优先检测 JSON 格式（以 '[' 开头）
  if bytes.starts_with(b"[")
    && let Ok(json_v) = sonic_rs::from_slice::<sonic_rs::Value>(bytes)
    && let Some(arr) = json_v.as_array()
  {
    out.clear();
    out.reserve(arr.len());
    for item in arr {
      if let Some(n) = item.as_f64() {
        out.push(n);
      }
    }
    if !out.is_empty() {
      return Ok(());
    }
  }

  // 二进制字节数组解析 (Float64: 8 字节/元素, Float32: 4 字节/元素)
  let elem_size = vector_type.byte_size();
  if bytes.len().is_multiple_of(elem_size) {
    match vector_type {
      VectorType::Float64 => {
        let count = bytes.len() / 8;
        out.clear();
        out.reserve(count);
        for chunk in bytes.as_chunks::<8>().0 {
          out.push(f64::from_le_bytes(*chunk));
        }
        return Ok(());
      }
      VectorType::Float32 => {
        let count = bytes.len() / 4;
        out.clear();
        out.reserve(count);
        for chunk in bytes.as_chunks::<4>().0 {
          out.push(f32::from_le_bytes(*chunk) as f64);
        }
        return Ok(());
      }
    }
  }

  // 纯文本逗号分隔（含 ',' 且为有效 UTF-8 文本）
  if let Ok(s) = str::from_utf8(bytes)
    && s.contains(',')
  {
    let clean = s.trim().trim_start_matches('[').trim_end_matches(']');
    out.clear();
    let mut valid = true;
    for part in clean.split(',') {
      let p = part.trim();
      if p.is_empty() {
        continue;
      }
      if let Ok(num) = p.parse::<f64>() {
        out.push(num);
      } else {
        valid = false;
        break;
      }
    }
    if valid && !out.is_empty() {
      return Ok(());
    }
  }

  Err(Error::invalid_data("invalid vector byte format or length"))
}
