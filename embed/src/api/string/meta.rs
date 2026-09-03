pub use crate::meta::KeyMeta as StringMeta;
use crate::{
  engine::{Engine, Partition},
  error::{Error, Result},
  key::cleanup_all_composite_data,
  meta::{KeyMeta, RedisType},
  wedb::Db,
};

/// SingleKV string header size (1-byte flags + 8-byte expire timestamp, aligned with Kvrocks 9-byte header).
/// 单 KV 字符串头部大小（1字节flags + 8字节过期时间，对标 Apache Kvrocks SingleKV 9字节头）
pub const STRING_HDR_SIZE: usize = KeyMeta::KVROCKS_SINGLE_KV_ENCODED_SIZE;

/// Encodes a 9-byte compact SingleKV metadata header (1B flags + 8B big-endian expiry timestamp).
/// 生成 9 字节 SingleKV 紧凑元数据头（1字节flags + 8字节大端过期毫秒时间戳）
#[inline]
pub const fn encode_string_header(expire_at_ms: u64) -> [u8; STRING_HDR_SIZE] {
  let flags = KeyMeta::META_64BIT_ENCODING_MASK | (RedisType::String as u8);
  let exp = expire_at_ms.to_be_bytes();
  [
    flags, exp[0], exp[1], exp[2], exp[3], exp[4], exp[5], exp[6], exp[7],
  ]
}

/// Precomputed 9-byte SingleKV header constant for keys without expiration.
/// 预计算无过期时间的 9 字节 SingleKV 常量头部（编译期常量，零运行时计算开销）
pub const STRING_NO_EXPIRY_HEADER: [u8; STRING_HDR_SIZE] = encode_string_header(0);

/// Encodes data into binary format.
/// 编码 SingleKV 字符串值（9 字节元数据头 + 载荷，对标 Apache Kvrocks Metadata::Encode + payload）
#[inline]
pub fn encode_string_value(value: &[u8], expire_at_ms: u64) -> Vec<u8> {
  let mut out = Vec::with_capacity(STRING_HDR_SIZE + value.len());
  if expire_at_ms == 0 {
    out.extend_from_slice(&STRING_NO_EXPIRY_HEADER);
  } else {
    out.extend_from_slice(&encode_string_header(expire_at_ms));
  }
  out.extend_from_slice(value);
  out
}

/// Encodes 9-byte header and payload into an existing byte buffer to avoid heap reallocation.
/// 将 9 字节头与载荷写入指定 Vec 缓冲区，避免额外堆分配
#[inline]
pub fn encode_string_value_into(value: &[u8], expire_at_ms: u64, out: &mut Vec<u8>) {
  out.reserve(STRING_HDR_SIZE + value.len());
  if expire_at_ms == 0 {
    out.extend_from_slice(&STRING_NO_EXPIRY_HEADER);
  } else {
    out.extend_from_slice(&encode_string_header(expire_at_ms));
  }
  out.extend_from_slice(value);
}

/// SingleKV header flags constant (0x81: META_64BIT_ENCODING_MASK | RedisType::String).
/// SingleKV 头部 Flags 掩码常量 (0x81: META_64BIT_ENCODING_MASK | RedisType::String)
pub const STRING_HEADER_FLAG: u8 = KeyMeta::META_64BIT_ENCODING_MASK | (RedisType::String as u8);

/// Decodes data from binary format.
/// 解码 SingleKV 字符串值（9 字节紧凑元数据头 + 载荷）
#[inline(always)]
pub fn decode_string_value(raw: &[u8]) -> (u64, &[u8]) {
  if raw.len() >= STRING_HDR_SIZE && raw[0] == STRING_HEADER_FLAG {
    let exp = u64::from_be_bytes([
      raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8],
    ]);
    (exp, &raw[STRING_HDR_SIZE..])
  } else {
    (0, raw)
  }
}

/// Checks whether string expiration timestamp is in the past.
/// 检查字符串是否已过期
#[inline]
pub const fn is_string_expired(expire_at_ms: u64, now_ms: u64) -> bool {
  expire_at_ms > 0 && expire_at_ms <= now_ms
}

/// Decodes live (unexpired) SingleKV string slice.
/// 解码未过期的有效字符串切片（已过期返回 None）
#[inline(always)]
pub fn decode_live_string_value(raw: &[u8], now_ms: u64) -> Option<&[u8]> {
  let (expire_at, payload) = decode_string_value(raw);
  if is_string_expired(expire_at, now_ms) {
    None
  } else {
    Some(payload)
  }
}

/// Encodes data into binary format.
/// 栈缓冲与动态缓冲统一就地编码调用（小载荷零堆分配）
#[inline(always)]
pub fn with_encoded_string_value<R>(
  val_bytes: &[u8],
  expire_at_ms: u64,
  dyn_buf: &mut Vec<u8>,
  f: impl FnOnce(&[u8]) -> R,
) -> R {
  if val_bytes.len() <= 55 {
    let mut stack_buf = [0u8; 64];
    let header = if expire_at_ms == 0 {
      STRING_NO_EXPIRY_HEADER
    } else {
      encode_string_header(expire_at_ms)
    };
    stack_buf[..STRING_HDR_SIZE].copy_from_slice(&header);
    let total = STRING_HDR_SIZE + val_bytes.len();
    stack_buf[STRING_HDR_SIZE..total].copy_from_slice(val_bytes);
    f(&stack_buf[..total])
  } else {
    dyn_buf.clear();
    encode_string_value_into(val_bytes, expire_at_ms, dyn_buf);
    f(dyn_buf)
  }
}
/// Atomically writes encoded string value, cleaning up composite meta if replacing a nonexistent SingleKV.
/// 写入 SingleKV 编码数据（原子写；若替换不存在的键则级联清理复合元数据）
#[inline]
pub fn write_string_val<E: Engine>(
  db: &Db<E>,
  raw_k: &[u8],
  key_bytes: &[u8],
  enc_val: &[u8],
  old_is_none: bool,
) -> Result<()>
where
  Error: From<E::Error>,
{
  if old_is_none && !db.meta().is_empty()? {
    let mut batch = db.batch();
    batch.insert_data(raw_k, enc_val);
    cleanup_all_composite_data(db, key_bytes, &mut batch)?;
    batch.commit()?;
  } else {
    db.data().insert(raw_k, enc_val)?;
  }
  Ok(())
}
