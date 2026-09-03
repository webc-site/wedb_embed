pub mod composer;
pub mod ns;
pub mod oppv;
pub mod slot;
pub mod small_key;
pub mod tag;

pub use composer::{KeyComposer, SubkeyComposer};
pub use ns::{
  CATALOG_PREFIX, DEFAULT_NAMESPACE, NS_NEXT_ID_KEY, catalog_db_key, catalog_ns_prefix,
  db_next_id_key, encode_catalog_db_key_fixed, encode_catalog_ns_prefix_fixed,
  encode_db_next_id_key_fixed, is_default_namespace,
};
pub use oppv::{decode_oppv_u64, encode_oppv_u64, encode_oppv_u64_fixed, oppv_len_u64};
pub use slot::{
  HASH_SLOTS_MASK, HASH_SLOTS_SIZE, compose_slot_key_prefix, compose_slot_key_upper_bound, crc16,
  encode_slot_key_prefix_fixed, get_slot_id_from_key, get_tag_from_key, matches_glob,
  matches_glob_bytes,
};
pub use small_key::{INLINE_CAP, SmallKey};
pub use tag::{ALL_COMPOSITE_META_TAGS, KeyTag, SysMetaTag, SystemDomainTag};
