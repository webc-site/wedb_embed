use crate::{
  IntoIndexRange,
  api::string::{
    MAX_STRING_SIZE, compute_lcs_with, get_string_raw, key,
    meta::{
      STRING_HDR_SIZE, STRING_NO_EXPIRY_HEADER, encode_string_header, with_encoded_string_value,
    },
    opt::{Lcs, StringLCSResult},
    r#const::{ERR_OFFSET_OUT_OF_RANGE, ERR_STRING_EXCEEDS_MAX_SIZE},
    string_digest,
  },
  engine::{Engine, Partition},
  error::{Error, Result},
  key::cleanup_all_composite_data,
  meta::normalize_range,
  wedb::Db,
};

/// Substring, range, append, and atomic CAS/CAD operations (GETRANGE, SETRANGE, APPEND, STRLEN, CAS, CAD, LCS).
/// 子区间切片、追加、CAS/CAD 与 LCS 字符串操作接口实现
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn strlen<K: AsRef<[u8]>>(&self, key: K) -> Result<usize> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => Ok(raw.len() - offset),
      None => Ok(0),
    }
  }

  #[inline]
  pub fn append<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<usize> {
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    let old_raw = get_string_raw(self, key_bytes)?;
    let (cur_len, cur_expire) = match old_raw {
      Some((ref raw, exp, offset)) => (raw.len() - offset, exp),
      None => (0, 0),
    };

    let new_len = cur_len
      .checked_add(val_bytes.len())
      .ok_or_else(|| Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE))?;
    if new_len > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    if new_len <= 55 {
      let mut stack_buf = [0u8; 64];
      let header = if cur_expire == 0 {
        STRING_NO_EXPIRY_HEADER
      } else {
        encode_string_header(cur_expire)
      };
      stack_buf[..STRING_HDR_SIZE].copy_from_slice(&header);
      let mut pos = STRING_HDR_SIZE;
      if let Some((ref raw, _, offset)) = old_raw {
        let old_slice = &raw[offset..];
        stack_buf[pos..pos + old_slice.len()].copy_from_slice(old_slice);
        pos += old_slice.len();
      }
      stack_buf[pos..pos + val_bytes.len()].copy_from_slice(val_bytes);
      pos += val_bytes.len();
      let enc_val = &stack_buf[..pos];

      if old_raw.is_none() && !self.meta().is_empty()? {
        let mut batch = self.batch();
        batch.insert_data(&raw_k, enc_val);
        cleanup_all_composite_data(self, key_bytes, &mut batch)?;
        batch.commit()?;
      } else {
        self.data().insert(&raw_k, enc_val)?;
      }
      return Ok(new_len);
    }

    let mut enc_val = Vec::with_capacity(STRING_HDR_SIZE + new_len);
    enc_val.extend_from_slice(&encode_string_header(cur_expire));
    if let Some((ref raw, _, offset)) = old_raw {
      enc_val.extend_from_slice(&raw[offset..]);
    }
    enc_val.extend_from_slice(val_bytes);

    if old_raw.is_none() && !self.meta().is_empty()? {
      let mut batch = self.batch();
      batch.insert_data(&raw_k, &enc_val);
      cleanup_all_composite_data(self, key_bytes, &mut batch)?;
      batch.commit()?;
    } else {
      self.data().insert(&raw_k, &enc_val)?;
    }
    Ok(new_len)
  }

  #[inline]
  pub fn getrange<K: AsRef<[u8]>>(&self, key: K, range: impl IntoIndexRange) -> Result<Vec<u8>> {
    let (start, end) = range.into_index_range();
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => {
        let payload = &raw[offset..];
        let len = payload.len() as i64;
        let (s, e) = normalize_range(start, end, len);
        if s > e || payload.is_empty() {
          Ok(Vec::new())
        } else {
          Ok(payload[s as usize..=e as usize].to_vec())
        }
      }
      None => Ok(Vec::new()),
    }
  }

  /// Zero-copy borrows the resolved substring range without heap allocation.
  /// 零拷贝借用子区间切片进行只读闭包处理（零堆分配）
  #[inline]
  pub fn with_getrange<K: AsRef<[u8]>, R>(
    &self,
    key: K,
    range: impl IntoIndexRange,
    f: impl FnOnce(&[u8]) -> R,
  ) -> Result<Option<R>> {
    let (start, end) = range.into_index_range();
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => {
        let payload = &raw[offset..];
        let len = payload.len() as i64;
        let (s, e) = normalize_range(start, end, len);
        if s > e || payload.is_empty() {
          Ok(Some(f(b"")))
        } else {
          Ok(Some(f(&payload[s as usize..=e as usize])))
        }
      }
      None => Ok(None),
    }
  }

  #[inline]
  pub fn setrange<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    key: K,
    offset: usize,
    val: V,
  ) -> Result<usize> {
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    if offset > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_OFFSET_OUT_OF_RANGE));
    }

    let required_len = offset
      .checked_add(val_bytes.len())
      .ok_or_else(|| Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE))?;
    if required_len > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    let old_raw = get_string_raw(self, key_bytes)?;
    if old_raw.is_none() && val_bytes.is_empty() {
      return Ok(0);
    }
    let (old_len, cur_expire) = match old_raw {
      Some((ref raw, exp, off)) => (raw.len() - off, exp),
      None => (0, 0),
    };

    let new_total_len = old_len.max(required_len);

    if new_total_len <= 55 {
      let mut stack_buf = [0u8; 64];
      let header = if cur_expire == 0 {
        STRING_NO_EXPIRY_HEADER
      } else {
        encode_string_header(cur_expire)
      };
      stack_buf[..STRING_HDR_SIZE].copy_from_slice(&header);

      let payload_start = STRING_HDR_SIZE;
      if let Some((ref raw, _, off)) = old_raw {
        let old_payload = &raw[off..];
        stack_buf[payload_start..payload_start + old_len].copy_from_slice(old_payload);
      }
      stack_buf[payload_start + offset..payload_start + offset + val_bytes.len()]
        .copy_from_slice(val_bytes);

      let enc_val = &stack_buf[..payload_start + new_total_len];
      if old_raw.is_none() && !self.meta().is_empty()? {
        let mut batch = self.batch();
        batch.insert_data(&raw_k, enc_val);
        cleanup_all_composite_data(self, key_bytes, &mut batch)?;
        batch.commit()?;
      } else {
        self.data().insert(&raw_k, enc_val)?;
      }
      return Ok(new_total_len);
    }

    let mut enc_val = Vec::with_capacity(STRING_HDR_SIZE + new_total_len);
    enc_val.extend_from_slice(&encode_string_header(cur_expire));

    if let Some((ref raw, _, off)) = old_raw {
      let old_payload = &raw[off..];
      if offset < old_len {
        enc_val.extend_from_slice(&old_payload[..offset]);
      } else {
        enc_val.extend_from_slice(old_payload);
        enc_val.resize(STRING_HDR_SIZE + offset, 0);
      }
    } else {
      enc_val.resize(STRING_HDR_SIZE + offset, 0);
    }

    enc_val.extend_from_slice(val_bytes);
    if old_len > required_len
      && let Some((ref raw, _, off)) = old_raw
    {
      let old_payload = &raw[off..];
      enc_val.extend_from_slice(&old_payload[required_len..]);
    }

    if old_raw.is_none() && !self.meta().is_empty()? {
      let mut batch = self.batch();
      batch.insert_data(&raw_k, &enc_val);
      cleanup_all_composite_data(self, key_bytes, &mut batch)?;
      batch.commit()?;
    } else {
      self.data().insert(&raw_k, &enc_val)?;
    }
    Ok(new_total_len)
  }

  #[inline]
  pub fn cas<K: AsRef<[u8]>, V1: AsRef<[u8]>, V2: AsRef<[u8]>>(
    &self,
    key: K,
    old_val: V1,
    new_val: V2,
    expire_ms: u64,
  ) -> Result<i32> {
    let key_bytes = key.as_ref();
    let old_bytes = old_val.as_ref();
    let new_bytes = new_val.as_ref();

    if new_bytes.len() > MAX_STRING_SIZE {
      return Err(Error::invalid_data(ERR_STRING_EXCEEDS_MAX_SIZE));
    }

    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);
    let data_ks = self.data();

    match get_string_raw(self, key_bytes)? {
      Some((raw, _, offset)) => {
        if &raw[offset..] == old_bytes {
          let mut dyn_buf = Vec::new();
          with_encoded_string_value(new_bytes, expire_ms, &mut dyn_buf, |enc_val| {
            data_ks.insert(&raw_k, enc_val)
          })?;
          Ok(1)
        } else {
          Ok(0)
        }
      }
      None => Ok(-1),
    }
  }

  #[inline]
  pub fn cad<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, val: V) -> Result<i32> {
    let key_bytes = key.as_ref();
    let val_bytes = val.as_ref();
    let kc = self.kc();
    let raw_k = key::raw(&kc, key_bytes);

    match get_string_raw(self, key_bytes)? {
      Some((raw, _, offset)) => {
        if &raw[offset..] == val_bytes {
          self.data().rm(&raw_k)?;
          Ok(1)
        } else {
          Ok(0)
        }
      }
      None => Ok(-1),
    }
  }

  #[inline]
  pub fn digest<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<String>> {
    match get_string_raw(self, key.as_ref())? {
      Some((raw, _, offset)) => Ok(Some(string_digest(&raw[offset..]))),
      None => Ok(None),
    }
  }

  #[inline]
  pub fn lcs<K1: AsRef<[u8]>, K2: AsRef<[u8]>>(
    &self,
    key1: K1,
    key2: K2,
    opt_li: impl IntoIterator<Item = Lcs>,
  ) -> Result<StringLCSResult> {
    let args = Lcs::parse_options(opt_li);
    let raw1 = get_string_raw(self, key1.as_ref())?;
    let raw2 = get_string_raw(self, key2.as_ref())?;

    let empty = &[][..];
    let s1 = raw1.as_ref().map(|(r, _, o)| &r[*o..]).unwrap_or(empty);
    let s2 = raw2.as_ref().map(|(r, _, o)| &r[*o..]).unwrap_or(empty);

    compute_lcs_with(s1, s2, args)
  }
}
