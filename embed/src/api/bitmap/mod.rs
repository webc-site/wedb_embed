pub mod bitops;
pub mod r#const;
pub mod field;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod op;
pub mod opt;
pub mod pos;
pub use bitops::{
  BITMAP_SEGMENT_BITS, BITMAP_SEGMENT_BYTES, bit_op_exec, bit_op_exec_into,
  expand_bitmap_segment, find_bit_in_byte_lsb, find_bit_in_byte_msb,
  get_bit_from_bytes, get_bit_lsb, normalize_bit_range_to_byte_mask, normalize_range,
  normalize_to_byte_range_with_padding_mask, raw_bitpos, raw_bitpos_lsb, raw_popcount,
  segment_byte_offset_for_bit, segment_index_for_bit, set_bit_in_bytes, set_bit_lsb,
  string_bitcount, string_bitpos,
};
pub use field::{
  ArrayBitfieldBitmap, bitfield_op_calc, signed_bitfield_plus, unsigned_bitfield_plus,
};
pub use r#const::*;
pub use key::{
  meta as compose_bitmap_meta_key, prefix as compose_bitmap_prefix,
  segment as compose_bitmap_segment,
};
pub use meta::BitmapMeta;
pub use opt::{
  BitCount, BitOp, BitPos, BitUnit, BitfieldEncoding, BitfieldOpType, BitfieldOperation,
  BitfieldOverflow, BitfieldValue, parse_bitfield_offset,
};
