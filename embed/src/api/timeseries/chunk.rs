use std::ptr::read_unaligned;

use super::{
  gorilla::{TSSample, compress_timestamps, decompress_last_timestamp, decompress_timestamps_into},
  meta::{ChunkType, DuplicatePolicy},
};
use crate::error::{Error, Result};

/// TSChunk header metadata (8 bytes: 4-byte flag + 4-byte count).
/// TSChunk 头部元数据（8 字节：4 字节 is_compressed + 4 字节 count）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChunkHeader {
  pub is_compressed: bool,
  pub count: u32,
}

impl ChunkHeader {
  pub const ENCODED_SIZE: usize = 8;

  #[inline]
  pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    let flag = if self.is_compressed { 1u32 } else { 0u32 };
    buf[0..4].copy_from_slice(&flag.to_be_bytes());
    buf[4..8].copy_from_slice(&self.count.to_be_bytes());
    buf
  }

  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    let chunk = bytes.first_chunk::<8>()?;
    let flag = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    let count = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
    Some(Self {
      is_compressed: flag != 0,
      count,
    })
  }
}

/// Operation definition.
/// 样本合并统计结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeStats {
  pub inserted: usize,
  pub updated: usize,
  pub skipped: usize,
}

/// Operation definition.
/// TSChunk 操作封装（支持 Uncompressed 原始块与 FastALP 列式压缩块）
pub struct TSChunk;

impl TSChunk {
  /// Encodes data into binary format.
  /// 编码为未压缩块（Header + [TSSample (16B)] * count）
  #[inline]
  pub fn encode_uncompressed(samples: &[TSSample]) -> Vec<u8> {
    let header = ChunkHeader {
      is_compressed: false,
      count: samples.len() as u32,
    };
    let mut buf = Vec::with_capacity(ChunkHeader::ENCODED_SIZE + samples.len() * 16);
    buf.extend_from_slice(&header.encode());
    for s in samples {
      buf.extend_from_slice(&s.ts.to_be_bytes());
      buf.extend_from_slice(&s.v.to_be_bytes());
    }
    buf
  }

  /// Encodes data into binary format.
  /// 编码为 FastALP 列式压缩块（Header + [ts_len: u32] + [compressed_ts] + [compressed_values_fastalp]）
  #[inline]
  pub fn encode_compressed(samples: &[TSSample]) -> Vec<u8> {
    if samples.is_empty() {
      return Self::encode_uncompressed(samples);
    }
    let count = samples.len() as u32;
    let mut timestamps = Vec::with_capacity(samples.len());
    let mut values = Vec::with_capacity(samples.len());
    for s in samples {
      timestamps.push(s.ts);
      values.push(s.v);
    }

    let ts_payload = compress_timestamps(&timestamps);
    let header = ChunkHeader {
      is_compressed: true,
      count,
    };
    let mut buf =
      Vec::with_capacity(ChunkHeader::ENCODED_SIZE + 4 + ts_payload.len() + values.len() * 2 + 16);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&(ts_payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&ts_payload);
    fastalp::compress_into(&values, &mut buf);
    buf
  }

  /// Encodes data into binary format.
  /// 根据 ChunkType 自动编码
  #[inline]
  pub fn encode_with_type(samples: &[TSSample], chunk_type: ChunkType) -> Vec<u8> {
    match chunk_type {
      ChunkType::Compressed => Self::encode_compressed(samples),
      ChunkType::Uncompressed => Self::encode_uncompressed(samples),
    }
  }

  /// Decodes data from binary format.
  /// 解码 Chunk 字节流并写入指定样本点向量（复用堆分配）
  pub fn decode_samples_into(chunk_data: &[u8], samples: &mut Vec<TSSample>) -> Result<()> {
    if chunk_data.is_empty() {
      return Ok(());
    }
    if chunk_data.len() < ChunkHeader::ENCODED_SIZE {
      if chunk_data.len() == 8 {
        let mut b = [0u8; 8];
        b.copy_from_slice(chunk_data);
        samples.push(TSSample::new(0, f64::from_be_bytes(b)));
        return Ok(());
      }
      return Err(Error::invalid_data(
        "ERR TSDB: TSChunk payload data too short",
      ));
    }

    let header = ChunkHeader::decode(chunk_data)
      .ok_or_else(|| Error::invalid_data("ERR TSDB: invalid TSChunk header"))?;

    if header.count == 0 {
      return Ok(());
    }

    let payload = &chunk_data[ChunkHeader::ENCODED_SIZE..];

    if header.is_compressed {
      if payload.len() < 4 {
        return Err(Error::invalid_data(
          "ERR TSDB: corrupted compressed chunk payload",
        ));
      }
      let ts_len = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
      if payload.len() < 4 + ts_len {
        return Err(Error::invalid_data(
          "ERR TSDB: corrupted compressed chunk ts stream",
        ));
      }
      let ts_payload = &payload[4..4 + ts_len];
      let val_payload = &payload[4 + ts_len..];

      let count = header.count as usize;
      let mut timestamps = Vec::with_capacity(count);
      decompress_timestamps_into(ts_payload, count, &mut timestamps)?;

      let mut values = Vec::with_capacity(count);
      fastalp::decompress_into(val_payload, &mut values)
        .map_err(|e| Error::invalid_data(format!("ERR TSDB: ALP decode error: {e}")))?;

      if timestamps.len() != count || values.len() != count {
        return Err(Error::invalid_data(
          "ERR TSDB: compressed chunk length mismatch",
        ));
      }

      samples.reserve(count);
      for (&ts, &v) in timestamps.iter().zip(values.iter()) {
        samples.push(TSSample::new(ts, v));
      }
      Ok(())
    } else {
      let count = header.count as usize;
      let needed = count * 16;
      if payload.len() < needed {
        return Err(Error::invalid_data(
          "ERR TSDB: uncompressed TSChunk payload too short",
        ));
      }
      samples.reserve(count);
      for chunk in payload[..needed].as_chunks::<16>().0 {
        let ts = u64::from_be_bytes([
          chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        let v = f64::from_be_bytes([
          chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14], chunk[15],
        ]);
        samples.push(TSSample::new(ts, v));
      }
      Ok(())
    }
  }

  /// Decodes data from binary format.
  /// 解码 Chunk 字节流为采样点向量
  pub fn decode_samples(chunk_data: &[u8]) -> Result<Vec<TSSample>> {
    let mut samples = Vec::new();
    Self::decode_samples_into(chunk_data, &mut samples)?;
    Ok(samples)
  }

  /// Domain operation (aligned with Kvrocks GetFirstTimestamp).
  /// 提取 Chunk 首个时间戳（零全量解压轻量级提取，对标 Kvrocks GetFirstTimestamp）
  #[inline]
  pub fn get_first_timestamp(chunk_data: &[u8]) -> Option<u64> {
    if chunk_data.len() < ChunkHeader::ENCODED_SIZE {
      return None;
    }
    let header = ChunkHeader::decode(chunk_data)?;
    if header.count == 0 {
      return None;
    }
    let payload = &chunk_data[ChunkHeader::ENCODED_SIZE..];
    if header.is_compressed {
      if payload.len() < 4 + 8 {
        return None;
      }
      let b8: [u8; 8] = payload[4..12].try_into().ok()?;
      Some(u64::from_be_bytes(b8))
    } else {
      if payload.len() < 8 {
        return None;
      }
      let b8: [u8; 8] = payload[..8].try_into().ok()?;
      Some(u64::from_be_bytes(b8))
    }
  }

  /// Domain operation (aligned with Kvrocks GetLastTimestamp).
  /// 提取 Chunk 末尾时间戳（未压缩与压缩均实现 O(1) 空间零堆分配提取，对标 Kvrocks GetLastTimestamp）
  #[inline]
  pub fn get_last_timestamp(chunk_data: &[u8]) -> Option<u64> {
    if chunk_data.len() < ChunkHeader::ENCODED_SIZE {
      return None;
    }
    let header = ChunkHeader::decode(chunk_data)?;
    if header.count == 0 {
      return None;
    }
    let payload = &chunk_data[ChunkHeader::ENCODED_SIZE..];
    if header.is_compressed {
      if payload.len() < 4 {
        return None;
      }
      let ts_len = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
      if payload.len() < 4 + ts_len {
        return None;
      }
      let ts_payload = &payload[4..4 + ts_len];
      decompress_last_timestamp(ts_payload, header.count as usize)
    } else {
      let offset = (header.count as usize - 1) * 16;
      if payload.len() >= offset + 8 {
        let b8: [u8; 8] = payload[offset..offset + 8].try_into().ok()?;
        return Some(u64::from_be_bytes(b8));
      }
      None
    }
  }

  /// Domain operation (aligned with Kvrocks GetLatestSample).
  /// 提取末尾采样点 (ts, val)（未压缩模式零拷贝/零堆分配 O(1) 提取，对标 Kvrocks GetLatestSample）
  #[inline]
  pub fn get_latest_sample(chunk_data: &[u8]) -> Result<Option<(u64, f64)>> {
    if chunk_data.len() < ChunkHeader::ENCODED_SIZE {
      return Ok(None);
    }
    let header = match ChunkHeader::decode(chunk_data) {
      Some(h) => h,
      None => return Ok(None),
    };
    if header.count == 0 {
      return Ok(None);
    }
    let payload = &chunk_data[ChunkHeader::ENCODED_SIZE..];
    if !header.is_compressed {
      let offset = (header.count as usize - 1) * 16;
      if let Some(chunk) = payload.get(offset..offset + 16) {
        // SAFETY: payload.get(offset..offset + 16) 保证 chunk 具有 16 字节有效可读内存，read_unaligned 避免对齐与越界开销。
        let (ts, v) = unsafe {
          let ts_bytes = read_unaligned(chunk.as_ptr().cast::<[u8; 8]>());
          let v_bytes = read_unaligned(chunk.as_ptr().add(8).cast::<[u8; 8]>());
          (u64::from_be_bytes(ts_bytes), f64::from_be_bytes(v_bytes))
        };
        return Ok(Some((ts, v)));
      }
      Ok(None)
    } else {
      let samples = Self::decode_samples(chunk_data)?;
      Ok(samples.last().map(|s| (s.ts, s.v)))
    }
  }
}

#[inline]
fn apply_duplicate_policy(
  old_v: f64,
  new_v: f64,
  policy: DuplicatePolicy,
  stats: &mut MergeStats,
) -> Result<f64> {
  match policy {
    DuplicatePolicy::Block => Err(Error::invalid_data(
      "ERR TSDB: Error at upsert, update is not supported when DUPLICATE_POLICY is set to BLOCK mode",
    )),
    DuplicatePolicy::First => {
      stats.skipped += 1;
      Ok(old_v)
    }
    DuplicatePolicy::Last => {
      if (old_v - new_v).abs() < f64::EPSILON {
        stats.skipped += 1;
      } else {
        stats.updated += 1;
      }
      Ok(new_v)
    }
    DuplicatePolicy::Min => {
      if new_v < old_v {
        stats.updated += 1;
        Ok(new_v)
      } else {
        stats.skipped += 1;
        Ok(old_v)
      }
    }
    DuplicatePolicy::Max => {
      if new_v > old_v {
        stats.updated += 1;
        Ok(new_v)
      } else {
        stats.skipped += 1;
        Ok(old_v)
      }
    }
    DuplicatePolicy::Sum => {
      if new_v == 0.0 {
        stats.skipped += 1;
      } else {
        stats.updated += 1;
      }
      Ok(old_v + new_v)
    }
  }
}

impl TSChunk {
  /// Operation definition.
  /// 提取 Chunk 采样点总数
  #[inline]
  pub fn get_count(chunk_data: &[u8]) -> u32 {
    if chunk_data.len() < ChunkHeader::ENCODED_SIZE {
      return 0;
    }
    ChunkHeader::decode(chunk_data)
      .map(|h| h.count)
      .unwrap_or(0)
  }

  /// Operation definition.
  /// 合并新样本点并应用 DuplicatePolicy（若 Block 策略冲突则返回错误）
  pub fn merge_samples(
    existing: &mut Vec<TSSample>,
    new_samples: &[TSSample],
    policy: DuplicatePolicy,
  ) -> Result<MergeStats> {
    let mut stats = MergeStats::default();
    if new_samples.is_empty() {
      return Ok(stats);
    }

    // 单样本优化快路径
    if new_samples.len() == 1 {
      let new_s = new_samples[0];
      match existing.binary_search_by_key(&new_s.ts, |s| s.ts) {
        Ok(idx) => {
          let final_v = apply_duplicate_policy(existing[idx].v, new_s.v, policy, &mut stats)?;
          existing[idx].v = final_v;
        }
        Err(idx) => {
          existing.insert(idx, new_s);
          stats.inserted += 1;
        }
      }
      return Ok(stats);
    }

    // 多样本双指针归并（先对新样本排序并应用自身内部重复策略）
    let mut sorted_new = new_samples.to_vec();
    sorted_new.sort_by_key(|s| s.ts);

    let mut deduped_new: Vec<TSSample> = Vec::with_capacity(sorted_new.len());
    for s in sorted_new {
      if let Some(last) = deduped_new.last_mut()
        && last.ts == s.ts
      {
        let final_v = apply_duplicate_policy(last.v, s.v, policy, &mut stats)?;
        last.v = final_v;
      } else {
        deduped_new.push(s);
      }
    }

    let mut merged = Vec::with_capacity(existing.len() + deduped_new.len());
    let mut i = 0;
    let mut j = 0;

    while i < existing.len() && j < deduped_new.len() {
      let e = existing[i];
      let n = deduped_new[j];
      if e.ts < n.ts {
        merged.push(e);
        i += 1;
      } else if e.ts > n.ts {
        merged.push(n);
        stats.inserted += 1;
        j += 1;
      } else {
        let final_v = apply_duplicate_policy(e.v, n.v, policy, &mut stats)?;
        merged.push(TSSample::new(e.ts, final_v));
        i += 1;
        j += 1;
      }
    }

    while i < existing.len() {
      merged.push(existing[i]);
      i += 1;
    }

    while j < deduped_new.len() {
      merged.push(deduped_new[j]);
      stats.inserted += 1;
      j += 1;
    }

    *existing = merged;
    Ok(stats)
  }

  /// Domain operation (aligned with Apache Kvrocks UpsertSampleAndSplit).
  /// Upsert 并按 chunk_size 拆分（对标 Apache Kvrocks UpsertSampleAndSplit）
  pub fn upsert_and_split(
    existing_data: &[u8],
    new_samples: &[TSSample],
    policy: DuplicatePolicy,
    preferred_chunk_size: usize,
    chunk_type: ChunkType,
  ) -> Result<Vec<Vec<u8>>> {
    let mut samples = if existing_data.is_empty() {
      Vec::new()
    } else {
      Self::decode_samples(existing_data)?
    };

    Self::merge_samples(&mut samples, new_samples, policy)?;

    if samples.is_empty() {
      return Ok(Vec::new());
    }

    let chunk_size = preferred_chunk_size.max(1);
    let chunks = samples
      .chunks(chunk_size)
      .map(|chunk_slice| Self::encode_with_type(chunk_slice, chunk_type))
      .collect();

    Ok(chunks)
  }

  /// Operation definition.
  /// 删除指定时间范围内的样本点 [from_ts, to_ts]
  pub fn remove_samples_between(
    chunk_data: &[u8],
    from_ts: u64,
    to_ts: u64,
    chunk_type: ChunkType,
  ) -> Result<(Vec<u8>, usize)> {
    if from_ts > to_ts || chunk_data.is_empty() {
      return Ok((chunk_data.to_vec(), 0));
    }
    let mut samples = Self::decode_samples(chunk_data)?;
    let orig_len = samples.len();
    samples.retain(|s| s.ts < from_ts || s.ts > to_ts);
    let deleted = orig_len - samples.len();
    if deleted == 0 {
      return Ok((chunk_data.to_vec(), 0));
    }
    let encoded = Self::encode_with_type(&samples, chunk_type);
    Ok((encoded, deleted))
  }

  /// Operation definition.
  /// 更新指定时间戳的样本点数值
  pub fn update_sample_value(
    chunk_data: &[u8],
    ts: u64,
    value: f64,
    is_add_on: bool,
    chunk_type: ChunkType,
  ) -> Result<Option<Vec<u8>>> {
    let mut samples = Self::decode_samples(chunk_data)?;
    if let Ok(idx) = samples.binary_search_by_key(&ts, |s| s.ts) {
      if is_add_on {
        samples[idx].v += value;
      } else {
        samples[idx].v = value;
      }
      Ok(Some(Self::encode_with_type(&samples, chunk_type)))
    } else {
      Ok(None)
    }
  }
}
