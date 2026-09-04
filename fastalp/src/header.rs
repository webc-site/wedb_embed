use core::{hint::unreachable_unchecked, mem::size_of};

use crate::error::{Error, Result};
pub use crate::{
  constants::{
    CHUNK_SIZE, CHUNK_SIZE_1024, FLAG_REPEAT, LEN_TAG_1024, LEN_TAG_MASK, LEN_TAG_SHIFT,
    LEN_TAG_U8, LEN_TAG_U16, LEN_TAG_U32, MAX_TYPE_BYTE, TYPE_F32, TYPE_F32_DEC,
    TYPE_F32_DEC_DELTA, TYPE_F32_DELTA, TYPE_F32_DICT, TYPE_F32_RAW, TYPE_F32_RD, TYPE_F64,
    TYPE_F64_DEC, TYPE_F64_DEC_DELTA, TYPE_F64_DELTA, TYPE_F64_DICT, TYPE_F64_RAW, TYPE_F64_RD,
    TYPE_MASK,
  },
  params::AlpParams,
};

/// Descriptor byte length (1B).
const DESC_LEN: usize = 1;
/// Maximum count field length in bytes (u32: 4B).
const MAX_COUNT_LEN: usize = size_of::<u32>();
/// Packed parameters field length in bytes (u16: 2B).
const PARAMS_LEN: usize = size_of::<u16>();

/// Maximum header length in bytes (1B desc + 4B count + 2B params).
/// 自描述头部最大字节长度 (1B 描述符 + 4B u32 长度 + 2B 参数)
pub const MAX_HEADER_LEN: usize = DESC_LEN + MAX_COUNT_LEN + PARAMS_LEN;

/// Strongly typed compression chunk format identifier.
/// 强类型压缩数据块格式枚举标识 (以 u8 紧凑编码，零内存与抽象开销)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ChunkType {
  F64 = TYPE_F64,
  F32 = TYPE_F32,
  F64Raw = TYPE_F64_RAW,
  F32Raw = TYPE_F32_RAW,
  F64Delta = TYPE_F64_DELTA,
  F32Delta = TYPE_F32_DELTA,
  F64Dec = TYPE_F64_DEC,
  F32Dec = TYPE_F32_DEC,
  F64DecDelta = TYPE_F64_DEC_DELTA,
  F32DecDelta = TYPE_F32_DEC_DELTA,
  F64Dict = TYPE_F64_DICT,
  F32Dict = TYPE_F32_DICT,
  F64Rd = TYPE_F64_RD,
  F32Rd = TYPE_F32_RD,
}

impl ChunkType {
  /// Parses raw byte into strongly typed `ChunkType`.
  /// 从原始字节解析为强类型 `ChunkType`
  #[inline(always)]
  pub const fn from_u8(val: u8) -> Option<Self> {
    match val {
      TYPE_F64 => Some(Self::F64),
      TYPE_F32 => Some(Self::F32),
      TYPE_F64_RAW => Some(Self::F64Raw),
      TYPE_F32_RAW => Some(Self::F32Raw),
      TYPE_F64_DELTA => Some(Self::F64Delta),
      TYPE_F32_DELTA => Some(Self::F32Delta),
      TYPE_F64_DEC => Some(Self::F64Dec),
      TYPE_F32_DEC => Some(Self::F32Dec),
      TYPE_F64_DEC_DELTA => Some(Self::F64DecDelta),
      TYPE_F32_DEC_DELTA => Some(Self::F32DecDelta),
      TYPE_F64_DICT => Some(Self::F64Dict),
      TYPE_F32_DICT => Some(Self::F32Dict),
      TYPE_F64_RD => Some(Self::F64Rd),
      TYPE_F32_RD => Some(Self::F32Rd),
      _ => None,
    }
  }

  /// Returns underlying byte identifier.
  /// 获取底层原始字节标识
  #[inline(always)]
  pub const fn as_u8(self) -> u8 {
    self as u8
  }
}

impl TryFrom<u8> for ChunkType {
  type Error = Error;

  #[inline(always)]
  fn try_from(val: u8) -> Result<Self> {
    Self::from_u8(val).ok_or(Error::InvalidHeader)
  }
}

/// Parsed header components from self-describing byte sequence.
/// 自描述字节序列解析得到的头部信息
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedHeader {
  pub type_byte: u8,
  pub count: usize,
  pub len_tag: u8,
  pub params: Option<AlpParams>,
  pub cursor: usize,
  pub has_repeat: bool,
}

impl ParsedHeader {
  /// Returns strongly typed `ChunkType` if known.
  /// 获取强类型 `ChunkType` 枚举标识
  #[inline(always)]
  pub const fn chunk_type(&self) -> Option<ChunkType> {
    ChunkType::from_u8(self.type_byte)
  }
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

  let flag_repeat = type_byte & FLAG_REPEAT;
  buf[0] = (type_byte & TYPE_MASK) | (len_tag << LEN_TAG_SHIFT) | flag_repeat;
  len += count_len;

  if let Some(p) = params {
    buf[len..len + 2].copy_from_slice(&p.to_le_bytes());
    len += 2;
  }

  dst.extend_from_slice(&buf[..len]);
}

/// Reads the element count from the compact self-describing header in O(1) time without reading params or payload.
///
/// 从紧凑自描述头部快速读取元素总数（O(1) 复杂度，零内存分配，无需解析后续参数与有效载荷）。
#[inline(always)]
pub fn read_count(src: &[u8]) -> Result<usize> {
  if src.is_empty() {
    return Err(Error::UnexpectedEof {
      needed: 1,
      available: 0,
    });
  }

  let desc_byte = src[0];
  let type_byte = desc_byte & TYPE_MASK;
  if type_byte == 0 || type_byte > MAX_TYPE_BYTE {
    return Err(Error::InvalidHeader);
  }

  let len_tag = (desc_byte >> LEN_TAG_SHIFT) & LEN_TAG_MASK;
  match len_tag {
    LEN_TAG_1024 => Ok(CHUNK_SIZE_1024),
    LEN_TAG_U8 => {
      if src.len() < 2 {
        return Err(Error::UnexpectedEof {
          needed: 2,
          available: src.len(),
        });
      }
      Ok(src[1] as usize)
    }
    LEN_TAG_U16 => {
      if src.len() < 3 {
        return Err(Error::UnexpectedEof {
          needed: 3,
          available: src.len(),
        });
      }
      let c = u16::from_le_bytes([src[1], src[2]]) as usize;
      Ok(c)
    }
    LEN_TAG_U32 => {
      if src.len() < 5 {
        return Err(Error::UnexpectedEof {
          needed: 5,
          available: src.len(),
        });
      }
      let c = u32::from_le_bytes(src[1..5].try_into().map_err(|_| Error::InvalidHeader)?) as usize;
      Ok(c)
    }
    // SAFETY: len_tag is 2 bits masked with 0x03, 0..=3 fully covered above
    // SAFETY: len_tag 取值严格为 0..=3，上方 4 个分支已完全穷尽全部可能取值
    _ => unsafe { unreachable_unchecked() },
  }
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
  let has_repeat = (desc_byte & FLAG_REPEAT) != 0;
  if type_byte == 0 || type_byte > MAX_TYPE_BYTE {
    return Err(Error::InvalidHeader);
  }

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
      let c = u16::from_le_bytes([src[cursor], src[cursor + 1]]) as usize;
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
      let c = u32::from_le_bytes(
        src[cursor..cursor + 4]
          .try_into()
          .map_err(|_| Error::InvalidHeader)?,
      ) as usize;
      cursor += 4;
      c
    }
    // SAFETY: len_tag is (desc_byte >> LEN_TAG_SHIFT) & 0x03 in 0..=3, fully covered by the 4 branches above
    // SAFETY: len_tag 是 (desc_byte >> LEN_TAG_SHIFT) & 0x03，取值范围严格为 0..=3，上方 4 个分支已完全穷尽全部取值
    _ => unsafe { unreachable_unchecked() },
  };

  let is_raw = type_byte == TYPE_F64_RAW || type_byte == TYPE_F32_RAW;
  let is_dict = type_byte == TYPE_F64_DICT || type_byte == TYPE_F32_DICT;
  let is_rd = type_byte == TYPE_F64_RD || type_byte == TYPE_F32_RD;
  if is_raw || is_dict || is_rd || count == 0 {
    return Ok(ParsedHeader {
      type_byte,
      count,
      len_tag,
      params: None,
      cursor,
      has_repeat,
    });
  }

  if src.len() < cursor + 2 {
    return Err(Error::UnexpectedEof {
      needed: cursor + 2,
      available: src.len(),
    });
  }

  let raw_params = u16::from_le_bytes([src[cursor], src[cursor + 1]]);
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
    has_repeat,
  })
}
