use std::ops::{Deref, DerefMut};

pub use crate::api::hash::key::{
  ItemKeyComposer as HashItemKeyComposer, field as compose_hash_key, meta as compose_hash_meta_key,
  prefix as compose_hash_prefix, prefix_stack as compose_hash_prefix_stack,
};
use crate::{
  error::{Error, Result},
  hash::opt::HExpire,
  key_composer::{KeyTag, SmallKey},
  meta::{KeyMeta, MetaOps, RedisType, generate_version},
};

/// Encodes data into binary format.
/// 哈希子键编码模式（对标 Apache Kvrocks HashSubkeyEncodingMode）
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, bitcode::Encode, bitcode::Decode, strum::FromRepr,
)]
#[repr(u8)]
pub enum HashSubkeyEncodingMode {
  #[default]
  Legacy = 0,
  FieldExpiration = 1,
}

/// Domain operation (aligned with Apache Kvrocks HashFieldStateKind).
/// 哈希字段内部状态类别（对标 Apache Kvrocks HashFieldStateKind）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HashFieldStateKind {
  #[default]
  Missing,
  Persistent,
  LiveTTL,
  ExpiredTTLPhysical,
}

/// Domain operation (aligned with Apache Kvrocks HashFieldState).
/// 哈希字段状态（对标 Apache Kvrocks HashFieldState）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashFieldState<'a> {
  pub kind: HashFieldStateKind,
  pub expire: u64,
  pub value: &'a [u8],
}

/// Domain operation (aligned with Apache Kvrocks HExpireConditionPasses).
/// 校验 HEXPIRE 条件是否满足（对标 Apache Kvrocks HExpireConditionPasses）
#[inline]
pub fn hexpire_condition_passes(
  condition: HExpire,
  kind: HashFieldStateKind,
  current_expire_at: u64,
  target_expire_at: u64,
) -> bool {
  match kind {
    HashFieldStateKind::Missing | HashFieldStateKind::ExpiredTTLPhysical => false,
    HashFieldStateKind::Persistent => match condition {
      HExpire::None | HExpire::Nx | HExpire::Lt => true,
      HExpire::Xx | HExpire::Gt => false,
    },
    HashFieldStateKind::LiveTTL => match condition {
      HExpire::None | HExpire::Xx => true,
      HExpire::Nx => false,
      HExpire::Gt => target_expire_at > current_expire_at,
      HExpire::Lt => target_expire_at < current_expire_at,
    },
  }
}

/// Decodes data from binary format.
/// 解码字段状态（对标 Apache Kvrocks DecodeFieldState）
#[inline]
pub fn decode_field_state<'a>(
  meta: &HashMeta,
  raw_value: &'a [u8],
  now_ms: u64,
) -> Option<HashFieldState<'a>> {
  let (expire, value) = meta.decode_subkey_value(raw_value)?;
  let kind = if expire == 0 {
    HashFieldStateKind::Persistent
  } else if is_field_expired(expire, now_ms) {
    HashFieldStateKind::ExpiredTTLPhysical
  } else {
    HashFieldStateKind::LiveTTL
  };
  Some(HashFieldState {
    kind,
    expire,
    value,
  })
}

/// Structure metadata.
/// 哈希结构元数据（对标 Apache Kvrocks HashMetadata 51字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct HashMeta {
  pub base: KeyMeta,
  pub mode: HashSubkeyEncodingMode,
  pub persist: u64,
  pub lower: u64,
  pub upper: u64,
}

impl Deref for HashMeta {
  type Target = KeyMeta;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.base
  }
}

impl DerefMut for HashMeta {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.base
  }
}

impl HashMeta {
  pub const FIELD_EXPIRATION_PREFIX_SIZE: usize = 8;
  pub const ENCODED_SIZE: usize = KeyMeta::ENCODED_SIZE + 1 + 8 + 8 + 8; // 26 + 25 = 51

  #[inline]
  pub fn new(expire_at: u64, version: u64, size: u64) -> Self {
    Self {
      base: KeyMeta::new(RedisType::Hash, expire_at, version, size),
      mode: HashSubkeyEncodingMode::FieldExpiration,
      persist: size,
      lower: 0,
      upper: 0,
    }
  }

  #[inline]
  pub fn new_with_version(expire_at: u64, size: u64) -> Self {
    Self {
      base: KeyMeta::new(RedisType::Hash, expire_at, generate_version(), size),
      mode: HashSubkeyEncodingMode::FieldExpiration,
      persist: size,
      lower: 0,
      upper: 0,
    }
  }

  #[inline]
  pub fn new_with_mode(
    mode: HashSubkeyEncodingMode,
    expire_at: u64,
    version: u64,
    size: u64,
  ) -> Self {
    Self {
      base: KeyMeta::new(RedisType::Hash, expire_at, version, size),
      mode,
      persist: size,
      lower: 0,
      upper: 0,
    }
  }

  #[inline]
  pub fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  pub fn is_legacy_subkey_encoding(&self) -> bool {
    self.mode == HashSubkeyEncodingMode::Legacy
  }

  #[inline]
  pub fn is_field_expiration_encoding(&self) -> bool {
    self.mode == HashSubkeyEncodingMode::FieldExpiration
  }

  /// Domain operation (aligned with Kvrocks ValidateHashFieldExpirationMetadata).
  /// 校验元数据一致性（对标 Kvrocks ValidateHashFieldExpirationMetadata）
  #[inline]
  pub fn validate_metadata(&self) -> Result<()> {
    if self.persist > self.base.size {
      return Err(Error::invalid_data(
        "invalid hash field expiration metadata: persist exceeds size",
      ));
    }
    Ok(())
  }

  /// Operation definition.
  /// 校验从 Missing 新增字段的合法性
  #[inline]
  pub fn validate_missing_field_transition(&self) -> Result<()> {
    self.validate_metadata()?;
    if self.base.size == u64::MAX {
      return Err(Error::invalid_data(
        "invalid hash field expiration metadata: size overflow",
      ));
    }
    Ok(())
  }

  /// Operation definition.
  /// 校验 Persistent 字段状态转移的合法性
  #[inline]
  pub fn validate_persistent_field_transition(&self) -> Result<()> {
    self.validate_metadata()?;
    if self.base.size == 0 || self.persist == 0 {
      return Err(Error::invalid_data(
        "invalid hash field expiration metadata: no persistent field to update",
      ));
    }
    Ok(())
  }

  /// Operation definition.
  /// 校验 TTL 字段状态转移的合法性
  #[inline]
  pub fn validate_ttl_field_transition(&self) -> Result<()> {
    self.validate_metadata()?;
    if self.base.size == 0 || self.persist == self.base.size {
      return Err(Error::invalid_data(
        "invalid hash field expiration metadata: no TTL field to update",
      ));
    }
    Ok(())
  }

  /// Encodes data into binary format.
  /// 编码子键字段值（对标 Kvrocks HashMetadata::EncodeSubkeyValue）
  #[inline]
  pub fn encode_subkey_value(&self, value: &[u8], expire_at_ms: u64) -> Vec<u8> {
    if self.is_legacy_subkey_encoding() {
      value.to_vec()
    } else {
      encode_hash_value(value, expire_at_ms)
    }
  }

  /// Encodes subkey field value in-place without heap allocation using a stack buffer for small values.
  /// 零堆分配栈内联编码子键字段值（小字段直接使用栈缓冲）
  #[inline(always)]
  pub fn with_encoded_subkey_value<R>(
    &self,
    value: &[u8],
    expire_at_ms: u64,
    f: impl FnOnce(&[u8]) -> R,
  ) -> R {
    if self.is_legacy_subkey_encoding() {
      f(value)
    } else if value.len() <= 56 {
      let mut buf = [0u8; 64];
      buf[..Self::FIELD_EXPIRATION_PREFIX_SIZE].copy_from_slice(&expire_at_ms.to_be_bytes());
      buf[Self::FIELD_EXPIRATION_PREFIX_SIZE..Self::FIELD_EXPIRATION_PREFIX_SIZE + value.len()]
        .copy_from_slice(value);
      f(&buf[..Self::FIELD_EXPIRATION_PREFIX_SIZE + value.len()])
    } else {
      f(&encode_hash_value(value, expire_at_ms))
    }
  }

  /// Decodes data from binary format.
  /// 解码子键字段值（对标 Kvrocks HashMetadata::DecodeSubkeyValue）
  #[inline]
  pub fn decode_subkey_value<'a>(&self, raw: &'a [u8]) -> Option<(u64, &'a [u8])> {
    if self.is_legacy_subkey_encoding() {
      Some((0, raw))
    } else if raw.len() < Self::FIELD_EXPIRATION_PREFIX_SIZE {
      None
    } else {
      Some(decode_hash_value(raw))
    }
  }

  /// Decodes live (unexpired) subkey field value.
  /// 解码未过期的有效子键字段值（已过期返回 None）
  #[inline]
  pub fn decode_live_subkey_value<'a>(
    &self,
    raw: &'a [u8],
    now_ms: u64,
  ) -> Option<(u64, &'a [u8])> {
    let (exp, payload) = self.decode_subkey_value(raw)?;
    if is_field_expired(exp, now_ms) {
      None
    } else {
      Some((exp, payload))
    }
  }

  // ================= 状态转移与过期上下界维护（对标 Apache Kvrocks redis_hash.cc） =================

  #[inline]
  pub fn clear_bounds_if_no_ttl_candidates(&mut self) {
    if self.is_field_expiration_encoding() && self.base.size == self.persist {
      self.lower = 0;
      self.upper = 0;
    }
  }

  #[inline]
  pub fn expand_expire_bounds(&mut self, expire_at: u64) {
    if !self.is_field_expiration_encoding() || expire_at == 0 {
      return;
    }
    if self.base.size == self.persist {
      self.lower = expire_at;
      self.upper = expire_at;
      return;
    }
    if self.lower == 0 || expire_at < self.lower {
      self.lower = expire_at;
    }
    self.upper = self.upper.max(expire_at);
  }

  #[inline]
  pub fn apply_missing_to_persistent(&mut self) {
    self.base.size = self.base.size.saturating_add(1);
    if self.is_field_expiration_encoding() {
      self.persist = self.persist.saturating_add(1);
      self.clear_bounds_if_no_ttl_candidates();
    }
  }

  #[inline]
  pub fn apply_missing_to_ttl(&mut self, expire_at: u64) {
    self.expand_expire_bounds(expire_at);
    self.base.size = self.base.size.saturating_add(1);
  }

  #[inline]
  pub fn apply_persistent_to_ttl(&mut self, expire_at: u64) {
    self.expand_expire_bounds(expire_at);
    self.persist = self.persist.saturating_sub(1);
  }

  #[inline]
  pub fn apply_ttl_to_ttl(&mut self, expire_at: u64) {
    self.expand_expire_bounds(expire_at);
  }

  #[inline]
  pub fn apply_ttl_to_persistent(&mut self) {
    self.persist = self.persist.saturating_add(1).min(self.base.size);
    self.clear_bounds_if_no_ttl_candidates();
  }

  #[inline]
  pub fn apply_persistent_to_deleted(&mut self) {
    self.base.size = self.base.size.saturating_sub(1);
    self.persist = self.persist.saturating_sub(1);
    self.clear_bounds_if_no_ttl_candidates();
  }

  #[inline]
  pub fn apply_ttl_to_deleted(&mut self) {
    self.base.size = self.base.size.saturating_sub(1);
    if self.persist > self.base.size {
      self.persist = self.base.size;
    }
    self.clear_bounds_if_no_ttl_candidates();
  }

  /// Encodes data into binary format.
  /// 编码 Hash 元数据到固定大小栈缓冲区
  #[inline]
  pub fn encode_fixed(&self) -> ([u8; Self::ENCODED_SIZE], usize) {
    let mut buf = [0u8; Self::ENCODED_SIZE];
    let base_bytes = self.base.encode();
    buf[..KeyMeta::ENCODED_SIZE].copy_from_slice(&base_bytes);
    if self.is_legacy_subkey_encoding() {
      (buf, KeyMeta::ENCODED_SIZE)
    } else {
      buf[KeyMeta::ENCODED_SIZE] = self.mode as u8;
      buf[KeyMeta::ENCODED_SIZE + 1..KeyMeta::ENCODED_SIZE + 9]
        .copy_from_slice(&self.persist.to_be_bytes());
      buf[KeyMeta::ENCODED_SIZE + 9..KeyMeta::ENCODED_SIZE + 17]
        .copy_from_slice(&self.lower.to_be_bytes());
      buf[KeyMeta::ENCODED_SIZE + 17..KeyMeta::ENCODED_SIZE + 25]
        .copy_from_slice(&self.upper.to_be_bytes());
      (buf, Self::ENCODED_SIZE)
    }
  }

  /// Encodes data into binary format.
  /// 编码 Hash 元数据 (51 字节，栈上零堆分配)
  #[inline]
  pub fn encode(&self) -> SmallKey {
    let (buf, len) = self.encode_fixed();
    SmallKey::from_slice(&buf[..len])
  }

  /// Decodes data from binary format.
  /// 解码 Hash 元数据（支持 Legacy 26/25 字节与 FieldExpiration 51/50 字节自适应解码）
  #[inline]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE {
      return None;
    }
    let base = KeyMeta::decode(bytes)?;
    let base_len = if bytes.len() >= KeyMeta::ENCODED_SIZE && bytes[0] <= 14 {
      KeyMeta::ENCODED_SIZE
    } else {
      KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE
    };

    if bytes.len() <= base_len {
      return Some(Self {
        base,
        mode: HashSubkeyEncodingMode::Legacy,
        persist: base.size,
        lower: 0,
        upper: 0,
      });
    }

    let remain = &bytes[base_len..];
    if remain.len() < 1 + 8 + 8 + 8 {
      return Some(Self {
        base,
        mode: HashSubkeyEncodingMode::Legacy,
        persist: base.size,
        lower: 0,
        upper: 0,
      });
    }

    let mode = match remain[0] {
      1 => HashSubkeyEncodingMode::FieldExpiration,
      _ => HashSubkeyEncodingMode::Legacy,
    };

    let persist_bytes: [u8; 8] = remain[1..9].try_into().ok()?;
    let persist = u64::from_be_bytes(persist_bytes);

    let lower_bytes: [u8; 8] = remain[9..17].try_into().ok()?;
    let lower = u64::from_be_bytes(lower_bytes);

    let upper_bytes: [u8; 8] = remain[17..25].try_into().ok()?;
    let upper = u64::from_be_bytes(upper_bytes);

    Some(Self {
      base,
      mode,
      persist,
      lower,
      upper,
    })
  }
}

/// Operation definition.
/// 字段过期时间前缀长度（8 字节毫秒时间戳，等同于 `HashMeta::FIELD_EXPIRATION_PREFIX_SIZE`）
pub const FIELD_EXPIRE_PREFIX_LEN: usize = HashMeta::FIELD_EXPIRATION_PREFIX_SIZE;

/// Encodes data into binary format.
/// 编码哈希字段值（带或不带过期时间）
#[inline]
pub fn encode_hash_value(val: &[u8], expire_at_ms: u64) -> Vec<u8> {
  let mut buf = Vec::with_capacity(FIELD_EXPIRE_PREFIX_LEN + val.len());
  encode_hash_value_into(val, expire_at_ms, &mut buf);
  buf
}

/// Encodes hash field value into an existing Vec buffer without reallocations.
/// 编码哈希字段值到已有 Vec 缓冲区（零多余堆分配）
#[inline]
pub fn encode_hash_value_into(val: &[u8], expire_at_ms: u64, out: &mut Vec<u8>) {
  out.clear();
  out.reserve(FIELD_EXPIRE_PREFIX_LEN + val.len());
  out.extend_from_slice(&expire_at_ms.to_be_bytes());
  out.extend_from_slice(val);
}

/// Decodes data from binary format.
/// 解码哈希字段值：返回 (expire_at_ms, payload_slice)
#[inline(always)]
pub fn decode_hash_value(bytes: &[u8]) -> (u64, &[u8]) {
  if bytes.len() >= FIELD_EXPIRE_PREFIX_LEN
    && let Ok(exp_bytes) = bytes[..FIELD_EXPIRE_PREFIX_LEN].try_into()
  {
    let expire_at = u64::from_be_bytes(exp_bytes);
    (expire_at, &bytes[FIELD_EXPIRE_PREFIX_LEN..])
  } else {
    (0, bytes)
  }
}

/// Decodes live (unexpired) hash field payload.
/// 解码未过期的有效哈希字段载荷（已过期返回 None）
#[inline(always)]
pub fn decode_live_hash_value(bytes: &[u8], now_ms: u64) -> Option<&[u8]> {
  let (expire_at, payload) = decode_hash_value(bytes);
  if is_field_expired(expire_at, now_ms) {
    None
  } else {
    Some(payload)
  }
}

/// Domain operation (aligned with Kvrocks IsFieldExpired).
/// 检查字段是否过期（对标 Kvrocks IsFieldExpired）
#[inline(always)]
pub fn is_field_expired(expire_at: u64, now_ms: u64) -> bool {
  expire_at > 0 && expire_at <= now_ms
}

/// Domain operation (aligned with Kvrocks IsImmediateExpire).
/// 检查是否立即过期（对标 Kvrocks IsImmediateExpire）
#[inline(always)]
pub fn is_immediate_expire(expire_at: u64, now_ms: u64) -> bool {
  expire_at <= now_ms
}

impl MetaOps for HashMeta {
  const TAG: &[u8] = KeyTag::HashMeta.as_slice();
  type EncodedBytes = SmallKey;

  #[inline]
  fn decode(bytes: &[u8]) -> Option<Self> {
    Self::decode(bytes)
  }

  #[inline]
  fn is_expired(&self, now_ms: u64) -> bool {
    self.base.is_expired(now_ms)
  }

  #[inline]
  fn encode_bytes(&self) -> Self::EncodedBytes {
    self.encode()
  }

  #[inline]
  fn base(&self) -> &KeyMeta {
    &self.base
  }

  #[inline]
  fn base_mut(&mut self) -> &mut KeyMeta {
    &mut self.base
  }
}
