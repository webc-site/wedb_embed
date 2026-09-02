use crate::{
  api::sortedint::r#const::{ERR_MAX_NOT_INT, ERR_MIN_GT_MAX, ERR_MIN_NOT_INT},
  error::{Error, Result},
};

/// 64-bit sorted integer range specification aligned with Apache Kvrocks SortedintRangeSpec.
/// 64 位有序整型集合范围查询规则（对标 Apache Kvrocks SortedintRangeSpec）
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct SortedintRange {
  pub min: u64,
  pub max: u64,
  pub minex: bool,
  pub maxex: bool,
  pub offset: usize,
  pub count: Option<usize>,
  pub reversed: bool,
}

impl Default for SortedintRange {
  #[inline]
  fn default() -> Self {
    Self {
      min: u64::MIN,
      max: u64::MAX,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl SortedintRange {
  /// Creates a full range specification covering [0, u64::MAX].
  /// 创建全区间范围规则 [0, u64::MAX]
  #[inline]
  pub const fn all() -> Self {
    Self {
      min: u64::MIN,
      max: u64::MAX,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }

  /// Sets pagination offset.
  /// 设置分页偏移量
  #[inline]
  pub const fn with_offset(mut self, offset: usize) -> Self {
    self.offset = offset;
    self
  }

  /// Sets maximum return limit.
  /// 设置最大返回数量
  #[inline]
  pub const fn with_count(mut self, count: usize) -> Self {
    self.count = Some(count);
    self
  }

  /// Sets whether to scan in reverse order.
  /// 设置是否逆序
  #[inline]
  pub const fn with_reversed(mut self, reversed: bool) -> Self {
    self.reversed = reversed;
    self
  }

  /// Sets lower bound with open/closed interval specification.
  /// 设置下界及开闭区间
  #[inline]
  pub const fn with_min(mut self, min: u64, minex: bool) -> Self {
    self.min = min;
    self.minex = minex;
    self
  }

  /// Sets upper bound with open/closed interval specification.
  /// 设置上界及开闭区间
  #[inline]
  pub const fn with_max(mut self, max: u64, maxex: bool) -> Self {
    self.max = max;
    self.maxex = maxex;
    self
  }

  /// Checks whether range interval is empty (e.g. min > max or min == max with open bound).
  /// 检查范围区间是否为空（如 min > max，或 min == max 且存在开区间）
  #[inline]
  pub const fn is_empty_range(&self) -> bool {
    if self.min > self.max {
      return true;
    }
    if self.min == self.max && (self.minex || self.maxex) {
      return true;
    }
    if self.minex && self.min == u64::MAX {
      return true;
    }
    if self.maxex && self.max == 0 {
      return true;
    }
    false
  }

  /// Determines whether a given value falls within the range interval.
  /// 判断指定值是否落在该范围区间内
  #[inline]
  pub const fn contains(&self, val: u64) -> bool {
    if self.minex {
      if val <= self.min {
        return false;
      }
    } else if val < self.min {
      return false;
    }

    if self.maxex {
      if val >= self.max {
        return false;
      }
    } else if val > self.max {
      return false;
    }

    true
  }
}

/// Parses single-side boundary supporting '(' open, '[' closed, '+' prefix, or raw number.
/// 解析单侧边界值（支持 '(' 开区间、'[' 闭区间、'+' 号前缀及无前缀数字）
#[inline]
fn parse_bound(s: &str, is_min: bool) -> Result<(u64, bool)> {
  let (num_str, ex) = if let Some(stripped) = s.strip_prefix('(') {
    (stripped, true)
  } else if let Some(stripped) = s.strip_prefix('[') {
    (stripped, false)
  } else {
    (s, false)
  };
  let num_str = num_str.strip_prefix('+').unwrap_or(num_str);
  let val = num_str.parse::<u64>().map_err(|_| {
    if is_min {
      Error::redis(ERR_MIN_NOT_INT)
    } else {
      Error::redis(ERR_MAX_NOT_INT)
    }
  })?;
  Ok((val, ex))
}

/// Parses 64-bit unsigned integer range specification aligned with Kvrocks Sortedint::ParseRangeSpec.
/// 解析 64 位无符号整型范围规则（对标 Apache Kvrocks Sortedint::ParseRangeSpec）
pub fn parse_range_spec(min_str: &str, max_str: &str) -> Result<SortedintRange> {
  let min_str = min_str.trim();
  let max_str = max_str.trim();

  if min_str == "+inf" || max_str == "-inf" {
    return Err(Error::redis(ERR_MIN_GT_MAX));
  }

  let (min, minex) = if min_str == "-inf" {
    (u64::MIN, false)
  } else {
    parse_bound(min_str, true)?
  };

  let (max, maxex) = if max_str == "+inf" {
    (u64::MAX, false)
  } else {
    parse_bound(max_str, false)?
  };

  Ok(SortedintRange {
    min,
    max,
    minex,
    maxex,
    offset: 0,
    count: None,
    reversed: false,
  })
}

/// Encodes data into binary format.
/// 64 位无符号整数编码为 8 字节大端序原生二进制数组
#[inline(always)]
pub const fn encode_be_u64(val: u64) -> [u8; 8] {
  val.to_be_bytes()
}

/// Decodes data from binary format.
/// 8 字节大端序原生二进制解码为 64 位无符号整数
#[inline(always)]
pub const fn decode_be_u64(bytes: &[u8]) -> Option<u64> {
  if bytes.len() < 8 {
    return None;
  }
  let buf = [
    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
  ];
  Some(u64::from_be_bytes(buf))
}

use std::ops::{Bound, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

pub trait IntoSortedintRange {
  fn into_sortedint_range(self) -> SortedintRange;
}

impl IntoSortedintRange for SortedintRange {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    self
  }
}

impl IntoSortedintRange for &SortedintRange {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    self.clone()
  }
}

impl IntoSortedintRange for (u64, u64) {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: self.0,
      max: self.1,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for Range<u64> {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: self.start,
      max: self.end,
      minex: false,
      maxex: true,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for RangeInclusive<u64> {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: *self.start(),
      max: *self.end(),
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for RangeFrom<u64> {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: self.start,
      max: u64::MAX,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for RangeTo<u64> {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: u64::MIN,
      max: self.end,
      minex: false,
      maxex: true,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for RangeToInclusive<u64> {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: u64::MIN,
      max: self.end,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for RangeFull {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    SortedintRange {
      min: u64::MIN,
      max: u64::MAX,
      minex: false,
      maxex: false,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}

impl IntoSortedintRange for (Bound<u64>, Bound<u64>) {
  #[inline]
  fn into_sortedint_range(self) -> SortedintRange {
    let (min, minex) = match self.0 {
      Bound::Included(v) => (v, false),
      Bound::Excluded(v) => (v, true),
      Bound::Unbounded => (u64::MIN, false),
    };
    let (max, maxex) = match self.1 {
      Bound::Included(v) => (v, false),
      Bound::Excluded(v) => (v, true),
      Bound::Unbounded => (u64::MAX, false),
    };
    SortedintRange {
      min,
      max,
      minex,
      maxex,
      offset: 0,
      count: None,
      reversed: false,
    }
  }
}
