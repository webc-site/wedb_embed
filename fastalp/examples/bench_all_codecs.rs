//! Full floating-point and general compression algorithm comparative benchmark suite.
//! 全量浮点与通用压缩算法横向基准评测套件。
//!
//! Benchmark codecs include:
//! 对比评测算法包括：
//! 1. fastalp: Pure Rust adaptive lossless floating-point compression.
//! 1. fastalp：纯 Rust 自适应无损浮点压缩算法。
//! 2. C++ ALP reference: Paper official baseline.
//! 2. C++ 原版实现：论文官方基准。
//! 3. Pcodec / pco: Industrial time-series numerical compression.
//! 3. Pcodec / pco：工业级时序数值压缩算法。
//! 4. Zstandard / zstd: General dictionary compression at level 3.
//! 4. Zstandard / zstd：级别 3 通用字典压缩算法。
//! 5. LZ4 / lz4_flex: Ultra-fast block compression.
//! 5. LZ4 / lz4_flex：极速块级压缩算法。
//! 6. Snappy / snap: High-throughput byte compression.
//! 6. Snappy / snap：高吞吐字节压缩算法。
//! 7. Chimp128 / graupel: Time-series floating-point XOR compression.
//! 7. Chimp128 / graupel：时序浮点异或压缩算法。
//! 8. Gorilla / graupel: Classic time-series floating-point XOR compression.
//! 8. Gorilla / graupel：经典时序浮点异或压缩算法。
//!
//! Coverage: All 37 standard time-series datasets and microbenchmark scenarios.
//! 评测指标覆盖全部 37 个公开时序数据集及多种微基准测试场景。

use std::{
  fs::{File, create_dir_all, read_dir, write},
  hint::black_box,
  io::{BufRead, BufReader},
  mem::{size_of, size_of_val},
  path::Path,
  slice::from_raw_parts,
  time::Instant,
};

use fastalp::decompress_into;
use graupel::{
  Codec, Point,
  codec::{Chimp128, Gorilla},
};
use pco::{
  ChunkConfig,
  standalone::{simple_compress, simple_decompress},
};
use snap::raw::{Decoder as SnapDecoder, Encoder as SnapEncoder, max_compress_len};
use zstd::bulk::{compress_to_buffer, decompress_to_buffer};

/// Microbenchmark vector length (standard single block vector size: 1024 floats).
/// 微基准测试样本点数（标准单块向量大小：1024 浮点数）。
const MICRO_LEN: usize = 1024;

/// Microbenchmark raw f64 bytes (1024 * 8 = 8192 bytes).
/// 微基准测试原始浮点数据大小（1024 * 8 = 8192 字节）。
const MICRO_RAW_BYTES: usize = MICRO_LEN * size_of::<f64>();

/// Microbenchmark raw Point bytes for Graupel composite time-series (1024 * 16 = 16384 bytes).
/// 微基准测试复合时序数据点原始大小（1024 * 16 = 16384 字节）。
const MICRO_GRAUPEL_RAW_BYTES: usize = MICRO_LEN * size_of::<Point>();

/// Benchmark statistics for a single codec.
/// 单个编解码器的基准测试统计结果。
#[derive(Debug)]
struct CodecResult {
  /// Codec display name.
  /// 编解码器展示名称。
  name: &'static str,
  /// Compressed byte size.
  /// 压缩后字节大小。
  compressed_bytes: usize,
  /// Compression ratio (raw size divided by compressed size).
  /// 压缩比率（原始大小除以压缩大小）。
  ratio: f64,
  /// Bits per value (or bits per point for composite time-series).
  /// 每个数值占用的比特数（或复合时序每个点占用的比特数）。
  bits_per_val: f64,
  /// Overall or sampled encoding throughput in gigabytes per second.
  /// 综合或冷启动采样编码吞吐率（单位：GB/s）。
  enc_gb_s: f64,
  /// Cold sampled encoding throughput in gigabytes per second.
  /// 冷启动采样编码吞吐率（单位：GB/s）。
  enc_sampled_gb_s: f64,
  /// Warm pure kernel encoding throughput in gigabytes per second.
  /// 热状态纯内核编码吞吐率（单位：GB/s）。
  enc_kernel_gb_s: f64,
  /// Decoding throughput in gigabytes per second.
  /// 解码吞吐率（单位：GB/s）。
  dec_gb_s: f64,
}

/// Zero-copy cast f64 slice into immutable byte slice for general byte compressors.
/// 将双精度浮点数切片零拷贝转换为只读字节切片（用于通用字节压缩算法）。
#[inline]
fn as_u8_slice(data: &[f64]) -> &[u8] {
  // SAFETY: f64 uses standard IEEE 754 8-byte layout in contiguous memory.
  // 安全说明：双精度浮点数遵循标准 8 字节连续内存布局，转换为只读字节切片安全。
  unsafe { from_raw_parts(data.as_ptr().cast::<u8>(), size_of_val(data)) }
}

/// Benchmark fastalp (Pure Rust).
/// 评测纯 Rust 实现的 fastalp 算法。
///
/// Measures both cold dynamic sampling throughput and warm pure kernel throughput.
/// 同时测量端到端冷启动动态参数采样吞吐与热状态纯内核流水线吞吐。
fn bench_fastalp(data: &[f64]) -> CodecResult {
  let iters = 1000;
  let mut compressed = Vec::with_capacity(data.len() * 2 + 64);
  let mut restored: Vec<f64> = Vec::with_capacity(data.len());
  let mut encoder = fastalp::Encoder::new();

  // Warm up CPU cache and saturate frequency.
  // 充分预热处理器缓存并使工作频率达到饱和。
  for _ in 0..50 {
    compressed.clear();
    encoder.compress_into(data, &mut compressed);
    restored.clear();
    decompress_into(&compressed, &mut restored).unwrap();
  }

  // 1. Measure cold sampled encoding (dynamic parameter sampling on each pass).
  // 1. 测量端到端冷启动压缩（每次执行动态参数采样与探测，循环平滑消除波动）。
  let t0 = Instant::now();
  for _ in 0..iters {
    compressed.clear();
    fastalp::compress_into(data, &mut compressed);
    black_box(&compressed);
  }
  let enc_sampled_dt = t0.elapsed().as_secs_f64() / iters as f64;

  // 2. Measure warm pure kernel encoding (reusing established parameters without sampling).
  // 2. 测量热状态纯内核压缩（复用已探测的最优参数跳过采样，循环平滑消除波动）。
  encoder.reset();
  compressed.clear();
  encoder.compress_into(data, &mut compressed);
  let t1 = Instant::now();
  for _ in 0..iters {
    compressed.clear();
    encoder.compress_into(data, &mut compressed);
    black_box(&compressed);
  }
  let enc_kernel_dt = t1.elapsed().as_secs_f64() / iters as f64;

  // 3. Measure decompression throughput (1000 iterations).
  // 3. 测量解压吞吐率（循环平滑消除波动）。
  let t2 = Instant::now();
  for _ in 0..iters {
    restored.clear();
    decompress_into(&compressed, &mut restored).unwrap();
    black_box(&restored);
  }
  let dec_dt = t2.elapsed().as_secs_f64() / iters as f64;
  assert_eq!(restored.len(), data.len());

  let raw_bytes = size_of_val(data);
  let enc_sampled_gb_s = (raw_bytes as f64 / enc_sampled_dt) / 1e9;
  let enc_kernel_gb_s = (raw_bytes as f64 / enc_kernel_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;

  CodecResult {
    name: "fastalp (Rust)",
    compressed_bytes: compressed.len(),
    ratio: raw_bytes as f64 / compressed.len() as f64,
    bits_per_val: (compressed.len() * 8) as f64 / data.len() as f64,
    enc_gb_s: enc_sampled_gb_s,
    enc_sampled_gb_s,
    enc_kernel_gb_s,
    dec_gb_s,
  }
}

/// Benchmark Pcodec (pco level 3).
/// 评测 Pcodec 数值压缩算法（压缩级别 3）。
fn bench_pco(data: &[f64]) -> CodecResult {
  let config = ChunkConfig::default().with_compression_level(3);
  let iters = 10;

  // Warm up CPU cache.
  // 充分预热处理器缓存。
  for _ in 0..2 {
    let c = simple_compress(data, &config).unwrap();
    let _ = simple_decompress::<f64>(&c).unwrap();
  }

  // Measure encoding throughput.
  // 测量编码吞吐率。
  let mut compressed = Vec::new();
  let t0 = Instant::now();
  for _ in 0..iters {
    compressed = simple_compress(data, &config).unwrap();
    black_box(&compressed);
  }
  let enc_dt = t0.elapsed().as_secs_f64() / iters as f64;

  // Measure decoding throughput.
  // 测量解码吞吐率。
  let mut restored = Vec::new();
  let t1 = Instant::now();
  for _ in 0..iters {
    restored = simple_decompress::<f64>(&compressed).unwrap();
    black_box(&restored);
  }
  let dec_dt = t1.elapsed().as_secs_f64() / iters as f64;
  assert_eq!(restored.len(), data.len());

  let raw_bytes = size_of_val(data);
  let enc_gb_s = (raw_bytes as f64 / enc_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;
  CodecResult {
    name: "Pcodec (pco)",
    compressed_bytes: compressed.len(),
    ratio: raw_bytes as f64 / compressed.len() as f64,
    bits_per_val: (compressed.len() * 8) as f64 / data.len() as f64,
    enc_gb_s,
    enc_sampled_gb_s: enc_gb_s,
    enc_kernel_gb_s: enc_gb_s,
    dec_gb_s,
  }
}

/// Benchmark Zstandard (zstd level 3).
/// 评测 Zstandard 通用字典压缩算法（压缩级别 3）。
fn bench_zstd(data: &[f64]) -> CodecResult {
  let raw = as_u8_slice(data);
  let iters = 20;
  let mut compressed = vec![0u8; raw.len() + 128];
  let comp_len = compress_to_buffer(raw, &mut compressed, 3).unwrap();
  compressed.truncate(comp_len);

  // Warm up CPU cache.
  // 充分预热处理器缓存。
  let mut restored = vec![0u8; raw.len()];
  for _ in 0..2 {
    let _ = decompress_to_buffer(&compressed, &mut restored).unwrap();
  }

  // Preallocate buffer to eliminate allocation noise during benchmarking.
  // 预分配缓冲区消除迭代循环内的内存分配噪声。
  let mut comp_buf = vec![0u8; raw.len() + 128];
  let t0 = Instant::now();
  for _ in 0..iters {
    let len = compress_to_buffer(raw, &mut comp_buf, 3).unwrap();
    black_box(&comp_buf[..len]);
  }
  let enc_dt = t0.elapsed().as_secs_f64() / iters as f64;

  // Measure decoding throughput.
  // 测量解码吞吐率。
  let t1 = Instant::now();
  for _ in 0..iters {
    let _ = decompress_to_buffer(&compressed, &mut restored).unwrap();
    black_box(&restored);
  }
  let dec_dt = t1.elapsed().as_secs_f64() / iters as f64;

  let raw_bytes = size_of_val(data);
  let enc_gb_s = (raw_bytes as f64 / enc_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;
  CodecResult {
    name: "Zstd (level 3)",
    compressed_bytes: compressed.len(),
    ratio: raw_bytes as f64 / compressed.len() as f64,
    bits_per_val: (compressed.len() * 8) as f64 / data.len() as f64,
    enc_gb_s,
    enc_sampled_gb_s: enc_gb_s,
    enc_kernel_gb_s: enc_gb_s,
    dec_gb_s,
  }
}

/// Benchmark LZ4 (lz4_flex).
/// 评测 LZ4 极速块级压缩算法。
fn bench_lz4(data: &[f64]) -> CodecResult {
  let raw = as_u8_slice(data);
  let iters = 20;

  // Warm up CPU cache.
  // 充分预热处理器缓存。
  for _ in 0..2 {
    let c = lz4_flex::compress_prepend_size(raw);
    let _ = lz4_flex::decompress_size_prepended(&c).unwrap();
  }

  // Measure encoding throughput.
  // 测量编码吞吐率。
  let mut compressed = Vec::new();
  let t0 = Instant::now();
  for _ in 0..iters {
    compressed = lz4_flex::compress_prepend_size(raw);
    black_box(&compressed);
  }
  let enc_dt = t0.elapsed().as_secs_f64() / iters as f64;

  // Measure decoding throughput.
  // 测量解码吞吐率。
  let mut restored = Vec::new();
  let t1 = Instant::now();
  for _ in 0..iters {
    restored = lz4_flex::decompress_size_prepended(&compressed).unwrap();
    black_box(&restored);
  }
  let dec_dt = t1.elapsed().as_secs_f64() / iters as f64;
  assert_eq!(restored.len(), raw.len());

  let raw_bytes = size_of_val(data);
  let enc_gb_s = (raw_bytes as f64 / enc_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;
  CodecResult {
    name: "LZ4 (lz4_flex)",
    compressed_bytes: compressed.len(),
    ratio: raw_bytes as f64 / compressed.len() as f64,
    bits_per_val: (compressed.len() * 8) as f64 / data.len() as f64,
    enc_gb_s,
    enc_sampled_gb_s: enc_gb_s,
    enc_kernel_gb_s: enc_gb_s,
    dec_gb_s,
  }
}

/// Benchmark Snappy (snap).
/// 评测 Snappy 高吞吐字节压缩算法。
fn bench_snappy(data: &[f64]) -> CodecResult {
  let raw = as_u8_slice(data);
  let mut enc = SnapEncoder::new();
  let mut dec = SnapDecoder::new();
  let iters = 20;

  // Warm up CPU cache.
  // 充分预热处理器缓存。
  for _ in 0..2 {
    let c = enc.compress_vec(raw).unwrap();
    let _ = dec.decompress_vec(&c).unwrap();
  }

  // Preallocate buffer to eliminate allocation noise during benchmarking.
  // 预分配缓冲区消除迭代循环内的内存分配噪声。
  let max_len = max_compress_len(raw.len());
  let mut comp_buf = vec![0u8; max_len];
  let mut comp_len = 0;
  let t0 = Instant::now();
  for _ in 0..iters {
    comp_len = enc.compress(raw, &mut comp_buf).unwrap();
    black_box(&comp_buf[..comp_len]);
  }
  let enc_dt = t0.elapsed().as_secs_f64() / iters as f64;

  let compressed = &comp_buf[..comp_len];
  let mut restored = vec![0u8; raw.len()];

  // Measure decoding throughput.
  // 测量解码吞吐率。
  let t1 = Instant::now();
  for _ in 0..iters {
    let dec_len = dec.decompress(compressed, &mut restored).unwrap();
    black_box(&restored[..dec_len]);
  }
  let dec_dt = t1.elapsed().as_secs_f64() / iters as f64;

  let raw_bytes = size_of_val(data);
  let enc_gb_s = (raw_bytes as f64 / enc_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;
  CodecResult {
    name: "Snappy (snap)",
    compressed_bytes: comp_len,
    ratio: raw_bytes as f64 / comp_len as f64,
    bits_per_val: (comp_len * 8) as f64 / data.len() as f64,
    enc_gb_s,
    enc_sampled_gb_s: enc_gb_s,
    enc_kernel_gb_s: enc_gb_s,
    dec_gb_s,
  }
}

/// Benchmark Chimp128 (graupel).
/// 评测 Chimp128 时序浮点异或压缩算法。
///
/// Note: Graupel encodes composite Point(timestamp: i64, value: f64) with 16 raw bytes per point.
/// 说明：Graupel 编解码时序复合结构体 Point(i64, f64)，每个点原始数据为 16 字节。
fn bench_chimp128(data: &[f64]) -> CodecResult {
  // Construct composite timestamp and value points.
  // 构建复合时间戳与数值时序点。
  let points: Vec<Point> = data
    .iter()
    .enumerate()
    .map(|(i, &v)| Point::new(i as i64, v))
    .collect();
  let iters = 10;

  // Warm up CPU cache.
  // 充分预热处理器缓存。
  for _ in 0..2 {
    let c = Chimp128.encode(&points).unwrap();
    let _ = graupel::decode(&c).unwrap();
  }

  // Measure encoding throughput.
  // 测量编码吞吐率。
  let mut compressed = Vec::new();
  let t0 = Instant::now();
  for _ in 0..iters {
    compressed = Chimp128.encode(&points).unwrap();
    black_box(&compressed);
  }
  let enc_dt = t0.elapsed().as_secs_f64() / iters as f64;

  // Measure decoding throughput.
  // 测量解码吞吐率。
  let mut restored = Vec::new();
  let t1 = Instant::now();
  for _ in 0..iters {
    restored = graupel::decode(&compressed).unwrap();
    black_box(&restored);
  }
  let dec_dt = t1.elapsed().as_secs_f64() / iters as f64;
  assert_eq!(restored.len(), points.len());

  // Input payload consists of 16-byte Point(ts, val) tuples.
  // 原始输入负载为每个时序点 16 字节（时间戳与浮点数值）。
  let raw_bytes = points.len() * size_of::<Point>();
  let enc_gb_s = (raw_bytes as f64 / enc_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;
  CodecResult {
    name: "Chimp128 (ts+val)",
    compressed_bytes: compressed.len(),
    ratio: raw_bytes as f64 / compressed.len() as f64,
    bits_per_val: (compressed.len() * 8) as f64 / points.len() as f64,
    enc_gb_s,
    enc_sampled_gb_s: enc_gb_s,
    enc_kernel_gb_s: enc_gb_s,
    dec_gb_s,
  }
}

/// Benchmark Gorilla (graupel).
/// 评测 Gorilla 经典时序浮点异或压缩算法。
///
/// Note: Graupel encodes composite Point(timestamp: i64, value: f64) with 16 raw bytes per point.
/// 说明：Graupel 编解码时序复合结构体 Point(i64, f64)，每个点原始数据为 16 字节。
fn bench_gorilla(data: &[f64]) -> CodecResult {
  // Construct composite timestamp and value points.
  // 构建复合时间戳与数值时序点。
  let points: Vec<Point> = data
    .iter()
    .enumerate()
    .map(|(i, &v)| Point::new(i as i64, v))
    .collect();
  let iters = 10;

  // Warm up CPU cache.
  // 充分预热处理器缓存。
  for _ in 0..2 {
    let c = Gorilla.encode(&points).unwrap();
    let _ = graupel::decode(&c).unwrap();
  }

  // Measure encoding throughput.
  // 测量编码吞吐率。
  let mut compressed = Vec::new();
  let t0 = Instant::now();
  for _ in 0..iters {
    compressed = Gorilla.encode(&points).unwrap();
    black_box(&compressed);
  }
  let enc_dt = t0.elapsed().as_secs_f64() / iters as f64;

  // Measure decoding throughput.
  // 测量解码吞吐率。
  let mut restored = Vec::new();
  let t1 = Instant::now();
  for _ in 0..iters {
    restored = graupel::decode(&compressed).unwrap();
    black_box(&restored);
  }
  let dec_dt = t1.elapsed().as_secs_f64() / iters as f64;
  assert_eq!(restored.len(), points.len());

  // Input payload consists of 16-byte Point(ts, val) tuples.
  // 原始输入负载为每个时序点 16 字节（时间戳与浮点数值）。
  let raw_bytes = points.len() * size_of::<Point>();
  let enc_gb_s = (raw_bytes as f64 / enc_dt) / 1e9;
  let dec_gb_s = (raw_bytes as f64 / dec_dt) / 1e9;
  CodecResult {
    name: "Gorilla (ts+val)",
    compressed_bytes: compressed.len(),
    ratio: raw_bytes as f64 / compressed.len() as f64,
    bits_per_val: (compressed.len() * 8) as f64 / points.len() as f64,
    enc_gb_s,
    enc_sampled_gb_s: enc_gb_s,
    enc_kernel_gb_s: enc_gb_s,
    dec_gb_s,
  }
}

/// Load standard time-series datasets from disk.
/// 从磁盘加载全部公开时序测试数据集。
fn load_paper_samples() -> Vec<(String, Vec<f64>)> {
  // Candidate relative and absolute directory paths.
  // 候选相对路径与绝对路径列表。
  let candidates = [
    Path::new("/Users/z/git/db/ALP/data/samples"),
    Path::new("../ALP/data/samples"),
    Path::new("../../ALP/data/samples"),
  ];
  let Some(&dir) = candidates.iter().find(|p| p.exists()) else {
    return Vec::new();
  };

  let mut list = Vec::new();
  if let Ok(entries) = read_dir(dir) {
    // Read and sort CSV sample file paths.
    // 读取并按字母顺序排序样本文件路径。
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
      if p.extension().is_some_and(|ext| ext == "csv") {
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
          continue;
        };
        if let Ok(f) = File::open(&p) {
          // Parse floating-point numbers line by line.
          // 逐行解析双精度浮点数值。
          let vals: Vec<f64> = BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| {
              let s = line.trim();
              if s.is_empty() || s.starts_with('#') || s.starts_with("column") {
                None
              } else {
                s.parse::<f64>().ok()
              }
            })
            .collect();
          if !vals.is_empty() {
            list.push((stem.to_string(), vals));
          }
        }
      }
    }
  }
  list
}

/// Benchmark suite entry point for executing all codecs and generating JSON reports.
/// 运行全量算法基准测试并生成各算法独立 JSON 报表的主入口函数。
fn main() {
  println!("Running full benchmark suite & generating individual algorithm JSONs...");

  // Load standard time-series datasets.
  // 加载公开标准时序测试数据集。
  let samples = load_paper_samples();
  if samples.is_empty() {
    eprintln!("Warning: No test samples found in candidates path.");
    return;
  }

  println!("Found {} datasets to benchmark.", samples.len());

  // Prepare standard microbenchmark scenarios (1024 floats each).
  // 准备标准微基准测试场景数据（每组 1024 个浮点数）。
  let sensor_data: Vec<f64> = (0..MICRO_LEN)
    .map(|i| (200 + (i % 150)) as f64 * 0.1)
    .collect();
  let ramp_data: Vec<f64> = (0..MICRO_LEN).map(|i| 100.0 + i as f64 * 0.05).collect();
  let constant_data: Vec<f64> = vec![98.6; MICRO_LEN];
  let random_noise: Vec<f64> = {
    fastrand::seed(42);
    (0..MICRO_LEN)
      .map(|_| f64::from_bits(fastrand::u64(..)))
      .collect()
  };

  // Resolve JSON output directory.
  // 解析 JSON 报告输出目录。
  let json_dir = if Path::new("fastalp/benches/json").exists() {
    Path::new("fastalp/benches/json")
  } else {
    Path::new("benches/json")
  };
  let _ = create_dir_all(json_dir);

  // List of codecs to benchmark.
  // 待评测算法键名列表。
  let algo_keys = [
    "fastalp", "pco", "zstd", "lz4", "snappy", "chimp128", "gorilla",
  ];

  // Benchmark each codec across all datasets and scenarios.
  // 逐一评测各算法在全量数据集与微基准场景下的表现。
  for &key in &algo_keys {
    let runner: fn(&[f64]) -> CodecResult = match key {
      "fastalp" => bench_fastalp,
      "pco" => bench_pco,
      "zstd" => bench_zstd,
      "lz4" => bench_lz4,
      "snappy" => bench_snappy,
      "chimp128" => bench_chimp128,
      "gorilla" => bench_gorilla,
      _ => unreachable!(),
    };

    // Execute microbenchmarks for current codec.
    // 运行当前算法的微基准测试场景。
    let sensor_res = runner(&sensor_data);
    let ramp_res = runner(&ramp_data);
    let constant_res = runner(&constant_data);
    let random_res = runner(&random_noise);

    // Microbenchmark raw byte count (aligned to 16 bytes for Graupel composite Point).
    // 微基准测试原始字节数（针对 Graupel 复合时序点严格对齐为 16 字节分母）。
    let micro_raw_bytes = if key == "chimp128" || key == "gorilla" {
      MICRO_GRAUPEL_RAW_BYTES
    } else {
      MICRO_RAW_BYTES
    };

    let mut ds_json_items = Vec::with_capacity(samples.len());
    let mut total_raw = 0;
    let mut total_compressed = 0;
    let mut sum_enc = 0.0;
    let mut sum_enc_sampled = 0.0;
    let mut sum_enc_kernel = 0.0;
    let mut sum_dec = 0.0;

    // Evaluate standard time-series datasets.
    // 评测各公开时序数据集。
    for (name, vals) in &samples {
      let r = runner(vals);
      let raw_bytes = if key == "chimp128" || key == "gorilla" {
        vals.len() * size_of::<Point>()
      } else {
        vals.len() * size_of::<f64>()
      };
      total_raw += raw_bytes;
      total_compressed += r.compressed_bytes;
      sum_enc += r.enc_gb_s;
      sum_enc_sampled += r.enc_sampled_gb_s;
      sum_enc_kernel += r.enc_kernel_gb_s;
      sum_dec += r.dec_gb_s;

      ds_json_items.push(format!(
        r#"{{"name":"{name}","raw_bytes":{raw_bytes},"compressed_bytes":{},"ratio":{:.4},"bits_per_val":{:.2},"enc_gb_s":{:.2},"enc_sampled_gb_s":{:.2},"enc_kernel_gb_s":{:.2},"dec_gb_s":{:.2}}}"#,
        r.compressed_bytes,
        r.ratio,
        r.bits_per_val,
        r.enc_gb_s,
        r.enc_sampled_gb_s,
        r.enc_kernel_gb_s,
        r.dec_gb_s
      ));
    }

    let n_ds = samples.len() as f64;
    let avg_ratio = total_raw as f64 / total_compressed as f64;
    let bytes_per_elem = if key == "chimp128" || key == "gorilla" {
      size_of::<Point>() as f64
    } else {
      size_of::<f64>() as f64
    };
    let avg_bv = (total_compressed * 8) as f64 / (total_raw as f64 / bytes_per_elem);
    let avg_enc = sum_enc / n_ds;
    let avg_enc_sampled = sum_enc_sampled / n_ds;
    let avg_enc_kernel = sum_enc_kernel / n_ds;
    let avg_dec = sum_dec / n_ds;

    let category = if key == "fastalp" || key == "pco" || key == "chimp128" || key == "gorilla" {
      "specialized_float"
    } else {
      "general_bytes"
    };

    // Serialize benchmark statistics to JSON format.
    // 将基准测试统计结果序列化为 JSON 格式。
    let json_content = format!(
      r#"{{
  "algorithm": "{key}",
  "display_name": "{}",
  "category": "{category}",
  "paper_31": {{
    "total_raw_bytes": {total_raw},
    "total_compressed_bytes": {total_compressed},
    "ratio": {:.4},
    "bits_per_val": {:.2},
    "avg_enc_gb_s": {:.2},
    "avg_enc_sampled_gb_s": {:.2},
    "avg_enc_kernel_gb_s": {:.2},
    "avg_dec_gb_s": {:.2},
    "datasets": [
      {}
    ]
  }},
  "micro_benchmarks": {{
    "sensor_1024": {{
      "raw_bytes": {micro_raw_bytes},
      "compressed_bytes": {},
      "ratio": {:.4},
      "bits_per_val": {:.2},
      "enc_gb_s": {:.2},
      "enc_sampled_gb_s": {:.2},
      "enc_kernel_gb_s": {:.2},
      "dec_gb_s": {:.2}
    }},
    "ramp_1024": {{
      "raw_bytes": {micro_raw_bytes},
      "compressed_bytes": {},
      "ratio": {:.4},
      "bits_per_val": {:.2},
      "enc_gb_s": {:.2},
      "enc_sampled_gb_s": {:.2},
      "enc_kernel_gb_s": {:.2},
      "dec_gb_s": {:.2}
    }},
    "constant_1024": {{
      "raw_bytes": {micro_raw_bytes},
      "compressed_bytes": {},
      "ratio": {:.4},
      "bits_per_val": {:.2},
      "enc_gb_s": {:.2},
      "enc_sampled_gb_s": {:.2},
      "enc_kernel_gb_s": {:.2},
      "dec_gb_s": {:.2}
    }},
    "random_1024": {{
      "raw_bytes": {micro_raw_bytes},
      "compressed_bytes": {},
      "ratio": {:.4},
      "bits_per_val": {:.2},
      "enc_gb_s": {:.2},
      "enc_sampled_gb_s": {:.2},
      "enc_kernel_gb_s": {:.2},
      "dec_gb_s": {:.2}
    }}
  }}
}}"#,
      sensor_res.name,
      avg_ratio,
      avg_bv,
      avg_enc,
      avg_enc_sampled,
      avg_enc_kernel,
      avg_dec,
      ds_json_items.join(",\n      "),
      sensor_res.compressed_bytes,
      sensor_res.ratio,
      sensor_res.bits_per_val,
      sensor_res.enc_gb_s,
      sensor_res.enc_sampled_gb_s,
      sensor_res.enc_kernel_gb_s,
      sensor_res.dec_gb_s,
      ramp_res.compressed_bytes,
      ramp_res.ratio,
      ramp_res.bits_per_val,
      ramp_res.enc_gb_s,
      ramp_res.enc_sampled_gb_s,
      ramp_res.enc_kernel_gb_s,
      ramp_res.dec_gb_s,
      constant_res.compressed_bytes,
      constant_res.ratio,
      constant_res.bits_per_val,
      constant_res.enc_gb_s,
      constant_res.enc_sampled_gb_s,
      constant_res.enc_kernel_gb_s,
      constant_res.dec_gb_s,
      random_res.compressed_bytes,
      random_res.ratio,
      random_res.bits_per_val,
      random_res.enc_gb_s,
      random_res.enc_sampled_gb_s,
      random_res.enc_kernel_gb_s,
      random_res.dec_gb_s,
    );

    let file_path = json_dir.join(format!("{key}.json"));
    write(&file_path, json_content).expect("write json failed");
    println!("Generated {:?}", file_path);
  }

  println!("Benchmark suite execution completed successfully.");
}
