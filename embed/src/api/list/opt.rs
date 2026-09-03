/// LPOS command options enumeration.
/// LPOS 选项枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LPos {
  Rank(i64),
  Count(usize),
  MaxLen(usize),
}
