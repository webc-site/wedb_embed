use std::cmp::Ordering;

use aok::Void;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  tdigest::{
    Centroid, ScalerK1, TDigestMerge, TDigestMerger, TDigestMergerTool, TDigestMeta, TDigestState,
    calculate_capacity, decode_double_from_u64, double_compare, double_equal, encode_double_to_u64,
    lerp, tdigest_by_rank_calc, tdigest_cdf_calc, tdigest_quantile_calc, tdigest_rank_calc,
    tdigest_trimmed_mean_calc,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_tdigest_double_order_preservation() {
  let vals = [-1000.0, -50.5, -0.0, 0.0, 0.5, 42.0, 9999.9];
  let mut prev_u = 0u64;
  for (i, &v) in vals.iter().enumerate() {
    let u = encode_double_to_u64(v);
    let decoded = decode_double_from_u64(u);
    assert!((decoded - v).abs() < 1e-12 || (v == 0.0 && decoded == 0.0));
    if i > 0 && v != vals[i - 1] {
      assert!(u > prev_u, "Encoded u64 not strictly increasing for {v}");
    }
    prev_u = u;
  }
}

#[test]
fn test_tdigest_double_compare_and_lerp() {
  assert!(double_equal(1.0, 1.0));
  assert!(double_equal(1.0, 1.0 + 1e-13));
  assert!(!double_equal(1.0, 1.1));
  assert_eq!(double_compare(1.0, 2.0, 1e-12, 1e-9), Ordering::Less);
  assert_eq!(double_compare(2.0, 1.0, 1e-12, 1e-9), Ordering::Greater);
  assert_eq!(double_compare(1.0, 1.0, 1e-12, 1e-9), Ordering::Equal);

  assert_eq!(lerp(10.0, 20.0, 0.5), 15.0);
  assert_eq!(lerp(10.0, 20.0, 0.0), 10.0);
  assert_eq!(lerp(10.0, 20.0, 1.0), 20.0);
}

#[test]
fn test_tdigest_centroid_merge() {
  let mut c1 = Centroid::new(2.0, 3.0);
  let c2 = Centroid::new(3.0, 4.0);
  c1.merge(&c2);

  assert!((c1.weight - 7.0).abs() < 0.01);
  assert!((c1.mean - 2.5714).abs() < 0.01);
}

#[test]
fn test_tdigest_meta_encoding() {
  let mut meta = TDigestMeta::new(100, 1700000000, 101);
  meta.minimum = 1.5;
  meta.maximum = 99.5;
  meta.merged_nodes = 50;
  meta.total_weight = 1000;
  meta.merged_weight = 1000;
  meta.total_observations = 1000;
  meta.merge_times = 5;

  let bytes = meta.encode();
  assert_eq!(bytes.len(), TDigestMeta::ENCODED_SIZE);
  assert_eq!(bytes.len(), 98);

  let decoded = TDigestMeta::decode(&bytes).expect("decode failed");
  assert_eq!(decoded.compression, 100);
  assert_eq!(decoded.capacity, 100 * 6 + 10);
  assert_eq!(decoded.merged_nodes, 50);
  assert_eq!(decoded.total_weight, 1000);
  assert_eq!(decoded.merged_weight, 1000);
  assert_eq!(decoded.minimum, 1.5);
  assert_eq!(decoded.maximum, 99.5);
  assert_eq!(decoded.total_observations, 1000);
  assert_eq!(decoded.merge_times, 5);

  meta.reset();
  assert_eq!(meta.merged_nodes, 0);
  assert_eq!(meta.unmerged_nodes, 0);
  assert_eq!(meta.total_weight, 0);
  assert_eq!(meta.total_observations, 0);
  assert_eq!(meta.minimum, f64::MAX);
  assert_eq!(meta.maximum, -f64::MAX);
}

#[test]
fn test_tdigest_scaler_k1() {
  let scaler = ScalerK1::new(100);
  for q in [0.0, 0.1, 0.5, 0.9, 1.0] {
    let k = scaler.k(q);
    let round_q = scaler.q(k);
    assert!((round_q - q).abs() < 1e-9);
  }
}

#[test]
fn test_tdigest_standalone() {
  let mut td = TDigestState::new(100.0);
  assert!(td.is_empty());
  assert!(td.min().is_nan());
  assert!(td.max().is_nan());

  for i in 1..=100 {
    td.add(i as f64, 1.0);
  }
  td.ensure_merged();
  assert_eq!(td.total_observations, 100);
  assert_eq!(td.min(), 1.0);
  assert_eq!(td.max(), 100.0);

  let p50 = td.quantile(0.5);
  assert!((p50 - 50.5).abs() < 2.0);

  let p99 = td.quantile(0.99);
  assert!((p99 - 99.5).abs() < 2.0);

  let cdf_50 = td.cdf(50.0);
  assert!((cdf_50 - 0.5).abs() < 0.05);

  let rank_50 = td.rank(50.0);
  assert!((rank_50 - 49).abs() <= 2);

  let revrank_50 = td.revrank(50.0);
  assert!((revrank_50 - 50).abs() <= 2);

  let byrank_50 = td.byrank(50);
  assert!((byrank_50 - 50.5).abs() < 2.0);

  let trimmed = td.trimmed_mean(0.1, 0.9);
  assert!((trimmed - 50.5).abs() < 2.0);

  let info = td.info();
  assert_eq!(info.compression, 100);
  assert_eq!(info.observations, 100);
  assert_eq!(info.minimum, Some(1.0));
  assert_eq!(info.maximum, Some(100.0));
  assert!(info.total_compressions >= 1);
}

#[test]
fn test_tdigest_empty_and_single_centroid() {
  // 空 TDigest
  let centroids: Vec<Centroid> = Vec::new();
  assert!(tdigest_quantile_calc(&centroids, 0.0, 0.0, 0.0, 0.5).is_nan());
  assert!(tdigest_trimmed_mean_calc(&centroids, 0.0, 0.1, 0.9).is_nan());
  let cdfs = tdigest_cdf_calc(&centroids, 0.0, 0.0, 0.0, &[10.0]);
  assert!(cdfs[0].is_nan());
  let ranks = tdigest_rank_calc(&centroids, 0.0, 0.0, 0.0, &[10.0], false);
  assert_eq!(ranks[0], -2);
  let byranks = tdigest_by_rank_calc(&centroids, 0.0, &[0], false);
  assert!(byranks[0].is_nan());

  // 单一质心
  let single = vec![Centroid::new(42.0, 1.0)];
  let q = tdigest_quantile_calc(&single, 42.0, 42.0, 1.0, 0.5);
  assert_eq!(q, 42.0);

  let single_cdfs = tdigest_cdf_calc(&single, 42.0, 42.0, 1.0, &[40.0, 42.0, 50.0]);
  assert_eq!(single_cdfs[0], 0.0);
  assert_eq!(single_cdfs[1], 0.5);
  assert_eq!(single_cdfs[2], 1.0);
}

#[test]
fn test_tdigest_cdf_exact_and_boundary() {
  let centroids = vec![
    Centroid::new(10.0, 1.0),
    Centroid::new(20.0, 2.0),
    Centroid::new(30.0, 1.0),
  ];
  let cdfs = tdigest_cdf_calc(&centroids, 10.0, 30.0, 4.0, &[5.0, 10.0, 20.0, 30.0, 40.0]);
  assert_eq!(cdfs[0], 0.0); // < min
  assert_eq!(cdfs[4], 1.0); // > max
  assert!(cdfs[1] > 0.0 && cdfs[1] < 0.5);
  assert!((cdfs[2] - 0.5).abs() < 0.1);
}

#[test]
fn test_tdigest_rank_and_byrank() {
  let centroids = vec![
    Centroid::new(10.0, 10.0),
    Centroid::new(20.0, 20.0),
    Centroid::new(30.0, 10.0),
  ];
  let ranks = tdigest_rank_calc(
    &centroids,
    10.0,
    30.0,
    40.0,
    &[5.0, 10.0, 20.0, 30.0, 35.0],
    false,
  );
  assert_eq!(ranks[0], -1); // < min
  assert_eq!(ranks[4], 40); // >= max

  let byranks = tdigest_by_rank_calc(&centroids, 40.0, &[0, 15, 35, 100], false);
  assert_eq!(byranks[0], 10.0);
  assert_eq!(byranks[1], 20.0);
  assert_eq!(byranks[2], 30.0);
  assert_eq!(byranks[3], f64::INFINITY);
}

#[test]
fn test_tdigest_revrank_and_rank_different_elements() {
  let mut td = TDigestState::new(100.0);
  td.add_batch(&[10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
  td.ensure_merged();

  let values = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0];
  let rev_ranks: Vec<i64> = values.iter().map(|&v| td.revrank(v)).collect();
  let expected_revrank = [6, 5, 4, 3, 2, 1, 0, -1];
  assert_eq!(rev_ranks, expected_revrank);

  let ranks: Vec<i64> = values.iter().map(|&v| td.rank(v)).collect();
  let expected_rank = [-1, 0, 1, 2, 3, 4, 5, 6];
  assert_eq!(ranks, expected_rank);
}

#[test]
fn test_tdigest_revrank_and_rank_identical_elements() {
  let mut td = TDigestState::new(100.0);
  td.add_batch(&[10.0, 10.0, 10.0, 20.0, 20.0]);
  td.ensure_merged();

  let values = [10.0, 20.0];
  let rev_ranks: Vec<i64> = values.iter().map(|&v| td.revrank(v)).collect();
  assert_eq!(rev_ranks, [3, 1]);

  let ranks: Vec<i64> = values.iter().map(|&v| td.rank(v)).collect();
  assert_eq!(ranks, [1, 4]);

  td.add(10.0, 1.0);
  td.ensure_merged();

  let new_rev: Vec<i64> = values.iter().map(|&v| td.revrank(v)).collect();
  assert_eq!(new_rev, [4, 1]);

  let new_rank: Vec<i64> = values.iter().map(|&v| td.rank(v)).collect();
  assert_eq!(new_rank, [2, 5]);
}

#[test]
fn test_tdigest_revrank_and_rank_unordered() {
  let mut td = TDigestState::new(100.0);
  let input = [
    12.0, 100.0, 50.0, 36.0, 75.0, 81.0, 35.5, 46.0, 36.0, 8.8, 15.0, 4.0, 32.5, 12.0, 8.8, 7.0,
    99.0, 0.0,
  ];
  td.add_batch(&input);
  td.ensure_merged();

  let values_rank = [50.0, 36.0, 4.0, 99.0, 8.8];
  let ranks: Vec<i64> = values_rank.iter().map(|&v| td.rank(v)).collect();
  assert_eq!(ranks, [13, 11, 1, 16, 4]);

  let values_rev = [50.0, 36.0, 4.0, 99.0, 8.8, 12.0];
  let rev_ranks: Vec<i64> = values_rev.iter().map(|&v| td.revrank(v)).collect();
  assert_eq!(rev_ranks, [4, 7, 16, 1, 14, 12]);
}

#[test]
fn test_tdigest_byrank_and_byrevrank_full() {
  let mut td = TDigestState::new(100.0);
  let values = [
    1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0, 4.0, 5.0, 5.0, 5.0, 5.0, 5.0,
  ];
  td.add_batch(&values);
  td.ensure_merged();

  let ranks = [0, 1, 2, 3, 6, 9, 10, 14, 15];
  let expected_byrank = [1.0, 2.0, 2.0, 3.0, 4.0, 4.0, 5.0, 5.0, f64::INFINITY];
  let by_rank: Vec<f64> = ranks.iter().map(|&r| td.byrank(r)).collect();
  for (i, (&got, &exp)) in by_rank.iter().zip(expected_byrank.iter()).enumerate() {
    if exp.is_infinite() {
      assert!(got.is_infinite() && got > 0.0, "Mismatch at rank {i}");
    } else {
      assert!(
        (got - exp).abs() < 1e-6,
        "Mismatch at rank {i}: got {got}, exp {exp}"
      );
    }
  }

  let expected_byrevrank = [5.0, 5.0, 5.0, 5.0, 4.0, 3.0, 3.0, 1.0, -f64::INFINITY];
  let by_rev: Vec<f64> = ranks.iter().map(|&r| td.byrevrank(r)).collect();
  for (i, (&got, &exp)) in by_rev.iter().zip(expected_byrevrank.iter()).enumerate() {
    if exp.is_infinite() {
      assert!(got.is_infinite() && got < 0.0, "Mismatch at revrank {i}");
    } else {
      assert!(
        (got - exp).abs() < 1e-6,
        "Mismatch at revrank {i}: got {got}, exp {exp}"
      );
    }
  }
}

#[test]
fn test_tdigest_trimmed_mean_suite() {
  let mut td = TDigestState::new(100.0);
  td.add_batch(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
  td.ensure_merged();

  assert!((td.trimmed_mean(0.1, 0.9) - 5.5).abs() < 0.01);
  assert!((td.trimmed_mean(0.0, 1.0) - 5.5).abs() < 0.01);
  assert!((td.trimmed_mean(0.25, 0.75) - 5.5).abs() < 0.01);

  // 空 digest
  let mut empty_td = TDigestState::new(100.0);
  assert!(empty_td.trimmed_mean(0.1, 0.9).is_nan());

  // 无序输入
  let mut un_td = TDigestState::new(100.0);
  un_td.add_batch(&[5.0, 2.0, 8.0, 1.0, 9.0, 3.0, 7.0, 4.0, 6.0, 10.0]);
  un_td.ensure_merged();
  assert!((un_td.trimmed_mean(0.1, 0.9) - 5.5).abs() < 0.01);

  // 复杂带负数分布
  let mut c_td = TDigestState::new(100.0);
  c_td.add_batch(&[-10.0, 5.0, -3.0, 5.0, 0.0, 5.0, 3.0, -5.0, 10.0, -10.0]);
  c_td.ensure_merged();
  let c_mean = c_td.trimmed_mean(0.2, 0.8);
  assert!(!c_mean.is_nan());
  assert!((c_mean - 5.0 / 6.0).abs() < 0.02);
}

#[test]
fn test_tdigest_cdf_kvrocks_comprehensive() {
  // 1. 基本 CDF 测试
  let mut td = TDigestState::new(100.0);
  let samples = [
    1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0, 4.0, 5.0, 5.0, 5.0, 5.0, 5.0,
  ];
  td.add_batch(&samples);
  td.ensure_merged();

  let cdf_vals = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
  let expected = [0.00, 0.03, 0.13, 0.29, 0.53, 0.83, 1.00];
  for (i, (&v, &exp)) in cdf_vals.iter().zip(expected.iter()).enumerate() {
    let got = td.cdf(v);
    assert!(
      (got - exp).abs() < 0.02,
      "Mismatch at index {i}: got {got}, exp {exp}"
    );
  }

  // 2. 空 TDigest
  let mut empty_td = TDigestState::new(100.0);
  assert!(empty_td.cdf(1.0).is_nan());

  // 3. 重复值
  let mut dup_td = TDigestState::new(100.0);
  dup_td.add_batch(&[10.0, 10.0, 10.0, 20.0, 20.0]);
  dup_td.ensure_merged();
  let dup_res: Vec<f64> = [5.0, 10.0, 20.0, 25.0]
    .iter()
    .map(|&v| dup_td.cdf(v))
    .collect();
  let dup_exp = [0.0, 0.3, 0.8, 1.0];
  for (i, (&got, &exp)) in dup_res.iter().zip(dup_exp.iter()).enumerate() {
    assert!(
      (got - exp).abs() < 0.01,
      "Dup mismatch at {i}: got {got}, exp {exp}"
    );
  }

  // 4. 带符号 0
  let mut sz_td = TDigestState::new(100.0);
  sz_td.add_batch(&[-1.0, 0.0, 1.0]);
  sz_td.ensure_merged();
  assert!((sz_td.cdf(-0.0) - 0.5).abs() < 0.01);
  assert!((sz_td.cdf(0.0) - 0.5).abs() < 0.01);

  // 5. 单一加权质心 (10 个 5.0)
  let mut s_td = TDigestState::new(100.0);
  s_td.add_batch(&[5.0; 10]);
  s_td.ensure_merged();
  assert_eq!(s_td.cdf(4.0), 0.0);
  assert_eq!(s_td.cdf(5.0), 0.5);
  assert_eq!(s_td.cdf(6.0), 1.0);

  // 6. 全小于 min / 全大于 max
  let mut b_td = TDigestState::new(100.0);
  b_td.add_batch(&[1.0, 2.0]);
  b_td.ensure_merged();
  assert_eq!(b_td.cdf(-2.0), 0.0);
  assert_eq!(b_td.cdf(0.0), 0.0);
  assert_eq!(b_td.cdf(3.0), 1.0);
  assert_eq!(b_td.cdf(5.0), 1.0);

  // 7. 单例质心不跨质心插值
  let mut sing_td = TDigestState::new(100.0);
  sing_td.add_batch(&[0.0, 10.0, 20.0]);
  sing_td.ensure_merged();
  assert!((sing_td.cdf(11.0) - 2.0 / 3.0).abs() < 0.01);
}

#[test]
fn test_tdigest_embed_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 错误处理：不存在的 key
  assert!(db.tdigest_add("non_exist", &[1.0]).is_err());
  assert!(db.tdigest_min("non_exist").is_err());
  assert!(db.tdigest_max("non_exist").is_err());
  assert!(db.tdigest_quantile("non_exist", &[0.5]).is_err());
  assert!(db.tdigest_cdf("non_exist", &[5.0]).is_err());
  assert!(db.tdigest_rank("non_exist", &[5.0]).is_err());
  assert!(db.tdigest_revrank("non_exist", &[5.0]).is_err());
  assert!(db.tdigest_byrank("non_exist", &[0]).is_err());
  assert!(db.tdigest_byrevrank("non_exist", &[0]).is_err());
  assert!(db.tdigest_trimmed_mean("non_exist", 0.1, 0.9).is_err());
  assert!(db.tdigest_reset("non_exist").is_err());
  assert!(db.tdigest_info("non_exist").is_err());

  db.tdigest_create("td_key", 100.0)?;
  assert!(db.tdigest_create("td_key", 100.0).is_err());

  // 空 key 状态下的各命令返回值
  assert!(db.tdigest_min("td_key")?.is_nan());
  assert!(db.tdigest_max("td_key")?.is_nan());
  let empty_q = db.tdigest_quantile("td_key", &[0.5])?;
  assert_eq!(empty_q, vec![None]);
  let empty_cdf = db.tdigest_cdf("td_key", &[5.0])?;
  assert_eq!(empty_cdf, vec![None]);
  let empty_rank = db.tdigest_rank("td_key", &[5.0])?;
  assert_eq!(empty_rank, vec![-2]);
  let empty_revrank = db.tdigest_revrank("td_key", &[5.0])?;
  assert_eq!(empty_revrank, vec![-2]);
  let empty_byrank = db.tdigest_byrank("td_key", &[0])?;
  assert_eq!(empty_byrank, vec![None]);
  let empty_byrevrank = db.tdigest_byrevrank("td_key", &[0])?;
  assert_eq!(empty_byrevrank, vec![None]);
  let empty_mean = db.tdigest_trimmed_mean("td_key", 0.1, 0.9)?;
  assert_eq!(empty_mean, None);

  let vals: Vec<f64> = (1..=100).map(|i| i as f64).collect();
  db.tdigest_add("td_key", &vals)?;

  assert_eq!(db.tdigest_min("td_key")?, 1.0);
  assert_eq!(db.tdigest_max("td_key")?, 100.0);

  let info = db.tdigest_info("td_key")?;
  assert_eq!(info.compression, 100);
  assert_eq!(info.observations, 100);
  assert_eq!(info.minimum, Some(1.0));
  assert_eq!(info.maximum, Some(100.0));

  let quantiles = db.tdigest_quantile("td_key", &[0.25, 0.5, 0.75, 0.99])?;
  assert_eq!(quantiles.len(), 4);
  for q in quantiles {
    assert!(q.is_some());
  }

  let cdfs = db.tdigest_cdf("td_key", &[25.0, 50.0, 75.0])?;
  assert_eq!(cdfs.len(), 3);
  for c in cdfs {
    assert!(c.is_some());
  }

  let ranks = db.tdigest_rank("td_key", &[25.0, 50.0, 75.0])?;
  assert_eq!(ranks.len(), 3);

  let revranks = db.tdigest_revrank("td_key", &[25.0, 50.0, 75.0])?;
  assert_eq!(revranks.len(), 3);

  let byranks = db.tdigest_byrank("td_key", &[25, 50, 75])?;
  assert_eq!(byranks.len(), 3);

  let byrevranks = db.tdigest_byrevrank("td_key", &[25, 50, 75])?;
  assert_eq!(byrevranks.len(), 3);

  let mean = db.tdigest_trimmed_mean("td_key", 0.1, 0.9)?;
  assert!(mean.is_some());

  // 截断均值参数校验
  assert!(db.tdigest_trimmed_mean("td_key", 0.9, 0.1).is_err());
  assert!(db.tdigest_trimmed_mean("td_key", -0.1, 0.9).is_err());
  assert!(db.tdigest_trimmed_mean("td_key", 0.1, 1.5).is_err());

  // 测试合并 (TDIGEST.MERGE)
  db.tdigest_create("src1", 100.0)?;
  db.tdigest_add("src1", &[10.0, 20.0, 30.0])?;
  db.tdigest_create("src2", 100.0)?;
  db.tdigest_add("src2", &[40.0, 50.0, 60.0])?;

  db.tdigest_merge(
    "merged_dst",
    &["src1", "src2"],
    [TDigestMerge::Compression(200)],
  )?;

  let merged_info = db.tdigest_info("merged_dst")?;
  assert_eq!(merged_info.compression, 200);
  assert_eq!(merged_info.observations, 6);
  assert_eq!(merged_info.minimum, Some(10.0));
  assert_eq!(merged_info.maximum, Some(60.0));

  // 测试已有 dest 合并 (不使用 OVERRIDE，累加数据)
  db.tdigest_merge("merged_dst", &["src1"], [])?;
  let merged_info2 = db.tdigest_info("merged_dst")?;
  assert_eq!(merged_info2.observations, 9); // 6 + 3 = 9

  // 测试使用 OVERRIDE 覆盖已有 dest
  db.tdigest_merge("merged_dst", &["src1"], [TDigestMerge::Override])?;
  let merged_info3 = db.tdigest_info("merged_dst")?;
  assert_eq!(merged_info3.observations, 3); // 覆盖为 3

  // 测试重置 (TDIGEST.RESET)
  db.tdigest_reset("td_key")?;
  let q_after_reset = db.tdigest_quantile("td_key", &[0.5])?;
  assert_eq!(q_after_reset[0], None);

  Ok(())
}

#[test]
fn test_tdigest_kvrocks_metadata_compatibility() {
  let mut meta = TDigestMeta::new(100, 1700000000, 101);
  meta.minimum = 1.5;
  meta.maximum = 99.5;
  meta.merged_nodes = 50;
  meta.total_weight = 1000;
  meta.merged_weight = 1000;
  meta.total_observations = 1000;
  meta.merge_times = 5;

  let kvrocks_bytes = meta.encode_kvrocks();
  assert_eq!(kvrocks_bytes.len(), TDigestMeta::KVROCKS_ENCODED_SIZE);
  assert_eq!(kvrocks_bytes.len(), 97);

  let decoded = TDigestMeta::decode(&kvrocks_bytes).expect("decode kvrocks tdigest failed");
  assert_eq!(decoded.compression, 100);
  assert_eq!(decoded.capacity, 100 * 6 + 10);
  assert_eq!(decoded.merged_nodes, 50);
  assert_eq!(decoded.total_weight, 1000);
  assert_eq!(decoded.merged_weight, 1000);
  assert_eq!(decoded.minimum, 1.5);
  assert_eq!(decoded.maximum, 99.5);
  assert_eq!(decoded.total_observations, 1000);
  assert_eq!(decoded.merge_times, 5);
}

#[test]
fn test_tdigest_capacity_calculation() {
  assert_eq!(calculate_capacity(1), 16);
  assert_eq!(calculate_capacity(100), 610);
  assert_eq!(calculate_capacity(200), 1024);
  assert_eq!(calculate_capacity(1000), 1024);
}

#[test]
fn test_tdigest_merger_validate() {
  let merger = TDigestMerger::new(100);
  let centroids = vec![
    Centroid::new(1.0, 1.0),
    Centroid::new(2.0, 1.0),
    Centroid::new(3.0, 1.0),
  ];
  assert!(merger.validate(&centroids, 3.0).is_ok());

  // 异常大的单一质心
  let oversized = vec![Centroid::new(1.0, 10.0), Centroid::new(100.0, 1000.0)];
  let merger_small = TDigestMerger::new(1000);
  assert!(merger_small.validate(&oversized, 1010.0).is_err());
}

#[test]
fn test_tdigest_merger_tool() {
  let mut dest = TDigestState::new(100.0);
  dest.add(10.0, 1.0);
  dest.add(20.0, 1.0);

  let mut src1 = TDigestState::new(100.0);
  src1.add(30.0, 1.0);
  src1.add(40.0, 1.0);

  let mut src2 = TDigestState::new(100.0);
  src2.add(50.0, 1.0);

  TDigestMergerTool::merge(&mut dest, &mut [src1, src2]);
  assert_eq!(dest.total_observations, 5);
  assert_eq!(dest.min(), 10.0);
  assert_eq!(dest.max(), 50.0);
}

#[test]
fn test_tdigest_kvrocks_merge_matrix() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. MergeIntoExistingDestWithoutOverride: dest(3) + src1(3) + src2(5) = 11, min=-200, max=100
  db.tdigest_create("merge_no_ovr_src1", 100.0)?;
  db.tdigest_add("merge_no_ovr_src1", &[1.0, 2.0, 3.0])?;
  db.tdigest_create("merge_no_ovr_src2", 100.0)?;
  db.tdigest_add("merge_no_ovr_src2", &[4.0, 5.0, 6.0, 100.0, -200.0])?;
  db.tdigest_create("merge_no_ovr_dest", 100.0)?;
  db.tdigest_add("merge_no_ovr_dest", &[7.0, 8.0, 9.0])?;

  db.tdigest_merge(
    "merge_no_ovr_dest",
    &["merge_no_ovr_src1", "merge_no_ovr_src2"],
    [],
  )?;
  let info = db.tdigest_info("merge_no_ovr_dest")?;
  assert_eq!(info.observations, 11);
  assert_eq!(info.minimum, Some(-200.0));
  assert_eq!(info.maximum, Some(100.0));

  // 2. MergeIntoExistingDestWithOverride: dest data overwritten, observations = 8
  db.tdigest_create("merge_ovr_src1", 100.0)?;
  db.tdigest_add("merge_ovr_src1", &[1.0, 2.0, 3.0])?;
  db.tdigest_create("merge_ovr_src2", 100.0)?;
  db.tdigest_add("merge_ovr_src2", &[4.0, 5.0, 6.0, 100.0, -200.0])?;
  db.tdigest_create("merge_ovr_dest", 100.0)?;
  db.tdigest_add("merge_ovr_dest", &[7.0, 8.0, 9.0])?;

  db.tdigest_merge(
    "merge_ovr_dest",
    &["merge_ovr_src1", "merge_ovr_src2"],
    [TDigestMerge::Override],
  )?;
  let info_ovr = db.tdigest_info("merge_ovr_dest")?;
  assert_eq!(info_ovr.observations, 8);
  assert_eq!(info_ovr.minimum, Some(-200.0));
  assert_eq!(info_ovr.maximum, Some(100.0));

  // 3. MergeDestInSourceListWithoutOverride: dest merged twice -> 3 + 3 + 2 = 8
  db.tdigest_create("merge_dup_dest", 100.0)?;
  db.tdigest_add("merge_dup_dest", &[1.0, 2.0, 3.0])?;
  db.tdigest_create("merge_dup_src", 100.0)?;
  db.tdigest_add("merge_dup_src", &[10.0, 20.0])?;

  db.tdigest_merge("merge_dup_dest", &["merge_dup_dest", "merge_dup_src"], [])?;
  let info_dup = db.tdigest_info("merge_dup_dest")?;
  assert_eq!(info_dup.observations, 8);
  assert_eq!(info_dup.minimum, Some(1.0));
  assert_eq!(info_dup.maximum, Some(20.0));

  // 4. MergeDestInSourceListWithOverride: dest in source list counted once -> 3 + 2 = 5
  db.tdigest_create("merge_dup_ovr_dest", 100.0)?;
  db.tdigest_add("merge_dup_ovr_dest", &[1.0, 2.0, 3.0])?;
  db.tdigest_create("merge_dup_ovr_src", 100.0)?;
  db.tdigest_add("merge_dup_ovr_src", &[10.0, 20.0])?;

  db.tdigest_merge(
    "merge_dup_ovr_dest",
    &["merge_dup_ovr_dest", "merge_dup_ovr_src"],
    [TDigestMerge::Override],
  )?;
  let info_dup_ovr = db.tdigest_info("merge_dup_ovr_dest")?;
  assert_eq!(info_dup_ovr.observations, 5);
  assert_eq!(info_dup_ovr.minimum, Some(1.0));
  assert_eq!(info_dup_ovr.maximum, Some(20.0));

  // 5. MergeIntoNewDest: dest does not exist initially -> 3 + 3 = 6
  db.tdigest_create("new_src1", 100.0)?;
  db.tdigest_add("new_src1", &[1.0, 2.0, 3.0])?;
  db.tdigest_create("new_src2", 100.0)?;
  db.tdigest_add("new_src2", &[4.0, 5.0, 6.0])?;

  db.tdigest_merge("new_dest_sketch", &["new_src1", "new_src2"], [])?;
  let info_new = db.tdigest_info("new_dest_sketch")?;
  assert_eq!(info_new.observations, 6);
  assert_eq!(info_new.minimum, Some(1.0));
  assert_eq!(info_new.maximum, Some(6.0));

  // 6. MergeIntoExistingDestKeepsCompression: dest(100) + src(200) without OVERRIDE -> dest compression stays 100
  db.tdigest_create("comp_keep_src", 200.0)?;
  db.tdigest_add("comp_keep_src", &[1.0])?;
  db.tdigest_create("comp_keep_dest", 100.0)?;
  db.tdigest_add("comp_keep_dest", &[2.0])?;

  db.tdigest_merge("comp_keep_dest", &["comp_keep_src"], [])?;
  let info_keep = db.tdigest_info("comp_keep_dest")?;
  assert_eq!(info_keep.compression, 100);
  assert_eq!(info_keep.observations, 2);

  // 7. MergeWithOverrideTakesMaxCompression: dest(100) + src1(200) + src2(300) with OVERRIDE -> compression becomes 300
  db.tdigest_create("max_comp_src1", 200.0)?;
  db.tdigest_add("max_comp_src1", &[1.0])?;
  db.tdigest_create("max_comp_src2", 300.0)?;
  db.tdigest_add("max_comp_src2", &[2.0])?;
  db.tdigest_create("max_comp_dest", 100.0)?;
  db.tdigest_add("max_comp_dest", &[3.0])?;

  db.tdigest_merge(
    "max_comp_dest",
    &["max_comp_src1", "max_comp_src2"],
    [TDigestMerge::Override],
  )?;
  let info_max = db.tdigest_info("max_comp_dest")?;
  assert_eq!(info_max.compression, 300);
  assert_eq!(info_max.observations, 2);

  // 8. MergeWithUserSpecifiedCompression: dest(100) + src(200) with COMPRESSION 50 -> compression becomes 50
  db.tdigest_create("user_comp_src", 200.0)?;
  db.tdigest_add("user_comp_src", &[1.0])?;
  db.tdigest_create("user_comp_dest", 100.0)?;
  db.tdigest_add("user_comp_dest", &[2.0])?;

  db.tdigest_merge(
    "user_comp_dest",
    &["user_comp_src"],
    [TDigestMerge::Compression(50)],
  )?;
  let info_user = db.tdigest_info("user_comp_dest")?;
  assert_eq!(info_user.compression, 50);
  assert_eq!(info_user.observations, 2);

  Ok(())
}

#[test]
fn test_tdigest_plenty_quantile_and_stress() {
  let mut td = TDigestState::new(100.0);
  let sample_count = 10000;
  let from = -100.0;
  let to = 100.0;

  let samples: Vec<f64> = (0..sample_count)
    .map(|i| from + (i as f64) * (to - from) / (sample_count as f64))
    .collect();

  td.add_batch(&samples);
  td.ensure_merged();

  assert_eq!(td.total_observations, sample_count as u64);
  assert!((td.min() - (-100.0)).abs() < 1e-9);
  assert!((td.max() - 99.98).abs() < 0.1);

  let quantile_count = 144;
  for q_idx in 1..=quantile_count {
    let q = (q_idx as f64) / (quantile_count as f64);
    let approx = td.quantile(q);
    let exact = from + q * (to - from);
    assert!(
      (approx - exact).abs() < 3.0,
      "Quantile mismatch for q={q}: approx={approx}, exact={exact}"
    );
  }
}

#[test]
fn test_tdigest_add_chunks_and_repeated_values() {
  let mut td = TDigestState::new(100.0);
  let samples = [
    -10.0, -9.0, -8.0, -7.0, -6.0, -5.0, -4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0,
    7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
  ];

  for _ in 0..100 {
    td.add_batch(&samples);
  }
  td.ensure_merged();

  assert_eq!(td.total_observations, 2300);
  let qs = [0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99];
  let expected = [-10.0, -9.0, -8.0, -5.0, 1.0, 7.0, 10.0, 11.0, 12.0];

  for (&q, &exp) in qs.iter().zip(expected.iter()) {
    let got = td.quantile(q);
    assert!(
      (got - exp).abs() < 1.0,
      "Quantile mismatch for q={q}: got={got}, exp={exp}"
    );
  }
}

#[test]
fn test_tdigest_cdf_skewed_and_repeated_distributions() {
  // 1. Skewed distribution (100 zeros + 1..=10)
  let mut skewed = TDigestState::new(200.0);
  let zeros = vec![0.0; 100];
  let ones_to_ten: Vec<f64> = (1..=10).map(|i| i as f64).collect();
  skewed.add_batch(&zeros);
  skewed.add_batch(&ones_to_ten);
  skewed.ensure_merged();

  let skewed_vals = [0.0, 1.0, 5.0, 10.0];
  let skewed_exp = [0.4545, 0.91, 0.95, 1.00];
  for (&v, &exp) in skewed_vals.iter().zip(skewed_exp.iter()) {
    let got = skewed.cdf(v);
    assert!(
      (got - exp).abs() < 0.05,
      "Skewed CDF mismatch for v={v}: got={got}, exp={exp}"
    );
  }

  // 2. Repeated centroids distribution
  let mut rep = TDigestState::new(200.0);
  let rep_samples = [
    -40.0, -36.0, -27.0, -13.0, -12.0, 7.0, 7.0, 25.0, 47.0, 50.0,
  ];
  rep.add_batch(&rep_samples);
  rep.ensure_merged();

  let rep_vals = [0.0, 6.9, 7.0, 7.1, 10.0];
  let rep_exp = [0.5, 0.5, 0.6, 0.7, 0.7];
  for (&v, &exp) in rep_vals.iter().zip(rep_exp.iter()) {
    let got = rep.cdf(v);
    assert!(
      (got - exp).abs() < 0.05,
      "Repeated CDF mismatch for v={v}: got={got}, exp={exp}"
    );
  }

  // 3. Compressed centroids interpolation
  let mut comp_td = TDigestState::new(10.0);
  let hundred_samples: Vec<f64> = (0..100).map(|i| i as f64).collect();
  comp_td.add_batch(&hundred_samples);
  comp_td.ensure_merged();

  let cdf_vals = [20.0, 40.0, 50.0, 60.0, 80.0];
  let cdf_exp = [0.205, 0.405, 0.505, 0.605, 0.805];
  for (&v, &exp) in cdf_vals.iter().zip(cdf_exp.iter()) {
    let got = comp_td.cdf(v);
    assert!(
      (got - exp).abs() < 0.05,
      "Compressed CDF mismatch for v={v}: got={got}, exp={exp}"
    );
  }
}

#[test]
fn test_tdigest_nan_edge_cases() {
  let mut td = TDigestState::new(100.0);
  td.add_batch(&[1.0, 2.0, 3.0, 4.0, 5.0]);
  td.ensure_merged();

  // CDF with positive and negative NaNs
  let cdf_res = tdigest_cdf_calc(
    &td.centroids,
    td.min,
    td.max,
    td.total_weight,
    &[f64::NAN, -f64::NAN, 1.0, 3.0, 5.0],
  );
  assert!(cdf_res[0].is_nan());
  assert!(cdf_res[1].is_nan());
  assert!(!cdf_res[2].is_nan());
  assert!(!cdf_res[3].is_nan());
  assert!(!cdf_res[4].is_nan());

  // Rank with NaNs
  let rank_res = tdigest_rank_calc(
    &td.centroids,
    td.min,
    td.max,
    td.total_weight,
    &[f64::NAN, -f64::NAN, 1.0, 3.0, 5.0],
    false,
  );
  assert_eq!(rank_res[0], -2);
  assert_eq!(rank_res[1], -2);
  assert_eq!(rank_res[2], 0);

  // RevRank with NaNs
  let rev_res = tdigest_rank_calc(
    &td.centroids,
    td.min,
    td.max,
    td.total_weight,
    &[f64::NAN, -f64::NAN, 1.0, 3.0, 5.0],
    true,
  );
  assert_eq!(rev_res[0], -2);
  assert_eq!(rev_res[1], -2);
  assert_eq!(rev_res[2], 4);
}

#[test]
fn test_tdigest_unmerged_buffer_info_and_kway_merge() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 测试单次 add 后 unmerged_nodes 统计
  db.tdigest_create("td_unmerged_test", 100.0)?;
  db.tdigest_add("td_unmerged_test", &[42.0])?;

  let info_before = db.tdigest_info("td_unmerged_test")?;
  assert_eq!(info_before.unmerged_nodes, 1);
  assert_eq!(info_before.observations, 1);
  assert_eq!(info_before.merged_nodes, 0);

  // 调用 quantile 会隐式合并并写回
  let q = db.tdigest_quantile("td_unmerged_test", &[0.5])?;
  assert_eq!(q, vec![Some(42.0)]);

  let info_after = db.tdigest_info("td_unmerged_test")?;
  assert_eq!(info_after.unmerged_nodes, 0);
  assert_eq!(info_after.merged_nodes, 1);
  assert_eq!(info_after.observations, 1);

  // 2. 测试 5 路质心流 K-Way 堆归并
  let mut srcs = Vec::new();
  for i in 1..=5 {
    let name = format!("kway_src_{i}");
    db.tdigest_create(&name, 50.0)?;
    let data: Vec<f64> = (1..=20).map(|x| (x * i) as f64).collect();
    db.tdigest_add(&name, &data)?;
    srcs.push(name);
  }

  let src_refs: Vec<&str> = srcs.iter().map(|s| s.as_str()).collect();
  db.tdigest_merge("kway_dest", &src_refs, [TDigestMerge::Compression(100)])?;

  let kway_info = db.tdigest_info("kway_dest")?;
  assert_eq!(kway_info.observations, 100);
  assert_eq!(kway_info.minimum, Some(1.0));
  assert_eq!(kway_info.maximum, Some(100.0));

  let p50 = db.tdigest_quantile("kway_dest", &[0.5])?;
  assert!(p50[0].is_some());

  Ok(())
}

#[test]
fn test_tdigest_merge_leading_empty_centroids() {
  use wedb_embed::tdigest::{CentroidsWithDelta, tdigest_merge_buffer_and_centroids};

  let empty_list = CentroidsWithDelta {
    centroids: Vec::new(),
    delta: 100,
    min: f64::MAX,
    max: -f64::MAX,
    total_weight: 0.0,
  };
  let non_empty_list = CentroidsWithDelta {
    centroids: vec![Centroid::new(10.0, 1.0), Centroid::new(20.0, 2.0)],
    delta: 100,
    min: 10.0,
    max: 20.0,
    total_weight: 3.0,
  };

  let merged = tdigest_merge_buffer_and_centroids(&[], &[empty_list, non_empty_list], 100);
  assert_eq!(merged.centroids.len(), 2);
  assert_eq!(merged.centroids[0].mean, 10.0);
  assert_eq!(merged.centroids[1].mean, 20.0);
  assert_eq!(merged.total_weight, 3.0);
  assert_eq!(merged.min, 10.0);
  assert_eq!(merged.max, 20.0);
}

#[test]
fn test_tdigest_single_element_zero_alloc_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.tdigest_create("td_single", 100.0)?;
  db.tdigest_add_one("td_single", 10.0)?;
  db.tdigest_add_one("td_single", 20.0)?;
  db.tdigest_add_one("td_single", 30.0)?;

  assert_eq!(db.tdigest_min("td_single")?, 10.0);
  assert_eq!(db.tdigest_max("td_single")?, 30.0);

  let q50 = db.tdigest_quantile_one("td_single", 0.5)?;
  assert!(q50.is_some());
  assert!((q50.unwrap() - 20.0).abs() < 1.0);

  let rank = db.tdigest_rank_one("td_single", 20.0)?;
  assert!(rank >= 0);

  let revrank = db.tdigest_revrank_one("td_single", 20.0)?;
  assert!(revrank >= 0);

  let byrank = db.tdigest_byrank_one("td_single", 1)?;
  assert!(byrank.is_some());

  let byrevrank = db.tdigest_byrevrank_one("td_single", 1)?;
  assert!(byrevrank.is_some());

  let cdf = db.tdigest_cdf_one("td_single", 20.0)?;
  assert!(cdf.is_some());

  Ok(())
}
