//! # Bitmap
//!
//! ## Overview
//! The Bitmap data structure provides bit-level storage and manipulation on binary strings.
//! It supports individual bit offsets, population counts, bitwise logical operations, and arbitrary bitfield encodings.
//!
//! ## Use Cases
//! - Daily active user (DAU) and sign-in status tracking
//! - User permission matrices and feature flags
//! - High-density boolean state arrays
//! - Compact integer sequence packing via bitfields
//!
//! ---
//!
//! # 位图
//!
//! ## 概述
//! 位图结构支持在字节序列上进行精确到单比特级别的读写与操作。
//! 支持比特位定位、置位统计、位逻辑运算以及任意宽度的位域编码解析。
//!
//! ## 使用场景
//! - 每日用户活跃与签到状态记录
//! - 用户权限矩阵与功能开关
//! - 超高密度布尔状态数组
//! - 紧凑定长整型序列编码

use anyhow::Result;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  bitmap::{BitCount, BitOp, BitPos, BitfieldEncoding, BitfieldOperation},
};

fn main() -> Result<()> {
  let dir = tempdir()?;
  let wedb = WeDb::new(Fjall::open(dir.path())?);
  let db = wedb.ns(0)?.db(0)?;

  // Bit setting, getting, and population counting
  // 单比特设置、获取与置位计数
  db.setbit(b"bm1", 10, 1)?;
  db.setbit(b"bm1", 20, 1)?;
  assert_eq!(db.getbit(b"bm1", 10)?, 1);
  assert_eq!(db.getbit(b"bm1", 15)?, 0);
  assert_eq!(db.bitcount(b"bm1", [])?, 2);
  assert_eq!(db.bitcount(b"bm1", [BitCount::Range(0, 2)])?, 2);

  // Finding bit positions
  // 查找首个比特位置
  assert_eq!(db.bitpos(b"bm1", 1, [])?, 10);
  assert_eq!(db.bitpos(b"bm1", 1, [BitPos::Range(0, 2)])?, 10);

  // Bitwise logical operations
  // 多键位逻辑运算
  db.setbit(b"bm2", 10, 1)?;
  db.setbit(b"bm2", 30, 1)?;
  db.bitop(BitOp::Or, b"bm_or", &[b"bm1".as_slice(), b"bm2".as_slice()])?;
  assert_eq!(db.bitcount(b"bm_or", [])?, 3);

  // Arbitrary bitfield operations
  // 位域复合操作与只读执行
  let ops = [BitfieldOperation::get(BitfieldEncoding::Unsigned(8), 0)];
  let _ = db.bitfield(b"bm1", ops)?;
  let _ = db.bitfield_read_only(b"bm1", ops)?;

  // Raw byte retrieval and bitmap deletion
  // 底层原始字节获取与位图删除
  assert!(db.get_bitmap_bytes(b"bm1")?.is_some());
  assert_eq!(db.del(&[b"bm1".as_slice()])?, 1);

  println!("Bitmap 示例全部接口执行成功");
  Ok(())
}
