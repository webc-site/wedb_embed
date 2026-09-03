use crate::key_composer::{
  oppv::{encode_oppv_u64, encode_oppv_u64_slice},
  tag::{SysMetaTag, SystemDomainTag},
};

pub const DEFAULT_NAMESPACE: u64 = 0;

pub const NS_NEXT_ID_KEY: &[u8] = &[
  0xFF,
  SystemDomainTag::SysMeta as u8,
  SysMetaTag::NsNextId as u8,
];
pub const CATALOG_PREFIX: &[u8] = &[0xFF, SystemDomainTag::Catalog as u8];

/// Encodes catalog namespace prefix into an 11-byte stack buffer.
/// 栈上零堆分配编码 Catalog 目录中命名空间的全局前缀到 11 字节数组（返回写入字节数）
#[inline(always)]
pub fn encode_catalog_ns_prefix_fixed(ns_id: u64, buf: &mut [u8; 11]) -> usize {
  buf[0] = CATALOG_PREFIX[0];
  buf[1] = CATALOG_PREFIX[1];
  let n = encode_oppv_u64_slice(ns_id, &mut buf[2..]);
  2 + n
}

/// Compose namespace prefix in catalog directory (format: \xFF\x02[oppv(ns_id)]).
/// 构造 Catalog 目录中命名空间的全局前缀（格式：\xFF\x02[oppv(ns_id)]）
#[inline]
pub fn catalog_ns_prefix(ns_id: u64) -> Vec<u8> {
  let mut v = Vec::with_capacity(CATALOG_PREFIX.len() + 9);
  v.extend_from_slice(CATALOG_PREFIX);
  encode_oppv_u64(ns_id, &mut v);
  v
}

/// Encodes catalog DB index key into a 20-byte stack buffer.
/// 栈上零堆分配编码 Catalog 目录中特定 DB 的索引键到 20 字节数组（返回写入字节数）
#[inline(always)]
pub fn encode_catalog_db_key_fixed(ns_id: u64, db_id: u64, buf: &mut [u8; 20]) -> usize {
  buf[0] = CATALOG_PREFIX[0];
  buf[1] = CATALOG_PREFIX[1];
  let n1 = encode_oppv_u64_slice(ns_id, &mut buf[2..]);
  let n2 = encode_oppv_u64_slice(db_id, &mut buf[2 + n1..]);
  2 + n1 + n2
}

/// Compose catalog index key for specific database (format: \xFF\x02[oppv(ns_id)][oppv(db_id)]).
/// 构造 Catalog 目录中特定 DB 的索引键（格式：\xFF\x02[oppv(ns_id)][oppv(db_id)]）
#[inline]
pub fn catalog_db_key(ns_id: u64, db_id: u64) -> Vec<u8> {
  let mut v = Vec::with_capacity(CATALOG_PREFIX.len() + 9 + 9);
  v.extend_from_slice(CATALOG_PREFIX);
  encode_oppv_u64(ns_id, &mut v);
  encode_oppv_u64(db_id, &mut v);
  v
}

pub const DB_NEXT_ID_PREFIX: &[u8] = &[
  0xFF,
  SystemDomainTag::SysMeta as u8,
  SysMetaTag::DbNextId as u8,
];

/// Encodes DB auto-increment ID generator index key into a 12-byte stack buffer.
/// 栈上零堆分配编码 DB 自增 ID 发号器索引键到 12 字节数组（返回写入字节数）
#[inline(always)]
pub fn encode_db_next_id_key_fixed(ns_id: u64, buf: &mut [u8; 12]) -> usize {
  buf[0] = DB_NEXT_ID_PREFIX[0];
  buf[1] = DB_NEXT_ID_PREFIX[1];
  buf[2] = DB_NEXT_ID_PREFIX[2];
  let n = encode_oppv_u64_slice(ns_id, &mut buf[3..]);
  3 + n
}

/// Compose DB auto-increment ID generator index key in specified namespace (format: \xFF\x01\x05[oppv(ns_id)]).
/// 构造指定命名空间下 DB 自增 ID 发号器的索引键（格式：\xFF\x01\x05[oppv(ns_id)]）
#[inline]
pub fn db_next_id_key(ns_id: u64) -> Vec<u8> {
  let mut v = Vec::with_capacity(DB_NEXT_ID_PREFIX.len() + 9);
  v.extend_from_slice(DB_NEXT_ID_PREFIX);
  encode_oppv_u64(ns_id, &mut v);
  v
}

/// Check if it is the default namespace (id == 0).
/// 判断是否为默认命名空间 (id == 0)
#[inline(always)]
pub const fn is_default_namespace(ns_id: u64) -> bool {
  ns_id == DEFAULT_NAMESPACE
}
