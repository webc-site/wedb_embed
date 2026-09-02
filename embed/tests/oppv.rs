use wedb_embed::{decode_oppv_u64, encode_oppv_u64};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_oppv_strict_order_preservation() {
  let test_values = [
    0u64,
    1,
    2,
    127,
    128,
    129,
    1000,
    16383,
    16384,
    100_000,
    1_000_000,
    268_435_455,
    268_435_456,
    1_000_000_000,
    u64::MAX - 1,
    u64::MAX,
  ];

  let mut encoded_list = Vec::with_capacity(test_values.len());
  for &v in &test_values {
    let mut buf = Vec::new();
    encode_oppv_u64(v, &mut buf);
    let (decoded, consumed) = decode_oppv_u64(&buf).unwrap();
    assert_eq!(decoded, v);
    assert_eq!(consumed, buf.len());
    encoded_list.push((v, buf));
  }

  // 验证二进制字典序必须与数值大小完全等价
  for w in encoded_list.windows(2) {
    let (v1, ref b1) = w[0];
    let (v2, ref b2) = w[1];
    assert!(
      v1 < v2,
      "Value {v1} should be strictly less than value {v2}"
    );
    assert!(
      b1 < b2,
      "Encoded bytes {b1:?} should be strictly less than bytes {b2:?}"
    );
  }
}
