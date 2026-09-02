/// Domain operation (aligned with Apache Kvrocks HashFieldExpireCondition).
/// HEXPIRE / HPEXPIRE 条件选项（对标 Apache Kvrocks HashFieldExpireCondition）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, strum::EnumString, strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum HExpire {
  #[default]
  None,
  Nx,
  Xx,
  Gt,
  Lt,
}

/// Domain operation (aligned with Apache Kvrocks HashFieldSetCondition).
/// 字段设置条件（对标 Apache Kvrocks HashFieldSetCondition）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, strum::EnumString, strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum HashFieldSetCondition {
  #[default]
  None,
  Fnx,
  Fxx,
}

/// Domain operation (aligned with Apache Kvrocks HashSetExOpt::TTLAction / HashGetEx::TTLAction).
/// TTL 动作类型（对标 Apache Kvrocks HashSetExOpt::TTLAction / HashGetEx::TTLAction）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, strum::EnumString, strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
pub enum TTLAction {
  #[default]
  Discard,
  Keep,
  Set,
  Persist,
}

/// HSET command options enumeration.
/// HSET 选项枚举（统一 *Opt 后缀）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HSet {
  Ex(u64),
  Px(u64),
  ExAt(u64),
  PxAt(u64),
  KeepTtl,
  Fnx,
  Fxx,
}

/// Domain operation (aligned with Apache Kvrocks HashSetExOpt).
/// HSETEX 选项（对标 Apache Kvrocks HashSetExOpt）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HashSetEx {
  pub condition: HashFieldSetCondition,
  pub ttl_action: TTLAction,
  pub expire_at_ms: u64,
}

impl HashSetEx {
  #[inline]
  pub const fn new(
    condition: HashFieldSetCondition,
    ttl_action: TTLAction,
    expire_at_ms: u64,
  ) -> Self {
    Self {
      condition,
      ttl_action,
      expire_at_ms,
    }
  }

  /// Composes storage key or prefix.
  /// 从选项列表构造 HashSetEx
  pub fn from_options(options: impl IntoIterator<Item = HSet>, now_ms: u64) -> Self {
    let mut opts = Self::default();
    for flag in options {
      match flag {
        HSet::Ex(sec) => {
          opts.ttl_action = TTLAction::Set;
          opts.expire_at_ms = now_ms.saturating_add(sec.saturating_mul(1000));
        }
        HSet::Px(ms) => {
          opts.ttl_action = TTLAction::Set;
          opts.expire_at_ms = now_ms.saturating_add(ms);
        }
        HSet::ExAt(sec) => {
          opts.ttl_action = TTLAction::Set;
          opts.expire_at_ms = sec.saturating_mul(1000);
        }
        HSet::PxAt(ms) => {
          opts.ttl_action = TTLAction::Set;
          opts.expire_at_ms = ms;
        }
        HSet::KeepTtl => {
          opts.ttl_action = TTLAction::Keep;
        }
        HSet::Fnx => {
          opts.condition = HashFieldSetCondition::Fnx;
        }
        HSet::Fxx => {
          opts.condition = HashFieldSetCondition::Fxx;
        }
      }
    }
    opts
  }

  /// Composes storage key or prefix.
  /// 从便捷标志数组构造 HashSetEx
  pub fn from_flags(flags: &[HSet], now_ms: u64) -> Self {
    Self::from_options(flags.iter().copied(), now_ms)
  }
}

/// HGETEX command options enumeration.
/// HGETEX 选项枚举（统一 *Opt 后缀）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HGetEx {
  Ex(u64),
  Px(u64),
  ExAt(u64),
  PxAt(u64),
  Persist,
}

/// Domain operation (aligned with Apache Kvrocks HashGetEx).
/// HGETEX 选项（对标 Apache Kvrocks HashGetEx）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HashGetEx {
  pub ttl_action: TTLAction,
  pub expire_at_ms: u64,
}

impl HashGetEx {
  #[inline]
  pub const fn new(ttl_action: TTLAction, expire_at_ms: u64) -> Self {
    Self {
      ttl_action,
      expire_at_ms,
    }
  }

  #[inline]
  pub const fn persist() -> Self {
    Self {
      ttl_action: TTLAction::Persist,
      expire_at_ms: 0,
    }
  }

  /// Composes storage key or prefix.
  /// 从选项列表构造 HashGetEx
  pub fn from_options(options: impl IntoIterator<Item = HGetEx>, now_ms: u64) -> Self {
    let mut opt_iter = options.into_iter();
    if let Some(flag) = opt_iter.next() {
      match flag {
        HGetEx::Ex(sec) => Self::new(
          TTLAction::Set,
          now_ms.saturating_add(sec.saturating_mul(1000)),
        ),
        HGetEx::Px(ms) => Self::new(TTLAction::Set, now_ms.saturating_add(ms)),
        HGetEx::ExAt(sec) => Self::new(TTLAction::Set, sec.saturating_mul(1000)),
        HGetEx::PxAt(ms) => Self::new(TTLAction::Set, ms),
        HGetEx::Persist => Self::persist(),
      }
    } else {
      Self::default()
    }
  }

  /// Composes storage key or prefix.
  /// 从单个便捷标志构造 HashGetEx
  pub fn from_flag(flag: HGetEx, now_ms: u64) -> Self {
    Self::from_options([flag], now_ms)
  }
}

/// Hash length calculation mode (aligned with Kvrocks HashLengthMode).
/// 哈希长度计算模式（对标 Apache Kvrocks HashLengthMode）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, strum::EnumString, strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum HashLengthMode {
  #[default]
  Accurate = 0,
  Approximate = 1,
}

/// Re-exports RangeLex from zset for lexical range queries.
/// 从 zset 统一导入 RangeLex（消除重复定义）
pub use crate::zset::opt::RangeLex;

/// Field-value pair structure (aligned with Kvrocks FieldValue).
/// 字段与值结构对（对标 Apache Kvrocks FieldValue）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldValue<F = Vec<u8>, V = Vec<u8>> {
  pub field: F,
  pub value: V,
}

impl<F, V> FieldValue<F, V> {
  #[inline]
  pub const fn new(field: F, value: V) -> Self {
    Self { field, value }
  }
}
