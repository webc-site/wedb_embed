/// Expiration condition options for EXPIRE/PEXPIRE/EXPIREAT/PEXPIREAT (aligned with Redis 7.0+ / Kvrocks).
/// 过期时间设置条件选项（对标 Redis 7.0+ 与 Apache Kvrocks）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpireCondition {
  /// Always update expiration (default).
  /// 无条件更新（默认行为）
  #[default]
  None,
  /// Set expiry only when the key has no expiry.
  /// 仅当键当前未设置过期时间时才设置
  NX,
  /// Set expiry only when the key has an existing expiry.
  /// 仅当键当前已有过期时间时才更新
  XX,
  /// Set expiry only when the new expiry is greater than current expiry.
  /// 仅当新过期时间大于当前过期时间时才更新（当前无过期时间则失败）
  GT,
  /// Set expiry only when the new expiry is less than current expiry.
  /// 仅当新过期时间小于当前过期时间时才更新（当前无过期时间视为无限大，允许设置）
  LT,
}

impl ExpireCondition {
  #[inline(always)]
  pub const fn should_update(self, current_exp: u64, new_exp: u64) -> bool {
    match self {
      Self::None => true,
      Self::NX => current_exp == 0,
      Self::XX => current_exp > 0,
      Self::GT => current_exp > 0 && new_exp > current_exp,
      Self::LT => current_exp == 0 || new_exp < current_exp,
    }
  }
}
