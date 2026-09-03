use hipstr::HipStr;
use rapidhash::RapidHashSet;
use wedb_embed::search::{
  DistanceMetric, HnswGraph, HnswNode, SearchKey, VectorAlgorithm, VectorFieldMetadata, VectorType,
  compute_vector_distance, decode_hnsw_node_meta, decode_hnsw_vector_field_meta,
  encode_hnsw_node_meta, encode_hnsw_vector_field_meta,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_hnsw_creation_and_random_level() {
  let graph = HnswGraph::new(4, DistanceMetric::L2, 16, 200, 10, 0.01);
  assert_eq!(graph.dim, 4);
  assert_eq!(graph.distance_metric, DistanceMetric::L2);
  assert_eq!(graph.m, 16);
  assert_eq!(graph.ef_construction, 200);
  assert_eq!(graph.ef_runtime, 10);
  assert!((graph.epsilon - 0.01).abs() < 1e-6);
  assert_eq!(graph.num_levels(), 0);
  assert!(graph.entry_point.is_none());

  // 随机层数生成验证：应大部分落在 0..5 之间
  let mut levels = Vec::new();
  for _ in 0..1000 {
    let lvl = graph.random_level();
    levels.push(lvl);
  }
  let max_gen = *levels.iter().max().unwrap();
  assert!(max_gen < 20);
  assert!(levels.contains(&0));
}

#[test]
fn test_hnsw_vector_distance_metrics() {
  let v1 = vec![1.0, 2.0, 3.0];
  let v2 = vec![4.0, 5.0, 6.0];

  // L2: sqrt(3^2 + 3^2 + 3^2) = sqrt(27) ≈ 5.1961524
  let l2 = compute_vector_distance(&v1, &v2, DistanceMetric::L2).unwrap();
  assert!((l2 - (27.0f64).sqrt()).abs() < 1e-6);

  // IP: -(1*4 + 2*5 + 3*6) = -(4 + 10 + 18) = -32
  let ip = compute_vector_distance(&v1, &v2, DistanceMetric::IP).unwrap();
  assert!((ip - (-32.0)).abs() < 1e-6);

  // Cosine
  let v_same = vec![2.0, 4.0, 6.0];
  let cos_same = compute_vector_distance(&v1, &v_same, DistanceMetric::Cosine).unwrap();
  assert!(cos_same.abs() < 1e-6);

  let v_ortho = vec![0.0, 0.0, 1.0];
  let v_ortho2 = vec![1.0, 0.0, 0.0];
  let cos_ortho = compute_vector_distance(&v_ortho, &v_ortho2, DistanceMetric::Cosine).unwrap();
  assert!((cos_ortho - 1.0).abs() < 1e-6);

  // 维度不匹配报错
  let v_short = vec![1.0, 2.0];
  assert!(compute_vector_distance(&v1, &v_short, DistanceMetric::L2).is_err());
  assert!(compute_vector_distance(&[], &[], DistanceMetric::L2).is_err());
}

#[test]
fn test_hnsw_node_insertion_and_overwrite() {
  let mut graph = HnswGraph::new(3, DistanceMetric::L2, 4, 16, 8, 0.01);

  // 插入第一个节点
  let id1 = HipStr::from("doc1");
  let vec1 = vec![1.0, 0.0, 0.0];
  let node_id1 = graph.insert(id1.clone(), vec1.clone()).unwrap();
  assert_eq!(graph.nodes.len(), 1);
  assert_eq!(graph.entry_point, Some(node_id1));
  assert!(graph.num_levels() >= 1);

  // 插入第二个节点
  let id2 = HipStr::from("doc2");
  let vec2 = vec![0.0, 1.0, 0.0];
  graph.insert(id2.clone(), vec2).unwrap();
  assert_eq!(graph.nodes.len(), 2);

  // 覆盖更新第一个节点
  let vec1_new = vec![1.1, 0.1, 0.0];
  let node_id1_new = graph.insert(id1.clone(), vec1_new.clone()).unwrap();
  assert_eq!(graph.nodes.len(), 2);
  assert_eq!(graph.nodes.get(&node_id1_new).unwrap().vector, vec1_new);
}

#[test]
fn test_hnsw_knn_and_range_search_accuracy() {
  let mut graph = HnswGraph::new(2, DistanceMetric::L2, 4, 32, 16, 0.01);

  // 插入 2D 平面点集
  let points = vec![
    ("p0_0", vec![0.0, 0.0]),
    ("p1_0", vec![1.0, 0.0]),
    ("p0_1", vec![0.0, 1.0]),
    ("p1_1", vec![1.0, 1.0]),
    ("p5_5", vec![5.0, 5.0]),
    ("p10_10", vec![10.0, 10.0]),
  ];

  for (name, pt) in points {
    graph.insert(HipStr::from(name), pt).unwrap();
  }

  // KNN 搜索 (0.1, 0.1) 的最近 3 个邻居：应为 p0_0, p1_0/p0_1, p1_1
  let query = vec![0.1, 0.1];
  let knn = graph.search_knn(&query, 3, None).unwrap();
  assert_eq!(knn.len(), 3);
  assert_eq!(knn[0].1, HipStr::from("p0_0"));

  // 范围搜索：以原点为中心，半径 1.5 内的点应包含 p0_0, p1_0, p0_1, p1_1 (dist = sqrt(2) ≈ 1.414)
  let range = graph.search_range(&[0.0, 0.0], 1.5, None).unwrap();
  let ids: RapidHashSet<HipStr<'static>> = range.into_iter().map(|(_, id)| id).collect();
  assert_eq!(ids.len(), 4);
  assert!(ids.contains(&HipStr::from("p0_0")));
  assert!(ids.contains(&HipStr::from("p1_0")));
  assert!(ids.contains(&HipStr::from("p0_1")));
  assert!(ids.contains(&HipStr::from("p1_1")));
  assert!(!ids.contains(&HipStr::from("p5_5")));
}

#[test]
fn test_hnsw_expand_search_scope() {
  let mut graph = HnswGraph::new(2, DistanceMetric::L2, 4, 32, 16, 0.01);

  graph.insert(HipStr::from("a"), vec![0.0, 0.0]).unwrap();
  graph.insert(HipStr::from("b"), vec![1.0, 0.0]).unwrap();
  graph.insert(HipStr::from("c"), vec![0.0, 1.0]).unwrap();

  let initial = vec![(0.0, HipStr::from("a"))];
  let mut visited = RapidHashSet::default();
  visited.insert(HipStr::from("a"));

  let expanded = graph
    .expand_search_scope(&[0.5, 0.5], &initial, &mut visited)
    .unwrap();
  assert!(!expanded.is_empty());
  for (_, id) in expanded {
    assert!(id == "b" || id == "c");
  }
}

#[test]
fn test_hnsw_deletion_and_empty_reset() {
  let mut graph = HnswGraph::new(2, DistanceMetric::L2, 4, 16, 8, 0.01);

  graph.insert(HipStr::from("n1"), vec![0.0, 0.0]).unwrap();
  graph.insert(HipStr::from("n2"), vec![1.0, 1.0]).unwrap();
  graph.insert(HipStr::from("n3"), vec![2.0, 2.0]).unwrap();

  assert_eq!(graph.nodes.len(), 3);

  // 删除非存在节点
  assert!(!graph.delete("unknown"));

  // 删除 n1
  assert!(graph.delete("n1"));
  assert_eq!(graph.nodes.len(), 2);
  assert!(!graph.doc_to_node.contains_key("n1"));

  // 删除其余所有节点
  assert!(graph.delete("n2"));
  assert!(graph.delete("n3"));
  assert_eq!(graph.nodes.len(), 0);
  assert_eq!(graph.num_levels(), 0);
  assert!(graph.entry_point.is_none());

  // 再次查询返回空列表
  let knn = graph.search_knn(&[0.0, 0.0], 5, None).unwrap();
  assert!(knn.is_empty());

  // 清空图
  graph.clear();
  assert_eq!(graph.nodes.len(), 0);
}

#[test]
fn test_hnsw_binary_metadata_codecs() {
  // 1. HnswVectorFieldMetadata 编码与解码
  let meta = VectorFieldMetadata {
    vector_type: VectorType::Float64,
    dim: 128,
    distance_metric: DistanceMetric::Cosine,
    algorithm: VectorAlgorithm::Hnsw,
    initial_cap: 100_000,
    m: 32,
    ef_construction: 256,
    ef_runtime: 64,
    epsilon: 0.05,
    num_levels: 4,
  };

  let encoded_meta = encode_hnsw_vector_field_meta(&meta, false);
  let (decoded_meta, noindex) = decode_hnsw_vector_field_meta(&encoded_meta).unwrap();
  assert!(!noindex);
  assert_eq!(decoded_meta.vector_type, VectorType::Float64);
  assert_eq!(decoded_meta.dim, 128);
  assert_eq!(decoded_meta.distance_metric, DistanceMetric::Cosine);
  assert_eq!(decoded_meta.initial_cap, 100_000);
  assert_eq!(decoded_meta.m, 32);
  assert_eq!(decoded_meta.ef_construction, 256);
  assert_eq!(decoded_meta.ef_runtime, 64);
  assert!((decoded_meta.epsilon - 0.05).abs() < 1e-6);
  assert_eq!(decoded_meta.num_levels, 4);

  // 2. HnswNodeFieldMetadata 编码与解码
  let vec_data = vec![1.25, -3.5, 0.0, 99.125];
  let encoded_node = encode_hnsw_node_meta(8, &vec_data);
  let (num_neighbours, decoded_vec) = decode_hnsw_node_meta(&encoded_node).unwrap();
  assert_eq!(num_neighbours, 8);
  assert_eq!(decoded_vec.len(), 4);
  for (a, b) in vec_data.iter().zip(decoded_vec.iter()) {
    assert!((a - b).abs() < 1e-9);
  }

  // 3. SearchKey HNSW 前缀构造测试
  let key_builder = SearchKey::with_field(1, "idx_v", "embedding");
  let node_key = key_builder.construct_hnsw_node(2, "doc_100");
  assert!(node_key.len() > 10);

  let edge_key = key_builder.construct_hnsw_edge(1, "doc_100", "doc_200");
  assert!(edge_key.len() > node_key.len());

  let single_edge_key = key_builder.construct_hnsw_edge_with_single_end(1, "doc_100");
  assert!(edge_key.starts_with(&single_edge_key));
}

#[test]
fn test_hnsw_high_dimensional_stress_and_recall() {
  let dim = 16;
  let num_points = 100;
  let mut graph = HnswGraph::new(dim, DistanceMetric::L2, 16, 100, 50, 0.01);

  let mut points: Vec<(HipStr<'static>, Vec<f64>)> = Vec::with_capacity(num_points);
  for i in 0..num_points {
    let id = HipStr::from(format!("vec_{i}"));
    let v: Vec<f64> = (0..dim).map(|_| fastrand::f64() * 10.0 - 5.0).collect();
    graph.insert(id.clone(), v.clone()).unwrap();
    points.push((id, v));
  }

  assert_eq!(graph.nodes.len(), num_points);

  // 运行 20 次随机向量 KNN 检索并与暴力全量扫描比对 Recall
  let k = 10;
  let mut total_recall = 0.0;
  let num_queries = 20;

  for _ in 0..num_queries {
    let q: Vec<f64> = (0..dim).map(|_| fastrand::f64() * 10.0 - 5.0).collect();

    // 暴力全量扫描 Ground Truth
    let mut ground_truth: Vec<(f64, HipStr<'static>)> = points
      .iter()
      .map(|(id, v)| {
        let d = compute_vector_distance(&q, v, DistanceMetric::L2).unwrap();
        (d, id.clone())
      })
      .collect();
    ground_truth.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let gt_top_k: RapidHashSet<HipStr<'static>> =
      ground_truth.into_iter().take(k).map(|(_, id)| id).collect();

    // HNSW KNN
    let hnsw_results = graph.search_knn(&q, k, Some(100)).unwrap();
    let hnsw_top_k: RapidHashSet<HipStr<'static>> =
      hnsw_results.into_iter().map(|(_, id)| id).collect();

    let hits = gt_top_k.intersection(&hnsw_top_k).count();
    let recall = (hits as f64) / (k as f64);
    total_recall += recall;
  }

  let avg_recall = total_recall / (num_queries as f64);
  assert!(
    avg_recall >= 0.90,
    "HNSW average recall {avg_recall} should be >= 0.90"
  );
}

#[test]
fn test_hnsw_node_pack_and_robust_prune() {
  let mut graph = HnswGraph::new(3, DistanceMetric::L2, 4, 32, 16, 0.01);
  let id1 = HipStr::from("center");
  let vec1 = vec![0.0, 0.0, 0.0];
  let center_id = graph.insert(id1.clone(), vec1.clone()).unwrap();

  let p_near = HipStr::from("p_near");
  let near_id = graph.insert(p_near.clone(), vec![1.0, 0.0, 0.0]).unwrap();

  let p_behind = HipStr::from("p_behind");
  let behind_id = graph.insert(p_behind.clone(), vec![2.0, 0.0, 0.0]).unwrap();

  let p_side = HipStr::from("p_side");
  let side_id = graph.insert(p_side.clone(), vec![0.0, 1.0, 0.0]).unwrap();

  // 1. RobustPrune 验证：在 alpha = 1.2 下，p_behind 因为在 p_near 的同一直线上
  // 会优先保留具有方向多样性的 p_side 而非在同一方向上的冗余远点
  let candidates = vec![near_id, behind_id, side_id];
  let pruned = graph.robust_prune(&vec1, &candidates, 2, 1.2);
  assert_eq!(pruned.len(), 2);
  assert!(pruned.contains(&near_id));
  assert!(pruned.contains(&side_id)); // 保留侧向点，增强图连通性与探索广度

  // 2. NodePack 序列化与反序列化验证
  let node = graph.nodes.get(&center_id).unwrap();
  let mut pack_bytes = Vec::new();
  node.encode_level_pack(0, &mut pack_bytes);
  assert!(!pack_bytes.is_empty());

  let decoded_node =
    HnswNode::decode_level_pack(id1.clone(), center_id, 0, &pack_bytes, 3).unwrap();
  assert_eq!(decoded_node.doc_id, id1);
  assert_eq!(decoded_node.node_id, center_id);
  assert_eq!(decoded_node.neighbors[0], node.neighbors[0]);
}

#[test]
fn test_sq8_quantization_and_compression_accuracy() {
  use wedb_embed::search::{NodePackFormat, NodePackRef, Sq8Vector, compute_sq8_distance};

  // 1. 模拟 1536 维 OpenAI Embedding 向量
  let dim = 1536;
  let raw_vec: Vec<f64> = (0..dim).map(|i| ((i as f64) * 0.013).sin() * 0.1).collect();

  // 2. SQ8 编码与反量化
  let sq8 = Sq8Vector::encode(&raw_vec);
  assert_eq!(sq8.data.len(), dim);
  let decoded_vec = sq8.decode();
  assert_eq!(decoded_vec.len(), dim);

  let mut reused_buf = Vec::new();
  sq8.decode_into(&mut reused_buf);
  assert_eq!(reused_buf, decoded_vec);

  // 3. 计算反量化前后的余弦相似度（应 >= 0.999）
  let cos_dist = compute_vector_distance(&raw_vec, &decoded_vec, DistanceMetric::Cosine).unwrap();
  assert!(
    cos_dist < 0.005,
    "SQ8 cosine distortion {cos_dist} should be < 0.005 (similarity >= 0.995)"
  );

  // 4. NodePack 紧凑字节打包与压缩率测试
  let neighbors = vec![1001u64, 1005, 1020, 1035, 1080];
  let mut pack_bytes = Vec::new();
  NodePackRef::encode_sq8(
    sq8.scale,
    sq8.offset,
    &sq8.data,
    &neighbors,
    &mut pack_bytes,
  );

  // 原始 f64 向量需 1536 * 8 = 12,288 字节，SQ8 打包后仅需约 1,555 字节（压缩率 87.3%）
  let raw_size = dim * 8;
  let compressed_size = pack_bytes.len();
  let savings = (raw_size - compressed_size) as f64 / raw_size as f64;
  assert!(
    savings >= 0.85,
    "Storage savings {savings:.2} should be >= 85%"
  );

  // 5. 解码验证
  let pack_ref = NodePackRef::decode(&pack_bytes, dim).unwrap();
  assert_eq!(pack_ref.format, NodePackFormat::Sq8);
  assert_eq!(pack_ref.degree, 5);
  assert_eq!(pack_ref.to_neighbor_vec(), neighbors);

  // 6. simsimd SQ8 距离内核测试
  let sq8_2 = Sq8Vector::encode(&decoded_vec);
  let d_cos = compute_sq8_distance(&sq8.data, &sq8_2.data, DistanceMetric::Cosine).unwrap();
  assert!(d_cos < 0.01);
}
