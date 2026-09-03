use divan::{Bencher, black_box};
use fastalp::{compress, compress_into, decompress_into};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// 单块标准向量元素数 (1024 浮点数)
const BLOCK_SIZE: usize = 1024;

/// 大批量吞吐评测向量元素数 (65536 浮点数，对应 f64 512 KB，f32 256 KB)
const LARGE_BATCH_SIZE: usize = 65536;

fn main() {
  divan::main();
}

/// 生成模拟传感器十进制小数时序数据
fn generate_sensor_data(count: usize) -> Vec<f64> {
  (0..count).map(|i| (200 + (i % 150)) as f64 * 0.1).collect()
}

/// 生成模拟传感器 f32 小数时序数据
fn generate_sensor_data_f32(count: usize) -> Vec<f32> {
  (0..count)
    .map(|i| (200 + (i % 150)) as f32 * 0.1f32)
    .collect()
}

/// 生成平滑线性递增时序数据 (评测 Delta-ALP 差分模式)
fn generate_ramp_data(count: usize) -> Vec<f64> {
  (0..count).map(|i| 100.0 + i as f64 * 0.05).collect()
}

/// 生成平滑线性递增 f32 时序数据 (评测 Delta-ALP 差分模式)
fn generate_ramp_data_f32(count: usize) -> Vec<f32> {
  (0..count).map(|i| 100.0f32 + i as f32 * 0.05f32).collect()
}

/// 生成确定性随机浮点数数据 (带固定随机种子)
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
// 1. f64 压缩与解压基准测试 (1024 浮点数，标准向量大小)
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_compress_f64_sensor_1024(bencher: Bencher) {
  let data = generate_sensor_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data, &mut dst);
    black_box(&dst);
  });
}

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

#[divan::bench]
fn bench_compress_f64_ramp_1024(bencher: Bencher) {
  let data = generate_ramp_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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

#[divan::bench]
fn bench_compress_f64_random_1024(bencher: Bencher) {
  let data = generate_random_data(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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

#[divan::bench]
fn bench_compress_f64_identical_1024(bencher: Bencher) {
  let data = vec![98.6f64; BLOCK_SIZE];
  let mut dst = Vec::with_capacity(64);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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
// 2. f32 压缩与解压基准测试 (1024 浮点数)
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_compress_f32_sensor_1024(bencher: Bencher) {
  let data = generate_sensor_data_f32(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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

#[divan::bench]
fn bench_compress_f32_ramp_1024(bencher: Bencher) {
  let data = generate_ramp_data_f32(BLOCK_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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
// 3. 大块批量压缩与解压吞吐测试 (65536 浮点数，f64 为 512 KB，f32 为 256 KB)
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_compress_f64_large_batch(bencher: Bencher) {
  let data = generate_sensor_data(LARGE_BATCH_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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

#[divan::bench]
fn bench_compress_f32_large_batch(bencher: Bencher) {
  let data = generate_sensor_data_f32(LARGE_BATCH_SIZE);
  let mut dst = Vec::with_capacity(data.len() * 2 + 16);
  bencher.bench_local(|| {
    dst.clear();
    compress_into(&data[..], &mut dst);
    black_box(&dst);
  });
}

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
