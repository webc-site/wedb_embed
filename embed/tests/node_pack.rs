use wedb_embed::{NodePackFormat, NodePackRef};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_node_pack_encode_decode_roundtrip_oppv_delta_u64() {
  let vec_data = vec![1.0, -2.5, 3.5, 0.0, 999.125];
  // 测试百亿级超大 u64 节点 ID 序列
  let neighbors = vec![
    10_000_000_000u64,
    10_000_000_015,
    10_000_000_120,
    10_000_001_500,
    10_001_000_000,
  ];

  let mut encoded = Vec::new();
  NodePackRef::encode(&vec_data, &neighbors, &mut encoded);

  let decoded = NodePackRef::decode(&encoded, vec_data.len()).unwrap();
  assert_eq!(decoded.format, NodePackFormat::Sq8);
  assert_eq!(decoded.degree, neighbors.len());

  let decoded_vec = decoded.to_f64_vec();
  for (a, b) in decoded_vec.iter().zip(vec_data.iter()) {
    assert!((a - b).abs() < 5.0);
  }

  let parsed_neighbors: Vec<u64> = decoded.to_neighbor_vec();
  assert_eq!(parsed_neighbors, neighbors);

  // 验证压缩效率：5 个百亿级 u64 原本需要 40 字节，经 Delta + OP-PV 编码后仅约 14 字节
  let neighbor_bytes_len = encoded.len() - (1 + 8 + vec_data.len() + 2);
  assert!(neighbor_bytes_len <= 16);
}

#[test]
fn test_node_pack_empty_neighbors_oppv_delta() {
  let vec_data = vec![0.5, 0.25];
  let neighbors: Vec<u64> = Vec::new();

  let mut encoded = Vec::new();
  NodePackRef::encode(&vec_data, &neighbors, &mut encoded);

  let decoded = NodePackRef::decode(&encoded, 2).unwrap();
  assert_eq!(decoded.format, NodePackFormat::Sq8);
  assert_eq!(decoded.degree, 0);
  assert_eq!(decoded.iter_neighbors().count(), 0);
  let decoded_vec = decoded.to_f64_vec();
  for (a, b) in decoded_vec.iter().zip(vec_data.iter()) {
    assert!((a - b).abs() < 0.05);
  }
}
