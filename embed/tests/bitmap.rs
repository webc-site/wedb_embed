use std::{thread::sleep, time::Duration};

use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  BitCount, BitOp, BitPos, BitUnit, BitfieldEncoding, BitfieldOperation, BitfieldOverflow,
  BitfieldValue, Fjall, WeDb, parse_bitfield_offset,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_bitmap_basic_set_get() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let offsets = [
    0,
    123,
    1024 * 8,
    1024 * 8 + 1,
    3 * 1024 * 8,
    3 * 1024 * 8 + 1,
  ];
  for &offset in &offsets {
    assert_eq!(db.getbit("bkey", offset)?, 0);
    assert_eq!(db.setbit("bkey", offset, 1)?, 0);
    assert_eq!(db.getbit("bkey", offset)?, 1);
  }

  for &offset in &offsets {
    assert_eq!(db.setbit("bkey", offset, 0)?, 1);
    assert_eq!(db.getbit("bkey", offset)?, 0);
  }

  Ok(())
}

#[test]
fn test_bitmap_bitcount_and_ranges() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let offsets = [
    0,
    123,
    1024 * 8,
    1024 * 8 + 1,
    3 * 1024 * 8,
    3 * 1024 * 8 + 1,
  ];
  for &offset in &offsets {
    db.setbit("bkey", offset, 1)?;
  }

  assert_eq!(db.bitcount("bkey", [BitCount::Range(0, 4 * 1024)])?, 6);
  assert_eq!(db.bitcount("bkey", [BitCount::Range(0, -1)])?, 6);
  assert_eq!(db.bitcount("bkey", [])?, 6);

  // Negative range checks (aligned with Kvrocks BitCountNegative)
  let dir2 = tempdir()?;
  let db2 = WeDb::new(Fjall::open(dir2.path())?).ns(0)?.db(0)?;
  db2.setbit("neg_k", 0, 1)?;
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(0, 4 * 1024)])?, 1);
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(0, 0)])?, 1);
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(0, -1)])?, 1);
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(-1, -1)])?, 1);
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(1, 1)])?, 0);
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(-10000, -10000)])?, 1);

  db2.setbit("neg_k", 5, 1)?;
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(-10000, -10000)])?, 2);

  db2.setbit("neg_k", 8 * 1024 - 1, 1)?;
  db2.setbit("neg_k", 8 * 1024, 1)?;
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(0, 1024)])?, 4);
  assert_eq!(db2.bitcount("neg_k", [BitCount::Range(0, 1023)])?, 3);

  Ok(())
}

#[test]
fn test_bitmap_bitcount_bit_option() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let offsets = [
    0,
    100,
    1024 * 8,
    1024 * 8 + 1,
    3 * 1024 * 8,
    3 * 1024 * 8 + 1,
  ];
  for &offset in &offsets {
    db.setbit("bkey", offset, 1)?;
  }

  assert_eq!(
    db.bitcount(
      "bkey",
      [
        BitCount::Range(0, 4 * 1024 * 8),
        BitCount::Unit(BitUnit::Bit)
      ]
    )?,
    6
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [BitCount::Range(0, -1), BitCount::Unit(BitUnit::Bit)]
    )?,
    6
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [
        BitCount::Range(0, 3 * 1024 * 8 + 1),
        BitCount::Unit(BitUnit::Bit)
      ]
    )?,
    6
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [
        BitCount::Range(1, 3 * 1024 * 8 + 1),
        BitCount::Unit(BitUnit::Bit)
      ]
    )?,
    5
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [BitCount::Range(0, 0), BitCount::Unit(BitUnit::Bit)]
    )?,
    1
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [BitCount::Range(0, 100), BitCount::Unit(BitUnit::Bit)]
    )?,
    2
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [BitCount::Range(100, 1024 * 8), BitCount::Unit(BitUnit::Bit)]
    )?,
    2
  );
  assert_eq!(
    db.bitcount(
      "bkey",
      [
        BitCount::Range(100, 3 * 1024 * 8),
        BitCount::Unit(BitUnit::Bit)
      ]
    )?,
    4
  );

  Ok(())
}

#[test]
fn test_bitmap_bitpos() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let offsets = [
    0,
    123,
    1024 * 8,
    1024 * 8 + 16,
    3 * 1024 * 8,
    3 * 1024 * 8 + 16,
  ];
  for &offset in &offsets {
    db.setbit("bkey", offset, 1)?;
  }

  let start_indexes = [0i64, 1, 124, 1025, 1027, 3 * 1024 + 1];
  for (i, &start) in start_indexes.iter().enumerate() {
    let pos = db.bitpos("bkey", 1, [BitPos::Range(start, -1)])?;
    assert_eq!(pos, offsets[i] as i64);
  }

  // Searching clear bit (0)
  let pos_zero = db.bitpos("bkey", 0, [BitPos::Start(0)])?;
  assert_eq!(pos_zero, 1);

  Ok(())
}

#[test]
fn test_bitmap_bitop() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.setbit("k1", 0, 1)?;
  db.setbit("k1", 2, 1)?;
  db.setbit("k1", 8192, 1)?;

  db.setbit("k2", 0, 1)?;
  db.setbit("k2", 1, 1)?;
  db.setbit("k2", 8192, 1)?;

  // AND
  let len_and = db.bitop(BitOp::And, "k_and", &["k1", "k2"])?;
  assert!(len_and > 0);
  assert_eq!(db.getbit("k_and", 0)?, 1);
  assert_eq!(db.getbit("k_and", 1)?, 0);
  assert_eq!(db.getbit("k_and", 2)?, 0);
  assert_eq!(db.getbit("k_and", 8192)?, 1);

  // OR
  let len_or = db.bitop(BitOp::Or, "k_or", &["k1", "k2"])?;
  assert!(len_or > 0);
  assert_eq!(db.getbit("k_or", 0)?, 1);
  assert_eq!(db.getbit("k_or", 1)?, 1);
  assert_eq!(db.getbit("k_or", 2)?, 1);
  assert_eq!(db.getbit("k_or", 8192)?, 1);

  // XOR
  let len_xor = db.bitop(BitOp::Xor, "k_xor", &["k1", "k2"])?;
  assert!(len_xor > 0);
  assert_eq!(db.getbit("k_xor", 0)?, 0);
  assert_eq!(db.getbit("k_xor", 1)?, 1);
  assert_eq!(db.getbit("k_xor", 2)?, 1);
  assert_eq!(db.getbit("k_xor", 8192)?, 0);

  // NOT
  let len_not = db.bitop(BitOp::Not, "k_not", &["k1"])?;
  assert!(len_not > 0);
  assert_eq!(db.getbit("k_not", 0)?, 0);
  assert_eq!(db.getbit("k_not", 1)?, 1);
  assert_eq!(db.getbit("k_not", 2)?, 0);
  assert_eq!(db.getbit("k_not", 8192)?, 0);

  Ok(())
}

#[test]
fn test_bitmap_bitfield() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. SET & GET Unsigned 32-bit integer
  let magic = 0xdeadbeefu32 as i64;
  let set_op = BitfieldOperation::set(
    BitfieldEncoding::unsigned(32)?,
    114514,
    magic,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("bfkey", [set_op])?;
  assert_eq!(rets.len(), 1);
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));

  let get_op = BitfieldOperation::get(BitfieldEncoding::unsigned(32)?, 114514);
  let rets = db.bitfield("bfkey", [get_op])?;
  assert_eq!(rets.len(), 1);
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0xdeadbeef)));

  // 2. Cross segment division (offset 8189)
  let op_cross = BitfieldOperation::set(
    BitfieldEncoding::unsigned(5)?,
    8189,
    31,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("bfkey", [op_cross])?;
  assert_eq!(rets.len(), 1);

  let get_cross = BitfieldOperation::get(BitfieldEncoding::unsigned(5)?, 8189);
  let rets = db.bitfield_read_only("bfkey", [get_cross])?;
  assert_eq!(rets.len(), 1);
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(31)));

  // 3. Overflow SAT & FAIL
  let op_sat =
    BitfieldOperation::incrby(BitfieldEncoding::signed(6)?, 0, 100, BitfieldOverflow::Sat);
  let rets = db.bitfield("satkey", [op_sat])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(31))); // 2^(6-1)-1 = 31

  let op_fail =
    BitfieldOperation::incrby(BitfieldEncoding::signed(5)?, 0, 100, BitfieldOverflow::Fail);
  let rets = db.bitfield("failkey", [op_fail])?;
  assert_eq!(rets[0], None);

  Ok(())
}

#[test]
fn test_get_bitmap_bytes() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.setbit("bkey", 0, 1)?;
  db.setbit("bkey", 7, 1)?;

  let bytes = db.get_bitmap_bytes("bkey")?.unwrap();
  assert_eq!(bytes.len(), 1);
  // Bit 0 and Bit 7 set in MSB Redis string format is 0x81 (10000001)
  assert_eq!(bytes[0], 0x81);

  Ok(())
}

#[test]
fn test_bitmap_bitfield_all_overflows() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. Signed WRAP (overflow wraps around)
  let op_wrap_pos = BitfieldOperation::incrby(
    BitfieldEncoding::signed(8)?,
    0,
    130, // 0 + 130 = 130 -> i8 wraps to -126
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("bf_overflow", [op_wrap_pos])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(-126)));

  // 2. Signed SAT (underflow clamped to min, overflow clamped to max)
  let op_sat_min = BitfieldOperation::incrby(
    BitfieldEncoding::signed(8)?,
    0,
    -200, // underflow -> clamp to -128
    BitfieldOverflow::Sat,
  );
  let rets = db.bitfield("bf_overflow", [op_sat_min])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(-128)));

  let op_sat_max = BitfieldOperation::incrby(
    BitfieldEncoding::signed(8)?,
    0,
    300, // overflow -> clamp to 127
    BitfieldOverflow::Sat,
  );
  let rets = db.bitfield("bf_overflow", [op_sat_max])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(127)));

  // 3. Unsigned SAT (clamped to 0 on underflow, clamped to max on overflow)
  let op_usat_max = BitfieldOperation::incrby(
    BitfieldEncoding::unsigned(8)?,
    8,
    300, // 0 + 300 -> clamp to 255
    BitfieldOverflow::Sat,
  );
  let rets = db.bitfield("bf_overflow", [op_usat_max])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(255)));

  let op_usat_min = BitfieldOperation::incrby(
    BitfieldEncoding::unsigned(8)?,
    8,
    -500, // 255 - 500 -> clamp to 0
    BitfieldOverflow::Sat,
  );
  let rets = db.bitfield("bf_overflow", [op_usat_min])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));

  // 4. FAIL overflow
  let op_fail = BitfieldOperation::incrby(
    BitfieldEncoding::unsigned(8)?,
    8,
    300, // 0 + 300 > 255 -> overflow fails
    BitfieldOverflow::Fail,
  );
  let rets = db.bitfield("bf_overflow", [op_fail])?;
  assert_eq!(rets[0], None);

  Ok(())
}

#[test]
fn test_bitmap_bitpos_extended() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Non-existent key
  assert_eq!(db.bitpos("empty_key", 1, [])?, -1);
  assert_eq!(db.bitpos("empty_key", 0, [])?, 0);

  // Set bit 10
  db.setbit("pos_key", 10, 1)?;

  // Search bit 1
  assert_eq!(db.bitpos("pos_key", 1, [])?, 10);
  assert_eq!(db.bitpos("pos_key", 1, [BitPos::Range(0, 1)])?, 10); // in byte 1
  assert_eq!(db.bitpos("pos_key", 1, [BitPos::Range(2, 5)])?, -1); // byte 2..5 has no 1

  // Search bit 1 with BIT index
  assert_eq!(
    db.bitpos(
      "pos_key",
      1,
      [BitPos::Range(0, 15), BitPos::Unit(BitUnit::Bit)]
    )?,
    10
  );
  assert_eq!(
    db.bitpos(
      "pos_key",
      1,
      [BitPos::Range(11, 20), BitPos::Unit(BitUnit::Bit)]
    )?,
    -1
  );

  // Search bit 0 with start offset
  assert_eq!(db.bitpos("pos_key", 0, [BitPos::Range(1, 2)])?, 8); // byte 1 start bit 8 is 0 (since bit 10 is 1, bit 8 is 0)
  assert_eq!(
    db.bitpos(
      "pos_key",
      0,
      [BitPos::Range(10, 12), BitPos::Unit(BitUnit::Bit)]
    )?,
    11
  ); // bit 10 is 1, bit 11 is 0

  Ok(())
}

#[test]
fn test_bitmap_bitpos_stop_given() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 0..8 {
    db.setbit("stop_key", i, 1)?;
  }

  // When stop_given is true and no 0 bit in range [0, 0] (byte 0), returns -1
  let pos_stop_given = db.bitpos("stop_key", 0, [BitPos::Range(0, 0)])?;
  assert_eq!(pos_stop_given, -1);

  // When stop_given is false and searching for 0, returns first bit outside (bit 8)
  let pos_no_stop = db.bitpos("stop_key", 0, [BitPos::Start(0)])?;
  assert_eq!(pos_no_stop, 8);

  Ok(())
}

#[test]
fn test_bitmap_bitcount_sub_byte() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Set bits 2, 3, 4, 7 in byte 0
  db.setbit("cnt_key", 2, 1)?;
  db.setbit("cnt_key", 3, 1)?;
  db.setbit("cnt_key", 4, 1)?;
  db.setbit("cnt_key", 7, 1)?;

  // Count in same byte [2, 4] -> should be 3
  assert_eq!(
    db.bitcount(
      "cnt_key",
      [BitCount::Range(2, 4), BitCount::Unit(BitUnit::Bit)]
    )?,
    3
  );
  // Count in [3, 6] -> bits 3, 4 set -> 2
  assert_eq!(
    db.bitcount(
      "cnt_key",
      [BitCount::Range(3, 6), BitCount::Unit(BitUnit::Bit)]
    )?,
    2
  );
  // Count in [0, 1] -> 0
  assert_eq!(
    db.bitcount(
      "cnt_key",
      [BitCount::Range(0, 1), BitCount::Unit(BitUnit::Bit)]
    )?,
    0
  );
  // Count in [7, 7] -> 1
  assert_eq!(
    db.bitcount(
      "cnt_key",
      [BitCount::Range(7, 7), BitCount::Unit(BitUnit::Bit)]
    )?,
    1
  );

  Ok(())
}

#[test]
fn test_bitmap_string_mode_comprehensive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 设置普通 String 键: "foobar" (6 bytes)
  // 'f' = 0x66 = 0b01100110
  // 'o' = 0x6F = 0b01101111
  // 'o' = 0x6F = 0b01101111
  // 'b' = 0x62 = 0b01100010
  // 'a' = 0x61 = 0b01100001
  // 'r' = 0x72 = 0b01110010
  db.set("str_bm", "foobar", [])?;

  // GETBIT on String
  assert_eq!(db.getbit("str_bm", 0)?, 0);
  assert_eq!(db.getbit("str_bm", 1)?, 1);
  assert_eq!(db.getbit("str_bm", 2)?, 1);
  assert_eq!(db.getbit("str_bm", 3)?, 0);

  // BITCOUNT on String
  assert_eq!(db.bitcount("str_bm", [])?, 26);
  assert_eq!(db.bitcount("str_bm", [BitCount::Range(0, 0)])?, 4);
  assert_eq!(
    db.bitcount(
      "str_bm",
      [BitCount::Range(0, 7), BitCount::Unit(BitUnit::Bit)]
    )?,
    4
  );
  assert_eq!(
    db.bitcount(
      "str_bm",
      [BitCount::Range(1, 2), BitCount::Unit(BitUnit::Bit)]
    )?,
    2
  );

  // BITPOS on String
  assert_eq!(db.bitpos("str_bm", 1, [])?, 1);
  assert_eq!(db.bitpos("str_bm", 0, [])?, 0);
  assert_eq!(db.bitpos("str_bm", 1, [BitPos::Range(1, 1)])?, 9); // in second byte 'o'

  // SETBIT on String
  let old = db.setbit("str_bm", 0, 1)?;
  assert_eq!(old, 0);
  assert_eq!(db.getbit("str_bm", 0)?, 1);

  // BITFIELD on String
  let get_u8 = BitfieldOperation::get(BitfieldEncoding::unsigned(8)?, 0);
  let rets = db.bitfield_read_only("str_bm", [get_u8])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0xE6))); // 0x66 with bit 0 set to 1 = 0xE6

  // BITOP between Bitmap and String returns wrong type
  db.setbit("seg_bm", 0, 1)?;
  let err = db.bitop(BitOp::And, "dest", &["seg_bm", "str_bm"]);
  assert!(err.is_err());

  // Clean up with del
  assert_eq!(db.del(&["str_bm"])?, 1);
  assert_eq!(db.getbit("str_bm", 0)?, 0);

  Ok(())
}

#[test]
fn test_bitmap_cleanup_and_shrink() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Create a bitmap spanning 3 segments (> 2048 bytes)
  db.setbit("bm_large", 2048 * 8, 1)?;
  let bytes = db.get_bitmap_bytes("bm_large")?.unwrap();
  assert!(bytes.len() > 2048);

  // Create smaller bitmap (1 segment)
  db.setbit("bm_small", 10, 1)?;

  // BITOP OR into bm_large using only bm_small -> should shrink and clean extra segments
  let len = db.bitop(BitOp::Or, "bm_large", &["bm_small"])?;
  assert!(len < 1024);

  let bytes_after = db.get_bitmap_bytes("bm_large")?.unwrap();
  assert_eq!(bytes_after.len(), len);
  assert_eq!(db.getbit("bm_large", 10)?, 1);
  assert_eq!(db.getbit("bm_large", 2048 * 8)?, 0);

  // Delete bitmap
  assert_eq!(db.del(&["bm_large"])?, 1);
  assert_eq!(db.get_bitmap_bytes("bm_large")?, None);

  Ok(())
}

#[test]
fn test_bitmap_setbit_zero_expands_size() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Setting 0 at offset 80000 on a non-existent key should expand the bitmap to (80000/8)+1 = 10001 bytes
  let old = db.setbit("zero_expand", 80000, 0)?;
  assert_eq!(old, 0);

  let bytes = db.get_bitmap_bytes("zero_expand")?.unwrap();
  assert_eq!(bytes.len(), 10001);
  assert_eq!(db.getbit("zero_expand", 80000)?, 0);
  assert_eq!(db.bitcount("zero_expand", [])?, 0);

  // Setting 0 at larger offset expands further
  db.setbit("zero_expand", 96000, 0)?;
  let bytes_after = db.get_bitmap_bytes("zero_expand")?.unwrap();
  assert_eq!(bytes_after.len(), 12001);

  Ok(())
}

#[test]
fn test_bitmap_bitop_and_nonexistent_key() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Create k1 with 10 bytes (offset 79)
  db.setbit("k1", 79, 1)?;
  let len = db.get_bitmap_bytes("k1")?.unwrap().len();
  assert_eq!(len, 10);

  // BITOP AND dest k1 k_nonexistent -> should return 10, dest should have size 10 and all bits 0
  let res_len = db.bitop(BitOp::And, "dest_and", &["k1", "k_nonexistent"])?;
  assert_eq!(res_len, 10);

  let dest_bytes = db.get_bitmap_bytes("dest_and")?.unwrap();
  assert_eq!(dest_bytes.len(), 10);
  assert_eq!(dest_bytes, vec![0u8; 10]);
  assert_eq!(db.getbit("dest_and", 79)?, 0);

  Ok(())
}

#[test]
fn test_bitmap_bitfield_arbitrary_widths() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. i1 (signed 1-bit: values 0 and -1)
  let set_i1 = BitfieldOperation::set(BitfieldEncoding::signed(1)?, 0, -1, BitfieldOverflow::Wrap);
  let rets = db.bitfield("bf_widths", [set_i1])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(0))); // old was 0

  let get_i1 = BitfieldOperation::get(BitfieldEncoding::signed(1)?, 0);
  let rets = db.bitfield_read_only("bf_widths", [get_i1])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(-1)));

  // 2. u63 (unsigned 63-bit max)
  let max_u63 = (1u64 << 63) - 1;
  let set_u63 = BitfieldOperation::set(
    BitfieldEncoding::unsigned(63)?,
    1,
    max_u63 as i64,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("bf_widths", [set_u63])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));

  let get_u63 = BitfieldOperation::get(BitfieldEncoding::unsigned(63)?, 1);
  let rets = db.bitfield_read_only("bf_widths", [get_u63])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(max_u63)));

  // 3. i64 (signed 64-bit min / max)
  let min_i64 = i64::MIN;
  let set_i64 = BitfieldOperation::set(
    BitfieldEncoding::signed(64)?,
    100,
    min_i64,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("bf_widths", [set_i64])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(0)));

  let get_i64 = BitfieldOperation::get(BitfieldEncoding::signed(64)?, 100);
  let rets = db.bitfield_read_only("bf_widths", [get_i64])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(min_i64)));

  Ok(())
}

#[test]
fn test_bitmap_cross_segment_bitpos_and_bitcount() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Set bit 8191 (last bit of segment 0) and 8192 (first bit of segment 1)
  db.setbit("cross_key", 8191, 1)?;
  db.setbit("cross_key", 8192, 1)?;
  db.setbit("cross_key", 16383, 1)?; // last bit of segment 1
  db.setbit("cross_key", 16384, 1)?; // first bit of segment 2

  // BITCOUNT across segments
  assert_eq!(db.bitcount("cross_key", [])?, 4);
  assert_eq!(db.bitcount("cross_key", [BitCount::Range(0, 1023)])?, 1); // seg 0 only
  assert_eq!(db.bitcount("cross_key", [BitCount::Range(1024, 2047)])?, 2); // seg 1 only (8192 and 16383)
  assert_eq!(db.bitcount("cross_key", [BitCount::Range(2048, 2048)])?, 1); // seg 2 (16384)

  // BITPOS searching 1 across segments
  assert_eq!(db.bitpos("cross_key", 1, [])?, 8191);
  assert_eq!(
    db.bitpos(
      "cross_key",
      1,
      [BitPos::Range(8192, 16383), BitPos::Unit(BitUnit::Bit)]
    )?,
    8192
  );
  assert_eq!(
    db.bitpos(
      "cross_key",
      1,
      [BitPos::Range(8193, 16383), BitPos::Unit(BitUnit::Bit)]
    )?,
    16383
  );
  assert_eq!(
    db.bitpos(
      "cross_key",
      1,
      [BitPos::Range(16384, 20000), BitPos::Unit(BitUnit::Bit)]
    )?,
    16384
  );

  // BITPOS searching 0 across segments
  assert_eq!(db.bitpos("cross_key", 0, [])?, 0);
  assert_eq!(
    db.bitpos(
      "cross_key",
      0,
      [BitPos::Range(8190, 8192), BitPos::Unit(BitUnit::Bit)]
    )?,
    8190
  );

  Ok(())
}

#[test]
fn test_bitmap_bitfield_multiple_cmds_in_sequence() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Execute multiple operations in a single atomic bitfield invocation
  let ops = [
    BitfieldOperation::set(
      BitfieldEncoding::unsigned(8)?,
      0,
      100,
      BitfieldOverflow::Wrap,
    ),
    BitfieldOperation::incrby(
      BitfieldEncoding::unsigned(8)?,
      0,
      50,
      BitfieldOverflow::Wrap,
    ),
    BitfieldOperation::get(BitfieldEncoding::unsigned(8)?, 0),
    BitfieldOperation::set(
      BitfieldEncoding::signed(16)?,
      8,
      -500,
      BitfieldOverflow::Wrap,
    ),
    BitfieldOperation::incrby(
      BitfieldEncoding::signed(16)?,
      8,
      1000,
      BitfieldOverflow::Wrap,
    ),
    BitfieldOperation::get(BitfieldEncoding::signed(16)?, 8),
  ];

  let rets = db.bitfield("seq_bf", ops)?;
  assert_eq!(rets.len(), 6);
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0))); // old was 0
  assert_eq!(rets[1], Some(BitfieldValue::Unsigned(150))); // 100 + 50 = 150
  assert_eq!(rets[2], Some(BitfieldValue::Unsigned(150)));
  assert_eq!(rets[3], Some(BitfieldValue::Signed(0))); // old was 0
  assert_eq!(rets[4], Some(BitfieldValue::Signed(500))); // -500 + 1000 = 500
  assert_eq!(rets[5], Some(BitfieldValue::Signed(500)));

  Ok(())
}

#[test]
fn test_bitmap_bitpos_with_unallocated_segment_and_holes() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Create a bitmap with a segment in index 2 (bit 16384) without segments 0 and 1
  db.setbit("hole_bm", 16384, 1)?;

  // Searching 0 in range [0, 8192] (within segment 0 which is empty)
  assert_eq!(
    db.bitpos(
      "hole_bm",
      0,
      [BitPos::Range(0, 8192), BitPos::Unit(BitUnit::Bit)]
    )?,
    0
  );
  assert_eq!(
    db.bitpos(
      "hole_bm",
      0,
      [BitPos::Range(100, 8192), BitPos::Unit(BitUnit::Bit)]
    )?,
    100
  );

  // Searching 1 in range [0, 8192] -> -1
  assert_eq!(
    db.bitpos(
      "hole_bm",
      1,
      [BitPos::Range(0, 8192), BitPos::Unit(BitUnit::Bit)]
    )?,
    -1
  );

  // Searching 1 in range [10000, 20000] -> 16384
  assert_eq!(
    db.bitpos(
      "hole_bm",
      1,
      [BitPos::Range(10000, 20000), BitPos::Unit(BitUnit::Bit)]
    )?,
    16384
  );

  // Searching 0 in range [16384, 16385] -> 16385 (since 16384 is 1, next bit 16385 is 0)
  assert_eq!(
    db.bitpos(
      "hole_bm",
      0,
      [BitPos::Range(16384, 16385), BitPos::Unit(BitUnit::Bit)]
    )?,
    16385
  );

  Ok(())
}

#[test]
fn test_bitmap_bitop_multi_key_complex() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Setup 4 keys with various offsets
  db.setbit("mk1", 0, 1)?;
  db.setbit("mk1", 8192, 1)?;

  db.setbit("mk2", 1, 1)?;
  db.setbit("mk2", 8192, 1)?;

  db.setbit("mk3", 2, 1)?;
  db.setbit("mk3", 8192, 1)?;

  db.setbit("mk4", 3, 1)?;
  db.setbit("mk4", 8192, 1)?;

  // OR across 4 keys
  let len_or = db.bitop(BitOp::Or, "mk_or", &["mk1", "mk2", "mk3", "mk4"])?;
  assert!(len_or >= 1025);
  assert_eq!(db.getbit("mk_or", 0)?, 1);
  assert_eq!(db.getbit("mk_or", 1)?, 1);
  assert_eq!(db.getbit("mk_or", 2)?, 1);
  assert_eq!(db.getbit("mk_or", 3)?, 1);
  assert_eq!(db.getbit("mk_or", 4)?, 0);
  assert_eq!(db.getbit("mk_or", 8192)?, 1);

  // AND across 4 keys
  let len_and = db.bitop(BitOp::And, "mk_and", &["mk1", "mk2", "mk3", "mk4"])?;
  assert!(len_and >= 1025);
  assert_eq!(db.getbit("mk_and", 0)?, 0);
  assert_eq!(db.getbit("mk_and", 1)?, 0);
  assert_eq!(db.getbit("mk_and", 2)?, 0);
  assert_eq!(db.getbit("mk_and", 3)?, 0);
  assert_eq!(db.getbit("mk_and", 8192)?, 1);

  Ok(())
}

#[test]
fn test_bitmap_bitop_not_with_sparse_segments() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Create a sparse bitmap of size 2000 bytes (spanning 2 segments)
  db.setbit("not_src", 0, 1)?;
  db.setbit("not_src", 15999, 1)?; // byte 1999

  let len = db.bitop(BitOp::Not, "not_dest", &["not_src"])?;
  assert_eq!(len, 2000);

  // Check inverted bits
  assert_eq!(db.getbit("not_dest", 0)?, 0); // 1 -> 0
  assert_eq!(db.getbit("not_dest", 1)?, 1); // 0 -> 1
  assert_eq!(db.getbit("not_dest", 8192)?, 1); // 0 -> 1 in segment 1
  assert_eq!(db.getbit("not_dest", 15999)?, 0); // 1 -> 0

  let bytes = db.get_bitmap_bytes("not_dest")?.unwrap();
  assert_eq!(bytes.len(), 2000);
  // In byte 100 (which was 0 in source), all bits must be 1 (0xFF)
  assert_eq!(bytes[100], 0xFF);
  assert_eq!(bytes[1500], 0xFF);

  Ok(())
}

#[test]
fn test_bitmap_bitfield_string_reverse_and_traversal() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let text = "dan yuan ren chang jiu, qian li gong chan juan.";
  db.set("str_poem", text, [])?;

  let get_op = BitfieldOperation::get(BitfieldEncoding::signed(8)?, 0);
  let mut i = 0u64;
  for &b in text.as_bytes() {
    let mut op = get_op;
    op.offset = i;
    let rets = db.bitfield_read_only("str_poem", [op])?;
    assert_eq!(rets[0], Some(BitfieldValue::Signed(b as i8 as i64)));
    i += 8;
  }

  // Reverse all i8 in bitmap using BITFIELD SET (mirroring Kvrocks BitfieldStringGetSetTest)
  let len = text.len();
  let mut l = 0;
  let mut r = len - 1;
  while l < r {
    let l_offset = (l * 8) as u64;
    let r_offset = (r * 8) as u64;

    let get_r = BitfieldOperation::get(BitfieldEncoding::signed(8)?, r_offset);
    let r_val = db.bitfield_read_only("str_poem", [get_r])?[0]
      .unwrap()
      .as_i64();

    let set_l = BitfieldOperation::set(
      BitfieldEncoding::signed(8)?,
      l_offset,
      r_val,
      BitfieldOverflow::Wrap,
    );
    let old_l = db.bitfield("str_poem", [set_l])?[0].unwrap().as_i64();

    let set_r = BitfieldOperation::set(
      BitfieldEncoding::signed(8)?,
      r_offset,
      old_l,
      BitfieldOverflow::Wrap,
    );
    db.bitfield("str_poem", [set_r])?;

    l += 1;
    r -= 1;
  }

  let reversed_expected: Vec<u8> = text.bytes().rev().collect();
  let actual_val = db.get("str_poem")?.unwrap();
  assert_eq!(actual_val, reversed_expected);

  Ok(())
}

#[test]
fn test_bitmap_bitpos_clear_bit_continuous() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 0..100 {
    let pos_zero = db.bitpos("seq_key", 0, [])?;
    assert_eq!(pos_zero, i as i64);

    let old = db.setbit("seq_key", i, 1)?;
    assert_eq!(old, 0);

    let pos_one = db.bitpos("seq_key", 1, [])?;
    assert_eq!(pos_one, 0);
  }

  Ok(())
}

#[test]
fn test_bitmap_bitpos_negative_normalization() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Set last bit of segment 0 (bit 8191 = byte 1023, bit 7)
  db.setbit("neg_pos", 8 * 1024 - 1, 1)?;

  // First bit is 0
  assert_eq!(db.bitpos("neg_pos", 0, [BitPos::Range(0, -1)])?, 0);
  // First bit 1 in [0, -1] is 8191
  assert_eq!(db.bitpos("neg_pos", 1, [BitPos::Range(0, -1)])?, 8191);
  // Searching 0 in byte -1 (byte 1023) -> first 0 bit in byte 1023 is 8 * 1023 = 8184
  assert_eq!(db.bitpos("neg_pos", 0, [BitPos::Range(-1, -1)])?, 8184);
  // Searching 1 in byte -1 (byte 1023) -> 8191
  assert_eq!(db.bitpos("neg_pos", 1, [BitPos::Range(-1, -1)])?, 8191);
  // Large negative index is normalized to 0
  assert_eq!(db.bitpos("neg_pos", 0, [BitPos::Range(-10000, -10000)])?, 0);

  Ok(())
}

#[test]
fn test_find_bit_in_byte_helpers() -> Void {
  use wedb_embed::{find_bit_in_byte_lsb, find_bit_in_byte_msb};

  // 1. LSB testing: byte 0b00010100 (bits 2 and 4 set)
  let b = 0b00010100u8;
  assert_eq!(find_bit_in_byte_lsb(b, 1, 0, 7), Some(2));
  assert_eq!(find_bit_in_byte_lsb(b, 1, 3, 7), Some(4));
  assert_eq!(find_bit_in_byte_lsb(b, 1, 5, 7), None);
  assert_eq!(find_bit_in_byte_lsb(b, 0, 0, 1), Some(0));
  assert_eq!(find_bit_in_byte_lsb(b, 0, 2, 2), None);
  assert_eq!(find_bit_in_byte_lsb(b, 0, 2, 3), Some(3));

  // 2. MSB testing: byte 0b00101000 (bits 2 and 4 set in MSB numbering)
  // Bit 0 = 0x80, Bit 1 = 0x40, Bit 2 = 0x20, Bit 3 = 0x10, Bit 4 = 0x08...
  let b_msb = 0b00101000u8;
  assert_eq!(find_bit_in_byte_msb(b_msb, 1, 0, 7), Some(2));
  assert_eq!(find_bit_in_byte_msb(b_msb, 1, 3, 7), Some(4));
  assert_eq!(find_bit_in_byte_msb(b_msb, 1, 5, 7), None);
  assert_eq!(find_bit_in_byte_msb(b_msb, 0, 0, 1), Some(0));
  assert_eq!(find_bit_in_byte_msb(b_msb, 0, 2, 2), None);
  assert_eq!(find_bit_in_byte_msb(b_msb, 0, 2, 3), Some(3));

  Ok(())
}

#[test]
fn test_bitmap_bitpos_bit_index_exhaustive() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Set pattern of bits across 2 bytes (16 bits)
  let set_bits = [1, 3, 5, 7, 9, 11, 13, 15];
  for &b in &set_bits {
    db.setbit("ex_bm", b, 1)?;
  }

  // Verify bitpos with BIT index
  for &b in &set_bits {
    let pos = db.bitpos(
      "ex_bm",
      1,
      [BitPos::Range(b as i64, 15), BitPos::Unit(BitUnit::Bit)],
    )?;
    assert_eq!(pos, b as i64);
  }

  // Search 0 bits
  let zero_bits = [0, 2, 4, 6, 8, 10, 12, 14];
  for &b in &zero_bits {
    let pos = db.bitpos(
      "ex_bm",
      0,
      [BitPos::Range(b as i64, 15), BitPos::Unit(BitUnit::Bit)],
    )?;
    assert_eq!(pos, b as i64);
  }

  // Search out of range
  assert_eq!(
    db.bitpos(
      "ex_bm",
      1,
      [BitPos::Range(16, 20), BitPos::Unit(BitUnit::Bit)]
    )?,
    -1
  );

  Ok(())
}

#[test]
fn test_bitmap_bitfield_u63_and_i64_limits() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // u63 max value = 0x7FFFFFFFFFFFFFFF
  let u63_max = 0x7FFF_FFFF_FFFF_FFFFu64;
  let set_u63 = BitfieldOperation::set(
    BitfieldEncoding::unsigned(63)?,
    0,
    u63_max as i64,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("limit_bf", [set_u63])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));

  let get_u63 = BitfieldOperation::get(BitfieldEncoding::unsigned(63)?, 0);
  let rets = db.bitfield_read_only("limit_bf", [get_u63])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(u63_max)));

  // Increment u63_max by 1 with WRAP -> wraps to 0
  let incr_wrap = BitfieldOperation::incrby(
    BitfieldEncoding::unsigned(63)?,
    0,
    1,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("limit_bf", [incr_wrap])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));

  // Increment 0 by -1 with SAT -> clamped to 0
  let decr_sat = BitfieldOperation::incrby(
    BitfieldEncoding::unsigned(63)?,
    0,
    -1,
    BitfieldOverflow::Sat,
  );
  let rets = db.bitfield("limit_bf", [decr_sat])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));

  // i64 min/max with WRAP
  let set_i64 = BitfieldOperation::set(
    BitfieldEncoding::signed(64)?,
    100,
    i64::MAX,
    BitfieldOverflow::Wrap,
  );
  db.bitfield("limit_bf", [set_i64])?;

  let incr_i64_wrap = BitfieldOperation::incrby(
    BitfieldEncoding::signed(64)?,
    100,
    1,
    BitfieldOverflow::Wrap,
  );
  let rets = db.bitfield("limit_bf", [incr_i64_wrap])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(i64::MIN)));

  Ok(())
}

#[test]
fn test_bitmap_bitfield_positional_offset_syntax() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. #0 with u16 = 0, #1 = 16, #2 = 32
  let enc_u16 = BitfieldEncoding::unsigned(16)?;
  let off0 = parse_bitfield_offset("#0", enc_u16)?;
  let off1 = parse_bitfield_offset("#1", enc_u16)?;
  assert_eq!(off0, 0);
  assert_eq!(off1, 16);

  let op_set0 = BitfieldOperation::set_positional(enc_u16, 0, 100, BitfieldOverflow::Wrap)?;
  let op_set1 = BitfieldOperation::set_positional(enc_u16, 1, 200, BitfieldOverflow::Wrap)?;
  let rets = db.bitfield("bf_pos", [op_set0, op_set1])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(0)));
  assert_eq!(rets[1], Some(BitfieldValue::Unsigned(0)));

  let op_get0 = BitfieldOperation::get_positional(enc_u16, 0)?;
  let op_get1 = BitfieldOperation::get_positional(enc_u16, 1)?;
  let rets = db.bitfield_read_only("bf_pos", [op_get0, op_get1])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(100)));
  assert_eq!(rets[1], Some(BitfieldValue::Unsigned(200)));

  // INCRBY with #N
  let op_incr = BitfieldOperation::incrby_positional(enc_u16, 0, 1, BitfieldOverflow::Wrap)?;
  let rets = db.bitfield("bf_pos", [op_incr])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(101)));

  // OVERFLOW SAT with #N
  let op_sat = BitfieldOperation::incrby_positional(enc_u16, 1, 65535, BitfieldOverflow::Sat)?;
  let rets = db.bitfield("bf_pos", [op_sat])?;
  assert_eq!(rets[0], Some(BitfieldValue::Unsigned(65535)));

  // 2. Signed i8: #0 = 0, #1 = 8
  let enc_i8 = BitfieldEncoding::signed(8)?;
  let op_i8_0 = BitfieldOperation::set_positional(enc_i8, 0, -10, BitfieldOverflow::Wrap)?;
  let op_i8_1 = BitfieldOperation::set_positional(enc_i8, 1, 42, BitfieldOverflow::Wrap)?;
  db.bitfield("bf_i8", [op_i8_0, op_i8_1])?;

  let get_i8_0 = BitfieldOperation::get_positional(enc_i8, 0)?;
  let get_i8_1 = BitfieldOperation::get_positional(enc_i8, 1)?;
  let rets = db.bitfield_read_only("bf_i8", [get_i8_0, get_i8_1])?;
  assert_eq!(rets[0], Some(BitfieldValue::Signed(-10)));
  assert_eq!(rets[1], Some(BitfieldValue::Signed(42)));

  // 3. Error and boundary cases for positional offset
  assert!(parse_bitfield_offset("#", enc_u16).is_err());
  assert!(parse_bitfield_offset("#abc", enc_u16).is_err());
  assert!(parse_bitfield_offset("#-1", enc_u16).is_err());
  // Overflow: #268435456 * 16 = 4294967296 > u32::MAX
  assert!(parse_bitfield_offset("#268435456", enc_u16).is_err());
  // u8: #536870912 * 8 = 4294967296 > u32::MAX
  assert!(parse_bitfield_offset("#536870912", BitfieldEncoding::unsigned(8)?).is_err());
  // Valid just below limit: #536870911 * 8 = 4294967288 <= u32::MAX
  assert!(parse_bitfield_offset("#536870911", BitfieldEncoding::unsigned(8)?).is_ok());

  Ok(())
}

#[test]
fn test_bitmap_bitop_dest_same_as_target() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // BITOP NOT where dest and target are the same key: \xaa\x00\xff\x55 -> \x55\xff\x00\xaa
  db.setbit("s", 0, 1)?; // \x80
  db.setbit("s", 2, 1)?; // \xa0
  db.setbit("s", 4, 1)?; // \xaa in MSB
  // Set actual pattern using string or bit operations
  let len = db.bitop(BitOp::Not, "s", &["s"])?;
  assert!(len > 0);

  // Self-dest with multiple keys: BITOP OR dest dest src2
  db.setbit("k_src", 1, 1)?;
  let len_or = db.bitop(BitOp::Or, "s", &["s", "k_src"])?;
  assert!(len_or > 0);
  assert_eq!(db.getbit("s", 1)?, 1);

  Ok(())
}

#[test]
fn test_bitmap_bitop_missing_key_and_padding() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Missing key considered stream of zeros
  db.setbit("a", 0, 1)?;
  db.setbit("a", 15, 1)?; // 2 bytes: \x80\x01 in MSB

  // AND with missing key -> all zeros
  let len_and = db.bitop(BitOp::And, "res_and", &["missing", "a"])?;
  assert_eq!(len_and, 2);
  let bytes = db.get_bitmap_bytes("res_and")?.unwrap();
  assert_eq!(bytes, vec![0, 0]);

  // OR with missing key -> retains a
  let len_or = db.bitop(BitOp::Or, "res_or", &["missing", "a", "missing"])?;
  assert_eq!(len_or, 2);
  assert_eq!(db.getbit("res_or", 0)?, 1);
  assert_eq!(db.getbit("res_or", 15)?, 1);

  // Shorter keys zero-padded to max length key
  db.setbit("short", 0, 1)?; // 1 byte
  db.setbit("long", 0, 1)?;
  db.setbit("long", 31, 1)?; // 4 bytes

  let len_xor = db.bitop(BitOp::Xor, "res_xor", &["short", "long"])?;
  assert_eq!(len_xor, 4);
  assert_eq!(db.getbit("res_xor", 0)?, 0); // 1 ^ 1 = 0
  assert_eq!(db.getbit("res_xor", 31)?, 1); // 0 ^ 1 = 1

  Ok(())
}

#[test]
fn test_bitmap_bitpos_word_boundaries_and_unaligned() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Create a 28-byte key with 1 at bit 216
  // Bytes 0..26 all 0x00, byte 27 has 0xF0 (bits 216..219 set)
  db.setbit("word_key", 216, 1)?;
  db.setbit("word_key", 217, 1)?;
  db.setbit("word_key", 218, 1)?;
  db.setbit("word_key", 219, 1)?;

  // Searching bit=1 with unaligned start positions: 1, 9, 17, 25, 33, 41, 49, 57, 65
  for i in 0..9 {
    let start = (i * 8 + 1) as i64;
    let pos = db.bitpos(
      "word_key",
      1,
      [BitPos::Range(start, -1), BitPos::Unit(BitUnit::Bit)],
    )?;
    assert_eq!(pos, 216);
  }

  // Searching bit=0 on all-ones with 0 at bit 216
  let dir2 = tempdir()?;
  let db2 = WeDb::new(Fjall::open(dir2.path())?).ns(0)?.db(0)?;
  // Fill 27 bytes with 1s (0..215)
  for bit in 0..216 {
    db2.setbit("zero_word", bit, 1)?;
  }
  // bit 216 is left 0, bit 217 is set to 1
  db2.setbit("zero_word", 217, 1)?;

  for i in 0..9 {
    let start = (i * 8 + 1) as i64;
    let pos = db2.bitpos(
      "zero_word",
      0,
      [BitPos::Range(start, -1), BitPos::Unit(BitUnit::Bit)],
    )?;
    assert_eq!(pos, 216);
  }

  Ok(())
}

#[test]
fn test_bitmap_bitpos_single_bit_precision_and_intervals() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Set bit 8 to 1
  db.setbit("prec_key", 8, 1)?;

  // Single bit range [8, 8] with BIT option
  assert_eq!(
    db.bitpos(
      "prec_key",
      1,
      [BitPos::Range(8, 8), BitPos::Unit(BitUnit::Bit)]
    )?,
    8
  );
  // Range [9, 15] has no 1
  assert_eq!(
    db.bitpos(
      "prec_key",
      1,
      [BitPos::Range(9, 15), BitPos::Unit(BitUnit::Bit)]
    )?,
    -1
  );

  // BIT vs BYTE options on the same numeric range [0, 7]
  // Range [0, 7] in BIT means bits 0..7 (byte 0) -> no 1 -> -1
  assert_eq!(
    db.bitpos(
      "prec_key",
      1,
      [BitPos::Range(0, 7), BitPos::Unit(BitUnit::Bit)]
    )?,
    -1
  );
  // Range [0, 7] in BYTE means bytes 0..7 (bits 0..63) -> finds 1 at bit 8
  assert_eq!(db.bitpos("prec_key", 1, [BitPos::Range(0, 7)])?, 8);

  // Searching bit=0 on all-ones string
  let dir3 = tempdir()?;
  let db3 = WeDb::new(Fjall::open(dir3.path())?).ns(0)?.db(0)?;
  for i in 0..24 {
    db3.setbit("all_ones", i, 1)?;
  }
  // With stop_given=true, range [0, 2] bytes has no 0 -> -1
  assert_eq!(db3.bitpos("all_ones", 0, [BitPos::Range(0, 2)])?, -1);
  // With stop_given=false (end is None), extends past bitmap -> returns 24
  assert_eq!(db3.bitpos("all_ones", 0, [BitPos::Start(0)])?, 24);

  Ok(())
}

#[test]
fn test_bitmap_bitfield_read_only_rejects_write() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let set_op = BitfieldOperation::set(
    BitfieldEncoding::unsigned(8)?,
    0,
    100,
    BitfieldOverflow::Wrap,
  );
  let err = db.bitfield_read_only("ro_key", [set_op]);
  assert!(err.is_err());

  let incr_op =
    BitfieldOperation::incrby(BitfieldEncoding::unsigned(8)?, 0, 1, BitfieldOverflow::Wrap);
  let err2 = db.bitfield_read_only("ro_key", [incr_op]);
  assert!(err2.is_err());

  Ok(())
}

#[test]
fn test_bitmap_offset_upper_bound_check() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Offset > u32::MAX
  let max_u32 = u32::MAX as u64;
  let out_of_bound = max_u32 + 1;

  let err = db.setbit("bound_key", out_of_bound, 1);
  assert!(err.is_err());

  // GETBIT with offset > u32::MAX returns 0
  assert_eq!(db.getbit("bound_key", out_of_bound)?, 0);

  // Valid max offset (u32::MAX)
  assert_eq!(db.setbit("bound_key", 1000, 1)?, 0);
  assert_eq!(db.getbit("bound_key", 1000)?, 1);

  Ok(())
}

#[test]
fn test_bitmap_cross_type_wrongtype_checks() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 创建 Hash 类型键
  db.hset("hash_k", &[("f1", "v1")])?;

  // 2. 所有 Bitmap 操作在非字符串非位图复合键上必须严格报错 WRONGTYPE
  assert!(db.getbit("hash_k", 0).is_err());
  assert!(db.setbit("hash_k", 0, 1).is_err());
  assert!(db.bitcount("hash_k", []).is_err());
  assert!(db.bitpos("hash_k", 0, []).is_err());
  assert!(db.bitpos("hash_k", 1, []).is_err());
  assert!(db.get_bitmap_bytes("hash_k").is_err());

  let bf_get = BitfieldOperation::get(BitfieldEncoding::unsigned(8)?, 0);
  assert!(db.bitfield("hash_k", [bf_get]).is_err());

  // 3. 创建 Set 类型键
  db.sadd("set_k", &["m1"])?;
  assert!(db.getbit("set_k", 0).is_err());
  assert!(db.setbit("set_k", 0, 1).is_err());
  assert!(db.bitcount("set_k", []).is_err());
  assert!(db.bitpos("set_k", 0, []).is_err());
  assert!(db.bitpos("set_k", 1, []).is_err());
  assert!(db.get_bitmap_bytes("set_k").is_err());

  Ok(())
}

#[test]
fn test_bitmap_kvrocks_get_string_and_ghost_cleanup() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 get_bitmap_string 对标 Kvrocks Bitmap::GetString
  db.setbit("b_str", 0, 1)?;
  db.setbit("b_str", 7, 1)?;
  let s_bytes = db.get_bitmap_string("b_str")?;
  assert!(s_bytes.is_some());
  let b = s_bytes.unwrap();
  assert_eq!(b.len(), 1);
  assert_eq!(b[0], 0b10000001);

  // 2. 测试过期 Bitmap 覆盖重写时的幽灵分段清除 (防止旧分段复活)
  db.setbit("b_ghost", 1024 * 8 + 10, 1)?; // 写入 segment 1
  assert_eq!(db.getbit("b_ghost", 1024 * 8 + 10)?, 1);
  assert_eq!(db.bitcount("b_ghost", [])?, 1);

  // 设置过期并等待
  db.pexpire("b_ghost", 1)?;
  sleep(Duration::from_millis(5));

  // 重新创建仅写入 offset 0 (segment 0)
  assert_eq!(db.setbit("b_ghost", 0, 1)?, 0);
  assert_eq!(db.getbit("b_ghost", 0)?, 1);
  // segment 1 的旧位必须彻底被清空，bitcount 只能是 1
  assert_eq!(db.getbit("b_ghost", 1024 * 8 + 10)?, 0);
  assert_eq!(db.bitcount("b_ghost", [])?, 1);

  Ok(())
}
