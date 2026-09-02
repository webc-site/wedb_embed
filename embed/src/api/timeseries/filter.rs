use std::{borrow::Borrow, ops::Deref};

/// Time-series timestamp filter backed by a sorted, deduplicated flat slice.
/// 基于有序、去重扁平连续切片的时序时间戳过滤器。
///
/// 核心特性：
/// 1. 极致紧凑：采用 `Box<[u64]>` 连续物理内存存储，零冗余指针与元数据，硬件预取友好。
/// 2. 严格单调：构建时自动完成升序排序与去重，内部维持严格单调递增不变量。
/// 3. 双指针线性求交：与时序数据单调性无缝协同，实现 $O(M + N)$ 就地单次遍历过滤。
/// 4. Chunk 快速剪枝：支持 $O(1)$ 极值越界快速判定与 $O(\log N)$ 范围命中测试，跳过无关数据块的读取与解压。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TsFilter {
  pub(crate) timestamps: Box<[u64]>,
}

impl TsFilter {
  /// 创建空时间戳过滤器
  #[inline]
  pub fn empty() -> Self {
    Self {
      timestamps: Box::default(),
    }
  }

  /// 创建时间戳过滤器，自动进行原地排序与去重
  #[inline]
  pub fn new(mut ts: Vec<u64>) -> Self {
    ts.sort_unstable();
    ts.dedup();
    Self {
      timestamps: ts.into_boxed_slice(),
    }
  }

  /// 从切片创建时间戳过滤器
  #[inline]
  pub fn from_slice(ts: &[u64]) -> Self {
    let mut v = ts.to_vec();
    v.sort_unstable();
    v.dedup();
    Self {
      timestamps: v.into_boxed_slice(),
    }
  }

  /// 过滤器是否为空
  #[inline]
  pub fn is_empty(&self) -> bool {
    self.timestamps.is_empty()
  }

  /// 包含的去重后时间戳数量
  #[inline]
  pub fn len(&self) -> usize {
    self.timestamps.len()
  }

  /// 暴露底层连续切片
  #[inline]
  pub fn as_slice(&self) -> &[u64] {
    &self.timestamps
  }

  /// 单点二分判断是否存在指定时间戳
  #[inline]
  pub fn contains(&self, ts: u64) -> bool {
    self.timestamps.binary_search(&ts).is_ok()
  }

  /// 检查闭区间 `[start, end]` 内是否存在目标时间戳（用于存储引擎 Chunk 极速剪枝）
  #[inline]
  pub fn matches_range(&self, start: u64, end: u64) -> bool {
    if start > end {
      return false;
    }
    if self.timestamps.is_empty() {
      return true;
    }
    let len = self.timestamps.len();
    // 安全：已确保 self.timestamps 非空
    let min_ts = unsafe { *self.timestamps.get_unchecked(0) };
    let max_ts = unsafe { *self.timestamps.get_unchecked(len - 1) };

    // O(1) 快速范围排斥判定
    if start > max_ts || end < min_ts {
      return false;
    }
    // O(1) 快速全包含判定
    if start <= min_ts && end >= max_ts {
      return true;
    }

    let idx = self.timestamps.partition_point(|&ts| ts < start);
    idx < len && unsafe { *self.timestamps.get_unchecked(idx) } <= end
  }

  /// 将查询区间 `[start_ts, end_ts]` 收缩至过滤器实际包含的时间戳极值边界内
  ///
  /// 若过滤集合非空且与查询区间完全无交集，直接返回 `None`，调用方可完全跳过底层存储 I/O 与解压
  #[inline]
  pub fn clamp_range(&self, start_ts: u64, end_ts: u64) -> Option<(u64, u64)> {
    if start_ts > end_ts {
      return None;
    }
    if self.timestamps.is_empty() {
      return Some((start_ts, end_ts));
    }
    let len = self.timestamps.len();
    let min_ts = unsafe { *self.timestamps.get_unchecked(0) };
    let max_ts = unsafe { *self.timestamps.get_unchecked(len - 1) };

    // 极值快速排斥：查询范围完全在过滤器时间戳外侧
    if start_ts > max_ts || end_ts < min_ts {
      return None;
    }

    // 全包含极值快速收缩
    if start_ts <= min_ts && end_ts >= max_ts {
      return Some((min_ts, max_ts));
    }

    let start_idx = self.timestamps.partition_point(|&ts| ts < start_ts);
    if start_idx >= len {
      return None;
    }
    let first_ts = unsafe { *self.timestamps.get_unchecked(start_idx) };
    if first_ts > end_ts {
      return None;
    }

    let end_idx = self.timestamps.partition_point(|&ts| ts <= end_ts);
    let last_ts = unsafe { *self.timestamps.get_unchecked(end_idx - 1) };
    Some((first_ts, last_ts))
  }

  /// 对单调递增的时序样本进行双指针 $O(M + N)$ 极速就地过滤
  ///
  /// 单次遍历同时完成时间戳与数值范围过滤，无额外内存分配
  pub fn filter_samples(&self, samples: &mut Vec<(u64, f64)>, filter_by_value: Option<(f64, f64)>) {
    if samples.is_empty() {
      return;
    }

    match (self.timestamps.is_empty(), filter_by_value) {
      (true, None) => {}
      (true, Some((min_v, max_v))) => {
        samples.retain(|&(_, v)| v >= min_v && v <= max_v);
      }
      (false, val_filter) => {
        let ts_slice = &self.timestamps;
        let len = ts_slice.len();
        let mut f_idx = 0;
        let mut write_idx = 0;
        let total = samples.len();

        for read_idx in 0..total {
          let (ts, v) = unsafe { *samples.get_unchecked(read_idx) };
          while f_idx < len && unsafe { *ts_slice.get_unchecked(f_idx) } < ts {
            f_idx += 1;
          }
          if f_idx >= len {
            break;
          }
          if unsafe { *ts_slice.get_unchecked(f_idx) } == ts {
            if let Some((min_v, max_v)) = val_filter
              && (v < min_v || v > max_v)
            {
              continue;
            }
            if write_idx != read_idx {
              unsafe {
                *samples.get_unchecked_mut(write_idx) = (ts, v);
              }
            }
            write_idx += 1;
          }
        }
        samples.truncate(write_idx);
      }
    }
  }
}

impl Deref for TsFilter {
  type Target = [u64];

  #[inline]
  fn deref(&self) -> &Self::Target {
    &self.timestamps
  }
}

impl AsRef<[u64]> for TsFilter {
  #[inline]
  fn as_ref(&self) -> &[u64] {
    &self.timestamps
  }
}

impl Borrow<[u64]> for TsFilter {
  #[inline]
  fn borrow(&self) -> &[u64] {
    &self.timestamps
  }
}

impl From<Vec<u64>> for TsFilter {
  #[inline]
  fn from(v: Vec<u64>) -> Self {
    Self::new(v)
  }
}

impl From<&[u64]> for TsFilter {
  #[inline]
  fn from(s: &[u64]) -> Self {
    Self::from_slice(s)
  }
}

impl<const N: usize> From<[u64; N]> for TsFilter {
  #[inline]
  fn from(arr: [u64; N]) -> Self {
    Self::new(arr.to_vec())
  }
}

impl FromIterator<u64> for TsFilter {
  #[inline]
  fn from_iter<T: IntoIterator<Item = u64>>(iter: T) -> Self {
    Self::new(iter.into_iter().collect())
  }
}

/// 单个标签的匹配规则枚举，彻底消除哨兵值 Hack
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelMatcher {
  /// 标签不得存在（对应 key=）
  MustNotExist,
  /// 标签必须存在（对应 key!=）
  MustExist,
  /// 标签必须存在且其值在集合中（对应 key=val 或 key=(v1, v2)）
  In(Box<[String]>),
  /// 标签若存在则其值不得在集合中（对应 key!=val 或 key!=(v1, v2)）
  NotIn(Box<[String]>),
}

impl LabelMatcher {
  #[inline]
  pub fn matches(&self, actual: Option<&str>) -> bool {
    match self {
      Self::MustNotExist => actual.is_none(),
      Self::MustExist => actual.is_some(),
      Self::In(vals) => match actual {
        Some(v) => {
          if vals.len() <= 4 {
            vals.iter().any(|x| x.as_str() == v)
          } else {
            vals.binary_search_by(|x| x.as_str().cmp(v)).is_ok()
          }
        }
        None => false,
      },
      Self::NotIn(vals) => match actual {
        Some(v) => {
          if vals.len() <= 4 {
            !vals.iter().any(|x| x.as_str() == v)
          } else {
            vals.binary_search_by(|x| x.as_str().cmp(v)).is_err()
          }
        }
        None => true,
      },
    }
  }
}

/// Domain operation (aligned with Apache Kvrocks TSMQueryFilterParser).
/// 时序标签过滤器（对标 Apache Kvrocks TSMQueryFilterParser）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimeSeriesLabelFilter {
  pub matchers: Vec<(String, LabelMatcher)>,
}

impl TimeSeriesLabelFilter {
  #[inline]
  pub fn new() -> Self {
    Self::default()
  }

  /// 过滤器是否为空
  #[inline]
  pub fn is_empty(&self) -> bool {
    self.matchers.is_empty()
  }

  /// 包含的过滤规则数量
  #[inline]
  pub fn len(&self) -> usize {
    self.matchers.len()
  }

  /// 解析标签过滤表达式集合（支持泛型迭代器，零不必要转换）
  pub fn parse<S: AsRef<str>>(filters: impl IntoIterator<Item = S>) -> Self {
    let mut filter = Self::new();
    for f in filters {
      filter.add_filter(f.as_ref());
    }
    filter
  }

  /// 解析有序且去重的值列表切片
  fn parse_values(value_str: &str) -> Box<[String]> {
    let mut vals = Vec::new();
    if value_str.starts_with('(') && value_str.ends_with(')') && value_str.len() >= 2 {
      for item in Self::split_value_list(&value_str[1..value_str.len() - 1]) {
        let unquoted = Self::unquote(item);
        vals.push(unquoted.to_string());
      }
    } else if !value_str.is_empty() {
      vals.push(Self::unquote(value_str).to_string());
    }
    vals.sort_unstable();
    vals.dedup();
    vals.into_boxed_slice()
  }

  /// 添加单条标签过滤表达式
  pub fn add_filter(&mut self, expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
      return false;
    }

    let (op_pos, is_not_equal) = Self::find_operator(trimmed);
    if op_pos == usize::MAX {
      return false;
    }

    let label = trimmed[..op_pos].trim().to_string();
    let value_str = if is_not_equal {
      trimmed[op_pos + 2..].trim()
    } else {
      trimmed[op_pos + 1..].trim()
    };

    let matcher = if is_not_equal {
      if value_str.is_empty() {
        LabelMatcher::MustExist
      } else {
        LabelMatcher::NotIn(Self::parse_values(value_str))
      }
    } else if value_str.is_empty() {
      LabelMatcher::MustNotExist
    } else {
      LabelMatcher::In(Self::parse_values(value_str))
    };

    self.matchers.push((label, matcher));
    true
  }

  /// 有限状态机高效查找未被引号包裹的操作符
  fn find_operator(expr: &str) -> (usize, bool) {
    let mut quote = None;
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
      let b = bytes[i];
      match quote {
        Some(_) if b == b'\\' => {
          i += 1;
        }
        Some(q) if q == b => quote = None,
        None if b == b'\'' || b == b'"' => quote = Some(b),
        None => {
          if b == b'!' && bytes.get(i + 1) == Some(&b'=') {
            return (i, true);
          }
          if b == b'=' {
            return (i, false);
          }
        }
        _ => {}
      }
      i += 1;
    }
    (usize::MAX, false)
  }

  /// 状态机分割逗号分隔的值列表（考虑括号嵌套与引号包裹）
  fn split_value_list(list: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut quote = None;
    let mut depth = 0;
    let mut start = 0;
    let bytes = list.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
      let b = bytes[i];
      match quote {
        Some(_) if b == b'\\' => {
          i += 1;
        }
        Some(q) if q == b => quote = None,
        None if b == b'\'' || b == b'"' => quote = Some(b),
        None => match b {
          b'(' => depth += 1,
          b')' if depth > 0 => depth -= 1,
          b',' if depth == 0 => {
            let val = list[start..i].trim();
            if !val.is_empty() {
              values.push(val);
            }
            start = i + 1;
          }
          _ => {}
        },
        _ => {}
      }
      i += 1;
    }
    if start < list.len() {
      let val = list[start..].trim();
      if !val.is_empty() {
        values.push(val);
      }
    }
    values
  }

  /// 消除字符串两端的成对单双引号
  #[inline]
  fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2
      && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
      &s[1..s.len() - 1]
    } else {
      s
    }
  }

  /// 匹配标签键值对切片（支持泛型键值类型，完全零堆分配）
  pub fn matches<K: AsRef<str>, V: AsRef<str>>(&self, meta_labels: &[(K, V)]) -> bool {
    if self.matchers.is_empty() {
      return true;
    }

    for (k, matcher) in &self.matchers {
      let actual = meta_labels
        .iter()
        .find(|(lk, _)| lk.as_ref() == k.as_str())
        .map(|(_, lv)| lv.as_ref());
      if !matcher.matches(actual) {
        return false;
      }
    }

    true
  }
}

impl<S: AsRef<str>> FromIterator<S> for TimeSeriesLabelFilter {
  #[inline]
  fn from_iter<T: IntoIterator<Item = S>>(iter: T) -> Self {
    Self::parse(iter)
  }
}
