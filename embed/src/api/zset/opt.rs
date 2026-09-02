use std::str;

use crate::error::{Error, Result};

/// ZADD command option flags (aligned with Apache Kvrocks ZSetFlags).
/// ZADD 选项标志（对标 Apache Kvrocks ZSetFlags）
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString, strum::FromRepr)]
#[strum(ascii_case_insensitive)]
pub enum ZAdd {
  Nx,
  Xx,
  Gt,
  Lt,
  Ch,
  Incr,
}

/// Aggregation function method (aligned with Apache Kvrocks AggregateMethod).
/// 聚合函数类型（对标 Apache Kvrocks AggregateMethod）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, strum::EnumString, strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum Aggregate {
  #[default]
  Sum,
  Min,
  Max,
}

impl Aggregate {
  #[inline]
  pub fn parse(s: &str) -> Self {
    s.parse().unwrap_or_default()
  }

  #[inline]
  pub fn apply(&self, current: f64, new_val: f64) -> f64 {
    let res = match self {
      Self::Sum => current + new_val,
      Self::Min => current.min(new_val),
      Self::Max => current.max(new_val),
    };
    if res.is_nan() { 0.0 } else { res }
  }
}

/// Score range specification (aligned with Apache Kvrocks RangeScore).
/// 分数范围规格（对标 Apache Kvrocks RangeScore）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeScore {
  pub min: f64,
  pub max: f64,
  pub minex: bool,
  pub maxex: bool,
  pub offset: usize,
  pub count: Option<usize>,
}

impl Default for RangeScore {
  #[inline]
  fn default() -> Self {
    Self {
      min: f64::NEG_INFINITY,
      max: f64::INFINITY,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
    }
  }
}

impl RangeScore {
  #[inline]
  pub fn new(min: f64, max: f64) -> Self {
    Self {
      min,
      max,
      ..Default::default()
    }
  }

  #[inline]
  pub fn with_limit(min: f64, max: f64, offset: usize, count: usize) -> Self {
    Self {
      min,
      max,
      minex: false,
      maxex: false,
      offset,
      count: Some(count),
    }
  }

  #[inline]
  pub fn is_empty(&self) -> bool {
    self.min > self.max || (self.min == self.max && (self.minex || self.maxex))
  }

  #[inline]
  pub fn check(&self, score: f64) -> bool {
    let min_ok = if self.minex {
      score > self.min
    } else {
      score >= self.min
    };
    let max_ok = if self.maxex {
      score < self.max
    } else {
      score <= self.max
    };
    min_ok && max_ok
  }

  /// Constructs RangeScore from Redis score boundary string.
  /// 从 Redis 分数边界字符串构造 RangeScore
  pub fn from_bounds(
    min_bound: &str,
    max_bound: &str,
    offset: usize,
    count: Option<usize>,
  ) -> Result<Self> {
    let (min, minex) = Self::parse_bound(min_bound)?;
    let (max, maxex) = Self::parse_bound(max_bound)?;
    Ok(Self {
      min,
      max,
      minex,
      maxex,
      offset,
      count,
    })
  }

  /// Parses Redis-style score boundary string (e.g. "(1.5", "[10", "10", "-inf", "+inf").
  /// 解析 Redis 风格的分数边界字符串（例如 "(1.5", "[10", "10", "-inf", "+inf", "(-inf" 等）
  pub fn parse_bound(s: &str) -> Result<(f64, bool)> {
    let s = s.trim();
    if s.is_empty() {
      return Err(Error::invalid_data("ERR min or max is not a float"));
    }

    let (val_str, is_exclusive) = if let Some(rest) = s.strip_prefix('(') {
      (rest.trim(), true)
    } else if let Some(rest) = s.strip_prefix('[') {
      (rest.trim(), false)
    } else {
      (s, false)
    };

    if val_str.eq_ignore_ascii_case("-inf") || val_str.eq_ignore_ascii_case("-infinity") {
      return Ok((f64::NEG_INFINITY, is_exclusive));
    }
    if val_str.eq_ignore_ascii_case("+inf")
      || val_str.eq_ignore_ascii_case("+infinity")
      || val_str.eq_ignore_ascii_case("inf")
      || val_str.eq_ignore_ascii_case("infinity")
    {
      return Ok((f64::INFINITY, is_exclusive));
    }

    let val = val_str
      .parse::<f64>()
      .map_err(|_| Error::invalid_data("ERR min or max is not a float"))?;
    if val.is_nan() {
      return Err(Error::invalid_data("ERR min or max is not a float"));
    }
    Ok((val, is_exclusive))
  }
}

/// Lexicographical range specification (aligned with Apache Kvrocks RangeLex).
/// 字典序范围规格（对标 Apache Kvrocks RangeLex，统一用于 ZSet + Hash）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RangeLex {
  pub min: Vec<u8>,
  pub max: Vec<u8>,
  pub minex: bool,
  pub maxex: bool,
  pub min_infinite: bool,
  pub max_infinite: bool,
  pub offset: usize,
  pub count: Option<usize>,
  pub reversed: bool,
}

impl RangeLex {
  #[inline]
  pub fn new(min: impl Into<Vec<u8>>, max: impl Into<Vec<u8>>) -> Self {
    Self {
      min: min.into(),
      max: max.into(),
      minex: false,
      maxex: false,
      min_infinite: false,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }

  #[inline]
  pub fn unbounded() -> Self {
    Self {
      min_infinite: true,
      max_infinite: true,
      ..Default::default()
    }
  }

  #[inline]
  pub fn is_empty(&self) -> bool {
    !self.min_infinite
      && !self.max_infinite
      && (self.min > self.max || (self.min == self.max && (self.minex || self.maxex)))
  }

  #[inline]
  pub fn check(&self, member: &[u8]) -> bool {
    let min_ok = if self.min_infinite {
      true
    } else if self.minex {
      member > self.min.as_slice()
    } else {
      member >= self.min.as_slice()
    };

    let max_ok = if self.max_infinite {
      true
    } else if self.maxex {
      member < self.max.as_slice()
    } else {
      member <= self.max.as_slice()
    };

    min_ok && max_ok
  }

  /// Constructs RangeLex from Redis boundary string with strict validation.
  /// 从 Redis 边界字符串或字节切片构造 RangeLex（严格校验 min 为 -/(/[，max 为 +/(/[）
  pub fn from_bounds(
    min_bound: &[u8],
    max_bound: &[u8],
    offset: usize,
    count: Option<usize>,
  ) -> Result<Self> {
    let (min, minex, min_infinite) = Self::parse_min_bound(min_bound)?;
    let (max, maxex, max_infinite) = Self::parse_max_bound(max_bound)?;
    Ok(Self {
      min,
      max,
      minex,
      maxex,
      min_infinite,
      max_infinite,
      offset,
      count,
      reversed: false,
    })
  }

  /// Parses Redis lexicographical lower bound (valid min is "-" or starts with '(' / '[').
  /// 解析 Redis 字典序下界（合法的 min 为 "-" 或以 '(' / '[' 开头）
  pub fn parse_min_bound(bound: &[u8]) -> Result<(Vec<u8>, bool, bool)> {
    if bound == b"-" {
      return Ok((Vec::new(), false, true));
    }
    if bound == b"+" {
      return Err(Error::invalid_data(
        "ERR min or max not valid string range item",
      ));
    }
    if let Some(rest) = bound.strip_prefix(b"(") {
      Ok((rest.to_vec(), true, false))
    } else if let Some(rest) = bound.strip_prefix(b"[") {
      Ok((rest.to_vec(), false, false))
    } else {
      Err(Error::invalid_data(
        "ERR min or max not valid string range item",
      ))
    }
  }

  /// Parses Redis lexicographical upper bound (valid max is "+" or starts with '(' / '[').
  /// 解析 Redis 字典序上界（合法的 max 为 "+" 或以 '(' / '[' 开头）
  pub fn parse_max_bound(bound: &[u8]) -> Result<(Vec<u8>, bool, bool)> {
    if bound == b"+" {
      return Ok((Vec::new(), false, true));
    }
    if bound == b"-" {
      return Err(Error::invalid_data(
        "ERR min or max not valid string range item",
      ));
    }
    if let Some(rest) = bound.strip_prefix(b"(") {
      Ok((rest.to_vec(), true, false))
    } else if let Some(rest) = bound.strip_prefix(b"[") {
      Ok((rest.to_vec(), false, false))
    } else {
      Err(Error::invalid_data(
        "ERR min or max not valid string range item",
      ))
    }
  }

  /// Parses generic Redis lexicographical boundary (supports "-", "+", "(abc", "[abc").
  /// 通用解析 Redis 字典序边界（支持 "-", "+", "(abc", "[abc"）
  pub fn parse_bound(bound: &[u8]) -> Result<(Vec<u8>, bool, bool)> {
    if bound == b"-" {
      return Ok((Vec::new(), false, true));
    }
    if bound == b"+" {
      return Ok((Vec::new(), false, true));
    }
    if let Some(rest) = bound.strip_prefix(b"(") {
      Ok((rest.to_vec(), true, false))
    } else if let Some(rest) = bound.strip_prefix(b"[") {
      Ok((rest.to_vec(), false, false))
    } else {
      Err(Error::invalid_data(
        "ERR min or max not valid string range item",
      ))
    }
  }
}

/// Rank range specification (aligned with Apache Kvrocks RangeRank).
/// 排名范围规格（对标 Apache Kvrocks RangeRank）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RangeRank {
  pub start: i64,
  pub stop: i64,
  pub reversed: bool,
}

impl RangeRank {
  #[inline]
  pub fn new(start: i64, stop: i64) -> Self {
    Self {
      start,
      stop,
      reversed: false,
    }
  }

  #[inline]
  pub fn rev(start: i64, stop: i64) -> Self {
    Self {
      start,
      stop,
      reversed: true,
    }
  }
}

/// ZRANGE command options enumeration.
/// ZRANGE 选项枚举（统一 *Opt 后缀）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZRange {
  ByScore,
  ByLex,
  Rev,
  WithScores,
  Limit(usize, usize),
}

use std::ops::{Bound, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

pub trait IntoRangeScore {
  fn into_range_score(self) -> RangeScore;
}

impl IntoRangeScore for RangeScore {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    self
  }
}

impl IntoRangeScore for &RangeScore {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    *self
  }
}

impl IntoRangeScore for (f64, f64) {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: self.0,
      max: self.1,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for Range<f64> {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: self.start,
      max: self.end,
      minex: false,
      maxex: true,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for RangeInclusive<f64> {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: *self.start(),
      max: *self.end(),
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for RangeFrom<f64> {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: self.start,
      max: f64::INFINITY,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for RangeTo<f64> {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: f64::NEG_INFINITY,
      max: self.end,
      minex: false,
      maxex: true,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for RangeToInclusive<f64> {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: f64::NEG_INFINITY,
      max: self.end,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for RangeFull {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    RangeScore {
      min: f64::NEG_INFINITY,
      max: f64::INFINITY,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
    }
  }
}

impl IntoRangeScore for (Bound<f64>, Bound<f64>) {
  #[inline]
  fn into_range_score(self) -> RangeScore {
    let (min, minex) = match self.0 {
      Bound::Included(v) => (v, false),
      Bound::Excluded(v) => (v, true),
      Bound::Unbounded => (f64::NEG_INFINITY, false),
    };
    let (max, maxex) = match self.1 {
      Bound::Included(v) => (v, false),
      Bound::Excluded(v) => (v, true),
      Bound::Unbounded => (f64::INFINITY, false),
    };
    RangeScore {
      min,
      max,
      minex,
      maxex,
      offset: 0,
      count: None,
    }
  }
}

pub trait IntoRangeLex {
  fn into_range_lex(self) -> RangeLex;
}

impl IntoRangeLex for RangeLex {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    self
  }
}

impl IntoRangeLex for &RangeLex {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    self.clone()
  }
}

impl IntoRangeLex for (&[u8], &[u8]) {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: self.0.to_vec(),
      max: self.1.to_vec(),
      minex: false,
      maxex: false,
      min_infinite: false,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoRangeLex for (&str, &str) {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: self.0.as_bytes().to_vec(),
      max: self.1.as_bytes().to_vec(),
      minex: false,
      maxex: false,
      min_infinite: false,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoRangeLex for (Vec<u8>, Vec<u8>) {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: self.0,
      max: self.1,
      minex: false,
      maxex: false,
      min_infinite: false,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl<B: AsRef<[u8]>> IntoRangeLex for Range<B> {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: self.start.as_ref().to_vec(),
      max: self.end.as_ref().to_vec(),
      minex: false,
      maxex: true,
      min_infinite: false,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl<B: AsRef<[u8]>> IntoRangeLex for RangeInclusive<B> {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    let (start, end) = self.into_inner();
    RangeLex {
      min: start.as_ref().to_vec(),
      max: end.as_ref().to_vec(),
      minex: false,
      maxex: false,
      min_infinite: false,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl<B: AsRef<[u8]>> IntoRangeLex for RangeFrom<B> {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: self.start.as_ref().to_vec(),
      max: Vec::new(),
      minex: false,
      maxex: false,
      min_infinite: false,
      max_infinite: true,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl<B: AsRef<[u8]>> IntoRangeLex for RangeTo<B> {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: Vec::new(),
      max: self.end.as_ref().to_vec(),
      minex: false,
      maxex: true,
      min_infinite: true,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl<B: AsRef<[u8]>> IntoRangeLex for RangeToInclusive<B> {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: Vec::new(),
      max: self.end.as_ref().to_vec(),
      minex: false,
      maxex: false,
      min_infinite: true,
      max_infinite: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoRangeLex for RangeFull {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    RangeLex {
      min: Vec::new(),
      max: Vec::new(),
      minex: false,
      maxex: false,
      min_infinite: true,
      max_infinite: true,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl<B: AsRef<[u8]>> IntoRangeLex for (Bound<B>, Bound<B>) {
  #[inline]
  fn into_range_lex(self) -> RangeLex {
    let (min, minex, min_infinite) = match self.0 {
      Bound::Included(v) => (v.as_ref().to_vec(), false, false),
      Bound::Excluded(v) => (v.as_ref().to_vec(), true, false),
      Bound::Unbounded => (Vec::new(), false, true),
    };
    let (max, maxex, max_infinite) = match self.1 {
      Bound::Included(v) => (v.as_ref().to_vec(), false, false),
      Bound::Excluded(v) => (v.as_ref().to_vec(), true, false),
      Bound::Unbounded => (Vec::new(), false, true),
    };
    RangeLex {
      min,
      max,
      minex,
      maxex,
      min_infinite,
      max_infinite,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

pub trait IntoRangeRank {
  fn into_range_rank(self) -> RangeRank;
}

impl IntoRangeRank for RangeRank {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    self
  }
}

impl IntoRangeRank for &RangeRank {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    *self
  }
}

impl IntoRangeRank for (i64, i64) {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: self.0,
      stop: self.1,
      reversed: false,
    }
  }
}

impl IntoRangeRank for Range<i64> {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: self.start,
      stop: self.end.saturating_sub(1),
      reversed: false,
    }
  }
}

impl IntoRangeRank for RangeInclusive<i64> {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: *self.start(),
      stop: *self.end(),
      reversed: false,
    }
  }
}

impl IntoRangeRank for RangeFrom<i64> {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: self.start,
      stop: -1,
      reversed: false,
    }
  }
}

impl IntoRangeRank for RangeTo<i64> {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: 0,
      stop: self.end.saturating_sub(1),
      reversed: false,
    }
  }
}

impl IntoRangeRank for RangeToInclusive<i64> {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: 0,
      stop: self.end,
      reversed: false,
    }
  }
}

impl IntoRangeRank for RangeFull {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: 0,
      stop: -1,
      reversed: false,
    }
  }
}

impl IntoRangeRank for &(i64, i64) {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    RangeRank {
      start: self.0,
      stop: self.1,
      reversed: false,
    }
  }
}

impl IntoRangeRank for (Bound<i64>, Bound<i64>) {
  #[inline]
  fn into_range_rank(self) -> RangeRank {
    let start = match self.0 {
      Bound::Included(v) => v,
      Bound::Excluded(v) => v.saturating_add(1),
      Bound::Unbounded => 0,
    };
    let stop = match self.1 {
      Bound::Included(v) => v,
      Bound::Excluded(v) => v.saturating_sub(1),
      Bound::Unbounded => -1,
    };
    RangeRank {
      start,
      stop,
      reversed: false,
    }
  }
}
