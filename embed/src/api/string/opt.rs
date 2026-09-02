/// Key-value pair slice reference aligned with Apache Kvrocks StringPair.
/// 键值对引用结构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringPair<'a> {
  pub key: &'a [u8],
  pub value: &'a [u8],
}

impl<'a> StringPair<'a> {
  #[inline]
  pub const fn new(key: &'a [u8], value: &'a [u8]) -> Self {
    Self { key, value }
  }

  #[inline]
  pub const fn key(&self) -> &'a [u8] {
    self.key
  }

  #[inline]
  pub const fn value(&self) -> &'a [u8] {
    self.value
  }
}

/// Conditional set type for string operations aligned with Apache Kvrocks StringSetType.
/// 字符串设置条件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StringSetType {
  #[default]
  None,
  Nx,
  Xx,
  IfEq,
  IfNe,
  IfDeq,
  IfDne,
}

/// String set operation options aligned with Apache Kvrocks StringSet.
/// 字符串设置参数结构体
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StringSet<'a> {
  pub expire: u64,
  pub set_type: StringSetType,
  pub get: bool,
  pub keep_ttl: bool,
  pub cmp_value: Option<&'a [u8]>,
}

impl<'a> StringSet<'a> {
  /// Returns whether the options qualify for direct fast-path write optimization without preconditions.
  /// 判断是否满足常规快速直写通道条件（无复合条件、无需旧值、无 TTL 继承、无摘要对比）
  #[inline]
  pub const fn is_fast_path(&self) -> bool {
    matches!(self.set_type, StringSetType::None)
      && !self.get
      && !self.keep_ttl
      && self.cmp_value.is_none()
  }
}

/// String SET command options enumeration.
/// 字符串设置选项枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Set<'a> {
  Ex(u64),
  Px(u64),
  ExAt(u64),
  PxAt(u64),
  KeepTtl,
  Nx,
  Xx,
  IfEq(&'a [u8]),
  IfNe(&'a [u8]),
  IfDeq(&'a [u8]),
  IfDne(&'a [u8]),
  Get,
}

impl<'a> Set<'a> {
  /// Parses an iterator of Set options into a structured StringSet parameter object.
  /// 从选项列表解析为完整的 StringSet 参数对象
  pub fn parse_options(options: impl IntoIterator<Item = Set<'a>>, now_ms: u64) -> StringSet<'a> {
    let mut set_type = StringSetType::None;
    let mut get = false;
    let mut keep_ttl = false;
    let mut expire = 0u64;
    let mut cmp_value: Option<&'a [u8]> = None;

    for opt in options {
      match opt {
        Set::Ex(sec) => {
          expire = now_ms.saturating_add(sec.saturating_mul(1000));
          keep_ttl = false;
        }
        Set::Px(ms) => {
          expire = now_ms.saturating_add(ms);
          keep_ttl = false;
        }
        Set::ExAt(sec) => {
          expire = sec.saturating_mul(1000);
          keep_ttl = false;
        }
        Set::PxAt(ms) => {
          expire = ms;
          keep_ttl = false;
        }
        Set::KeepTtl => {
          keep_ttl = true;
          expire = 0;
        }
        Set::Nx => {
          set_type = StringSetType::Nx;
          cmp_value = None;
        }
        Set::Xx => {
          set_type = StringSetType::Xx;
          cmp_value = None;
        }
        Set::IfEq(expected) => {
          set_type = StringSetType::IfEq;
          cmp_value = Some(expected);
        }
        Set::IfNe(expected) => {
          set_type = StringSetType::IfNe;
          cmp_value = Some(expected);
        }
        Set::IfDeq(expected) => {
          set_type = StringSetType::IfDeq;
          cmp_value = Some(expected);
        }
        Set::IfDne(expected) => {
          set_type = StringSetType::IfDne;
          cmp_value = Some(expected);
        }
        Set::Get => get = true,
      }
    }

    StringSet {
      expire,
      set_type,
      get,
      keep_ttl,
      cmp_value,
    }
  }
}

/// GETEX command options enumeration.
/// GETEX 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetEx {
  Ex(u64),
  Px(u64),
  ExAt(u64),
  PxAt(u64),
  Persist,
}

impl GetEx {
  /// Computes absolute expiration timestamp in milliseconds based on current time.
  /// 根据当前时间计算新的绝对过期时间戳（毫秒）
  #[inline]
  pub const fn compute_expire(&self, now_ms: u64) -> u64 {
    match *self {
      Self::Persist => 0,
      Self::Ex(sec) => now_ms.saturating_add(sec.saturating_mul(1000)),
      Self::Px(ms) => now_ms.saturating_add(ms),
      Self::ExAt(sec) => sec.saturating_mul(1000),
      Self::PxAt(ms) => ms,
    }
  }
}

/// DELEX command options enumeration.
/// DELEX 选项枚举
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DelEx<'a> {
  #[default]
  None,
  IfEq(&'a [u8]),
  IfNe(&'a [u8]),
  IfDeq(&'a [u8]),
  IfDne(&'a [u8]),
}

/// String MSET operation options aligned with Apache Kvrocks StringMSet.
/// 字符串批量设置参数结构体
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StringMSet {
  pub expire: u64,
  pub set_type: StringSetType,
  pub keep_ttl: bool,
}

/// Longest Common Subsequence (LCS) output mode aligned with Apache Kvrocks StringLCSType.
/// 最长公共子序列选项类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StringLCSType {
  #[default]
  None,
  Len,
  Idx,
}

/// Longest Common Subsequence (LCS) operation parameters aligned with Apache Kvrocks StringLCSOpt.
/// 最长公共子序列参数结构体
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringLCS {
  pub lcs_type: StringLCSType,
  pub min_match_len: i64,
}

impl Default for StringLCS {
  fn default() -> Self {
    Self {
      lcs_type: StringLCSType::None,
      min_match_len: 0,
    }
  }
}

/// LCS command options enumeration.
/// LCS 选项枚举（统一 *Opt 后缀）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lcs {
  Len,
  Idx,
  WithMatchLen,
  MinMatchLen(i64),
}

impl Lcs {
  pub fn parse_options(options: impl IntoIterator<Item = Lcs>) -> StringLCS {
    let mut lcs_type = StringLCSType::None;
    let mut min_match_len = 0;
    for opt in options {
      match opt {
        Lcs::Len => lcs_type = StringLCSType::Len,
        Lcs::Idx | Lcs::WithMatchLen => lcs_type = StringLCSType::Idx,
        Lcs::MinMatchLen(len) => {
          lcs_type = StringLCSType::Idx;
          min_match_len = len;
        }
      }
    }
    StringLCS {
      lcs_type,
      min_match_len,
    }
  }
}

impl From<&[Lcs]> for StringLCS {
  fn from(options: &[Lcs]) -> Self {
    Lcs::parse_options(options.iter().copied())
  }
}

/// Longest Common Subsequence string sub-range aligned with Apache Kvrocks StringLCSRange.
/// 最长公共子序列字符串子区间
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StringLCSRange {
  pub start: u32,
  pub end: u32,
}

/// Longest Common Subsequence matched range pair aligned with Apache Kvrocks StringLCSMatchedRange.
/// 最长公共子序列匹配区间
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringLCSMatchedRange {
  pub a: StringLCSRange,
  pub b: StringLCSRange,
  pub match_len: u32,
}

impl StringLCSMatchedRange {
  #[inline]
  pub const fn new(a_start: u32, a_end: u32, b_start: u32, b_end: u32, match_len: u32) -> Self {
    Self {
      a: StringLCSRange {
        start: a_start,
        end: a_end,
      },
      b: StringLCSRange {
        start: b_start,
        end: b_end,
      },
      match_len,
    }
  }

  #[inline]
  pub const fn a_start(&self) -> u32 {
    self.a.start
  }

  #[inline]
  pub const fn a_end(&self) -> u32 {
    self.a.end
  }

  #[inline]
  pub const fn b_start(&self) -> u32 {
    self.b.start
  }

  #[inline]
  pub const fn b_end(&self) -> u32 {
    self.b.end
  }
}

/// Longest Common Subsequence index result with matched ranges aligned with Apache Kvrocks StringLCSIdxResult.
/// 最长公共子序列索引结果
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StringLCSIdxResult {
  pub matches: Vec<StringLCSMatchedRange>,
  pub len: u32,
}

/// Longest Common Subsequence combined result enumeration aligned with Apache Kvrocks StringLCSResult.
/// 最长公共子序列综合结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringLCSResult {
  Str(String),
  Len(u32),
  Idx(StringLCSIdxResult),
}
