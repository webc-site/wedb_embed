use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  hll::{
    HLL_DENSE_SIZE, HLL_HASH_SEED, HLL_REGISTERS, HLL_SEGMENT_BYTES, HLL_SEGMENT_COUNT,
    HllEncodeType, HllSparseOp, HyperLogLog, HyperLogLogMeta, decode_sparse_op,
    extract_dense_hll_result, hll_dense_estimate, hll_dense_estimate_segments,
    hll_dense_get_register, hll_dense_reg_histo, hll_dense_set_register, hll_dense_to_sparse,
    hll_estimate_from_histo, hll_merge_bytes, hll_merge_segments, hll_merge_sparse_into_dense,
    hll_murmur_hash_64a, hll_sigma, hll_sparse_estimate, hll_sparse_get_register,
    hll_sparse_is_valid, hll_sparse_new, hll_sparse_reg_histo, hll_sparse_set_register,
    hll_sparse_to_dense, hll_tau, murmur_hash_64a,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_hll_registers_bitops() {
  let mut registers = vec![0u8; HLL_DENSE_SIZE];

  // 测试 6-bit 寄存器设置与读取（包括跨字节边界）
  for i in 0..16 {
    let val = ((i * 7) % 64) as u8;
    hll_dense_set_register(&mut registers, i, val);
    assert_eq!(hll_dense_get_register(&registers, i), val);
  }

  // 测试最后一个寄存器
  hll_dense_set_register(&mut registers, HLL_REGISTERS - 1, 63);
  assert_eq!(hll_dense_get_register(&registers, HLL_REGISTERS - 1), 63);

  // 覆盖写：更新已有寄存器
  hll_dense_set_register(&mut registers, 0, 42);
  assert_eq!(hll_dense_get_register(&registers, 0), 42);
  hll_dense_set_register(&mut registers, 0, 15);
  assert_eq!(hll_dense_get_register(&registers, 0), 15);
}

#[test]
fn test_hll_extract_result() {
  // 0 哈希：索引 0，尾随零 50（加 1 变成 51）
  let (idx, ctz) = extract_dense_hll_result(0);
  assert_eq!(idx, 0);
  assert_eq!(ctz, 51);

  // 索引为 0x3FFF，第 14 位为 1：ctz 为 1
  let (idx2, ctz2) = extract_dense_hll_result(0x3FFF | (1 << 14));
  assert_eq!(idx2, 0x3FFF);
  assert_eq!(ctz2, 1);

  // 全 1 哈希
  let (idx3, ctz3) = extract_dense_hll_result(u64::MAX);
  assert_eq!(idx3, 0x3FFF);
  assert_eq!(ctz3, 1);
}

#[test]
fn test_hll_sigma_tau_math() {
  assert_eq!(hll_tau(0.0), 0.0);
  assert_eq!(hll_tau(1.0), 0.0);
  assert_eq!(hll_tau(-1.0), 0.0);
  assert_eq!(hll_tau(f64::NAN), 0.0);
  assert_eq!(hll_tau(f64::INFINITY), 0.0);
  assert!(hll_tau(0.5) > 0.0);

  assert_eq!(hll_sigma(0.0), 0.0);
  assert_eq!(hll_sigma(-1.0), 0.0);
  assert_eq!(hll_sigma(f64::NAN), 0.0);
  assert_eq!(hll_sigma(1.0), f64::INFINITY);
  assert_eq!(hll_sigma(2.0), f64::INFINITY);
  assert!(hll_sigma(0.5) > 0.0);
}

#[test]
fn test_hll_reghisto_and_empty_estimate() {
  let empty = vec![0u8; HLL_DENSE_SIZE];
  let mut reghisto = [0usize; 64];
  hll_dense_reg_histo(&empty, &mut reghisto);
  assert_eq!(reghisto[0], HLL_REGISTERS);
  assert_eq!(hll_dense_estimate(&empty), 0);
  assert_eq!(hll_estimate_from_histo(&reghisto), 0);
}

#[test]
fn test_hll_cardinality_estimation_accuracy() {
  let mut hll = HyperLogLog::new();
  let n = 10_000;
  for i in 0..n {
    let el = format!("item_{i}");
    hll.add(el.as_bytes());
  }

  let est = hll.count();
  // HyperLogLog 标准误差约为 1.04 / sqrt(16384) = 0.81%
  let err = (est as f64 - n as f64).abs() / (n as f64);
  assert!(
    err < 0.03,
    "estimation error too high: {err:.4} (estimated={est}, actual={n})"
  );
}

#[test]
fn test_hll_metadata_encoding() {
  let meta = HyperLogLogMeta::new(1700000000, 42);
  assert_eq!(meta.encode_type, HllEncodeType::Dense);

  let bytes = meta.encode();
  assert_eq!(bytes.len(), HyperLogLogMeta::ENCODED_SIZE);

  let decoded = HyperLogLogMeta::decode(&bytes).expect("decode failed");
  assert_eq!(decoded.base.version, 42);
  assert_eq!(decoded.base.expire_at, 1700000000);
  assert_eq!(decoded.encode_type, HllEncodeType::Dense);
}

#[test]
fn test_hll_sparse_encoding_and_operations() {
  let sparse = hll_sparse_new();
  assert_eq!(sparse.len(), 2);
  assert!(hll_sparse_is_valid(&sparse));
  assert_eq!(hll_sparse_estimate(&sparse).unwrap(), 0);

  let mut reghisto = [0usize; 64];
  hll_sparse_reg_histo(&sparse, &mut reghisto).unwrap();
  assert_eq!(reghisto[0], HLL_REGISTERS);

  // 测试解码操作码
  let (op, consumed) = decode_sparse_op(&sparse).unwrap();
  assert_eq!(consumed, 2);
  assert_eq!(op, HllSparseOp::XZero { len: 16384 });

  // 测试稀疏寄存器设置
  let mut mut_sparse = sparse.clone();
  let updated = hll_sparse_set_register(&mut mut_sparse, 10, 5).unwrap();
  assert!(updated);
  assert!(hll_sparse_is_valid(&mut_sparse));
  assert_eq!(hll_sparse_get_register(&mut_sparse, 10).unwrap(), 5);
  assert_eq!(hll_sparse_get_register(&mut_sparse, 0).unwrap(), 0);
  assert_eq!(hll_sparse_get_register(&mut_sparse, 11).unwrap(), 0);

  // 重复设置较小值不产生更新
  let not_updated = hll_sparse_set_register(&mut mut_sparse, 10, 3).unwrap();
  assert!(!not_updated);
  assert_eq!(hll_sparse_get_register(&mut_sparse, 10).unwrap(), 5);

  // 测试稀疏向密集转换
  let mut dense_buf = vec![0u8; HLL_DENSE_SIZE];
  hll_sparse_to_dense(&mut_sparse, &mut dense_buf).unwrap();
  assert_eq!(hll_dense_get_register(&dense_buf, 10), 5);
  assert_eq!(hll_dense_get_register(&dense_buf, 0), 0);

  // 测试密集向稀疏转换
  let re_sparse = hll_dense_to_sparse(&dense_buf).expect("dense to sparse failed");
  assert!(hll_sparse_is_valid(&re_sparse));
  assert_eq!(hll_sparse_get_register(&re_sparse, 10).unwrap(), 5);

  // 测试稀疏值超限 (> 32) 触发错误提示晋升
  let overflow_err = hll_sparse_set_register(&mut mut_sparse, 20, 33);
  assert!(overflow_err.is_err());
}

#[test]
fn test_hll_standalone_struct() {
  assert!(HyperLogLog::selftest());

  let mut hll = HyperLogLog::new();
  assert!(hll.is_empty());
  assert_eq!(hll.to_bytes().len(), HLL_DENSE_SIZE);
  assert_eq!(hll.as_slice().len(), HLL_DENSE_SIZE);
  assert_eq!(hll.as_mut_slice().len(), HLL_DENSE_SIZE);

  assert!(hll.add(b"hello"));
  assert!(!hll.is_empty());
  assert!(!hll.add(b"hello"));

  let mut hll2 = HyperLogLog::new();
  hll2.add(b"world");

  hll.merge(&hll2);
  assert_eq!(hll.count(), 2);

  // 验证 from_bytes 与 get/set_register
  let exported = hll.to_bytes().to_vec();
  let mut hll3 = HyperLogLog::from_bytes(&exported);
  assert_eq!(hll3.count(), 2);
  hll3.set_register(10, 33);
  assert_eq!(hll3.get_register(10), 33);

  hll.clear();
  assert!(hll.is_empty());
  assert_eq!(hll.count(), 0);

  // 验证稀疏模式下的 HyperLogLog
  let mut sparse_hll = HyperLogLog::new_sparse();
  assert_eq!(sparse_hll.encode_type(), HllEncodeType::Sparse);
  assert!(sparse_hll.is_empty());
  assert!(sparse_hll.add(b"sparse_key_1"));
  assert_eq!(sparse_hll.count(), 1);

  let dense_conv = sparse_hll.to_dense().unwrap();
  assert_eq!(dense_conv.len(), HLL_DENSE_SIZE);
}

#[test]
fn test_hll_pfadd_pfcount_pfmerge() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 对标 Kvrocks: 空 elements 添加
  assert!(!db.pfadd("hll_empty", &Vec::<&str>::new())?);
  assert_eq!(db.pfcount(&["hll_empty"])?, 0);

  assert!(db.pfadd("hll1", &["a", "b", "c", "d"])?);
  assert!(!db.pfadd("hll1", &["a", "b"])?);
  assert_eq!(db.pfcount(&["hll1"])?, 4);

  assert!(db.pfadd("hll2", &["c", "d", "e", "f"])?);
  assert_eq!(db.pfcount(&["hll2"])?, 4);

  // 多 key 计数（并集基数）
  assert_eq!(db.pfcount(&["hll1", "hll2"])?, 6);

  // 重复 key 计数（不重复累加）
  assert_eq!(db.pfcount(&["hll1", "hll1", "hll2"])?, 6);

  // 合并
  db.pfmerge("hll_merged", &["hll1", "hll2"])?;
  assert_eq!(db.pfcount(&["hll_merged"])?, 6);

  // 幂等合并
  db.pfmerge("hll_merged", &["hll1"])?;
  assert_eq!(db.pfcount(&["hll_merged"])?, 6);

  // 空 key 计数
  let empty_keys: Vec<&str> = Vec::new();
  assert_eq!(db.pfcount(&empty_keys)?, 0);

  // 空 source 合并
  let empty_sources: Vec<&str> = Vec::new();
  db.pfmerge("hll_merged", &empty_sources)?;

  Ok(())
}

#[test]
fn test_hll_segments_estimate_and_merge() {
  let mut segments: Vec<Option<Vec<u8>>> = vec![None; HLL_SEGMENT_COUNT];
  // 空分段基数估算为 0
  let seg_refs: Vec<Option<&[u8]>> = segments.iter().map(|s| s.as_deref()).collect();
  assert_eq!(hll_dense_estimate_segments(&seg_refs), 0);

  // 设置第 0 段和第 1 段
  let mut seg0 = vec![0u8; HLL_SEGMENT_BYTES];
  let mut seg1 = vec![0u8; HLL_SEGMENT_BYTES];
  hll_dense_set_register(&mut seg0, 0, 5);
  hll_dense_set_register(&mut seg1, 10, 8);
  segments[0] = Some(seg0);
  segments[1] = Some(seg1);

  let seg_refs: Vec<Option<&[u8]>> = segments.iter().map(|s| s.as_deref()).collect();
  let est = hll_dense_estimate_segments(&seg_refs);
  assert!(est > 0);

  // 测试分段合并
  let mut dest_segments: Vec<Vec<u8>> = vec![Vec::new(); HLL_SEGMENT_COUNT];
  hll_merge_segments(&mut dest_segments, &seg_refs);
  assert_eq!(dest_segments[0].len(), HLL_SEGMENT_BYTES);
  assert_eq!(hll_dense_get_register(&dest_segments[0], 0), 5);
  assert_eq!(dest_segments[1].len(), HLL_SEGMENT_BYTES);
  assert_eq!(hll_dense_get_register(&dest_segments[1], 10), 8);
}

#[test]
fn test_hll_kvrocks_metadata_compatibility() {
  let meta = HyperLogLogMeta::new(1700000000, 42);
  let kvrocks_bytes = meta.encode_kvrocks();
  assert_eq!(kvrocks_bytes.len(), HyperLogLogMeta::KVROCKS_ENCODED_SIZE);

  let decoded = HyperLogLogMeta::decode(&kvrocks_bytes).expect("decode kvrocks hll failed");
  assert_eq!(decoded.base.version, 42);
  assert_eq!(decoded.base.expire_at, 1700000000);
  assert_eq!(decoded.encode_type, HllEncodeType::Dense);
}

#[test]
fn test_hll_kvrocks_test_suite_cases() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 对标 Kvrocks TEST_F(RedisHyperLogLogTest, PFADD)
  assert!(db.pfadd("hll", &["a", "b", "c"])?);
  assert_eq!(db.pfcount(&["hll"])?, 3);
  assert!(!db.pfadd("hll", &["a", "b", "c"])?);
  // PFADD works with empty string
  assert!(db.pfadd("hll", &[""])?);
  assert_eq!(db.pfcount(&["hll"])?, 4);

  // 对标 Kvrocks TEST_F(RedisHyperLogLogTest, PFCOUNT_returns_approximated_cardinality_of_set)
  assert!(db.pfadd("hll_card", &["1", "2", "3", "4", "5"])?);
  assert_eq!(db.pfcount(&["hll_card"])?, 5);
  assert!(db.pfadd("hll_card", &["6", "7", "8", "8", "9", "10"])?);
  assert_eq!(db.pfcount(&["hll_card"])?, 10);

  // 对标 Kvrocks TEST_F(RedisHyperLogLogTest, PFMERGE_results_on_the_cardinality_of_union_of_sets)
  assert!(db.pfadd("hll_u1", &["a", "b", "c"])?);
  assert!(db.pfadd("hll_u2", &["b", "c", "d"])?);
  assert!(db.pfadd("hll_u3", &["c", "d", "e"])?);
  db.pfmerge("hll_u_dst", &["hll_u1", "hll_u2", "hll_u3"])?;
  assert_eq!(db.pfcount(&["hll_u_dst"])?, 5);

  // 对标 Kvrocks TEST_F(RedisHyperLogLogTest, PFCOUNT_multiple)
  assert_eq!(db.pfcount(&["hll_u1", "hll_u2", "hll_u3"])?, 5);
  assert_eq!(db.pfcount(&["hll_u1", "hll_u2", "hll_u3", "hll_u_dst"])?, 5);

  Ok(())
}

#[test]
fn test_hll_large_union_estimation_accuracy() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  for i in 1..=300 {
    let f0 = format!("foo-{i}");
    let f1 = format!("bar-{i}");
    let f2 = format!("zap-{i}");
    db.pfadd("hll0", &[f0.as_str()])?;
    db.pfadd("hll1", &[f1.as_str()])?;
    db.pfadd("hll2", &[f2.as_str()])?;
  }

  let c0 = db.pfcount(&["hll0"])?;
  let c1 = db.pfcount(&["hll1"])?;
  let c2 = db.pfcount(&["hll2"])?;
  let sum_card = (c0 + c1 + c2) as f64;
  let real_card = 900.0;
  let diff = (sum_card - real_card).abs();
  assert!(
    diff < real_card * 0.05,
    "large union estimation error too high: diff={diff}, real={real_card}, estimated={sum_card}"
  );

  let union_card = db.pfcount(&["hll0", "hll1", "hll2"])? as f64;
  let diff_union = (union_card - real_card).abs();
  assert!(
    diff_union < real_card * 0.05,
    "pfcount union estimation error: diff={diff_union}, real={real_card}, estimated={union_card}"
  );

  Ok(())
}

#[test]
fn test_hll_namespace_isolation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let ns1_db = db.wedb().ns(1)?.db(0)?;
  let ns2_db = db.wedb().ns(2)?.db(0)?;

  assert!(ns1_db.pfadd("my_hll", &["apple", "banana"])?);
  assert!(ns2_db.pfadd("my_hll", &["cherry", "date", "fig"])?);

  assert_eq!(ns1_db.pfcount(&["my_hll"])?, 2);
  assert_eq!(ns2_db.pfcount(&["my_hll"])?, 3);

  ns1_db.pfmerge("my_hll_merged", &["my_hll"])?;
  assert_eq!(ns1_db.pfcount(&["my_hll_merged"])?, 2);

  Ok(())
}

#[test]
fn test_hll_robustness_on_arbitrary_slices() {
  let mut short_buf = vec![0u8; 10];
  hll_dense_set_register(&mut short_buf, 0, 42);
  assert_eq!(hll_dense_get_register(&short_buf, 0), 42);
  assert_eq!(hll_dense_get_register(&short_buf, 1000), 0);

  let mut reghisto = [0usize; 64];
  hll_dense_reg_histo(&short_buf, &mut reghisto);
  assert_eq!(hll_dense_estimate(&short_buf), 1);

  let mut d = vec![0u8; 5];
  let s = vec![0xFFu8; 5];
  hll_merge_bytes(&mut d, &s);
  assert_eq!(d[0] & 0x3F, 63);

  // 非法稀疏字节流容错
  let invalid_sparse = vec![0x40, 0x00]; // 长度不足 2 字节或截断
  assert!(!hll_sparse_is_valid(&invalid_sparse));
  assert!(hll_sparse_estimate(&invalid_sparse).is_err());
}

#[test]
fn test_hll_murmur_hash_64a_and_add_murmur() {
  let h1 = murmur_hash_64a(b"hello world", HLL_HASH_SEED);
  let h2 = hll_murmur_hash_64a(b"hello world");
  assert_eq!(h1, h2);
  assert_ne!(h1, 0);

  let mut hll = HyperLogLog::new();
  assert!(hll.add_murmur(b"foo"));
  assert!(!hll.add_murmur(b"foo"));
  assert_eq!(hll.count(), 1);
}

#[test]
fn test_hll_merge_sparse_into_dense_direct() {
  let mut sparse = hll_sparse_new();
  hll_sparse_set_register(&mut sparse, 42, 18).unwrap();
  hll_sparse_set_register(&mut sparse, 100, 25).unwrap();

  let mut dense = vec![0u8; HLL_DENSE_SIZE];
  hll_merge_sparse_into_dense(&mut dense, &sparse);

  assert_eq!(hll_dense_get_register(&dense, 42), 18);
  assert_eq!(hll_dense_get_register(&dense, 100), 25);
  assert_eq!(hll_dense_get_register(&dense, 0), 0);
}

#[test]
fn test_hll_binary_keys() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  let bin_key1 = b"\xff\xfe\x00\x01";
  let bin_key2 = b"\x00\x01\x02\x03";
  let bin_val1 = b"\x80\x90\xa0";
  let bin_val2 = b"\xb0\xc0\xd0";

  assert!(db.pfadd(bin_key1, &[bin_val1])?);
  assert_eq!(db.pfcount(&[bin_key1])?, 1);

  assert!(db.pfadd(bin_key2, &[bin_val2])?);
  assert_eq!(db.pfcount(&[bin_key2])?, 1);

  let merged_key = b"\xde\xad\xbe\xef";
  db.pfmerge(merged_key, &[bin_key1, bin_key2])?;
  assert_eq!(db.pfcount(&[merged_key])?, 2);

  Ok(())
}

#[test]
fn test_hll_wrongtype_collision() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 先写入 String 类型
  db.set("str_key", b"string_value", [])?;

  // 对 String key 执行 HLL 操作，必须报错 WRONGTYPE
  let err_add = db.pfadd("str_key", &["elem"]);
  assert!(err_add.is_err());
  let err_count = db.pfcount(&["str_key"]);
  assert!(err_count.is_err());
  let err_merge = db.pfmerge("dst_key", &["str_key"]);
  assert!(err_merge.is_err());

  // 2. 先写入 Set 类型
  db.sadd("set_key", &["member1", "member2"])?;

  // 对 Set key 执行 HLL 操作，必须报错 WRONGTYPE
  let err_add_set = db.pfadd("set_key", &["elem"]);
  assert!(err_add_set.is_err());
  let err_count_set = db.pfcount(&["set_key"]);
  assert!(err_count_set.is_err());
  let err_merge_dst = db.pfmerge("set_key", &["some_hll"]);
  assert!(err_merge_dst.is_err());

  // 3. 先写入 List 类型
  db.lpush("list_key", &["val1", "val2"])?;
  assert!(db.pfadd("list_key", &["elem"]).is_err());
  assert!(db.pfcount(&["list_key"]).is_err());

  // 4. 先写入 Hash 类型
  db.hset("hash_key", &[("field", "val")])?;
  assert!(db.pfadd("hash_key", &["elem"]).is_err());
  assert!(db.pfcount(&["hash_key"]).is_err());

  // 5. 先写入 ZSet 类型
  db.zadd("zset_key", &[(1.0, "m1")], [])?;
  assert!(db.pfadd("zset_key", &["elem"]).is_err());
  assert!(db.pfcount(&["zset_key"]).is_err());

  Ok(())
}

#[test]
fn test_hll_del_and_recreate() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert!(db.pfadd("hll_del_test", &["a", "b", "c"])?);
  assert_eq!(db.pfcount(&["hll_del_test"])?, 3);

  // 删除 key
  let deleted = db.del(&["hll_del_test"])?;
  assert_eq!(deleted, 1);
  assert_eq!(db.pfcount(&["hll_del_test"])?, 0);

  // 重新创建
  assert!(db.pfadd("hll_del_test", &["x", "y"])?);
  assert_eq!(db.pfcount(&["hll_del_test"])?, 2);

  Ok(())
}

#[test]
fn test_hll_sparse_sequential_updates_and_promotion() {
  let mut hll = HyperLogLog::new_sparse();
  assert_eq!(hll.encode_type(), HllEncodeType::Sparse);

  // 连续设置多个稀疏寄存器
  for i in 0..50 {
    hll.set_register(i * 10, (i % 30) as u8 + 1);
  }
  assert_eq!(hll.encode_type(), HllEncodeType::Sparse);

  for i in 0..50 {
    assert_eq!(hll.get_register(i * 10), (i % 30) as u8 + 1);
  }

  // 触发超限晋升（值 > 32 触发 Dense 升级）
  hll.set_register(999, 45);
  assert_eq!(hll.encode_type(), HllEncodeType::Dense);
  assert_eq!(hll.get_register(999), 45);

  // 原有寄存器数据依然保持正确
  for i in 0..50 {
    assert_eq!(hll.get_register(i * 10), (i % 30) as u8 + 1);
  }
}

#[test]
fn test_hll_merge_self_and_idempotency() {
  let mut hll = HyperLogLog::new();
  hll.add(b"item1");
  hll.add(b"item2");

  let count_before = hll.count();
  let cloned = hll.clone();
  hll.merge(&cloned);
  assert_eq!(hll.count(), count_before);

  // merge 空结构
  let empty_hll = HyperLogLog::new();
  hll.merge(&empty_hll);
  assert_eq!(hll.count(), count_before);

  // merge 空字节
  hll.merge_bytes(&[]);
  assert_eq!(hll.count(), count_before);
}

#[test]
fn test_hll_kvrocks_extensions_and_hashes() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试 pfadd_murmur 与 pfadd_hashes
  let h1 = hll_murmur_hash_64a(b"elem1");
  let h2 = hll_murmur_hash_64a(b"elem2");
  assert!(db.pfadd_hashes("hll_hash", &[h1, h2])?);
  assert_eq!(db.pfcount(&["hll_hash"])?, 2);
  assert!(!db.pfadd_hashes("hll_hash", &[h1, h2])?);

  // 2. 测试 pfadd_murmur 对标 Redis / Kvrocks
  assert!(db.pfadd_murmur("hll_mur", &["a", "b", "c"])?);
  assert_eq!(db.pfcount(&["hll_mur"])?, 3);

  // 3. 测试 pfcount_multiple 多键联合统计
  assert_eq!(db.pfcount_multiple(&["hll_hash", "hll_mur"])?, 5);

  Ok(())
}
