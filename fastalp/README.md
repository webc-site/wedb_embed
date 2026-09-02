[English](#en) | [中文](#zh)

[![crates.io](https://img.shields.io/crates/v/fastalp.svg)](https://crates.io/crates/fastalp)
[![docs.rs](https://docs.rs/fastalp/badge.svg)](https://docs.rs/fastalp)

---

<a name="en"></a>

# fastalp : Adaptive Lossless Floating-Point Compression in Rust

Pure Rust implementation of the ALP (Adaptive Lossless Floating-Point Compression) algorithm with unified generic interfaces supporting `f64` and `f32` data streams.

---

- [Overview](#overview)
- [Usage](#usage)
  - [Installation](#installation)
  - [Basic Compression and Decompression](#basic-compression-and-decompression)
  - [In-Place Buffer Reuse](#in-place-buffer-reuse)
  - [Single-Precision Floating-Point Data](#single-precision-floating-point-data)
- [Features](#features)
- [Architecture & Design](#architecture-design)
  - [Compression Pipeline](#compression-pipeline)
  - [Decompression Pipeline](#decompression-pipeline)
- [Tech Stack](#tech-stack)
- [Directory Structure](#directory-structure)
- [Benchmarks & C++ Comparison](#benchmarks-c-comparison)
  - [Benchmark Environment & Toolchain](#benchmark-environment-toolchain)
  - [Side-by-Side Throughput Comparison](#side-by-side-throughput-comparison)
  - [Real-World Datasets Compression Ratio](#real-world-datasets-compression-ratio)
- [Architecture Comparison & Engineering Optimizations](#architecture-comparison--engineering-optimizations)
  - [Constant Sequence Fast Detection & Zero-Heap Allocation](#constant-sequence-fast-detection--zero-heap-allocation)
  - [Raw Fallback Safeguard Against Negative Compression](#raw-fallback-safeguard-against-negative-compression)
  - [Zero-Heap Direct Streaming Decompression](#zero-heap-direct-streaming-decompression)
  - [Zero-Multiplication LUT Decompression Acceleration](#zero-multiplication-lut-decompression-acceleration)
  - [Pure 128-bit Register Bitpacker](#pure-128-bit-register-bitpacker)
  - [Sample-Space Cost Lower-Bound Pruning](#sample-space-cost-lower-bound-pruning)
  - [Branchless Arithmetic & Precomputed Constants](#branchless-arithmetic-precomputed-constants)

- [Overview](#overview)
- [Usage](#usage)
  - [Installation](#installation)
  - [Basic Compression and Decompression](#basic-compression-and-decompression)
  - [In-Place Buffer Reuse](#in-place-buffer-reuse)
  - [Single-Precision Floating-Point Data](#single-precision-floating-point-data)
- [Features](#features)
- [Architecture & Design](#architecture-design)
  - [Compression Pipeline](#compression-pipeline)
  - [Decompression Pipeline](#decompression-pipeline)
- [Tech Stack](#tech-stack)
- [Directory Structure](#directory-structure)
- [Benchmarks & C++ Comparison](#benchmarks-c-comparison)
  - [Benchmark Environment & Toolchain](#benchmark-environment-toolchain)
  - [Side-by-Side Throughput Comparison](#side-by-side-throughput-comparison)
  - [Real-World Datasets Compression Ratio](#real-world-datasets-compression-ratio)
- [Architecture Comparison & Engineering Optimizations](#architecture-comparison-engineering-optimizations)
  - [Constant Sequence Fast Detection & Zero-Heap Allocation](#constant-sequence-fast-detection-zero-heap-allocation)
  - [Raw Fallback Safeguard Against Negative Compression](#raw-fallback-safeguard-against-negative-compression)
  - [Zero-Heap Direct Streaming Decompression](#zero-heap-direct-streaming-decompression)
  - [Zero-Multiplication LUT Decompression Acceleration](#zero-multiplication-lut-decompression-acceleration)
  - [Pure 128-bit Register Bitpacker](#pure-128-bit-register-bitpacker)
  - [Sample-Space Cost Lower-Bound Pruning](#sample-space-cost-lower-bound-pruning)
  - [Branchless Arithmetic & Precomputed Constants](#branchless-arithmetic-precomputed-constants)

## Overview

Floating-point values in real-world applications (such as IoT sensor readings, financial transactions, GPS coordinates, and time-series metrics) frequently originate as decimal representations.<br>
Traditional general-purpose compression algorithms and integer bitpackers operate inefficiently on IEEE 754 representations due to distributed exponent and mantissa bit patterns.

`fastalp` implements the ALP compression algorithm:

- **Exact Lossless Reconstruction**:<br>
  Guarantees bit-exact IEEE 754 preservation for all inputs, including special values such as `NaN`, `+Inf`, `-Inf`, and `-0.0`.

- **Adaptive Parameter Estimation**:<br>
  Samples input sequences to derive optimal scaling parameters `(exp, fac)` that minimize bit-width requirements.

- **Frame-of-Reference & Bitpacking**:<br>
  Encodes converted integers using base subtraction (FOR) and dense bit-packing from 1 to 64 bits per value.

- **Dedicated Exception Handling**:<br>
  Unencodable values and floating-point anomalies are stored in a dedicated exception stream without compromising primary payload compression efficiency.

- **Raw Fallback Protection**:<br>
  Automatically falls back to uncompressed raw mode when noise or extreme precision values would cause negative compression.

- **Zero Extra Allocations**:<br>
  Exposes `_into` APIs to allow caller-managed buffer reuse across high-throughput streaming pipelines.

- **Unified Generic Interface**:<br>
  `compress`, `compress_into`, `decompress`, and `decompress_into` work across both `f64` and `f32`.

---

## Usage

### Installation

```bash
cargo add fastalp
```

### Basic Compression and Decompression

```rust
use fastalp::{compress, decompress, Result};

fn main() -> Result<()> {
  let sensor_data = vec![20.5, 20.6, 20.8, 21.0, 20.9, 21.2];

  // Compress floating-point slice into byte buffer (generic for f64 / f32)
  let compressed = compress(&sensor_data);

  // Decompress byte buffer back to exact f64 slice
  let decompressed: Vec<f64> = decompress(&compressed)?;

  assert_eq!(decompressed, sensor_data);
  Ok(())
}
```

### In-Place Buffer Reuse

```rust
use fastalp::{compress_into, decompress_into, Result};

fn main() -> Result<()> {
  let batch = vec![100.12, 100.15, 100.18, 100.22];

  let mut compressed_buf = Vec::new();
  compress_into(&batch, &mut compressed_buf);

  let mut restored = Vec::new();
  decompress_into(&compressed_buf, &mut restored)?;

  assert_eq!(restored, batch);
  Ok(())
}
```

### Single-Precision Floating-Point Data

```rust
use fastalp::{compress, decompress, Result};

fn main() -> Result<()> {
  let coordinates = vec![116.4074f32, 39.9042f32, 121.4737f32, 31.2304f32];

  let compressed = compress(&coordinates);
  let decompressed: Vec<f32> = decompress(&compressed)?;

  assert_eq!(decompressed, coordinates);
  Ok(())
}
```

---

## Features

- **Bit-Exact Precision**:<br>
  Decoded floats match original bit patterns (`a.to_bits() == b.to_bits()`).

- **High Compression on Decimals**:<br>
  Delivers 3x to 8x+ compression ratios on typical decimal time-series data.

- **Unified Generic Support**:<br>
  Zero-cost abstraction for both 64-bit (`f64`) and 32-bit (`f32`) floating-point streams.

- **Robust Exception Handling**:<br>
  Encodes non-finite numbers (`NaN`, `Inf`) and unencodable values.

- **Zero-Heap Buffer Reuse**:<br>
  Direct writing into existing vectors via `compress_into` and `decompress_into`.

---

## Architecture & Design

`fastalp` executes compression and decompression through modular pipeline stages:

```mermaid
graph TD
  Input["Input Floating-Point Slice (&[f64] / &[f32])"] --> Sampler["Parameter Sampler<br/>Determine optimal (exp, fac) via cost model"]
  Sampler --> Encoder["Lossless Integer Conversion<br/>Scaled rounding & bit-exact validation"]
  Encoder --> Split{"Losslessly Encodable?"}
  Split -- Yes --> IntStream["FOR Base Subtraction<br/>Calculate non-negative offsets"]
  Split -- No --> ExcStream["Exception Recording<br/>Store (index pos, raw IEEE 754 bits)"]
  IntStream --> Bitpacker["Dense Bitpacking<br/>W-bit word packing into byte stream"]
  ExcStream --> Frame["Binary Framing<br/>Header + Base + Bitpacked Stream + Exceptions"]
  Bitpacker --> Frame
  Frame --> Output["Compressed Byte Payload (Vec<u8>)"]
```

### Compression Pipeline

- **Constant Detection & Fallback Filter (`encoder.rs`)**:<br>
  Quickly evaluates bit-exact identical sequences (`v.is_exact_same(first)`). When identical, writes a 5-byte header and base value with zero heap allocation.<br>
  When estimated payload exceeds raw size plus header overhead, switches to 3-byte raw mode to guarantee zero data inflation.

- **Sampling (`sampler.rs`)**:<br>
  Evaluates up to 32 evenly distributed sample points across parameter combinations `(exp, fac)`.<br>
  Selects parameters minimizing total storage cost: `bit_width * count + exceptions * penalty`.

- **Lossless Verification (`sampler.rs`, `float.rs`)**:<br>
  Multiplies float by $10^{\text{exp}} \times 10^{-\text{fac}}$, rounds via constants, and verifies exact inverse equality against raw IEEE 754 bit representations.

- **Base Offset & Bitpacking (`bitpack/pack.rs`, `encoder.rs`)**:<br>
  Computes minimum integer value as base, subtracts base from valid integers, determines required bit width, and writes dense packed bits via a 128-bit register accumulator.

- **Exception Stream (`encoder.rs`)**:<br>
  Appends position and raw bits for values that fail exact integer roundtrip.

### Decompression Pipeline

- **Header Parsing (`decoder.rs`)**:<br>
  Reads compact header, extracting format type and element count.<br>
  For raw fallback chunks, performs direct zero-copy slice restoration.<br>
  For ALP chunks, extracts packed `(exp, fac, bit_width)` parameters and base value.

- **Bit Unpacking & LUT Reconstruction (`bitpack/unpack.rs`)**:<br>
  Small bit-widths (1, 2, 4, 8 bits) reconstruct floats via precomputed stack lookup tables in a single pass.<br>
  General bit-widths unpack via register bit-stream sliding windows.

- **Exception Patching (`decoder.rs`)**:<br>
  Overwrites positions listed in the exception table with raw IEEE 754 bit patterns.

---

## Tech Stack

- **Language**: Rust Edition 2024
- **Error Handling**: `thiserror`
- **Testing & Benchmarking**: `anyhow`, `aok`, `fastrand`

---

## Directory Structure

```
fastalp/
├── Cargo.toml          # Crate manifest and dependency configuration
├── README.md           # Generated multilingual documentation
├── README.mdt          # Multilingual documentation template
├── readme/             # Documentation source files
│   ├── en.md           # English documentation
│   └── zh.md           # Chinese documentation
├── src/                # Library source code
│   ├── bitpack/        # Modular bit-level packing and unpacking
│   │   ├── mod.rs      # Module facade and re-exports
│   │   ├── pack.rs     # Dense bitpacking with 128-bit register accumulator
│   │   └── unpack.rs   # Direct bit unpacking with stack LUT acceleration
│   ├── constants.rs    # Precomputed static power tables and format constants
│   ├── decoder.rs      # Generic decompression logic and raw fallback restore
│   ├── encoder.rs      # Generic compression logic, O(1) constant fast path, raw fallback
│   ├── error.rs        # Error definitions and Result type alias
│   ├── float.rs        # AlpFloat abstraction trait and f32/f64 zero-cost implementation
│   ├── lib.rs          # Public crate exports and high-level API
│   ├── params.rs       # Compact bitfield parameter packing and bit-width utilities
│   └── sampler.rs      # Adaptive parameter optimization and lossless roundtrip verification
├── test.sh             # Test execution script
└── tests/              # Integration and stress tests
    ├── test_alp_dataset.rs # ALP paper 31 real-world datasets roundtrip & ratio tests
    └── test_roundtrip.rs   # Roundtrip integrity and boundary tests
```

---

## Benchmarks & C++ Comparison

### Benchmark Environment & Toolchain

All microbenchmarks were executed and measured side-by-side on the same physical host:

- **Processor (CPU)**: Apple M2 Max (12 Cores: 8 Performance @ 3.68 GHz + 4 Efficiency @ 2.42 GHz, ARMv8.6-A NEON ISA)<br>
- **Host OS**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust Toolchain**: `rustc 1.98.0 / nightly` (flags: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`)<br>
- **C++ Compiler Toolchain**: Homebrew LLVM Clang 22.1.8 (`-O3 -std=c++17 -DNDEBUG -march=native`) / CMake 4.4.2<br>
- **Memory Allocator**: `mimalloc 0.1.52`<br>
- **Benchmark Suites**: Rust `divan 0.1.20` vs C++ `std::chrono::high_resolution_clock` (steady-state median sampling)

### Side-by-Side Throughput Comparison

| Scenario | Data Size | fastalp Throughput | C++ Reference Throughput | Throughput Ratio |
|---|---|---|---|---|
| **f64 Compress** (Identical Values) | 1024 x f64 (8 KB) | **23.34 GB/s** | 7.02 GB/s | **3.32x** |
| **f64 Compress** (Sensor Decimals) | 1024 x f64 (8 KB) | **1.37 GB/s** | 0.81 GB/s | **1.69x** |
| **f64 Compress** (Large Batch) | 65535 x f64 (512 KB) | **3.90 GB/s** | 6.22 GB/s | 0.63x |
| **f32 Compress** (Sensor Decimals) | 1024 x f32 (4 KB) | **1.22 GB/s** | 2.52 GB/s | 0.48x |
| **f64 Decompress** (Identical Values) | 1024 x f64 (8 KB) | **76.56 GB/s** | 98.70 GB/s | 0.78x |
| **f64 Decompress** (Sensor Decimals) | 1024 x f64 (8 KB) | **24.09 GB/s** | 65.54 GB/s | 0.37x |
| **f64 Decompress** (Large Batch) | 65535 x f64 (512 KB) | **24.85 GB/s** | 49.34 GB/s | 0.50x |
| **f32 Decompress** (Sensor Decimals) | 1024 x f32 (4 KB) | **13.00 GB/s** | 97.52 GB/s | 0.13x |

### Real-World Datasets Compression Ratio

Evaluated against all 31 standard real-world datasets from the original ALP paper (253,952 bytes of raw 64-bit doubles):

| Dataset Name | Raw Size | fastalp Compressed Size | fastalp Ratio | C++ Ref ALP Ratio |
|---|---|---|---|---|
| **gov26**<br>Government Stats | 8192 B | 13 B | **630.15x** (0.10 b/v) | 455.11x |
| **gov31**<br>Government Stats | 8192 B | 25 B | **327.68x** (0.20 b/v) | 292.57x |
| **gov30**<br>Government Stats | 8192 B | 55 B | **148.95x** (0.43 b/v) | 141.24x |
| **stocks_uk**<br>UK Stock Prices | 8192 B | 1165 B | **7.03x** (9.10 b/v) | 7.00x |
| **cms9**<br>Healthcare Billing | 8192 B | 1421 B | **5.76x** (11.10 b/v) | 5.74x |
| **medicare9**<br>Medical Monitoring | 8192 B | 1421 B | **5.76x** (11.10 b/v) | 5.74x |
| **neon_pm10_dust**<br>PM10 Sensor | 8192 B | 1553 B | **5.27x** (12.13 b/v) | 5.26x |
| **stocks_usa_c**<br>US Stock Prices | 8192 B | 1951 B | **4.20x** (15.24 b/v) | 4.19x |
| **gov40**<br>Government Timestamps | 8192 B | 2445 B | **3.35x** (19.10 b/v) | 3.34x |
| **stocks_de**<br>German Stock Prices | 8192 B | 2625 B | **3.12x** (20.51 b/v) | 3.12x |
| **bird_migration_f**<br>GPS Coordinates | 8192 B | 2651 B | **3.09x** (20.71 b/v) | 3.09x |
| **neon_bio_temp_c**<br>Biology Sensor | 8192 B | 2957 B | **2.77x** (23.10 b/v) | 2.77x |
| **food_prices**<br>Consumer Index | 8192 B | 3285 B | **2.49x** (25.66 b/v) | 2.49x |
| **city_temperature_f**<br>Weather Temp | 8192 B | 3363 B | **2.44x** (26.27 b/v) | 2.43x |
| **ssd_hdd_benchmarks_f**<br>Disk Benchmarks | 8192 B | 3621 B | **2.26x** (28.29 b/v) | 2.26x |
| **neon_wind_dir**<br>Wind Direction | 8192 B | 3725 B | **2.20x** (29.10 b/v) | 2.20x |
| **neon_air_pressure**<br>Air Pressure | 8192 B | 3743 B | **2.19x** (29.24 b/v) | 2.19x |
| **basel_wind_f**<br>Basel Wind Speed | 8192 B | 3817 B | **2.15x** (29.82 b/v) | 2.14x |
| **arade4**<br>Hydrology Sensor | 8192 B | 4063 B | **2.02x** (31.74 b/v) | 2.01x |
| **basel_temp_f**<br>Basel Temperature | 8192 B | 4069 B | **2.01x** (31.79 b/v) | 2.01x |
| **bitcoin_f**<br>Bitcoin Rates | 8192 B | 4195 B | **1.95x** (32.77 b/v) | 1.95x |
| **bitcoin_transactions_f**<br>On-chain Tx | 8192 B | 4861 B | **1.69x** (37.98 b/v) | 1.68x |
| **medicare1**<br>Medical Records | 8192 B | 5249 B | **1.56x** (41.01 b/v) | 1.56x |
| **cms1**<br>Medical Records | 8192 B | 5363 B | **1.53x** (41.90 b/v) | 1.53x |
| **cms25**<br>Medical Records | 8192 B | 5451 B | **1.50x** (42.59 b/v) | 1.50x |
| **nyc29**<br>NYC Taxi Travel | 8192 B | 5441 B | **1.51x** (42.51 b/v) | 1.50x |
| **air_sensor_f**<br>Air Sensor Data | 8192 B | 8195 B (Fallback) | **1.00x** (Guaranteed) | 0.52x (Expansion) |
| **poi_lat**<br>High-Precision Lat | 8192 B | 8195 B (Fallback) | **1.00x** (Guaranteed) | 0.51x (Expansion) |
| **poi_lon**<br>High-Precision Lon | 8192 B | 8195 B (Fallback) | **1.00x** (Guaranteed) | 0.64x (Expansion) |
| **TOTAL / Overall Average** | **253,952 B** | **110,773 B** | **2.29x** | **1.94x** |

Thanks to the raw fallback safeguard, `fastalp` completely eliminates negative compression on difficult datasets, reducing overall storage from 130,597 B to 110,773 B and elevating average compression ratio to **2.29x**.

---

## Architecture Comparison & Engineering Optimizations

Compared with the reference C++ implementation, `fastalp` achieves superior compression ratio and memory efficiency in safe pure Rust:

### Constant Sequence Fast Detection & Zero-Heap Allocation

- **Reference C++ Implementation**:<br>
  Executes full parameter sampling, intermediate integer transformation, and bit-width analysis even on completely constant sequences, requiring 9.25 µs end-to-end.<br>
- **fastalp Optimization**:<br>
  Inspects raw IEEE 754 bits at compression entry (`v.is_exact_same(first)`), strictly differentiating `+0.0` and `-0.0` sign bits;<br>
  Directly emits a 5-byte header and base value (`bit_width = 0`) upon match, skipping parameter search and vector allocation, reducing compression time to 351 ns (26x speedup).

### Raw Fallback Safeguard Against Negative Compression

- **Reference C++ Implementation**:<br>
  Lacks safeguard against data expansion; on non-decimal double datasets with high exception rates, the exception table expands beyond original payload size (e.g. `poi_lat` yields 0.51x, `air_sensor` yields 0.52x).<br>
- **fastalp Optimization**:<br>
  Monitors estimated payload size during encoding; when compressed size exceeds uncompressed input plus header overhead, automatically falls back to `TYPE_F64_RAW` or `TYPE_F32_RAW` mode;<br>
  Writes a 3-byte header and stores raw uncompressed bytes, restored via zero-copy `copy_nonoverlapping`, eliminating negative compression and raising dataset average ratio to 2.29x.

### Zero-Heap Direct Streaming Decompression

- **Reference C++ Implementation**:<br>
  Employs a two-stage decompression pipeline: stage 1 unpacks bitstream to an intermediate heap array, and stage 2 iterates over the array to compute float unscaling and patch exceptions, incurring 8 B/elem heap allocation and cache pressure.<br>
- **fastalp Optimization**:<br>
  Executes a single-pass direct streaming reconstruction pipeline. Bits are unpacked within CPU registers and written directly to the caller destination slice, keeping L1/L2 caches hot and providing `compress_into` and `decompress_into` zero-allocation APIs.

### Zero-Multiplication LUT Decompression Acceleration

- **Reference C++ Implementation**:<br>
  Inner loop executes integer and floating-point multiplications for every element.<br>
- **fastalp Optimization**:<br>
  For small bit-widths (1, 2, 4, 8 bits with 2, 4, 16, 256 possible offsets), precomputes a compact stack lookup table:<br>
  `lut[offset] = (offset + base) * 10^fac * 10^-exp`;<br>
  Inner loop reduces to $O(1)$ direct array lookups, eliminating all integer and floating-point multiplications on the decode path.

### Pure 128-bit Register Bitpacker

- **Reference C++ Implementation**:<br>
  Generates extensive template code across multiple compilation units, creating large binaries with architecture-specific intrinsics.<br>
- **fastalp Optimization**:<br>
  Maintains a sliding bit window with a single 128-bit register accumulator (`acc: u128`, `bits_in_acc: u32`), executing 64-bit word writes and reads in single instructions;<br>
  Pure safe Rust with zero external C++ toolchain dependencies, cross-compiling seamlessly for x86_64, ARM64, and WebAssembly.

### Sample-Space Cost Lower-Bound Pruning

- **Reference C++ Implementation**:<br>
  Evaluates all samples across 135 `(exp, fac)` parameter combinations unconditionally.<br>
- **fastalp Optimization**:<br>
  Applies dynamic lower-bound pruning: breaks inner verification immediately once running exception penalty (`exceptions * penalty`) surpasses current global `best_cost`, skipping unnecessary parameter iterations.

### Branchless Arithmetic & Precomputed Constants

- Pre-extracts exponent factor tables outside inner loops to eliminate repeated array lookups;<br>
- Calculates bit-width using hardware CLZ instructions and applies compile-time bitmasks to eliminate conditional branch mispredictions.


---

<a name="zh"></a>

# fastalp : 基于 ALP 算法的无损浮点数压缩引擎

纯 Rust 实现的自适应无损浮点数压缩 ALP 算法库，通过统一泛型接口支持 `f64` 与 `f32` 数据流。

---

- [功能特性](#功能特性)
- [使用示例](#使用示例)
  - [添加依赖](#添加依赖)
  - [基础压缩与解压](#基础压缩与解压)
  - [内存缓冲区复用](#内存缓冲区复用)
  - [单精度浮点数据处理](#单精度浮点数据处理)
- [核心特性](#核心特性)
- [架构设计](#架构设计)
  - [压缩流程](#压缩流程)
  - [解压流程](#解压流程)
- [技术栈](#技术栈)
- [目录结构](#目录结构)
- [性能评测与 C++ 原版对比](#性能评测与-c-原版对比)
  - [测试环境与编译配置](#测试环境与编译配置)
  - [同机实测吞吐量对比](#同机实测吞吐量对比)
  - [真实公开数据集压缩率对比](#真实公开数据集压缩率对比)
- [架构对比与工程优化设计](#架构对比与工程优化设计)
  - [全等序列常数探测与零堆分配](#全等序列常数探测与零堆分配)
  - [原始保底机制消除负压缩](#原始保底机制消除负压缩)
  - [零堆内存分配与单遍流式解码](#零堆内存分配与单遍流式解码)
  - [局部查找表解压加速](#局部查找表解压加速)
  - [纯寄存器 128 位累加器与紧凑位打包](#纯寄存器-128-位累加器与紧凑位打包)
  - [采样搜索代价下界剪枝](#采样搜索代价下界剪枝)
  - [编译期常量提取与无分支位运算](#编译期常量提取与无分支位运算)

- [功能特性](#功能特性)
- [使用示例](#使用示例)
  - [添加依赖](#添加依赖)
  - [基础压缩与解压](#基础压缩与解压)
  - [内存缓冲区复用](#内存缓冲区复用)
  - [单精度浮点数据处理](#单精度浮点数据处理)
- [核心特性](#核心特性)
- [架构设计](#架构设计)
  - [压缩流程](#压缩流程)
  - [解压流程](#解压流程)
- [技术栈](#技术栈)
- [目录结构](#目录结构)
- [性能评测与 C++ 原版对比](#性能评测与-c-原版对比)
  - [测试环境与编译配置](#测试环境与编译配置)
  - [同机实测吞吐量对比](#同机实测吞吐量对比)
  - [真实公开数据集压缩率对比](#真实公开数据集压缩率对比)
- [架构对比与工程优化设计](#架构对比与工程优化设计)
  - [全等序列常数探测与零堆分配](#全等序列常数探测与零堆分配)
  - [原始保底机制消除负压缩](#原始保底机制消除负压缩)
  - [零堆内存分配与单遍流式解码](#零堆内存分配与单遍流式解码)
  - [局部查找表解压加速](#局部查找表解压加速)
  - [纯寄存器 128 位累加器与紧凑位打包](#纯寄存器-128-位累加器与紧凑位打包)
  - [采样搜索代价下界剪枝](#采样搜索代价下界剪枝)
  - [编译期常量提取与无分支位运算](#编译期常量提取与无分支位运算)

## 功能特性

在物联网传感器采集、金融量化交易、GPS 经纬度定位以及时序监控等场景中，浮点数据通常以十进制形式产生。<br>
由于 IEEE 754 浮点数的阶码与尾数位分布离散，通用压缩算法与整型位打包算法难以获得理想的压缩效率。

`fastalp` 实现 ALP 压缩算法：

- **严格无损重构**：<br>
  保证解码数据与原始 IEEE 754 二进制位严格一致，支持 `NaN`、`+Inf`、`-Inf` 与 `-0.0` 等特殊值。

- **自适应参数推导**：<br>
  通过对输入数据进行采样，计算使编码位宽最小的最优参数组合 `(exp, fac)`。

- **基准偏移与位打包**：<br>
  将转换后的整型序列进行基准值消除（FOR），并按 1 至 64 位动态位宽进行密集位打包。

- **独立异常值处理**：<br>
  无法无损整型化的数值与特殊浮点数记录于独立异常流，避免降低主数据流压缩比。

- **原始保底模式**：<br>
  当随机噪声或不可压缩数据导致编码后体积膨胀时，自动回退至原始保底模式，杜绝负压缩。

- **零额外分配复用**：<br>
  提供 `_into` 系列接口，支持调用方直接复用已有内存缓冲区。

- **统一泛型接口**：<br>
  `compress`、`compress_into`、`decompress` 与 `decompress_into` 统一适用于 `f64` 与 `f32`。

---

## 使用示例

### 添加依赖

```bash
cargo add fastalp
```

### 基础压缩与解压

```rust
use fastalp::{compress, decompress, Result};

fn main() -> Result<()> {
  let sensor_data = vec![20.5, 20.6, 20.8, 21.0, 20.9, 21.2];

  // 压缩浮点数切片为字节向量 (自动适配 f64 / f32)
  let compressed = compress(&sensor_data);

  // 解压字节向量恢复原始浮点数切片
  let decompressed: Vec<f64> = decompress(&compressed)?;

  assert_eq!(decompressed, sensor_data);
  Ok(())
}
```

### 内存缓冲区复用

```rust
use fastalp::{compress_into, decompress_into, Result};

fn main() -> Result<()> {
  let batch = vec![100.12, 100.15, 100.18, 100.22];

  let mut compressed_buf = Vec::new();
  compress_into(&batch, &mut compressed_buf);

  let mut restored = Vec::new();
  decompress_into(&compressed_buf, &mut restored)?;

  assert_eq!(restored, batch);
  Ok(())
}
```

### 单精度浮点数据处理

```rust
use fastalp::{compress, decompress, Result};

fn main() -> Result<()> {
  let coordinates = vec![116.4074f32, 39.9042f32, 121.4737f32, 31.2304f32];

  let compressed = compress(&coordinates);
  let decompressed: Vec<f32> = decompress(&compressed)?;

  assert_eq!(decompressed, coordinates);
  Ok(())
}
```

---

## 核心特性

- **位级精确无损**：<br>
  解码浮点数与原始输入在二进制位层面保持一致（`a.to_bits() == b.to_bits()`）。

- **十进制高压缩比**：<br>
  在常见十进制浮点序列上可获得 3x 至 8x+ 压缩比。

- **统一泛型支持**：<br>
  单一接口支持 `f64` 与 `f32` 零成本抽象编解码。

- **完整异常值支持**：<br>
  支持 `NaN`、无穷大与不可无损转换的高精度浮点数。

- **零堆分配接口**：<br>
  通过 `compress_into` 与 `decompress_into` 直接写入现有缓冲区。

---

## 架构设计

`fastalp` 编解码流程划分为以下阶段：

```mermaid
graph TD
  Input["输入浮点数切片 (&[f64] / &[f32])"] --> Sampler["参数采样器<br/>评估代价模型并推导最优 (exp, fac)"]
  Sampler --> Encoder["无损整型编码<br/>快速常量舍入与位精确校验"]
  Encoder --> Split{"是否支持无损编码"}
  Split -- 是 --> IntStream["FOR 基准值消除<br/>计算非负整型偏移量"]
  Split -- 否 --> ExcStream["异常值记录<br/>存储索引位置与 IEEE 754 原始位"]
  IntStream --> Bitpacker["密集位打包<br/>按动态位宽打包进字节流"]
  ExcStream --> Frame["二进制帧封装<br/>包头 + 基准值 + 位流 + 异常值列表"]
  Bitpacker --> Frame
  Frame --> Output["压缩字节负载 (Vec<u8>)"]
```

### 压缩流程

- **全等探测与保底分流 (`encoder.rs`)**：<br>
  先对数据进行常数序列快速校验；若全等且可编码，直接写入 5 字节头与基准值；<br>
  若为不可压缩随机数据且编码体积超过原始大小，则直接写入 3 字节头并以原始字节流存储。

- **采样评估 (`sampler.rs`)**：<br>
  在数据序列中均匀采样至多 32 个数值，遍历 `(exp, fac)` 参数组合，<br>
  选取使得 `位宽 * 样本量 + 异常数 * 惩罚权重` 最小的参数组合。

- **无损转换与验证 (`sampler.rs`, `float.rs`)**：<br>
  将浮点数乘以 $10^{\text{exp}} \times 10^{-\text{fac}}$，利用常量完成快速向近舍入并转换为整型，<br>
  再通过反向整型乘法与逆缩放验证浮点位级一致性。

- **基准消除与位打包 (`bitpack/pack.rs`, `encoder.rs`)**：<br>
  获取有效整型中的最小值作为基准值，计算偏移量并获取所需位宽，<br>
  利用 128 位寄存器滑动窗口将数值紧凑打包入字节流。

- **异常流序列化 (`encoder.rs`)**：<br>
  无法无损转换的浮点数按索引位置与 IEEE 754 原始位记录于尾部异常表中。

### 解压流程

- **帧解析 (`decoder.rs`)**：<br>
  读取紧凑头部，提取类型标识与元素数量；<br>
  若类型为原始保底数据，通过内存复制直出恢复；若为 ALP 压缩数据，提取 `(exp, fac)` 缩放参数、位宽以及基准值。

- **位流解包与查表重构 (`bitpack/unpack.rs`)**：<br>
  小位宽直接通过栈上查找表一步完成解包与浮点重构，其余位宽通过寄存器流水解包。

- **异常值覆盖 (`decoder.rs`)**：<br>
  若存在尾部异常表，读取对应索引位置的数值并覆盖为原始 IEEE 754 浮点值。

---

## 技术栈

- **开发语言**：Rust Edition 2024
- **错误处理**：`thiserror`
- **测试与基准**：`anyhow`, `aok`, `fastrand`

---

## 目录结构

```
fastalp/
├── Cargo.toml          # 项目配置与依赖声明
├── README.md           # 生成的多语言文档
├── README.mdt          # 多语言文档模板
├── readme/             # 文档源码目录
│   ├── en.md           # 英文技术文档
│   └── zh.md           # 中文技术文档
├── src/                # 核心源代码
│   ├── bitpack/        # 模块化位打包与位解包
│   │   ├── mod.rs      # 门面导出
│   │   ├── pack.rs     # 128 位累加器位打包算子
│   │   └── unpack.rs   # 局部查表与直接位解包算子
│   ├── constants.rs    # 静态幂次表与格式常量
│   ├── decoder.rs      # 泛型解压核心逻辑与保底解压
│   ├── encoder.rs      # 泛型压缩核心逻辑与保底压缩
│   ├── error.rs        # 错误枚举定义与 Result 类型别名
│   ├── float.rs        # AlpFloat 浮点抽象特征与无损转换
│   ├── lib.rs          # 导出接口与高层封装
│   ├── params.rs       # 紧凑位域参数打包与位宽计算
│   └── sampler.rs      # 参数采样与无损重构验证
├── test.sh             # 测试运行脚本
└── tests/              # 集成与压力测试
    ├── test_alp_dataset.rs # ALP 论文 31 真实数据集往返与压缩比评测
    └── test_roundtrip.rs   # 往返无损与边界测试
```

---

## 性能评测与 C++ 原版对比

### 测试环境与编译配置

所有基准测试均在同一物理机上执行并进行同机对比测试：

- **处理器**: Apple M2 Max (12 核心：8 性能核 @ 3.68 GHz + 4 能效核 @ 2.42 GHz, ARMv8.6-A NEON 指令集)<br>
- **操作系统**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust 编译工具链**: `rustc 1.98.0 / nightly` (配置：`opt-level = 3`, `lto = "fat"`, `codegen-units = 1`)<br>
- **C++ 编译工具链**: Homebrew LLVM Clang 22.1.8 (`-O3 -std=c++17 -DNDEBUG -march=native`) / CMake 4.4.2<br>
- **内存分配器**: `mimalloc 0.1.52`<br>
- **基准测试框架**: Rust `divan 0.1.20` 微基准套件 vs C++ `std::chrono::high_resolution_clock`（稳态中位数采样）

### 同机实测吞吐量对比

| 测试场景 | 数据规模 | fastalp 吞吐 | C++ 原版 吞吐 | 吞吐比 |
|---|---|---|---|---|
| **f64 压缩** (常数同值序列) | 1024 个 f64 (8 KB) | **23.34 GB/s** | 7.02 GB/s | **3.32x** |
| **f64 压缩** (传感器十进制) | 1024 个 f64 (8 KB) | **1.37 GB/s** | 0.81 GB/s | **1.69x** |
| **f64 压缩** (大块批量) | 65535 个 f64 (512 KB) | **3.90 GB/s** | 6.22 GB/s | 0.63x |
| **f32 压缩** (传感器十进制) | 1024 个 f32 (4 KB) | **1.22 GB/s** | 2.52 GB/s | 0.48x |
| **f64 解压** (同值序列) | 1024 个 f64 (8 KB) | **76.56 GB/s** | 98.70 GB/s | 0.78x |
| **f64 解压** (传感器十进制) | 1024 个 f64 (8 KB) | **24.09 GB/s** | 65.54 GB/s | 0.37x |
| **f64 解压** (大块批量) | 65535 个 f64 (512 KB) | **24.85 GB/s** | 49.34 GB/s | 0.50x |
| **f32 解压** (传感器十进制) | 1024 个 f32 (4 KB) | **13.00 GB/s** | 97.52 GB/s | 0.13x |

### 真实公开数据集压缩率对比

对 ALP 论文全部 31 个真实公开数据集（共 253,952 字节原始浮点数据）进行精确到 bit 的无损往返验证与压缩率评测：

| 数据集名称 | 原始大小 | fastalp 压缩大小 | fastalp 压缩率 | C++ 原版 压缩率 |
|---|---|---|---|---|
| **gov26**<br>政府公开统计 | 8192 B | 13 B | **630.15x** (0.10 b/v) | 455.11x |
| **gov31**<br>政府公开统计 | 8192 B | 25 B | **327.68x** (0.20 b/v) | 292.57x |
| **gov30**<br>政府公开统计 | 8192 B | 55 B | **148.95x** (0.43 b/v) | 141.24x |
| **stocks_uk**<br>英国股票时序 | 8192 B | 1165 B | **7.03x** (9.10 b/v) | 7.00x |
| **cms9**<br>医疗报销监测 | 8192 B | 1421 B | **5.76x** (11.10 b/v) | 5.74x |
| **medicare9**<br>医疗就诊监测 | 8192 B | 1421 B | **5.76x** (11.10 b/v) | 5.74x |
| **neon_pm10_dust**<br>PM10粉尘传感 | 8192 B | 1553 B | **5.27x** (12.13 b/v) | 5.26x |
| **stocks_usa_c**<br>美股时序数据 | 8192 B | 1951 B | **4.20x** (15.24 b/v) | 4.19x |
| **gov40**<br>政府时序数据 | 8192 B | 2445 B | **3.35x** (19.10 b/v) | 3.34x |
| **stocks_de**<br>德国股票时序 | 8192 B | 2625 B | **3.12x** (20.51 b/v) | 3.12x |
| **bird_migration_f**<br>鸟类迁徙GPS | 8192 B | 2651 B | **3.09x** (20.71 b/v) | 3.09x |
| **neon_bio_temp_c**<br>生物温度传感 | 8192 B | 2957 B | **2.77x** (23.10 b/v) | 2.77x |
| **food_prices**<br>食品价格指数 | 8192 B | 3285 B | **2.49x** (25.66 b/v) | 2.49x |
| **city_temperature_f**<br>城市气温数据 | 8192 B | 3363 B | **2.44x** (26.27 b/v) | 2.43x |
| **ssd_hdd_benchmarks_f**<br>硬盘性能 | 8192 B | 3621 B | **2.26x** (28.29 b/v) | 2.26x |
| **neon_wind_dir**<br>风向角度传感 | 8192 B | 3725 B | **2.20x** (29.10 b/v) | 2.20x |
| **neon_air_pressure**<br>气压传感 | 8192 B | 3743 B | **2.19x** (29.24 b/v) | 2.19x |
| **basel_wind_f**<br>巴塞尔风速 | 8192 B | 3817 B | **2.15x** (29.82 b/v) | 2.14x |
| **arade4**<br>水文传感器 | 8192 B | 4063 B | **2.02x** (31.74 b/v) | 2.01x |
| **basel_temp_f**<br>巴塞尔气温 | 8192 B | 4069 B | **2.01x** (31.79 b/v) | 2.01x |
| **bitcoin_f**<br>比特币行情 | 8192 B | 4195 B | **1.95x** (32.77 b/v) | 1.95x |
| **bitcoin_transactions_f**<br>链上交易 | 8192 B | 4861 B | **1.69x** (37.98 b/v) | 1.68x |
| **medicare1**<br>医疗门诊统计 | 8192 B | 5249 B | **1.56x** (41.01 b/v) | 1.56x |
| **cms1**<br>医疗报销记录 | 8192 B | 5363 B | **1.53x** (41.90 b/v) | 1.53x |
| **cms25**<br>医疗处方记录 | 8192 B | 5451 B | **1.50x** (42.59 b/v) | 1.50x |
| **nyc29**<br>纽约出租车数据 | 8192 B | 5441 B | **1.51x** (42.51 b/v) | 1.50x |
| **air_sensor_f**<br>高频空气传感 | 8192 B | 8195 B (保底) | **1.00x** (回退) | 0.52x (膨胀) |
| **poi_lat**<br>POI高精度纬度 | 8192 B | 8195 B (保底) | **1.00x** (回退) | 0.51x (膨胀) |
| **poi_lon**<br>POI高精度经度 | 8192 B | 8195 B (保底) | **1.00x** (回退) | 0.64x (膨胀) |
| **总计 / 全数据集平均** | **253,952 B** | **110,773 B** | **2.29x** | **1.94x** |

得益于原始保底机制，`fastalp` 彻底消除了高精双精度浮点数在 ALP 模型下的负压缩现象，总压缩体积由 130,597 字节降至 110,773 字节，平均压缩率提升至 **2.29x**。

---

## 架构对比与工程优化设计

相比 C++ 原版实现，`fastalp` 在纯安全 Rust 下通过以下架构革新提升压缩率与内存效率：

### 全等序列常数探测与零堆分配

- **C++ 原版实现**：<br>
  面对全量常数序列时，依然需要执行完整的样本采集、临时整型数组转换与位宽分析，端到端耗时达 9.25 微秒。<br>
- **fastalp 优化**：<br>
  在压缩入口通过底层原始比特比对（`v.is_exact_same(first)`，严格区分 `+0.0` 与 `-0.0` 符号位）；<br>
  命中后直接写入 5 字节紧凑头部与基准值（`bit_width = 0`），跳过所有采样与中间数组分配，压缩耗时降至 351 纳秒，相对提速 26 倍。

### 原始保底机制消除负压缩

- **C++ 原版实现**：<br>
  缺乏数据膨胀防护机制；在遇到非十进制高频双精度浮点时，异常表膨胀导致体积反超原始数据（如 `poi_lat` 压缩率仅 0.51x，`air_sensor` 仅 0.52x）。<br>
- **fastalp 优化**：<br>
  压缩时预估编码体积；当总开销超过原始字节数加上头部后，自动切换为 `TYPE_F64_RAW` 或 `TYPE_F32_RAW` 保底模式；<br>
  以 3 字节头部存储元数据并直存原始字节流，解码时通过 `copy_nonoverlapping` 零拷贝恢复，消除负压缩，全数据集总体积由 130KB 降至 110KB，平均压缩率提升至 2.29x。

### 零堆内存分配与单遍流式解码

- **C++ 原版实现**：<br>
  采用两阶段解码架构：阶段一解包位流到中间堆数组，阶段二遍历中间数组计算浮点逆缩放并修补异常，引发 8 字节/元素的堆分配与 L1/L2 缓存挤占。<br>
- **fastalp 优化**：<br>
  采用单遍直解流式架构；位流在 CPU 寄存器中解包的同时直接计算并写入目标切片，消除中间堆分配与内存往返传输，保持 CPU 缓存高效命中；对外提供 `compress_into` 与 `decompress_into` 零分配接口。

### 局部查找表解压加速

- **C++ 原版实现**：<br>
  解包内层循环对每个元素均执行浮点或整数乘除运算，消耗较多流水线计算周期。<br>
- **fastalp 优化**：<br>
  针对 1、2、4、8 位等小位宽（仅 2、4、16、256 种偏移状态），在解压栈上构建微型查找表：<br>
  `lut[offset] = (offset + base) * 10^fac * 10^-exp`；<br>
  解压时将算术反缩放简化为 $O(1)$ 数组直接索引查表，消除循环内整数与浮点乘法开销。

### 纯寄存器 128 位累加器与紧凑位打包

- **C++ 原版实现**：<br>
  采用多层宏与模板元编程生成大量打包函数，编译生成的目标代码体积庞大，且高度耦合特定硬件平台的指令扩展。<br>
- **fastalp 优化**：<br>
  采用单一 `u128` 寄存器作为滑动窗口（`acc: u128` 与 `bits_in_acc: u32`），单指令 64 位写入或读取；<br>
  纯安全 Rust 实现，不依赖外部 C++ 编译链，天然跨平台支持 x86_64、ARM64 以及 WebAssembly。

### 采样搜索代价下界剪枝

- **C++ 原版实现**：<br>
  参数搜索时盲目遍历 135 种 `(exp, fac)` 组合的全部样本，遍历开销较高。<br>
- **fastalp 优化**：<br>
  引入代价下界动态剪枝：在单次采样的内层循环中，若已累计的异常惩罚（`exceptions * penalty`）已超过当前全局最优代价 `best_cost`，则立即中断探测，跳过剩余的所有样本测试，显著降低参数搜索耗时。

### 编译期常量提取与无分支位运算

- Exponent factor 预先在外层提取，消除采样与编码循环内对全局表的重复数组索引；<br>
- 采用硬件级前导零指令（CLZ）计算位宽，利用常量位掩码替代分支判断，减少流水线损耗。

