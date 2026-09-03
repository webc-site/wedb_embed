use aok::Void;
use wedb_embed::{
  KeyMeta, RedisType, generate_version, init_version_counter, normalize_range, version_to_time,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_version_generation_and_time() -> Void {
  init_version_counter();
  let v1 = generate_version();
  let v2 = generate_version();
  assert!(v2 > v1);

  let (sec, usec) = version_to_time(v1);
  assert!(sec > 1_700_000_000);
  assert!(usec < 1_000_000);
  Ok(())
}

#[test]
fn test_key_meta_encode_decode_roundtrip() -> Void {
  let meta = KeyMeta::new(RedisType::Hash, 1_800_000_000_000, 123456789, 42);
  let encoded = meta.encode();
  assert_eq!(encoded.len(), KeyMeta::ENCODED_SIZE);

  let decoded = KeyMeta::decode(&encoded).expect("decode failed");
  assert_eq!(decoded.rtype, RedisType::Hash);
  assert_eq!(decoded.expire_at, 1_800_000_000_000);
  assert_eq!(decoded.version, 123456789);
  assert_eq!(decoded.size, 42);
  Ok(())
}

#[test]
fn test_key_meta_kvrocks_compatibility() -> Void {
  let meta = KeyMeta::new(RedisType::Set, 2_000_000_000_000, 9999, 10);
  let kvrocks_enc = meta.encode_kvrocks();
  assert_eq!(kvrocks_enc.len(), KeyMeta::KVROCKS_COMPLEX_ENCODED_SIZE);

  let decoded = KeyMeta::decode(&kvrocks_enc).expect("decode kvrocks failed");
  assert_eq!(decoded.rtype, RedisType::Set);
  assert_eq!(decoded.expire_at, 2_000_000_000_000);
  assert_eq!(decoded.version, 9999);
  assert_eq!(decoded.size, 10);
  Ok(())
}

#[test]
fn test_normalize_range_behavior() -> Void {
  assert_eq!(normalize_range(0, -1, 10), (0, 9));
  assert_eq!(normalize_range(-3, -1, 10), (7, 9));
  let (s, e) = normalize_range(-100, -50, 10);
  assert!(s > e);
  assert_eq!(normalize_range(0, 5, 0), (0, -1));
  Ok(())
}
