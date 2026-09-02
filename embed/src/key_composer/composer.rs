use crate::key_composer::{
  oppv::{decode_oppv_u64, encode_oppv_u64, encode_oppv_u64_slice, oppv_len_u64},
  small_key::{INLINE_CAP, SmallKey},
  tag::KeyTag,
};

/// Reusable subkey composer for zero-heap allocation across iterations.
/// 通用子键复用构建器（零堆分配，支持高频循环迭代）
#[derive(Debug, Clone)]
pub struct SubkeyComposer {
  buf: Vec<u8>,
  prefix_len: usize,
}

impl SubkeyComposer {
  #[inline]
  pub fn new(prefix: Vec<u8>) -> Self {
    let prefix_len = prefix.len();
    Self {
      buf: prefix,
      prefix_len,
    }
  }

  #[inline]
  pub fn from_slice(prefix: &[u8]) -> Self {
    let prefix_len = prefix.len();
    let mut buf = Vec::with_capacity(prefix_len + 64);
    buf.extend_from_slice(prefix);
    Self { buf, prefix_len }
  }

  #[inline]
  pub fn compose_sub(&mut self, subkey: &[u8]) -> &[u8] {
    self.buf.truncate(self.prefix_len);
    self.buf.extend_from_slice(subkey);
    &self.buf
  }

  #[inline]
  pub fn compose_sub_u64_be(&mut self, val: u64) -> &[u8] {
    self.buf.truncate(self.prefix_len);
    self.buf.extend_from_slice(&val.to_be_bytes());
    &self.buf
  }

  #[inline]
  pub fn prefix(&self) -> &[u8] {
    &self.buf[..self.prefix_len]
  }
}

/// Unified physical key composer for namespace and database isolation.
/// 统一的物理键编排器（负责命名空间与数据库隔离、复合数据结构 Key 与子键前缀构造，全面支持纯数字 OPPV 变长保序编码与前缀无关分帧）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct KeyComposer {
  ns_id: u64,
  db: u64,
}

impl KeyComposer {
  #[inline]
  pub const fn new(ns_id: u64, db: u64) -> Self {
    Self { ns_id, db }
  }

  #[inline]
  pub const fn new_db(db: u64) -> Self {
    Self { ns_id: 0, db }
  }

  #[inline(always)]
  pub const fn ns_id(&self) -> u64 {
    self.ns_id
  }

  #[inline(always)]
  pub const fn db(&self) -> u64 {
    self.db
  }

  #[inline(always)]
  pub const fn is_default(&self) -> bool {
    self.ns_id == 0 && self.db == 0
  }

  /// Calculates the physical byte length of the scoped prefix.
  /// 计算作用域前缀的物理字节长度（格式：\x00[oppv(ns_id)][oppv(db)]）
  #[inline(always)]
  pub const fn scope_prefix_len(&self) -> usize {
    1 + oppv_len_u64(self.ns_id) + oppv_len_u64(self.db)
  }

  /// Encodes scoped prefix into a fixed-size 19-byte stack buffer without heap allocation.
  /// 栈上定长编码作用域前缀到 19 字节数组（零堆分配，返回写入字节数）
  #[inline(always)]
  pub fn encode_scope_prefix_fixed(&self, buf: &mut [u8; 19]) -> usize {
    buf[0] = 0;
    let len1 = encode_oppv_u64_slice(self.ns_id, &mut buf[1..]);
    let len2 = encode_oppv_u64_slice(self.db, &mut buf[1 + len1..]);
    1 + len1 + len2
  }

  /// Encodes scoped physical prefix into a Vec<u8> buffer.
  /// 编码作用域物理前缀到 Vec<u8>
  #[inline]
  pub fn encode_scope_prefix(&self, buf: &mut Vec<u8>) {
    let mut tmp = [0u8; 19];
    let len = self.encode_scope_prefix_fixed(&mut tmp);
    buf.extend_from_slice(&tmp[..len]);
  }

  /// Encodes scoped physical prefix into a SmallKey with stack allocation.
  /// 编码作用域物理前缀到 SmallKey（栈上零堆分配）
  #[inline]
  pub fn encode_scope_prefix_small(&self, sk: &mut SmallKey) {
    let mut buf = [0u8; 19];
    let len = self.encode_scope_prefix_fixed(&mut buf);
    sk.extend_from_slice(&buf[..len]);
  }

  /// Composes composite metadata storage key (format: `[scope_prefix][tag][key]`).
  /// 构造复合结构元数据存储键（格式：`[scope_prefix][tag][key]`）
  #[inline]
  pub fn compose_meta_key_into(&self, tag: &[u8], key_bytes: &[u8], buf: &mut Vec<u8>) {
    buf.clear();
    if self.is_default() && tag.len() == 1 {
      let total_len = 4 + key_bytes.len();
      buf.reserve(total_len);
      buf.extend_from_slice(&[0, 0, 0, tag[0]]);
      buf.extend_from_slice(key_bytes);
      return;
    }
    buf.reserve(self.scope_prefix_len() + tag.len() + key_bytes.len());
    self.encode_scope_prefix(buf);
    buf.extend_from_slice(tag);
    buf.extend_from_slice(key_bytes);
  }

  #[inline]
  pub fn compose_meta_key_stack(&self, tag: &[u8], key_bytes: &[u8]) -> SmallKey {
    if self.is_default() && tag.len() == 1 {
      let total_len = 4 + key_bytes.len();
      if total_len <= INLINE_CAP {
        let mut buf = [0u8; INLINE_CAP];
        buf[0] = 0;
        buf[1] = 0;
        buf[2] = 0;
        buf[3] = tag[0];
        buf[4..total_len].copy_from_slice(key_bytes);
        return SmallKey::Inline {
          buf,
          len: total_len as u8,
        };
      }
    }
    let prefix_len = self.scope_prefix_len();
    let total_len = prefix_len + tag.len() + key_bytes.len();
    if total_len <= INLINE_CAP {
      let mut buf = [0u8; INLINE_CAP];
      buf[0] = 0;
      let len1 = encode_oppv_u64_slice(self.ns_id, &mut buf[1..]);
      encode_oppv_u64_slice(self.db, &mut buf[1 + len1..]);
      let tag_end = prefix_len + tag.len();
      buf[prefix_len..tag_end].copy_from_slice(tag);
      buf[tag_end..total_len].copy_from_slice(key_bytes);
      SmallKey::Inline {
        buf,
        len: total_len as u8,
      }
    } else {
      let mut v = Vec::with_capacity(total_len);
      self.encode_scope_prefix(&mut v);
      v.extend_from_slice(tag);
      v.extend_from_slice(key_bytes);
      SmallKey::Heap(v)
    }
  }

  /// Composes composite subkey data prefix with preallocated extra capacity.
  /// 构造复合结构子键数据前缀，并额外预留 extra_len 容量（避免二次内存重分配）
  #[inline]
  pub fn compose_prefix_into_with_extra(
    &self,
    tag: &[u8],
    key_bytes: &[u8],
    extra_len: usize,
    buf: &mut Vec<u8>,
  ) {
    buf.clear();
    if self.is_default() && tag.len() == 1 && key_bytes.len() < 128 {
      let total_len = 5 + key_bytes.len() + extra_len;
      buf.reserve(total_len);
      buf.extend_from_slice(&[0, 0, 0, tag[0], key_bytes.len() as u8]);
      buf.extend_from_slice(key_bytes);
      return;
    }
    buf.reserve(
      self.scope_prefix_len()
        + tag.len()
        + oppv_len_u64(key_bytes.len() as u64)
        + key_bytes.len()
        + extra_len,
    );
    self.encode_scope_prefix(buf);
    buf.extend_from_slice(tag);
    encode_oppv_u64(key_bytes.len() as u64, buf);
    buf.extend_from_slice(key_bytes);
  }

  /// Composes composite subkey data prefix with prefix-free OPPV encoding.
  /// 构造复合结构子键数据前缀（前缀无关编码：`[scope_prefix][tag][oppv(len(key))][key]`）
  #[inline]
  pub fn compose_prefix_into(&self, tag: &[u8], key_bytes: &[u8], buf: &mut Vec<u8>) {
    self.compose_prefix_into_with_extra(tag, key_bytes, 0, buf);
  }

  #[inline]
  pub fn compose_prefix(&self, tag: &[u8], key_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    self.compose_prefix_into(tag, key_bytes, &mut buf);
    buf
  }

  #[inline(always)]
  fn try_compose_stack_key(
    &self,
    tag: &[u8],
    key_bytes: &[u8],
    extra_slices: &[&[u8]],
  ) -> SmallKey {
    if self.is_default() && tag.len() == 1 && key_bytes.len() < 128 {
      let mut total_len = 5 + key_bytes.len();
      for s in extra_slices {
        total_len += s.len();
      }
      if total_len <= INLINE_CAP {
        let mut buf = [0u8; INLINE_CAP];
        buf[0] = 0;
        buf[1] = 0;
        buf[2] = 0;
        buf[3] = tag[0];
        buf[4] = key_bytes.len() as u8;
        let mut cursor = 5;
        buf[cursor..cursor + key_bytes.len()].copy_from_slice(key_bytes);
        cursor += key_bytes.len();
        for s in extra_slices {
          buf[cursor..cursor + s.len()].copy_from_slice(s);
          cursor += s.len();
        }
        return SmallKey::Inline {
          buf,
          len: total_len as u8,
        };
      }
    } else {
      let prefix_len = self.scope_prefix_len();
      let key_len_oppv = oppv_len_u64(key_bytes.len() as u64);
      let mut total_len = prefix_len + tag.len() + key_len_oppv + key_bytes.len();
      for s in extra_slices {
        total_len += s.len();
      }
      if total_len <= INLINE_CAP {
        let mut buf = [0u8; INLINE_CAP];
        let mut tmp = [0u8; 19];
        let written_prefix = self.encode_scope_prefix_fixed(&mut tmp);
        buf[..written_prefix].copy_from_slice(&tmp[..written_prefix]);
        let mut cursor = written_prefix;
        buf[cursor..cursor + tag.len()].copy_from_slice(tag);
        cursor += tag.len();
        let oppv_n = encode_oppv_u64_slice(key_bytes.len() as u64, &mut buf[cursor..]);
        cursor += oppv_n;
        buf[cursor..cursor + key_bytes.len()].copy_from_slice(key_bytes);
        cursor += key_bytes.len();
        for s in extra_slices {
          buf[cursor..cursor + s.len()].copy_from_slice(s);
          cursor += s.len();
        }
        return SmallKey::Inline {
          buf,
          len: total_len as u8,
        };
      }
    }

    let mut total_len =
      self.scope_prefix_len() + tag.len() + oppv_len_u64(key_bytes.len() as u64) + key_bytes.len();
    for s in extra_slices {
      total_len += s.len();
    }
    let mut v = Vec::with_capacity(total_len);
    self.encode_scope_prefix(&mut v);
    v.extend_from_slice(tag);
    encode_oppv_u64(key_bytes.len() as u64, &mut v);
    v.extend_from_slice(key_bytes);
    for s in extra_slices {
      v.extend_from_slice(s);
    }
    SmallKey::Heap(v)
  }

  #[inline]
  pub fn compose_prefix_stack(&self, tag: &[u8], key_bytes: &[u8]) -> SmallKey {
    self.try_compose_stack_key(tag, key_bytes, &[])
  }

  #[inline]
  pub fn compose_subkey_stack(&self, tag: &[u8], key_bytes: &[u8], subkey: &[u8]) -> SmallKey {
    self.try_compose_stack_key(tag, key_bytes, &[subkey])
  }

  #[inline]
  pub fn compose_subkey2_stack(
    &self,
    tag: &[u8],
    key_bytes: &[u8],
    sub1: &[u8],
    sub2: &[u8],
  ) -> SmallKey {
    self.try_compose_stack_key(tag, key_bytes, &[sub1, sub2])
  }

  #[inline]
  pub fn compose_oppv_subkey_stack(
    &self,
    tag: &[u8],
    key_bytes: &[u8],
    mid_bytes: &[u8],
    sub: &[u8],
  ) -> SmallKey {
    let prefix_len = self.scope_prefix_len();
    let key_oppv_len = oppv_len_u64(key_bytes.len() as u64);
    let mid_oppv_len = oppv_len_u64(mid_bytes.len() as u64);
    let total_len = prefix_len
      + tag.len()
      + key_oppv_len
      + key_bytes.len()
      + mid_oppv_len
      + mid_bytes.len()
      + sub.len();

    if total_len <= INLINE_CAP {
      let mut buf = [0u8; INLINE_CAP];
      let mut tmp = [0u8; 19];
      let p_len = self.encode_scope_prefix_fixed(&mut tmp);
      buf[..p_len].copy_from_slice(&tmp[..p_len]);
      let mut cursor = p_len;
      buf[cursor..cursor + tag.len()].copy_from_slice(tag);
      cursor += tag.len();
      let n1 = encode_oppv_u64_slice(key_bytes.len() as u64, &mut buf[cursor..]);
      cursor += n1;
      buf[cursor..cursor + key_bytes.len()].copy_from_slice(key_bytes);
      cursor += key_bytes.len();
      let n2 = encode_oppv_u64_slice(mid_bytes.len() as u64, &mut buf[cursor..]);
      cursor += n2;
      buf[cursor..cursor + mid_bytes.len()].copy_from_slice(mid_bytes);
      cursor += mid_bytes.len();
      buf[cursor..cursor + sub.len()].copy_from_slice(sub);
      return SmallKey::Inline {
        buf,
        len: total_len as u8,
      };
    }

    let mut v = Vec::with_capacity(total_len);
    self.encode_scope_prefix(&mut v);
    v.extend_from_slice(tag);
    encode_oppv_u64(key_bytes.len() as u64, &mut v);
    v.extend_from_slice(key_bytes);
    encode_oppv_u64(mid_bytes.len() as u64, &mut v);
    v.extend_from_slice(mid_bytes);
    v.extend_from_slice(sub);
    SmallKey::Heap(v)
  }

  #[inline]
  pub fn compose_meta_prefix_stack(&self, tag: &[u8]) -> SmallKey {
    if self.is_default() && tag.len() == 1 {
      let mut buf = [0u8; INLINE_CAP];
      buf[0] = 0;
      buf[1] = 0;
      buf[2] = 0;
      buf[3] = tag[0];
      return SmallKey::Inline { buf, len: 4 };
    }
    let mut sk = SmallKey::new();
    let mut buf = [0u8; 19];
    let prefix_len = self.encode_scope_prefix_fixed(&mut buf);
    sk.extend_from_slice(&buf[..prefix_len]);
    sk.extend_from_slice(tag);
    sk
  }

  #[inline]
  pub fn compose_meta_prefix(&self, tag: &[u8]) -> Vec<u8> {
    if self.is_default() && tag.len() == 1 {
      return vec![0, 0, 0, tag[0]];
    }
    let mut v = Vec::with_capacity(self.scope_prefix_len() + tag.len());
    self.encode_scope_prefix(&mut v);
    v.extend_from_slice(tag);
    v
  }

  #[inline]
  pub fn namespace_prefix(&self) -> Vec<u8> {
    let mut v = Vec::with_capacity(self.scope_prefix_len());
    self.encode_scope_prefix(&mut v);
    v
  }

  /// Composes namespace prefix on stack without heap allocation.
  /// 栈上零堆分配构造当前命名空间前缀
  #[inline(always)]
  pub fn namespace_prefix_stack(&self) -> SmallKey {
    let mut buf = [0u8; INLINE_CAP];
    let mut tmp = [0u8; 19];
    let n = self.encode_scope_prefix_fixed(&mut tmp);
    buf[..n].copy_from_slice(&tmp[..n]);
    SmallKey::Inline { buf, len: n as u8 }
  }

  /// Encodes global prefix for all databases in a namespace into a 10-byte stack buffer.
  /// 栈上零堆分配编码指定命名空间下所有 DB 的全局前缀到 10 字节数组（返回写入字节数）
  #[inline(always)]
  pub fn encode_ns_prefix_fixed(ns_id: u64, buf: &mut [u8; 10]) -> usize {
    buf[0] = 0;
    let n = encode_oppv_u64_slice(ns_id, &mut buf[1..]);
    1 + n
  }

  /// Composes global prefix for all databases under specified namespace (format: \x00[oppv(ns_id)]).
  /// 构造指定命名空间下所有 DB 的全局前缀（格式：\x00[oppv(ns_id)]）
  #[inline]
  pub fn ns_prefix(ns_id: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(1 + 9);
    v.push(0);
    encode_oppv_u64(ns_id, &mut v);
    v
  }

  // ==================== 作用域反查与提取 ====================

  /// Parses scoped prefix strictly from a physical key slice.
  /// 从物理键中严格解析作用域前缀，返回 `(KeyComposer, prefix_len, remain_slice)`。
  #[inline]
  pub fn parse_scoped_prefix(full_key: &[u8]) -> Option<(Self, usize, &[u8])> {
    if full_key.len() < 3 || full_key[0] != 0 {
      return None;
    }
    let (ns_id, c1) = decode_oppv_u64(&full_key[1..])?;
    let (db, c2) = decode_oppv_u64(full_key.get(1 + c1..)?)?;
    let prefix_len = 1 + c1 + c2;
    if full_key.len() > prefix_len {
      Some((Self::new(ns_id, db), prefix_len, &full_key[prefix_len..]))
    } else {
      None
    }
  }

  /// Checks whether a physical key belongs to the current KeyComposer scope.
  /// 判断物理键是否属于当前 KeyComposer 所在的作用域
  #[inline(always)]
  pub fn is_key_in_ns(&self, full_key: &[u8]) -> bool {
    let mut buf = [0u8; 19];
    let len = self.encode_scope_prefix_fixed(&mut buf);
    let prefix_slice = &buf[..len];
    full_key.starts_with(prefix_slice)
      && full_key.len() > prefix_slice.len()
      && KeyTag::from_u8(full_key[prefix_slice.len()]).is_some()
  }

  /// Extracts user key from physical key with zero allocation.
  /// 高性能零分配提取用户 Key
  #[inline]
  pub fn extract_user_key<'b>(&self, full_key: &'b [u8]) -> Option<&'b [u8]> {
    let mut buf = [0u8; 19];
    let len = self.encode_scope_prefix_fixed(&mut buf);
    let prefix_slice = &buf[..len];
    if !full_key.starts_with(prefix_slice) {
      return None;
    }
    let remain = &full_key[len..];
    if remain.is_empty() {
      return None;
    }

    let tag = KeyTag::from_u8(remain[0])?;

    match tag {
      KeyTag::RawString
      | KeyTag::HashMeta
      | KeyTag::ListMeta
      | KeyTag::SetMeta
      | KeyTag::ZSetMeta
      | KeyTag::BloomMeta
      | KeyTag::CuckooMeta
      | KeyTag::BitmapMeta
      | KeyTag::HllMeta
      | KeyTag::HllRaw
      | KeyTag::JsonMeta
      | KeyTag::SortedIntMeta
      | KeyTag::StreamMeta
      | KeyTag::TDigestMeta
      | KeyTag::TimeSeriesMeta
      | KeyTag::FtSchema
      | KeyTag::FtAlias => Some(&remain[1..]),

      KeyTag::HashData
      | KeyTag::ListData
      | KeyTag::SetData
      | KeyTag::ZSetData
      | KeyTag::ZSetScore
      | KeyTag::BloomData
      | KeyTag::CuckooData
      | KeyTag::BitmapData
      | KeyTag::JsonData
      | KeyTag::SortedIntData
      | KeyTag::StreamData
      | KeyTag::StreamGroup
      | KeyTag::StreamConsumer
      | KeyTag::StreamPel
      | KeyTag::TDigestData
      | KeyTag::TimeSeriesData
      | KeyTag::FtIndex
      | KeyTag::FtData => {
        let (key_len, consumed) = decode_oppv_u64(&remain[1..])?;
        let start = 1 + consumed;
        let end = start.checked_add(key_len as usize)?;
        remain.get(start..end)
      }
    }
  }

  /// Converts an underlying physical key from current scope to target scope.
  /// 将当前作用域的底层键转换为目标作用域的底层键
  #[inline]
  pub fn transform_key_to_target_bytes(
    &self,
    full_key: &[u8],
    target_kc: &KeyComposer,
  ) -> Option<Vec<u8>> {
    let mut buf = [0u8; 19];
    let len = self.encode_scope_prefix_fixed(&mut buf);
    let prefix_slice = &buf[..len];
    if !full_key.starts_with(prefix_slice) {
      return None;
    }
    let rem = &full_key[len..];
    if rem.is_empty() || KeyTag::from_u8(rem[0]).is_none() {
      return None;
    }

    let mut out = Vec::with_capacity(target_kc.scope_prefix_len() + rem.len());
    target_kc.encode_scope_prefix(&mut out);
    out.extend_from_slice(rem);
    Some(out)
  }
}
