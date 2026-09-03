use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  CfInsert, CfReserve, CuckooChainMeta, CuckooFilterHelper, CuckooFilterInfo, Fjall, WeDb,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_cuckoo_filter_helper_and_meta_math() -> Void {
  // 桶计算与容量限制测试（对标 Kvrocks CalculateRequiredBuckets）
  let req_buckets = CuckooFilterHelper::calculate_required_buckets(1000, 2)?;
  assert!(req_buckets.is_power_of_two());
  assert!(req_buckets >= 512);

  // 边界条件：无效 bucket_size
  assert!(CuckooFilterHelper::calculate_required_buckets(1000, 0).is_err());

  // 规范化扩容因子测试（对标 Kvrocks NormalizeExpansion）
  assert_eq!(CuckooFilterHelper::normalize_expansion(0), 0);
  assert_eq!(CuckooFilterHelper::normalize_expansion(1), 1);
  assert_eq!(CuckooFilterHelper::normalize_expansion(2), 2);
  assert_eq!(CuckooFilterHelper::normalize_expansion(3), 4);
  assert_eq!(CuckooFilterHelper::normalize_expansion(7), 8);
  assert_eq!(CuckooFilterHelper::normalize_expansion(32768), 32768);
  assert_eq!(CuckooFilterHelper::normalize_expansion(40000), 32768);

  // 指纹生成与异或备选哈希测试（对标 Kvrocks GenerateFingerprint / GetAltHash / GetAltBucketIndex）
  let hash = CuckooFilterHelper::hash(b"test_cuckoo_item");
  let fp = CuckooFilterHelper::generate_fingerprint(hash);
  assert_ne!(fp, 0); // 指纹绝对不为 0 (1..=255)

  let alt_hash = CuckooFilterHelper::get_alt_hash(fp, hash);
  let original_hash = CuckooFilterHelper::get_alt_hash(fp, alt_hash);
  assert_eq!(hash, original_hash); // 异或对称性

  let num_buckets = 128u32;
  let b1 = (hash % (num_buckets as u64)) as u32;
  let b2 = CuckooFilterHelper::get_alt_bucket_index(b1, fp, num_buckets);
  let b1_recovered = CuckooFilterHelper::get_alt_bucket_index(b2, fp, num_buckets);
  assert_eq!(b1, b1_recovered);

  // 元数据编码/解码测试（53 字节二进制对齐）
  let meta = CuckooChainMeta::new(1024, 2, 20, 1, 2048, 1, 0);
  assert_eq!(meta.get_total_capacity(), 1024);
  assert!(meta.is_scaling());

  let encoded = meta.encode();
  assert_eq!(encoded.len(), CuckooChainMeta::ENCODED_SIZE);

  let decoded = CuckooChainMeta::decode(&encoded).expect("cuckoo decode failed");
  assert_eq!(decoded.base_capacity, 1024);
  assert_eq!(decoded.bucket_size, 2);
  assert_eq!(decoded.max_iterations, 20);
  assert_eq!(decoded.expansion, 1);
  assert_eq!(decoded.page_size, 2048);
  assert_eq!(decoded.n_filters, 1);
  assert_eq!(decoded.num_deleted_items, 0);

  // 非扩容元数据容量测试
  let non_scaling_meta = CuckooChainMeta::new(512, 4, 10, 0, 2048, 1, 0);
  assert!(!non_scaling_meta.is_scaling());
  assert_eq!(non_scaling_meta.get_total_capacity(), 512);

  Ok(())
}

#[test]
fn test_cuckoo_filter_crud_and_kickout() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // CF.RESERVE
  db.cf_reserve(
    "cfkey",
    1000,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(1),
    ],
  )?;
  // 重复 RESERVE 报错
  assert!(
    db.cf_reserve(
      "cfkey",
      1000,
      [
        CfReserve::BucketSize(2),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(1)
      ]
    )
    .is_err()
  );

  // CF.ADD & CF.EXISTS
  assert!(db.cf_add("cfkey", "alpha")?);
  assert!(db.cf_add("cfkey", "beta")?);
  assert!(db.cf_exists("cfkey", "alpha")?);
  assert!(db.cf_exists("cfkey", "beta")?);
  assert!(!db.cf_exists("cfkey", "gamma")?);

  // CF.ADDNX
  assert!(!db.cf_addnx("cfkey", "alpha")?);
  assert!(db.cf_addnx("cfkey", "gamma")?);
  assert!(db.cf_exists("cfkey", "gamma")?);

  // CF.COUNT & 重复项
  assert_eq!(db.cf_count("cfkey", "alpha")?, 1);
  assert!(db.cf_add("cfkey", "alpha")?);
  assert_eq!(db.cf_count("cfkey", "alpha")?, 2);

  // CF.DEL
  assert!(db.cf_del("cfkey", "alpha")?);
  assert_eq!(db.cf_count("cfkey", "alpha")?, 1);
  assert!(db.cf_del("cfkey", "alpha")?);
  assert_eq!(db.cf_count("cfkey", "alpha")?, 0);
  assert!(!db.cf_exists("cfkey", "alpha")?);
  assert!(!db.cf_del("cfkey", "alpha")?);

  // CF.MEXISTS
  let mex = db.cf_mexists("cfkey", &["alpha", "beta", "gamma", "delta"])?;
  assert_eq!(mex, vec![false, true, true, false]);

  // CF.INFO
  let info: CuckooFilterInfo = db.cf_info("cfkey")?;
  assert_eq!(info.bucket_size, 2);
  assert_eq!(info.num_filters, 1);
  assert_eq!(info.num_items_deleted, 2);
  assert_eq!(info.size, 2); // beta, gamma

  // 踢出算法与扩容测试
  let dir_dense = tempdir()?;
  let db_dense = WeDb::new(Fjall::open(dir_dense.path())?).ns(0)?.db(0)?;
  db_dense.cf_reserve(
    "cf_dense",
    16,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(2),
    ],
  )?;

  for i in 0..50 {
    let item = format!("dense_item_{i}");
    assert!(db_dense.cf_add("cf_dense", item)?);
  }

  for i in 0..50 {
    let item = format!("dense_item_{i}");
    assert!(db_dense.cf_exists("cf_dense", item)?);
  }

  let dense_info = db_dense.cf_info("cf_dense")?;
  assert!(dense_info.num_filters > 1);
  assert_eq!(dense_info.size, 50);

  // CF.INSERT & CF.INSERT (with nx = true) 测试
  let ins_res = db_dense.cf_insert("cf_ins", &["one", "two"], [])?;
  assert_eq!(ins_res, vec![true, true]);

  let nx_res = db_dense.cf_insert("cf_ins", &["one", "three"], [CfInsert::Nx])?;
  assert_eq!(nx_res, vec![false, true]);

  Ok(())
}

#[test]
fn test_cuckoo_filter_edge_cases_and_multi_item_batch() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 边界参数校验
  assert!(
    db.cf_reserve(
      "cf_err",
      1,
      [
        CfReserve::BucketSize(2),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(1)
      ]
    )
    .is_err()
  ); // capacity < 2
  assert!(
    db.cf_reserve(
      "cf_err",
      100,
      [
        CfReserve::BucketSize(0),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(1)
      ]
    )
    .is_err()
  ); // bucket_size == 0
  assert!(
    db.cf_reserve(
      "cf_err",
      100,
      [
        CfReserve::BucketSize(2),
        CfReserve::MaxIterations(0),
        CfReserve::Expansion(1)
      ]
    )
    .is_err()
  ); // max_iterations == 0
  assert!(
    db.cf_reserve(
      "cf_err",
      100,
      [
        CfReserve::BucketSize(2),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(40000)
      ]
    )
    .is_err()
  ); // expansion > 32768

  // cf_reserve 自定义 page_size 校验
  assert!(
    db.cf_reserve(
      "cf_err_page",
      100,
      [
        CfReserve::BucketSize(4),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(1),
        CfReserve::PageSize(2)
      ]
    )
    .is_err()
  ); // page_size < bucket_size
  assert!(
    db.cf_reserve(
      "cf_err_page0",
      100,
      [
        CfReserve::BucketSize(4),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(1),
        CfReserve::PageSize(0)
      ]
    )
    .is_err()
  ); // page_size == 0
  assert!(
    db.cf_reserve(
      "cf_custom_page",
      100,
      [
        CfReserve::BucketSize(4),
        CfReserve::MaxIterations(20),
        CfReserve::Expansion(1),
        CfReserve::PageSize(1024)
      ]
    )
    .is_ok()
  );

  // 空 items 测试
  let empty_items: &[&str] = &[];
  let empty_res = db.cf_insert("cf_empty", empty_items, [])?;
  assert!(empty_res.is_empty());

  // CF.INSERT NOCREATE 对不存在的 key 应报错
  assert!(
    db.cf_insert("cf_nocreate", &["item1"], [CfInsert::NoCreate])
      .is_err()
  );

  // CF.INSERT 自动创建时无效参数报错
  assert!(
    db.cf_insert("cf_inv_cap", &["item1"], [CfInsert::Capacity(1)])
      .is_err()
  );
  assert!(
    db.cf_insert("cf_inv_bs", &["item1"], [CfInsert::BucketSize(0)])
      .is_err()
  );
  assert!(
    db.cf_insert("cf_inv_page", &["item1"], [CfInsert::PageSize(1)])
      .is_err()
  );

  // CF.COUNT 和 CF.DEL 对不存在的 key
  assert_eq!(db.cf_count("non_existent_cf", "foo")?, 0);
  assert!(!db.cf_del("non_existent_cf", "foo")?);
  assert!(!db.cf_exists("non_existent_cf", "foo")?);
  let mex_none = db.cf_mexists("non_existent_cf", &["a", "b"])?;
  assert_eq!(mex_none, vec![false, false]);

  // CF.INFO 对不存在的 key 报错
  assert!(db.cf_info("non_existent_cf").is_err());

  // 多元素批量插入并发扩容事务一致性测试
  let mut items = Vec::new();
  for i in 0..100 {
    items.push(format!("batch_item_{i}"));
  }

  let insert_res = db.cf_insert(
    "cf_batch",
    &items,
    [
      CfInsert::Capacity(8),
      CfInsert::BucketSize(2),
      CfInsert::MaxIterations(10),
      CfInsert::Expansion(2),
      CfInsert::PageSize(2048),
    ],
  )?;
  assert_eq!(insert_res.len(), 100);
  assert!(insert_res.iter().all(|&ok| ok));

  // 验证所有 100 个元素均可查到
  for item in &items {
    assert!(db.cf_exists("cf_batch", item)?);
  }

  let info = db.cf_info("cf_batch")?;
  assert_eq!(info.size, 100);
  assert!(info.num_filters > 1);

  // 非扩容满过滤器测试（expansion = 0）
  db.cf_reserve(
    "cf_fixed",
    4,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(5),
      CfReserve::Expansion(0),
    ],
  )?;
  let mut count = 0;
  for i in 0..100 {
    let it = format!("fixed_{i}");
    if db.cf_add("cf_fixed", it).is_ok() {
      count += 1;
    } else {
      break;
    }
  }
  // 达到容量上限后应报错
  assert!(count > 0 && count < 100);

  Ok(())
}

#[test]
fn test_cuckoo_filter_duplicates_and_binary_items() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 重复添加同一元素计数与逐个删除测试
  db.cf_reserve(
    "cf_dup",
    64,
    [
      CfReserve::BucketSize(4),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(2),
    ],
  )?;
  let dup_target = "same_key_item";

  for count in 1..=5 {
    assert!(db.cf_add("cf_dup", dup_target)?);
    assert_eq!(db.cf_count("cf_dup", dup_target)?, count);
  }

  for expected_left in (0..5).rev() {
    assert!(db.cf_del("cf_dup", dup_target)?);
    assert_eq!(db.cf_count("cf_dup", dup_target)?, expected_left as u64);
  }
  // 全部删除后应不再存在
  assert!(!db.cf_del("cf_dup", dup_target)?);
  assert!(!db.cf_exists("cf_dup", dup_target)?);

  // 2. 二进制项测试
  db.cf_reserve(
    "cf_bin",
    128,
    [
      CfReserve::BucketSize(4),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(2),
    ],
  )?;
  let bin_items: Vec<Vec<u8>> = vec![
    vec![],
    vec![0u8],
    vec![0u8, 1, 2, 3, 255, 254],
    b"binary_payload_test".to_vec(),
    vec![b'c'; 512],
  ];

  for item in &bin_items {
    assert!(!db.cf_exists("cf_bin", item)?);
    assert!(db.cf_add("cf_bin", item)?);
    assert!(db.cf_exists("cf_bin", item)?);
  }

  let bin_info = db.cf_info("cf_bin")?;
  assert_eq!(bin_info.size, bin_items.len() as u64);

  Ok(())
}

#[test]
fn test_cuckoo_filter_kickout_deep_scaling_and_saturation() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试容量较小、扩容因子为 2 的连续扩展
  db.cf_reserve(
    "cf_scale_chain",
    4,
    [
      CfReserve::BucketSize(1),
      CfReserve::MaxIterations(1),
      CfReserve::Expansion(2),
    ],
  )?;
  let mut added = 0;
  for i in 0..50 {
    let it = format!("scale_chain_item_{i}");
    if db.cf_add("cf_scale_chain", it).is_ok() {
      added += 1;
    }
  }
  assert_eq!(added, 50);

  let info = db.cf_info("cf_scale_chain")?;
  assert!(info.num_filters > 2);
  assert_eq!(info.size, 50);

  for i in 0..50 {
    let it = format!("scale_chain_item_{i}");
    assert!(db.cf_exists("cf_scale_chain", it)?);
  }

  // 2. 测试非扩容模式（expansion = 0）满载后明确报错
  db.cf_reserve(
    "cf_strict_fixed",
    4,
    [
      CfReserve::BucketSize(1),
      CfReserve::MaxIterations(1),
      CfReserve::Expansion(0),
    ],
  )?;
  let mut full_reached = false;
  for i in 0..50 {
    let it = format!("fixed_item_{i}");
    let res = db.cf_add("cf_strict_fixed", it);
    if res.is_err() {
      full_reached = true;
      break;
    }
  }
  assert!(full_reached, "Non-scaling filter must reject when full");

  Ok(())
}

#[test]
fn test_cuckoo_expired_meta() -> Void {
  let mut meta = CuckooChainMeta::new(1024, 2, 20, 1, 2048, 1, 1000);
  assert!(!meta.is_expired(500));
  assert!(meta.is_expired(1500));

  meta.base.expire_at = 500;
  assert!(meta.is_expired(600));

  Ok(())
}

#[test]
fn test_cuckoo_card_and_point_lookups() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // Key 不存在时 cf_card 返回 0
  assert_eq!(db.cf_card("cf_card_test")?, 0);
  assert!(!db.cf_exists("cf_card_test", "elem1")?);

  assert!(db.cf_add("cf_card_test", "elem1")?);
  assert!(db.cf_add("cf_card_test", "elem2")?);
  assert_eq!(db.cf_card("cf_card_test")?, 2);

  // 验证 cf_exists 单点探测快路径
  assert!(db.cf_exists("cf_card_test", "elem1")?);
  assert!(db.cf_exists("cf_card_test", "elem2")?);
  assert!(!db.cf_exists("cf_card_test", "non_exist")?);

  // 删除元素后基数更新
  assert!(db.cf_del("cf_card_test", "elem1")?);
  assert_eq!(db.cf_card("cf_card_test")?, 1);
  assert!(!db.cf_exists("cf_card_test", "elem1")?);

  Ok(())
}
