use core::hint::unreachable_unchecked;
use std::ptr::read_unaligned;

use crate::{
  constants::{
    CHUNK_SIZE_1024, LEN_TAG_1024, LEN_TAG_MASK, LEN_TAG_SHIFT, LEN_TAG_U8, LEN_TAG_U16,
    LEN_TAG_U32, TYPE_F32_DEC, TYPE_F32_DEC_DELTA, TYPE_F32_RAW, TYPE_F64_DEC, TYPE_F64_DEC_DELTA,
    TYPE_F64_RAW, TYPE_MASK,
  },
  error::{Error, Result},
  params::AlpParams,
};

/// Maximum header length in bytes (1B desc + 4B count + 2B params).
/// 自描述头部最大字节长度 (1B 描述符 + 4B u32 长度 + 2B 参数)
pub const MAX_HEADER_LEN: usize = 7;

/// Parsed header components from self-describing byte sequence.
/// 自描述字节序列解析得到的头部信息
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedHeader {
  pub type_byte: u8,
  pub count: usize,
  pub len_tag: u8,
  pub params: Option<AlpParams>,
  pub cursor: usize,
}

/// Calculates byte size needed to encode `count` in the compact header.
/// 编译期计算存储元素数量 `count` 所需的字节数
#[inline(always)]
pub const fn count_bytes(count: usize) -> usize {
  if count == CHUNK_SIZE_1024 {
    0
  } else if count <= u8::MAX as usize {
    1
  } else if count <= u16::MAX as usize {
    2
  } else {
    4
  }
}

/// Calculates total header length in bytes for a given count and params presence.
/// 计算给定元素数量下完整自描述头部的字节长度 (1B 描述符 + count 字节 + 2B params)
#[inline(always)]
pub const fn header_len(count: usize) -> usize {
  1 + count_bytes(count) + 2
}

/// Calculates RAW mode header length in bytes for a given count (no params).
/// 计算 RAW 回退保底模式下头部的字节长度 (无 params, 1B 描述符 + count 字节)
#[inline(always)]
pub const fn raw_header_len(count: usize) -> usize {
  1 + count_bytes(count)
}

/// Writes compact self-describing header into dst with a single memory write.
/// 写入紧凑自描述头部至 dst 缓冲区（栈上构建，单次拷贝，零多余容量检查）
#[inline(always)]
pub fn write_header(type_byte: u8, count: usize, params: Option<u16>, dst: &mut Vec<u8>) {
  debug_assert!(count <= u32::MAX as usize, "count exceeds u32::MAX");
  let mut buf = [0u8; MAX_HEADER_LEN];
  let mut len = 1;

  let (len_tag, count_len) = if count == CHUNK_SIZE_1024 {
    (LEN_TAG_1024, 0)
  } else if count <= u8::MAX as usize {
    buf[1] = count as u8;
    (LEN_TAG_U8, 1)
  } else if count <= u16::MAX as usize {
    buf[1..3].copy_from_slice(&(count as u16).to_le_bytes());
    (LEN_TAG_U16, 2)
  } else {
    buf[1..5].copy_from_slice(&(count as u32).to_le_bytes());
    (LEN_TAG_U32, 4)
  };

  buf[0] = (type_byte & TYPE_MASK) | (len_tag << LEN_TAG_SHIFT);
  len += count_len;

  if let Some(p) = params {
    buf[len..len + 2].copy_from_slice(&p.to_le_bytes());
    len += 2;
  }

  dst.extend_from_slice(&buf[..len]);
}

/// Parses compact self-describing header from src.
/// 从 src 字节序列中解析紧凑自描述头部
#[inline(always)]
pub fn read_header(src: &[u8]) -> Result<ParsedHeader> {
  if src.is_empty() {
    return Err(Error::UnexpectedEof {
      needed: 1,
      available: 0,
    });
  }

  let desc_byte = src[0];
  let type_byte = desc_byte & TYPE_MASK;
  let len_tag = (desc_byte >> LEN_TAG_SHIFT) & LEN_TAG_MASK;
  let mut cursor = 1;

  let count = match len_tag {
    LEN_TAG_1024 => CHUNK_SIZE_1024,
    LEN_TAG_U8 => {
      if src.len() < cursor + 1 {
        return Err(Error::UnexpectedEof {
          needed: cursor + 1,
          available: src.len(),
        });
      }
      let c = src[cursor] as usize;
      cursor += 1;
      c
    }
    LEN_TAG_U16 => {
      if src.len() < cursor + 2 {
        return Err(Error::UnexpectedEof {
          needed: cursor + 2,
          available: src.len(),
        });
      }
      // SAFETY: 上方已校验可用字节充足，read_unaligned 安全读取小端 u16
      let c =
        unsafe { u16::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u16>())) } as usize;
      cursor += 2;
      c
    }
    LEN_TAG_U32 => {
      if src.len() < cursor + 4 {
        return Err(Error::UnexpectedEof {
          needed: cursor + 4,
          available: src.len(),
        });
      }
      // SAFETY: 上方已校验可用字节充足，read_unaligned 安全读取小端 u32
      let c =
        unsafe { u32::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u32>())) } as usize;
      cursor += 4;
      c
    }
    // SAFETY: len_tag 是 (desc_byte >> LEN_TAG_SHIFT) & 0x03，取值范围严格为 0..=3，上方 4 个分支已完全穷尽全部取值
    _ => unsafe { unreachable_unchecked() },
  };

  let is_raw = type_byte == TYPE_F64_RAW || type_byte == TYPE_F32_RAW;
  if is_raw || count == 0 {
    return Ok(ParsedHeader {
      type_byte,
      count,
      len_tag,
      params: None,
      cursor,
    });
  }

  if src.len() < cursor + 2 {
    return Err(Error::UnexpectedEof {
      needed: cursor + 2,
      available: src.len(),
    });
  }

  // SAFETY: 上方已校验可用字节充足，read_unaligned 安全读取 2 字节 packed params
  let raw_params = unsafe { u16::from_le(read_unaligned(src.as_ptr().add(cursor).cast::<u16>())) };
  cursor += 2;
  let is_dec = type_byte == TYPE_F64_DEC
    || type_byte == TYPE_F32_DEC
    || type_byte == TYPE_F64_DEC_DELTA
    || type_byte == TYPE_F32_DEC_DELTA;
  let alp_params = AlpParams::from_packed(raw_params, is_dec);

  Ok(ParsedHeader {
    type_byte,
    count,
    len_tag,
    params: Some(alp_params),
    cursor,
  })
}
