pub mod r#const;
pub mod group;
pub mod r#impl;
pub mod info;
pub mod key;
pub mod meta;
pub mod opt;
pub mod pel;
pub mod range;
pub mod trim;
use std::str;

pub use r#const::*;
pub use group::check_lag_valid;
pub use key::{
  consumer_meta as compose_stream_consumer_meta, consumer_prefix as compose_stream_consumer_prefix,
  consumer_prefix_all as compose_stream_consumer_prefix_all,
  group_meta as compose_stream_group_meta, group_prefix as compose_stream_group_prefix,
  item as compose_stream_item, meta as compose_stream_meta_key,
  pel_item as compose_stream_pel_item, pel_prefix as compose_stream_pel_prefix,
  pel_prefix_all as compose_stream_pel_prefix_all, prefix as compose_stream_prefix,
};
pub use meta::{
  NextStreamEntryIdStrategy, StreamAutoClaimResult, StreamClaimResult, StreamConsumerGroupMeta,
  StreamConsumerMeta, StreamGetPendingEntryResult, StreamId, StreamInfo, StreamMeta, StreamNack,
  StreamPelEntry, StreamReadResult, StreamSubkeyType,
};
pub use opt::{
  StreamAdd, StreamAutoClaim, StreamClaim, StreamLen, StreamPending, StreamRange, StreamRead,
  StreamTrim, StreamTrimStrategy, StreamXGroupCreate, XAdd, XAutoClaim, XClaim, XGroupCreate,
  XPending, XRange, XRead, XTrim,
};
pub use pel::{
  stream_ack, stream_autoclaim, stream_claim, stream_pending_range, stream_pending_summary,
};
pub use range::{stream_range, stream_range_with_options, stream_revrange};
pub use trim::{stream_del, stream_setid, stream_trim};
/// Stream message entry type containing StreamId and field-value pairs.
/// 流消息项 (StreamId, Fields)
pub type StreamEntry = (StreamId, Vec<(String, String)>);

/// Fast zero-copy binary extraction of StreamId from 16-byte subkey [ms: 8B][seq: 8B].
/// 从子键字节中快速提取 StreamId [ms: 8B][seq: 8B]（零拷贝紧凑二进制解析）
#[inline]
pub(crate) fn parse_stream_id_from_subkey(sub: &[u8]) -> Option<StreamId> {
  if sub.len() == 16 {
    let ms = u64::from_be_bytes(sub[..8].try_into().ok()?);
    let seq = u64::from_be_bytes(sub[8..16].try_into().ok()?);
    Some(StreamId::new(ms, seq))
  } else {
    None
  }
}

/// Encodes data into binary format.
/// 紧凑二进制打包编码 Stream 任意键值对 [count: u32][k_len: u32][k_bytes][v_len: u32][v_bytes]
#[inline]
pub fn encode_stream_entry_pairs<FK: AsRef<[u8]>, FV: AsRef<[u8]>>(fields: &[(FK, FV)]) -> Vec<u8> {
  let payload_len: usize = fields
    .iter()
    .map(|(k, v)| 8 + k.as_ref().len() + v.as_ref().len())
    .sum();
  let mut buf = Vec::with_capacity(4 + payload_len);
  buf.extend_from_slice(&(fields.len() as u32).to_be_bytes());
  for (k, v) in fields {
    let kb = k.as_ref();
    let vb = v.as_ref();
    buf.extend_from_slice(&(kb.len() as u32).to_be_bytes());
    buf.extend_from_slice(kb);
    buf.extend_from_slice(&(vb.len() as u32).to_be_bytes());
    buf.extend_from_slice(vb);
  }
  buf
}

/// Encodes data into binary format.
/// 紧凑二进制打包编码 Stream 字符串键值对
#[inline]
pub fn encode_stream_entry_fields(fields: &[(String, String)]) -> Vec<u8> {
  encode_stream_entry_pairs(fields)
}

/// Zero-copy streaming field-value iterator with zero heap allocation and O(1) space complexity.
/// 零拷贝流式按需字段迭代器（零堆分配，O(1) 空间复杂度）
#[derive(Debug, Clone)]
pub struct StreamEntryFieldIter<'a> {
  bytes: &'a [u8],
  remaining: usize,
}

impl<'a> Iterator for StreamEntryFieldIter<'a> {
  type Item = (&'a [u8], &'a [u8]);

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    if self.remaining == 0 || self.bytes.len() < 4 {
      return None;
    }
    let k_len = u32::from_be_bytes(self.bytes[..4].try_into().ok()?) as usize;
    self.bytes = &self.bytes[4..];
    if self.bytes.len() < k_len {
      return None;
    }
    let k_bytes = &self.bytes[..k_len];
    self.bytes = &self.bytes[k_len..];

    if self.bytes.len() < 4 {
      return None;
    }
    let v_len = u32::from_be_bytes(self.bytes[..4].try_into().ok()?) as usize;
    self.bytes = &self.bytes[4..];
    if self.bytes.len() < v_len {
      return None;
    }
    let v_bytes = &self.bytes[..v_len];
    self.bytes = &self.bytes[v_len..];

    self.remaining -= 1;
    Some((k_bytes, v_bytes))
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.remaining, Some(self.remaining))
  }
}

impl<'a> ExactSizeIterator for StreamEntryFieldIter<'a> {}

/// Iterates and parses raw stream entry key-value byte slice with zero heap allocation.
/// 零拷贝流式迭代解析 Stream 键值对原始字节切片（零堆分配，O(1) 空间）
#[inline]
pub fn decode_stream_entry_raw_iter(bytes: &[u8]) -> Option<StreamEntryFieldIter<'_>> {
  if bytes.len() < 4 {
    return None;
  }
  let count = u32::from_be_bytes(bytes[..4].try_into().ok()?) as usize;
  Some(StreamEntryFieldIter {
    bytes: &bytes[4..],
    remaining: count,
  })
}

/// Decodes data from binary format.
/// 零拷贝二进制解码 Stream 键值对原始字节切片（完全二进制安全）
#[inline]
pub fn decode_stream_entry_raw_bytes(bytes: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
  let iter = decode_stream_entry_raw_iter(bytes)?;
  Some(iter.collect())
}

/// Decodes data from binary format.
/// 零拷贝二进制解码 Stream 键值对（借用底层切片字符串）
#[inline]
pub fn decode_stream_entry_fields_borrowed(bytes: &[u8]) -> Option<Vec<(&str, &str)>> {
  let iter = decode_stream_entry_raw_iter(bytes)?;
  let mut fields = Vec::with_capacity(iter.remaining);
  for (k, v) in iter {
    fields.push((str::from_utf8(k).ok()?, str::from_utf8(v).ok()?));
  }
  Some(fields)
}

/// Decodes data from binary format.
/// 二进制解码 Stream 键值对（生成 String Vector）
#[inline]
pub fn decode_stream_entry_fields(bytes: &[u8]) -> Option<Vec<(String, String)>> {
  let borrowed = decode_stream_entry_fields_borrowed(bytes)?;
  Some(
    borrowed
      .into_iter()
      .map(|(k, v)| (k.to_string(), v.to_string()))
      .collect(),
  )
}
