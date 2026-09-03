use super::{
  compose_si_meta_key, compose_si_prefix_stack, extract_id,
  meta::SortedintMeta,
};
use crate::{
  engine::{Engine, KvEntry, Partition},
  error::{Error, Result},
  key::get_meta_checked,
  meta::current_now_ms,
  wedb::Db,
};

/// Streaming iteration operations for Sortedint.
/// 有序整数集合流式迭代接口
impl<E: Engine> Db<E>
where
  Error: From<E::Error>,
{
  #[inline]
  pub fn si_iter<K: AsRef<[u8]>, F: FnMut(u64) -> bool>(&self, key: K, mut f: F) -> Result<()> {
    let k_bytes = key.as_ref();
    let kc = self.kc();
    let meta_k = compose_si_meta_key(&kc, k_bytes);
    let now_ms = current_now_ms();

    if get_meta_checked::<SortedintMeta, _>(self, k_bytes, &meta_k, now_ms)?.is_none() {
      return Ok(());
    }

    let prefix = compose_si_prefix_stack(&kc, k_bytes);
    let prefix_len = prefix.len();

    for g in self.data().prefix(&prefix) {
      let entry = g?;
      let k = entry.key();
      if !k.starts_with(&prefix) {
        break;
      }
      if let Some(id) = extract_id(k, prefix_len)
        && !f(id)
      {
        break;
      }
    }
    Ok(())
  }
}
