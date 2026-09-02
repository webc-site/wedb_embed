pub mod r#const;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod opt;

pub use r#const::ERR_WRONG_TYPE;
pub use r#impl::prepare_zset_meta_for_write;
pub use key::{
  member as compose_zset_key, meta as compose_zset_meta_key, prefix as compose_zset_prefix,
  score as compose_zset_score_key, score_prefix as compose_zset_score_prefix,
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

use std::ops::Bound;

use crate::{error::Result, meta::parse_redis_float};

/// Parses range boundary string (supports (+inf, (-inf, [val, (val).
/// 解析范围边界（支持 (+inf, (-inf, [val, (val）
pub fn parse_score_bound(s: &str) -> Result<Bound<f64>> {
  let s = s.trim();
  if s == "+inf" {
    return Ok(Bound::Unbounded);
  }
  if s == "-inf" {
    return Ok(Bound::Unbounded);
  }
  if let Some(rest) = s.strip_prefix('(') {
    let val = parse_redis_float(rest.as_bytes(), "ERR min or max is not a float")?;
    Ok(Bound::Excluded(val))
  } else if let Some(rest) = s.strip_prefix('[') {
    let val = parse_redis_float(rest.as_bytes(), "ERR min or max is not a float")?;
    Ok(Bound::Included(val))
  } else {
    let val = parse_redis_float(s.as_bytes(), "ERR min or max is not a float")?;
    Ok(Bound::Included(val))
  }
}
