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
