//! # Stateful Encoder
//!
//! ## Overview
//! This example demonstrates stateful compression using `Encoder`.
//! In streaming and columnar scenarios, consecutive chunks typically share the same decimal precision.
//! By caching optimal parameters and reusing pre-allocated scratch buffers, `Encoder` avoids repeated sampling across adjacent chunks.
//!
//! ## Key Benefits
//! - Parameter caching: bypasses parameter sampling on adjacent chunks.
//! - Scratch buffer reuse: avoids heap reallocations during chunk compression.
//! - Automatic invalidation: transparently falls back to re-sampling when distribution shifts.
//!
//! ---
//!
//! # 状态化编码器
//!
//! ## 概述
//! 本示例演示使用 `Encoder` 进行状态化压缩。
//! 在流式数据与列式存储场景中，连续数据块通常共享相同的十进制精度。
//! 通过缓存最优参数并复用预分配的工作缓冲区，`Encoder` 消除了相邻数据块之间的重复采样开销。
//!
//! ## 核心优势
//! - 参数缓存：相邻数据块跳过参数采样探测。
//! - 工作缓冲区复用：避免块压缩过程中的堆内存重新分配。
//! - 自动失效检验：当数据分布发生突变时透明重新采样。

use anyhow::Result;
use fastalp::{Encoder, decompress};

fn main() -> Result<()> {
  // Initialize stateful encoder with pre-allocated buffer capacity
  // 初始化状态化编码器并指定预分配缓冲区容量
  let mut encoder = Encoder::<f64>::with_capacity(1024);

  // Generate synthetic decimal time-series chunks with 2 decimal places
  // 生成具有两位小数的时序模拟数据块
  let chunk_a: Vec<f64> = (0..1024).map(|i| 25.0 + (i as f64) * 0.25).collect();
  let chunk_b: Vec<f64> = (1024..2048).map(|i| 25.0 + (i as f64) * 0.25).collect();

  let mut compressed = Vec::new();

  // First chunk: detects and caches optimal parameters
  // 首个数据块：探测并缓存最优参数
  encoder.compress_into(&chunk_a, &mut compressed);
  let params_a = encoder.cached_params.expect("parameters should be cached");
  println!(
    "Chunk A cached params: exp={}, fac={}, use_div={}",
    params_a.exp, params_a.fac, params_a.use_div
  );

  // Verify reconstruction accuracy
  // 验证反解精度
  let restored_a: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored_a, chunk_a);

  // Second chunk: reuses cached parameters without re-sampling
  // 第二个数据块：直接复用缓存参数，无需重新采样
  compressed.clear();
  encoder.compress_into(&chunk_b, &mut compressed);
  assert_eq!(encoder.cached_params, Some(params_a));

  // Verify second chunk reconstruction
  // 验证第二个数据块反解
  let restored_b: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored_b, chunk_b);

  // Third chunk with shifted distribution to trigger automatic rescue
  // 分布发生突变的第三个数据块，触发自动挽救机制
  let mut chunk_c: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0];
  for i in 4..1024 {
    chunk_c.push(100.0 + (i as f64) * 0.000001);
  }

  compressed.clear();
  encoder.compress_into(&chunk_c, &mut compressed);
  let params_c = encoder.cached_params.expect("parameters should be updated");
  println!(
    "Chunk C updated params: exp={}, fac={}, use_div={}",
    params_c.exp, params_c.fac, params_c.use_div
  );
  assert_eq!(params_c.exp, 6);

  let restored_c: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored_c, chunk_c);

  // Reset encoder when switching to a different column or metric
  // 当切换至不同列或不同指标时重置编码器
  encoder.reset();
  assert!(encoder.cached_params.is_none());

  println!("Stateful encoder roundtrip completed successfully");
  Ok(())
}
