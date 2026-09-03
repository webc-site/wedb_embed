use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  BfInsert, BfReserve, BlockSplitBloomFilter, BloomChainMeta, BloomFilterAddResult,
  BloomFilterInfo, Fjall, WeDb,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_block_split_bloom_math_and_filter() -> Void {
  // 验证最优位与字节计算（对标 Kvrocks OptimalNumOfBits/Bytes）
  let bits_1000 = BlockSplitBloomFilter::optimal_num_of_bits(1000, 0.01);
  assert!(bits_1000.is_power_of_two());
  assert!(bits_1000 >= 32 * 8);

  let bytes_1000 = BlockSplitBloomFilter::optimal_num_of_bytes(1000, 0.01);
  assert_eq!(bytes_1000, bits_1000 / 8);

  // 单块 32 字节测试
  let mut data = vec![0u8; 32];
  let h1 = BlockSplitBloomFilter::hash(b"hello");
  let h2 = BlockSplitBloomFilter::hash(b"world");

  assert!(!BlockSplitBloomFilter::find_hash(&data, h1));
  assert!(!BlockSplitBloomFilter::find_hash(&data, h2));

  BlockSplitBloomFilter::insert_hash(&mut data, h1);
  assert!(BlockSplitBloomFilter::find_hash(&data, h1));
  assert!(!BlockSplitBloomFilter::find_hash(&data, h2));

  BlockSplitBloomFilter::insert_hash(&mut data, h2);
  assert!(BlockSplitBloomFilter::find_hash(&data, h1));
  assert!(BlockSplitBloomFilter::find_hash(&data, h2));

  Ok(())
}

#[test]
fn test_bloom_chain_metadata_codec_and_capacity() -> Void {
  let meta = BloomChainMeta::new(100, 0.01, 2, 1, 1000, 256);
  assert_eq!(meta.get_capacity(), 100);
  assert!(meta.is_scaling());

  let encoded = meta.encode();
  assert_eq!(encoded.len(), BloomChainMeta::ENCODED_SIZE);

  let decoded = BloomChainMeta::decode(&encoded).expect("decode failed");
  assert_eq!(decoded.base_capacity, 100);
  assert_eq!(decoded.error_rate, 0.01);
  assert_eq!(decoded.expansion, 2);
  assert_eq!(decoded.n_filters, 1);
  assert_eq!(decoded.bloom_bytes, 256);

  // 非扩容模式
  let non_scaling_meta = BloomChainMeta::new(500, 0.01, 0, 1, 0, 512);
  assert!(!non_scaling_meta.is_scaling());
  assert_eq!(non_scaling_meta.get_capacity(), 500);

  Ok(())
}

#[test]
fn test_bloom_reserve_add_exists_info_scaling() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // BF.RESERVE
  db.bf_reserve("bfkey", 0.01, 1000, [BfReserve::Expansion(2)])?;
  assert!(
    db.bf_reserve("bfkey", 0.01, 1000, [BfReserve::Expansion(2)])
      .is_err()
  );

  // BF.ADD & BF.EXISTS
  assert!(db.bf_add("bfkey", "item1")?);
  assert!(!db.bf_add("bfkey", "item1")?);

  assert!(db.bf_exists("bfkey", "item1")?);
  assert!(!db.bf_exists("bfkey", "item2")?);

  // BF.MADD & BF.MEXISTS
  let madd_res = db.bf_madd("bfkey", &["item2", "item3"])?;
  assert_eq!(madd_res, vec![true, true]);

  let mex_res = db.bf_mexists("bfkey", &["item1", "item2", "item4"])?;
  assert_eq!(mex_res, vec![true, true, false]);

  // BF.INFO & BF.CARD
  let info: BloomFilterInfo = db.bf_info("bfkey")?;
  assert_eq!(info.n_filters, 1);
  assert_eq!(info.size, 3);
  assert_eq!(db.bf_card("bfkey")?, 3);

  // 自动扩容 (Scaling) 测试
  let dir_scale = tempdir()?;
  let db_scale = WeDb::new(Fjall::open(dir_scale.path())?).ns(0)?.db(0)?;
  db_scale.bf_reserve("bf_scale", 0.01, 5, [BfReserve::Expansion(2)])?;

  for i in 0..15 {
    let item = format!("item_{i}");
    assert!(db_scale.bf_add("bf_scale", item)?);
  }

  let scale_info = db_scale.bf_info("bf_scale")?;
  assert!(scale_info.n_filters > 1);
  assert_eq!(scale_info.size, 15);

  for i in 0..15 {
    let item = format!("item_{i}");
    assert!(db_scale.bf_exists("bf_scale", item)?);
  }

  // 非扩容满过滤器 (Non-scaling full) 测试
  let dir_fixed = tempdir()?;
  let db_fixed = WeDb::new(Fjall::open(dir_fixed.path())?).ns(0)?.db(0)?;
  db_fixed.bf_reserve("bf_fixed", 0.01, 3, [BfReserve::NonScaling])?;

  for i in 0..3 {
    let item = format!("elem_{i}");
    assert!(db_fixed.bf_add("bf_fixed", item)?);
  }
  // 超过容量应报错 Full
  assert!(db_fixed.bf_add("bf_fixed", "elem_overflow").is_err());

  // 非扩容模式下的 expansion 为 0
  assert_eq!(db_fixed.bf_info("bf_fixed")?.expansion, 0);

  Ok(())
}

#[test]
fn test_bloom_edge_cases_and_insert_options() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 边界条件：无效容量与错误率
  assert!(
    db.bf_reserve("err_k", 0.01, 0, [BfReserve::Expansion(2)])
      .is_err()
  );
  assert!(
    db.bf_reserve("err_k", 0.0, 100, [BfReserve::Expansion(2)])
      .is_err()
  );
  assert!(
    db.bf_reserve("err_k", 1.0, 100, [BfReserve::Expansion(2)])
      .is_err()
  );
  assert!(
    db.bf_reserve("err_k", -0.5, 100, [BfReserve::Expansion(2)])
      .is_err()
  );

  // 空 items 测试
  let empty_items: &[&str] = &[];
  let empty_res = db.bf_insert("bf_empty", empty_items, [])?;
  assert!(empty_res.is_empty());

  // BF.INSERT NOCREATE 对不存在的 key 应报错
  assert!(
    db.bf_insert("bf_nocreate", &["item1"], [BfInsert::NoCreate])
      .is_err()
  );

  // BF.CARD 对不存在的 key 返回 0
  assert_eq!(db.bf_card("non_existent_bf")?, 0);

  // BF.EXISTS 对不存在的 key 返回 false
  assert!(!db.bf_exists("non_existent_bf", "foo")?);
  let mex = db.bf_mexists("non_existent_bf", &["a", "b"])?;
  assert_eq!(mex, vec![false, false]);

  // BF.INFO 对不存在的 key 返回 Err
  assert!(db.bf_info("non_existent_bf").is_err());

  // BF.INSERT 自动创建与自定义选项测试
  let bf_ins_res = db.bf_insert(
    "bf_custom_ins",
    &["x", "y"],
    [
      BfInsert::Capacity(500),
      BfInsert::ErrorRate(0.001),
      BfInsert::Expansion(2),
    ],
  )?;
  assert_eq!(
    bf_ins_res,
    vec![BloomFilterAddResult::Ok, BloomFilterAddResult::Ok]
  );

  // 重复插入应返回 Exist
  let bf_ins_dup = db.bf_insert("bf_custom_ins", &["x", "z"], [])?;
  assert_eq!(
    bf_ins_dup,
    vec![BloomFilterAddResult::Exist, BloomFilterAddResult::Ok]
  );

  // 自动创建时无效参数报错
  assert!(
    db.bf_insert("bf_inv_cap", &["item"], [BfInsert::Capacity(0)])
      .is_err()
  );
  assert!(
    db.bf_insert("bf_inv_err", &["item"], [BfInsert::ErrorRate(0.0)])
      .is_err()
  );

  Ok(())
}

#[test]
fn test_bloom_deep_scaling_and_binary_items() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试特殊二进制/空字符串项在 Bloom Filter 中的行为
  db.bf_reserve("bf_bin", 0.01, 100, [BfReserve::Expansion(2)])?;
  let bin_items: Vec<Vec<u8>> = vec![
    vec![],
    vec![0u8],
    vec![0u8, 1, 2, 3, 255, 254],
    b"normal_text".to_vec(),
    vec![b'a'; 1000],
  ];

  for item in &bin_items {
    assert!(!db.bf_exists("bf_bin", item)?);
    assert!(db.bf_add("bf_bin", item)?);
    assert!(db.bf_exists("bf_bin", item)?);
  }

  // 重复插入二进制项应返回 false
  for item in &bin_items {
    assert!(!db.bf_add("bf_bin", item)?);
  }

  // 2. 多级扩容链深度遍历测试 (Bloom 扩容因子 1: 每次扩展容量相同)
  db.bf_reserve("bf_deep_scale", 0.01, 10, [BfReserve::Expansion(1)])?;
  for i in 0..60 {
    let it = format!("deep_item_{i}");
    assert!(db.bf_add("bf_deep_scale", it)?);
  }
  let deep_info = db.bf_info("bf_deep_scale")?;
  assert!(deep_info.n_filters >= 5);
  assert_eq!(deep_info.size, 60);

  for i in 0..60 {
    let it = format!("deep_item_{i}");
    assert!(db.bf_exists("bf_deep_scale", it)?);
  }

  Ok(())
}

#[test]
fn test_bloom_comprehensive_info_and_large_batch() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 批量插入 200 个元素
  let items: Vec<String> = (0..200).map(|i| format!("batch_bloom_{i}")).collect();
  let res = db.bf_insert(
    "bf_large_batch",
    &items,
    [
      BfInsert::Capacity(50),
      BfInsert::ErrorRate(0.01),
      BfInsert::Expansion(2),
    ],
  )?;
  assert_eq!(res.len(), 200);
  assert!(
    res
      .iter()
      .all(|r| matches!(r, BloomFilterAddResult::Ok | BloomFilterAddResult::Exist))
  );

  // 验证所有元素均存在
  let exists = db.bf_mexists("bf_large_batch", &items)?;
  assert_eq!(exists.len(), 200);
  assert!(exists.iter().all(|&e| e));

  // 验证 info 各个类型
  let info = db.bf_info("bf_large_batch")?;
  assert!(info.size >= 195);
  assert!(info.n_filters >= 3);
  assert_eq!(info.expansion, 2);

  Ok(())
}

#[test]
fn test_bloom_expired_meta() -> Void {
  let mut meta = BloomChainMeta::new(100, 0.01, 2, 1, 1000, 256);
  assert!(!meta.is_expired(500));
  assert!(meta.is_expired(1500));

  // 过期 meta 返回 None 容量
  meta.base.expire_at = 500;
  assert!(meta.is_expired(600));

  Ok(())
}

#[test]
fn test_bloom_card_and_point_lookups() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  assert_eq!(db.bf_card("bf_card_key")?, 0);
  assert!(!db.bf_exists("bf_card_key", "elem1")?);

  assert!(db.bf_add("bf_card_key", "elem1")?);
  assert!(db.bf_add("bf_card_key", "elem2")?);
  assert_eq!(db.bf_card("bf_card_key")?, 2);

  // 验证 bf_exists 单点反向探测快路径
  assert!(db.bf_exists("bf_card_key", "elem1")?);
  assert!(db.bf_exists("bf_card_key", "elem2")?);
  assert!(!db.bf_exists("bf_card_key", "non_exist")?);

  Ok(())
}
