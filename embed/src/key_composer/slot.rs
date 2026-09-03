use memchr::memchr;

use crate::key_composer::oppv::{encode_oppv_u64, encode_oppv_u64_slice, oppv_len_u64};

pub const HASH_SLOTS_MASK: u16 = 0x3fff;
pub const HASH_SLOTS_SIZE: u16 = HASH_SLOTS_MASK + 1; // 16384

#[rustfmt::skip]
const CRC16_TAB: [u16; 256] = [
    0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7, 0x8108, 0x9129, 0xa14a, 0xb16b, 0xc18c, 0xd1ad,
    0xe1ce, 0xf1ef, 0x1231, 0x0210, 0x3273, 0x2252, 0x52b5, 0x4294, 0x72f7, 0x62d6, 0x9339, 0x8318, 0xb37b, 0xa35a,
    0xd3bd, 0xc39c, 0xf3ff, 0xe3de, 0x2462, 0x3443, 0x0420, 0x1401, 0x64e6, 0x74c7, 0x44a4, 0x5485, 0xa56a, 0xb54b,
    0x8528, 0x9509, 0xe5ee, 0xf5cf, 0xc5ac, 0xd58d, 0x3653, 0x2672, 0x1611, 0x0630, 0x76d7, 0x66f6, 0x5695, 0x46b4,
    0xb75b, 0xa77a, 0x9719, 0x8738, 0xf7df, 0xe7fe, 0xd79d, 0xc7bc, 0x48c4, 0x58e5, 0x6886, 0x78a7, 0x0840, 0x1861,
    0x2802, 0x3823, 0xc9cc, 0xd9ed, 0xe98e, 0xf9af, 0x8948, 0x9969, 0xa90a, 0xb92b, 0x5af5, 0x4ad4, 0x7ab7, 0x6a96,
    0x1a71, 0x0a50, 0x3a33, 0x2a12, 0xdbfd, 0xcbdc, 0xfbbf, 0xeb9e, 0x9b79, 0x8b58, 0xbb3b, 0xab1a, 0x6ca6, 0x7c87,
    0x4ce4, 0x5cc5, 0x2c22, 0x3c03, 0x0c60, 0x1c41, 0xedae, 0xfd8f, 0xcdec, 0xddcd, 0xad2a, 0xbd0b, 0x8d68, 0x9d49,
    0x7e97, 0x6eb6, 0x5ed5, 0x4ef4, 0x3e13, 0x2e32, 0x1e51, 0x0e70, 0xff9f, 0xefbe, 0xdfdd, 0xcffc, 0xbf1b, 0xaf3a,
    0x9f59, 0x8f78, 0x9188, 0x81a9, 0xb1ca, 0xa1eb, 0xd10c, 0xc12d, 0xf14e, 0xe16f, 0x1080, 0x00a1, 0x30c2, 0x20e3,
    0x5004, 0x4025, 0x7046, 0x6067, 0x83b9, 0x9398, 0xa3fb, 0xb3da, 0xc33d, 0xd31c, 0xe37f, 0xf35e, 0x02b1, 0x1290,
    0x22f3, 0x32d2, 0x4235, 0x5214, 0x6277, 0x7256, 0xb5ea, 0xa5cb, 0x95a8, 0x8589, 0xf56e, 0xe54f, 0xd52c, 0xc50d,
    0x34e2, 0x24c3, 0x14a0, 0x0481, 0x7466, 0x6447, 0x5424, 0x4405, 0xa7db, 0xb7fa, 0x8799, 0x97b8, 0xe75f, 0xf77e,
    0xc71d, 0xd73c, 0x26d3, 0x36f2, 0x0691, 0x16b0, 0x6657, 0x7676, 0x4615, 0x5634, 0xd94c, 0xc96d, 0xf90e, 0xe92f,
    0x99c8, 0x89e9, 0xb98a, 0xa9ab, 0x5844, 0x4865, 0x7806, 0x6827, 0x18c0, 0x08e1, 0x3882, 0x28a3, 0xcb7d, 0xdb5c,
    0xeb3f, 0xfb1e, 0x8bf9, 0x9bd8, 0xabbb, 0xbb9a, 0x4a75, 0x5a54, 0x6a37, 0x7a16, 0x0af1, 0x1ad0, 0x2ab3, 0x3a92,
    0xfd2e, 0xed0f, 0xdd6c, 0xcd4d, 0xbdaa, 0xad8b, 0x9de8, 0x8dc9, 0x7c26, 0x6c07, 0x5c64, 0x4c45, 0x3ca2, 0x2c83,
    0x1ce0, 0x0cc1, 0xef1f, 0xff3e, 0xcf5d, 0xdf7c, 0xaf9b, 0xbfba, 0x8fd9, 0x9ff8, 0x6e17, 0x7e36, 0x4e55, 0x5e74,
    0x2e93, 0x3eb2, 0x0ed1, 0x1ef0,
];

/// Computes Redis Cluster Slot CRC16 checksum (aligned with Kvrocks Crc16).
/// 计算 Redis Cluster Slot CRC16 校验和（对标 Kvrocks Crc16）
#[inline]
pub fn crc16(buf: &[u8]) -> u16 {
  let mut crc = 0u16;
  for &b in buf {
    crc = (crc << 8) ^ CRC16_TAB[((crc >> 8) as u8 ^ b) as usize];
  }
  crc
}

/// Extracts Redis Hash Tag from key (aligned with Kvrocks GetTagFromKey).
/// 提取 Redis Hash Tag（对标 Kvrocks GetTagFromKey）
#[inline]
pub fn get_tag_from_key(key: &[u8]) -> &[u8] {
  if let Some(left_pos) = memchr(b'{', key)
    && let Some(right_pos) = memchr(b'}', &key[left_pos + 1..])
    && right_pos > 0
  {
    return &key[left_pos + 1..left_pos + 1 + right_pos];
  }
  b""
}

/// Computes Cluster Slot ID for a key (aligned with Kvrocks GetSlotIdFromKey).
/// 计算 Key 的 Slot ID（对标 Kvrocks GetSlotIdFromKey）
#[inline]
pub fn get_slot_id_from_key(key: &[u8]) -> u16 {
  let tag = get_tag_from_key(key);
  let target = if tag.is_empty() { key } else { tag };
  crc16(target) & HASH_SLOTS_MASK
}

/// Encodes Slot Key prefix into a fixed 12-byte stack buffer without heap allocation.
/// 栈上零堆分配编码 Slot Key 前缀到 12 字节数组（返回写入字节数）
#[inline(always)]
pub fn encode_slot_key_prefix_fixed(ns_id: u64, slot_id: u16, buf: &mut [u8; 12]) -> usize {
  buf[0] = 0;
  let n = encode_oppv_u64_slice(ns_id, &mut buf[1..]);
  buf[1 + n..1 + n + 2].copy_from_slice(&slot_id.to_be_bytes());
  1 + n + 2
}

/// Composes Slot Key prefix with OPPV variable-length encoding (\x00[oppv(ns_id)][slot_id_be]).
/// 构造 Slot Key 前缀（\x00[oppv(ns_id)][slot_id_be]）
#[inline]
pub fn compose_slot_key_prefix(ns_id: u64, slot_id: u16) -> Vec<u8> {
  let mut output = Vec::with_capacity(1 + oppv_len_u64(ns_id) + 2);
  output.push(0);
  encode_oppv_u64(ns_id, &mut output);
  output.extend_from_slice(&slot_id.to_be_bytes());
  output
}

/// Composes Slot Key upper bound (aligned with Kvrocks ComposeSlotKeyUpperBound).
/// 构造 Slot Key 上界（对标 Kvrocks ComposeSlotKeyUpperBound）
#[inline]
pub fn compose_slot_key_upper_bound(ns_id: u64, slot_id: u16) -> Vec<u8> {
  compose_slot_key_prefix(ns_id, slot_id.saturating_add(1))
}

/// Matches glob pattern against byte slice with zero-copy evaluation.
/// 匹配通配符 Pattern (支持 `*`, `?`, `[a-z]`, `[^a-z]`, `\x` 转义，字节切片零拷贝)
#[inline]
pub fn matches_glob_bytes(mut pattern: &[u8], mut text: &[u8]) -> bool {
  while !pattern.is_empty() {
    match pattern[0] {
      b'*' => {
        while pattern.len() > 1 && pattern[1] == b'*' {
          pattern = &pattern[1..];
        }
        if pattern.len() == 1 {
          return true;
        }
        while !text.is_empty() {
          if matches_glob_bytes(&pattern[1..], text) {
            return true;
          }
          text = &text[1..];
        }
        return matches_glob_bytes(&pattern[1..], text);
      }
      b'?' => {
        if text.is_empty() {
          return false;
        }
        pattern = &pattern[1..];
        text = &text[1..];
      }
      b'[' => {
        if text.is_empty() {
          return false;
        }
        pattern = &pattern[1..];
        let not = if !pattern.is_empty() && (pattern[0] == b'^' || pattern[0] == b'!') {
          pattern = &pattern[1..];
          true
        } else {
          false
        };
        let mut match_found = false;
        let c = text[0];
        loop {
          if pattern.is_empty() {
            return false;
          }
          if pattern[0] == b'\\' && pattern.len() > 1 {
            pattern = &pattern[1..];
            if pattern[0] == c {
              match_found = true;
            }
          } else if pattern[0] == b']' {
            pattern = &pattern[1..];
            break;
          } else if pattern.len() >= 3 && pattern[1] == b'-' && pattern[2] != b']' {
            let start = pattern[0];
            let end = pattern[2];
            if (start <= c && c <= end) || (end <= c && c <= start) {
              match_found = true;
            }
            pattern = &pattern[2..];
          } else if pattern[0] == c {
            match_found = true;
          }
          pattern = &pattern[1..];
        }
        if not {
          match_found = !match_found;
        }
        if !match_found {
          return false;
        }
        text = &text[1..];
      }
      b'\\' => {
        if pattern.len() > 1 {
          pattern = &pattern[1..];
        }
        if text.is_empty() || pattern[0] != text[0] {
          return false;
        }
        pattern = &pattern[1..];
        text = &text[1..];
      }
      c => {
        if text.is_empty() || text[0] != c {
          return false;
        }
        pattern = &pattern[1..];
        text = &text[1..];
      }
    }
  }
  text.is_empty()
}

/// Matches glob pattern against string slice.
/// 匹配通配符 Pattern (字符串包装)
#[inline]
pub fn matches_glob(pattern: &str, text: &str) -> bool {
  matches_glob_bytes(pattern.as_bytes(), text.as_bytes())
}
