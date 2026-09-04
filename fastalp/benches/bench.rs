//! Microbenchmark suite for fastalp compression and decompression algorithms.
//! fastalp 浮点压缩与解压算法微基准测试套件。

use divan::{Bencher, black_box};
use fastalp::{Encoder, compress, compress_into, decompress_into};

/// Global high-performance memory allocator (mimalloc).
/// 全局高性能内存分配器（mimalloc）。
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Single block standard vector element count (1024 floats).
/// 单块标准向量元素数（1024 浮点数）。
const BLOCK_SIZE: usize = 1024;

/// Large batch throughput evaluation vector size (65536 floats, f64 is 512 KB, f32 is 256 KB).
/// 大批量吞吐评测向量元素数（65536 浮点数，对应 f64 512 KB，f32 256 KB）。
const LARGE_BATCH_SIZE: usize = 65536;

/// Divan benchmark runner main entry point.
/// Divan 基准测试运行器主入口函数。
fn main() {
  divan::main();
}

/// Generate simulated sensor decimal fractional f64 time-series data.
/// 生成模拟传感器十进制小数 f64 时序数据。
fn generate_sensor_data(count: usize) -> Vec<f64> {
  (0..count).map(|i| (200 + (i % 150)) as f64 * 0.1).collect()
}

/// Generate simulated sensor decimal fractional f32 time-series data.
/// 生成模拟传感器十进制小数 f32 时序数据。
fn generate_sensor_data_f32(count: usize) -> Vec<f32> {
  (0..count)
    .map(|i| (200 + (i % 150)) as f32 * 0.1f32)
    .collect()
}

/// Generate smooth monotonically increasing f64 time-series data.
/// 生成平滑线性递增 f64 时序数据。
fn generate_ramp_data(count: usize) -> Vec<f64> {
  (0..count).map(|i| 100.0 + i as f64 * 0.05).collect()
}

/// Generate smooth monotonically increasing f32 time-series data.
/// 生成平滑线性递增 f32 时序数据。
fn generate_ramp_data_f32(count: usize) -> Vec<f32> {
  (0..count).map(|i| 100.0f32 + i as f32 * 0.05f32).collect()
}

/// Generate deterministic pseudo-random f64 data with fixed seed.
/// 生成带固定随机种子的确定性伪随机 f64 数据。
fn generate_random_data(count: usize) -> Vec<f64> {
  fastrand::seed(42);
  (0..count)
    .map(|_| {
      let base = fastrand::i32(-1000..1000) as f64;
      let dec = fastrand::u32(0..1000) as f64 * 0.01;
      base + dec
    })
    .collect()
}

// ───────────────────────────────────────────────
// 1. f64 compression & decompression benchmarks (1024 floats, standard vector size)
// 1. f64 压缩与解压基准测试（1024 浮点数，标准向量大小）
// ───────────────────────────────────────────────

/// Benchmark f64 sensor decimal data compression with dynamic parameter sampling (cold mode).
/// 评测 f64 传感器十进制小数数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f64_sensor_sampled_1024(bencher: Bencher) {
  let data = generate_sensor_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 sensor decimal data compression with cached parameters (warm kernel mode).
/// 评测 f64 传感器十进制小数数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f64_sensor_cached_1024(bencher: Bencher) {
  let data = generate_sensor_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 sensor decimal data decompression throughput.
/// 评测 f64 传感器十进制小数数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f64_sensor_1024(bencher: Bencher) {
  let data = generate_sensor_data(BLOCK_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f64> = Vec::with_capacity(BLOCK_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

/// Benchmark f64 ramp linear data compression with dynamic parameter sampling (cold mode).
/// 评测 f64 线性递增数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f64_ramp_sampled_1024(bencher: Bencher) {
  let data = generate_ramp_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 ramp linear data compression with cached parameters (warm kernel mode).
/// 评测 f64 线性递增数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f64_ramp_cached_1024(bencher: Bencher) {
  let data = generate_ramp_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 ramp linear data decompression throughput.
/// 评测 f64 线性递增数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f64_ramp_1024(bencher: Bencher) {
  let data = generate_ramp_data(BLOCK_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f64> = Vec::with_capacity(BLOCK_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

/// Benchmark f64 random noise data compression with dynamic parameter sampling (cold mode).
/// 评测 f64 随机噪声数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f64_random_sampled_1024(bencher: Bencher) {
  let data = generate_random_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 random noise data compression with cached parameters (warm kernel mode).
/// 评测 f64 随机噪声数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f64_random_cached_1024(bencher: Bencher) {
  let data = generate_random_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 random noise data decompression throughput.
/// 评测 f64 随机噪声数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f64_random_1024(bencher: Bencher) {
  let data = generate_random_data(BLOCK_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f64> = Vec::with_capacity(BLOCK_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

/// Benchmark f64 identical constant data compression with dynamic parameter sampling (cold mode).
/// 评测 f64 完全相同常量数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f64_identical_sampled_1024(bencher: Bencher) {
  let data = vec![98.6f64; BLOCK_SIZE];
  let mut dst = Vec::with_capacity(64);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 identical constant data compression with cached parameters (warm kernel mode).
/// 评测 f64 完全相同常量数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f64_identical_cached_1024(bencher: Bencher) {
  let data = vec![98.6f64; BLOCK_SIZE];
  let mut dst = Vec::with_capacity(64);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 identical constant data decompression throughput.
/// 评测 f64 完全相同常量数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f64_identical_1024(bencher: Bencher) {
  let data = vec![98.6f64; BLOCK_SIZE];
  let compressed = compress(&data[..]);
  let mut dst: Vec<f64> = Vec::with_capacity(BLOCK_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

// ───────────────────────────────────────────────
// 2. f32 compression & decompression benchmarks (1024 floats)
// 2. f32 压缩与解压基准测试（1024 浮点数）
// ───────────────────────────────────────────────

/// Benchmark f32 sensor decimal data compression with dynamic parameter sampling (cold mode).
/// 评测 f32 传感器十进制小数数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f32_sensor_sampled_1024(bencher: Bencher) {
  let data = generate_sensor_data_f32(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f32 sensor decimal data compression with cached parameters (warm kernel mode).
/// 评测 f32 传感器十进制小数数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f32_sensor_cached_1024(bencher: Bencher) {
  let data = generate_sensor_data_f32(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f32 sensor decimal data decompression throughput.
/// 评测 f32 传感器十进制小数数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f32_sensor_1024(bencher: Bencher) {
  let data = generate_sensor_data_f32(BLOCK_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f32> = Vec::with_capacity(BLOCK_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

/// Benchmark f32 ramp linear data compression with dynamic parameter sampling (cold mode).
/// 评测 f32 线性递增数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f32_ramp_sampled_1024(bencher: Bencher) {
  let data = generate_ramp_data_f32(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f32 ramp linear data compression with cached parameters (warm kernel mode).
/// 评测 f32 线性递增数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f32_ramp_cached_1024(bencher: Bencher) {
  let data = generate_ramp_data_f32(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f32 ramp linear data decompression throughput.
/// 评测 f32 线性递增数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f32_ramp_1024(bencher: Bencher) {
  let data = generate_ramp_data_f32(BLOCK_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f32> = Vec::with_capacity(BLOCK_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

// ───────────────────────────────────────────────
// 3. Large batch throughput benchmarks (65536 floats, f64 512 KB, f32 256 KB)
// 3. 大块批量压缩与解压吞吐测试（65536 浮点数，f64 为 512 KB，f32 为 256 KB）
// ───────────────────────────────────────────────

/// Benchmark f64 large batch data compression with dynamic parameter sampling (cold mode).
/// 评测 f64 大批量数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f64_large_batch_sampled(bencher: Bencher) {
  let data = generate_sensor_data(LARGE_BATCH_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 large batch data compression with cached parameters (warm kernel mode).
/// 评测 f64 大批量数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f64_large_batch_cached(bencher: Bencher) {
  let data = generate_sensor_data(LARGE_BATCH_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f64 large batch data decompression throughput.
/// 评测 f64 大批量数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f64_large_batch(bencher: Bencher) {
  let data = generate_sensor_data(LARGE_BATCH_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f64> = Vec::with_capacity(LARGE_BATCH_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}

/// Benchmark f32 large batch data compression with dynamic parameter sampling (cold mode).
/// 评测 f32 大批量数据动态参数采样压缩（冷启动模式）。
#[divan::bench]
fn bench_compress_f32_large_batch_sampled(bencher: Bencher) {
  let data = generate_sensor_data_f32(LARGE_BATCH_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f32 large batch data compression with cached parameters (warm kernel mode).
/// 评测 f32 大批量数据复用已缓存参数压缩（热状态纯内核模式）。
#[divan::bench]
fn bench_compress_f32_large_batch_cached(bencher: Bencher) {
  let data = generate_sensor_data_f32(LARGE_BATCH_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  let mut encoder = Encoder::new();
  encoder.compress_into(&data, &mut dst);
  bencher.bench_local(|| {
    dst.clear();
    encoder.compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

/// Benchmark f32 large batch data decompression throughput.
/// 评测 f32 大批量数据解压吞吐率。
#[divan::bench]
fn bench_decompress_f32_large_batch(bencher: Bencher) {
  let data = generate_sensor_data_f32(LARGE_BATCH_SIZE);
  let compressed = compress(&data[..]);
  let mut dst: Vec<f32> = Vec::with_capacity(LARGE_BATCH_SIZE);
  bencher.bench_local(|| {
    dst.clear();
    decompress_into(&compressed, &mut dst).unwrap();
    black_box(&dst);
  });
}
