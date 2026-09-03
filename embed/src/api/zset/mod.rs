pub mod r#const;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod opt;
pub mod pop;
pub mod range;
pub mod rank;
pub mod scan;
pub mod score;
pub mod set_ops;

pub use r#const::ERR_WRONG_TYPE;
pub use r#impl::prepare_zset_meta_for_write;
pub use key::{
  member as compose_zset_key, meta as compose_zset_meta_key, prefix as compose_zset_prefix,
  prefix_stack as compose_zset_prefix_stack, score as compose_zset_score_key,
  score_from_bytes as compose_zset_score_from_bytes_key, score_prefix as compose_zset_score_prefix,
  score_prefix_stack as compose_zset_score_prefix_stack,
};
pub use meta::{ZSetMeta, decode_sortable_f64, encode_sortable_f64};
pub use opt::{
  Aggregate, IntoRangeLex, IntoRangeRank, IntoRangeScore, RangeLex, RangeRank, RangeScore, ZAdd,
  ZRange,
};
pub type ZSetMemberScore = (Vec<u8>, f64);
pub type ZSetKeyMemberScore = (Vec<u8>, Vec<u8>, f64);
pub type ZScanResult = (u64, Vec<ZSetMemberScore>);
pub type ZSetPopResult = (Vec<u8>, Vec<ZSetMemberScore>);
pub type ZSetScanByMemberResult = (Option<Vec<u8>>, Vec<ZSetMemberScore>);
