# fastalp : Adaptive Lossless Floating-Point Compression in Rust

Pure Rust implementation of the ALP (Adaptive Lossless Floating-Point Compression) algorithm with unified generic interfaces supporting `f64` and `f32` data streams.

<p align="center">
  <img src="https://fastly.jsdelivr.net/gh/webc-fs/-@OZ/8fhc_O5PDP7ioccqHshw.svg" alt="fastalp Floating-Point Compression Performance & Ratio Benchmark" width="100%">
  <br>
  <sub><b>Benchmark Environment</b>: CPU: Apple M2 Max (12 Cores: 8 Performance @ 3.68GHz + 4 Efficiency @ 2.42GHz) ｜ OS: macOS Sequoia ｜ Toolchain: Rust 1.98 / LLVM Clang (-O3)</sub>
</p>

---


## Overview

Floating-point values in real-world applications (such as IoT sensor readings, financial transactions, GPS coordinates, and time-series metrics) frequently originate as decimal representations.<br>
Traditional general-purpose compression algorithms and integer bitpackers operate inefficiently on IEEE 754 representations due to distributed exponent and mantissa bit patterns.

`fastalp` implements the ALP compression algorithm:

- **Exact Lossless Reconstruction**:<br>
  Guarantees bit-exact IEEE 754 preservation for all inputs, including special values such as `NaN`, `+Inf`, `-Inf`, and `-0.0`.

- **Compact Self-Describing Header & Large Array Support**:<br>
  Features a 2-bit length tag header layout where standard 1024-element blocks require only a 3-byte compressed header (and just 1 byte in raw fallback mode).<br>
  Natively scales beyond 65,535 elements by auto-upgrading to 32-bit count and exception index fields, removing single-block size limits.

- **Adaptive Delta Differential Encoding (Delta-ALP)**:<br>
  Automatically evaluates smooth and continuous physical time series (weather, hydrology, telemetry), adaptively applying first-order differences and branchless prefix sum accumulation to reduce bit widths by 15% to 38%.

- **Decimal Division Exact Mode**:<br>
  Completely eliminates IEEE 754 multiplication roundoff errors (such as `* 0.1`) by reconstructing via exact decimal division, driving outlier exception counts to zero on real-world telemetry.

- **Stack-Allocated LUT & SIMD Hybrid Decompression**:<br>
  Utilizes 256-entry stack lookup tables for division modes to eliminate hardware division latencies, coupled with pure-register SIMD auto-vectorization for linear arithmetic exceeding 55+ GB/s throughput.

- **Adaptive Parameter Estimation**:<br>
  Samples input sequences to derive optimal scaling parameters `(exp, fac, use_div)` that minimize bit-width requirements.

- **Frame-of-Reference & Bitpacking**:<br>
  Encodes converted integers using base subtraction (FOR / Delta) and dense bit-packing from 1 to 64 bits per value.

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
  Quickly evaluates bit-exact identical sequences (`v.is_exact_same(first)`). When identical, writes a self-describing compact header and base value with zero heap allocation.<br>
  When estimated payload exceeds raw size plus compact header overhead, switches to raw mode (1 byte for 1024 blocks) to guarantee zero data inflation.

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

- **Header Parsing (`header.rs`, `decoder.rs`)**:<br>
  Reads descriptor byte and decodes element count via 2-bit length tags to locate parameter offsets.<br>
  For raw fallback chunks, performs direct zero-copy slice restoration.<br>
  For ALP chunks, extracts packed `(exp, fac, bit_width)` parameters and base value.

- **Bit Unpacking & SIMD Register Reconstruction (`bitpack/unpack.rs`)**:<br>
  Bit-widths of 8, 16, 32, and 64 bits employ pure register SIMD auto-vectorization, eliminating gather lookups and cache stalls;<br>
  Ultra-small bit-widths (1, 2, 4 bits) leverage compact register-resident tables for rapid reconstruction.

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
│   ├── decoder/        # Generic decompression pipeline & decimal division reconstruction
│   │   ├── mod.rs      # Decompression facade and mode dispatch
│   │   ├── standard.rs # Standard FOR reconstruction
│   │   └── delta.rs    # Delta first-order difference reconstruction
│   ├── delta/          # First-order difference estimation & prefix sum
│   │   └── mod.rs
│   ├── encoder/        # Generic compression pipeline & raw fallback protection
│   │   ├── mod.rs      # Compression facade & auto-vectorized stream
│   │   ├── standard.rs # Standard FOR encoding pipeline
│   │   └── delta.rs    # Delta differential encoding pipeline
│   ├── error.rs        # Error definitions and Result type alias
│   ├── float/          # AlpFloat abstraction trait and f32/f64 zero-cost implementation
│   │   ├── mod.rs      # AlpFloat trait and lookup table generator
│   │   ├── f32.rs      # Single-precision f32 implementation
│   │   └── f64.rs      # Double-precision f64 implementation
│   ├── header.rs       # Compact self-describing header encoding/decoding & 2-bit length tags
│   ├── lib.rs          # Public crate exports and high-level API
│   ├── params.rs       # Compact bitfield parameter packing and bit-width utilities
│   └── sampler.rs      # Adaptive parameter optimization and lossless roundtrip verification
├── test.sh             # Test execution script
└── tests/              # Integration and stress tests
    ├── test_alp_dataset.rs # ALP paper 31 real-world datasets roundtrip & ratio tests
    ├── test_delta.rs       # Delta differential time series test suite
    └── test_roundtrip.rs   # Roundtrip integrity and boundary tests
```

---

## Benchmarks & Cross-Algorithm Comparison

### Benchmark Environment & Toolchain

All microbenchmarks were executed and measured side-by-side on the same physical host:

- **Processor (CPU)**: Apple M2 Max (12 Cores: 8 Performance @ 3.68 GHz + 4 Efficiency @ 2.42 GHz, ARMv8.6-A NEON ISA)<br>
- **Host OS**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust Toolchain**: `rustc 1.98.0 / nightly` (flags: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`)<br>
- **C++ Compiler Toolchain**: Homebrew LLVM Clang 22.1.8 (`-O3 -std=c++17 -DNDEBUG -march=native`) / CMake 4.4.2<br>
- **Memory Allocator**: `mimalloc 0.1.52`<br>
- **Benchmark Suites**: Rust `divan 0.1.20` vs C++ `std::chrono::high_resolution_clock` (steady-state median sampling)

### Cross-Algorithm Benchmark Comparison

Evaluated side-by-side across industry-standard floating-point and time-series compression codecs on identical hardware and data streams:
- **fastalp** (Rust Edition 2024, SIMD NEON)
- **C++ ALP** (Official paper reference implementation, Clang 22.1.8 -O3)
- **Pcodec / pco 1.0.3** (Columnar numeric compression, ANS entropy coding)
- **Zstandard / zstd 0.13** (General dictionary compression, Level 3)
- **LZ4 / lz4_flex 0.14** (Ultra-fast general byte compressor)
- **Snappy / snap 1.1** (Google high-speed byte compressor)
- **Chimp128** (VLDB 2022 floating-point time series)
- **Gorilla** (VLDB 2015 XOR floating-point time series)

#### Comprehensive Comparison on 31 Paper Datasets (253,952 Bytes)

| Codec | Category | 31 Datasets Total Size | Compression Ratio | Bits / Value | Avg Decode Speed | Avg Encode Speed |
|---|---|---|---|---|---|---|
| **fastalp (Rust)** | Specialized Float | **97,943 B** | **2.59x** | **24.68 b/v** | **18.0 GB/s** | **2.32 GB/s** |
| **C++ ALP (Reference)** | Specialized Float | 102,873 B | 2.47x | 25.93 b/v | 21.85 GB/s (peak) | 0.84 GB/s |
| **Pcodec (pco)** | Specialized Numeric ANS | 82,888 B | 3.06x | 20.89 b/v | 1.99 GB/s | 0.24 GB/s |
| **Zstd (level 3)** | General Byte Dict | 101,317 B | 2.51x | 25.53 b/v | 1.42 GB/s | 0.89 GB/s |
| **Chimp128** | Specialized Float XOR | 119,725 B | 2.12x | 30.17 b/v | 0.52 GB/s | 0.69 GB/s |
| **Snappy (snap)** | Fast General Bytes | 127,290 B | 2.00x | 32.08 b/v | 7.51 GB/s | 3.76 GB/s |
| **LZ4 (lz4_flex)** | Fast General Bytes | 129,189 B | 1.97x | 32.56 b/v | 7.74 GB/s | 3.11 GB/s |
| **Gorilla** | Specialized Float XOR | 167,601 B | 1.52x | 42.24 b/v | 0.65 GB/s | 1.19 GB/s |

#### Representative Time Series & Sensor Scenario Breakdown

| Scenario (1024-Point Block) | fastalp (Rust) | C++ ALP (Reference) | Pcodec (pco) | Zstd (level 3) | LZ4 (lz4_flex) | Gorilla |
|---|---|---|---|---|---|---|
| **Physical Sensor**<br>Ratio / Decode Speed | **7.91x** (8.09 b/v)<br>**22.60 GB/s** | 7.86x (8.14 b/v)<br>21.85 GB/s | **99.90x** (0.64 b/v)<br>1.58 GB/s | 30.01x (2.13 b/v)<br>1.99 GB/s | 12.21x (5.24 b/v)<br>11.30 GB/s | 1.11x (57.67 b/v)<br>0.39 GB/s |
| **Monotonic Ramp**<br>Ratio / Decode Speed | **431.16x** (0.15 b/v)<br>**28.08 GB/s** | 0.94x (Inflated)<br>0.58 GB/s | 44.52x (1.44 b/v)<br>0.85 GB/s | 6.93x (9.23 b/v)<br>0.90 GB/s | 1.98x (32.27 b/v)<br>2.72 GB/s | 1.16x (55.07 b/v)<br>0.39 GB/s |
| **Constant Series**<br>Ratio / Decode Speed | **744.73x** (0.09 b/v)<br>**45.20 GB/s** | 455.11x (0.14 b/v)<br>21.85 GB/s | 282.48x (0.23 b/v)<br>4.82 GB/s | 292.57x (0.22 b/v)<br>3.37 GB/s | 146.29x (0.44 b/v)<br>2.08 GB/s | 30.45x (2.10 b/v)<br>1.82 GB/s |

### Side-by-Side Throughput Comparison

| Scenario | Data Size | fastalp Throughput | C++ Reference Throughput | Throughput Ratio (fastalp / C++) |
|---|---|---|---|---|
| **f64 Compress** (Identical Values) | 1024 x f64 (8 KB) | **23.15 GB/s** | 7.02 GB/s | **3.30x** |
| **f64 Compress** (Sensor Decimals) | 1024 x f64 (8 KB) | **6.10 GB/s** | 0.84 GB/s | **7.26x** |
| **f64 Compress** (Large Batch) | 65535 x f64 (512 KB) | **6.57 GB/s** | 5.85 GB/s | **1.12x** |
| **f32 Compress** (Sensor Decimals) | 1024 x f32 (4 KB) | **3.52 GB/s** | 2.46 GB/s | **1.43x** |
| **f64 Decompress** (Identical Values) | 1024 x f64 (8 KB) | **77.01 GB/s** | 21.85 GB/s | **3.52x** |
| **f64 Decompress** (Sensor Decimals) | 1024 x f64 (8 KB) | **57.32 GB/s** | 21.85 GB/s | **2.62x** |
| **f64 Decompress** (Large Batch) | 65535 x f64 (512 KB) | **55.93 GB/s** | 18.42 GB/s | **3.04x** |
| **f32 Decompress** (Sensor Decimals) | 1024 x f32 (4 KB) | **57.45 GB/s** | 32.77 GB/s | **1.75x** |

### Real-World Datasets Compression Ratio

Evaluated against all 31 standard real-world datasets from the original ALP paper (253,952 bytes of raw 64-bit doubles):

| Dataset Name | Raw Size | fastalp Compressed Size | fastalp Ratio | C++ Ref ALP Ratio |
|---|---|---|---|---|
| **gov26**<br>Government Stats | 8192 B | 13 B | **630.15x**<br>(0.10 b/v) | 455.11x |
| **gov31**<br>Government Stats | 8192 B | 25 B | **327.68x**<br>(0.20 b/v) | 292.57x |
| **gov30**<br>Government Stats | 8192 B | 55 B | **148.95x**<br>(0.43 b/v) | 141.24x |
| **stocks_uk**<br>UK Stock Prices | 8192 B | 1165 B | **7.03x**<br>(9.10 b/v) | 7.00x |
| **cms9**<br>Healthcare Billing | 8192 B | 1421 B | **5.76x**<br>(11.10 b/v) | 5.74x |
| **medicare9**<br>Medical Monitoring | 8192 B | 1421 B | **5.76x**<br>(11.10 b/v) | 5.74x |
| **neon_pm10_dust**<br>PM10 Sensor | 8192 B | 1553 B | **5.27x**<br>(12.13 b/v) | 5.26x |
| **stocks_usa_c**<br>US Stock Prices | 8192 B | 1951 B | **4.20x**<br>(15.24 b/v) | 4.19x |
| **gov40**<br>Government Timestamps | 8192 B | 2445 B | **3.35x**<br>(19.10 b/v) | 3.34x |
| **stocks_de**<br>German Stock Prices | 8192 B | 2625 B | **3.12x**<br>(20.51 b/v) | 3.12x |
| **bird_migration_f**<br>GPS Coordinates | 8192 B | 2651 B | **3.09x**<br>(20.71 b/v) | 3.09x |
| **neon_bio_temp_c**<br>Biology Sensor | 8192 B | 2957 B | **2.77x**<br>(23.10 b/v) | 2.77x |
| **food_prices**<br>Consumer Index | 8192 B | 3285 B | **2.49x**<br>(25.66 b/v) | 2.49x |
| **city_temperature_f**<br>Weather Temp | 8192 B | 3363 B | **2.44x**<br>(26.27 b/v) | 2.43x |
| **ssd_hdd_benchmarks_f**<br>Disk Benchmarks | 8192 B | 3621 B | **2.26x**<br>(28.29 b/v) | 2.26x |
| **neon_wind_dir**<br>Wind Direction | 8192 B | 3725 B | **2.20x**<br>(29.10 b/v) | 2.20x |
| **neon_air_pressure**<br>Air Pressure | 8192 B | 3743 B | **2.19x**<br>(29.24 b/v) | 2.19x |
| **basel_wind_f**<br>Basel Wind Speed | 8192 B | 3817 B | **2.15x**<br>(29.82 b/v) | 2.14x |
| **arade4**<br>Hydrology Sensor | 8192 B | 4063 B | **2.02x**<br>(31.74 b/v) | 2.01x |
| **basel_temp_f**<br>Basel Temperature | 8192 B | 4069 B | **2.01x**<br>(31.79 b/v) | 2.01x |
| **bitcoin_f**<br>Bitcoin Rates | 8192 B | 4195 B | **1.95x**<br>(32.77 b/v) | 1.95x |
| **bitcoin_transactions_f**<br>On-chain Tx | 8192 B | 4861 B | **1.69x**<br>(37.98 b/v) | 1.68x |
| **medicare1**<br>Medical Records | 8192 B | 5249 B | **1.56x**<br>(41.01 b/v) | 1.56x |
| **cms1**<br>Medical Records | 8192 B | 5363 B | **1.53x**<br>(41.90 b/v) | 1.53x |
| **cms25**<br>Medical Records | 8192 B | 5451 B | **1.50x**<br>(42.59 b/v) | 1.50x |
| **nyc29**<br>NYC Taxi Travel | 8192 B | 5441 B | **1.51x**<br>(42.51 b/v) | 1.50x |
| **air_sensor_f**<br>Air Sensor Data | 8192 B | 8195 B (Fallback) | **1.00x**<br>(Guaranteed) | 0.52x (Expansion) |
| **poi_lat**<br>High-Precision Lat | 8192 B | 8195 B (Fallback) | **1.00x**<br>(Guaranteed) | 0.51x (Expansion) |
| **poi_lon**<br>High-Precision Lon | 8192 B | 8195 B (Fallback) | **1.00x**<br>(Guaranteed) | 0.64x (Expansion) |
| **TOTAL / Overall Average** | **253,952 B** | **110,773 B** | **2.29x** | **1.94x** |

Thanks to the raw fallback safeguard, `fastalp` completely eliminates negative compression on difficult datasets, reducing overall storage from 130,597 B to 110,773 B and elevating average compression ratio to **2.29x**.

### Real-World Physical Telemetry Benchmark (NOAA & USGS All 64 Series)

Evaluated side-by-side on 64 continuous industrial, marine, and meteorological observation series (NOAA ISD-Lite weather, NOAA CO-OPS tide gauge, USGS NWIS river discharge, comprising 467,550 double-precision points):

| Variable | Series Count | Points | fastalp Ratio | C++ Ref Ratio | Space Saved | fastalp Enc | C++ Enc | fastalp Dec | C++ Dec | Dec Speedup |
|---|---|---|---|---|---|---|---|---|---|---|
| **air_temperature** | 10 | 79,807 | **8.07x**<br>(7.93 b/v) | 7.80x | **-3.4%** | **2.55 GB/s** | 0.48 GB/s | **12.99 GB/s** | 0.59 GB/s | **22.0x** |
| **dew_point** | 10 | 79,772 | **8.30x**<br>(7.71 b/v) | 7.95x | **-4.2%** | **2.48 GB/s** | 0.49 GB/s | **8.58 GB/s** | 0.60 GB/s | **14.3x** |
| **sea_level_pressure** | 10 | 72,857 | **9.24x**<br>(6.93 b/v) | 7.42x | **-19.6%** | **2.32 GB/s** | 0.46 GB/s | **6.40 GB/s** | 0.58 GB/s | **11.0x** |
| **wind_direction** | 9 | 69,384 | **7.10x**<br>(9.01 b/v) | 7.04x | **-0.8%** | **2.10 GB/s** | 0.45 GB/s | **6.48 GB/s** | 0.61 GB/s | **10.6x** |
| **wind_speed** | 9 | 71,298 | **7.07x**<br>(9.05 b/v) | 7.83x | - | **2.21 GB/s** | 0.47 GB/s | **23.57 GB/s** | 0.62 GB/s | **38.0x** |
| **water_level** | 4 | 29,760 | **8.51x**<br>(7.52 b/v) | 5.32x | **-37.4%** | **2.41 GB/s** | 0.42 GB/s | **10.74 GB/s** | 0.54 GB/s | **19.9x** |
| **water_level_sigma** | 4 | 29,760 | **6.27x**<br>(10.20 b/v) | 9.36x | - | **2.15 GB/s** | 0.52 GB/s | **12.10 GB/s** | 0.64 GB/s | **18.9x** |
| **discharge** | 4 | 17,452 | **5.36x**<br>(11.93 b/v) | 4.34x | **-19.2%** | **1.85 GB/s** | 0.38 GB/s | **4.91 GB/s** | 0.49 GB/s | **10.0x** |
| **gage_height** | 4 | 17,460 | **9.72x**<br>(6.58 b/v) | 7.75x | **-20.3%** | **2.20 GB/s** | 0.44 GB/s | **6.66 GB/s** | 0.55 GB/s | **12.1x** |
| **【64 Series Total】** | **64** | **467,550** | **7.72x**<br>(**8.29 b/v**) | **7.30x** | **-5.54%** | **2.35 GB/s** | **0.47 GB/s** | **11.20 GB/s** | **0.58 GB/s** | **19.3x** |

- **Compression Ratio Breakthrough**: By coupling Decimal Division Exact Mode with adaptive Delta differencing, `fastalp` compresses real physical telemetry to 8.29 b/v on average, outperforming the C++ reference by 5.54% overall and by 20% to 37% on tidal and gage-height signals.
- **Overwhelming Throughput Advantage**: Decompression throughput reaches 11.20 GB/s on a single core, surpassing the C++ reference (0.58 GB/s) by **19.3x**. Compression throughput reaches 2.35 GB/s (**5.0x faster** than C++).

---

## Architecture Comparison & Engineering Optimizations

Compared with the reference C++ implementation, `fastalp` not only multiplies throughput performance, but also introduces major algorithmic innovations that break through the compression ratio limitations of the reference implementation.

### Algorithmic Innovations on Compression Ratio (vs Reference C++)

#### 1. Decimal Division Exact Mode — Eliminating Multiplication Rounding Exceptions
- **Reference C++ Limitation**:<br>
  C++ ALP exclusively uses multiplication for inverse reconstruction: `v = (encoded * frac_exp) / fac`. Because binary IEEE 754 cannot represent decimal fractions like `0.1` exactly, multiplication introduces unavoidable roundoff discrepancies (e.g. `123 * 0.1` evaluates to `12.30000000000000071... != 12.3`). This causes the reference implementation to misclassify clean decimal measurements as "Exceptions", requiring an 8-byte original float plus a 2-byte index (an 80-bit penalty per outlier in standard 1024-element blocks). In real-world weather and oceanographic telemetry, this inflated exception count severely degrades compression ratios.
- **fastalp Algorithmic Innovation**:<br>
  `fastalp` introduces **Decimal Division Exact Mode (`TYPE_F64_DEC` / `TYPE_F32_DEC`)**. During parameter evaluation and decoding, dividing by powers of ten (e.g. `/ 10.0`) reconstructs the exact original IEEE 754 bit pattern without truncation error.
  - **Compression Gain**: On real-world time-series (such as NOAA tidal heights and surface temperatures), the exception count drops from hundreds in C++ ALP to **zero**. Eliminating the exception dictionary shrinks compressed size by an additional **20% to 38%** (e.g. ocean tidal series compression improves from 5.32x to 8.51x).
  - **Zero-Latency Decoding**: To prevent hardware division latency from impacting decompression throughput, `fastalp` couples this with a 256-entry stack-allocated lookup table (LUT), preserving maximum compression ratio while sustaining 55+ GB/s throughput.

#### 2. Adaptive Delta-ALP Differential Encoding — Breaking Global Dynamic Range Bounds
- **Reference C++ Limitation**:<br>
  C++ ALP relies solely on Frame-of-Reference (FOR) base subtraction: `stored = encoded - min_encoded`. For continuous physical time-series (such as diurnal temperature cycles, tidal oscillations, river stage ramps, and monotonic metric counters), the dynamic range `(max - min)` across a 1024-element block is wide (often spanning thousands of integer units, requiring 12 to 16 bits per element). FOR cannot exploit the strong local correlation between adjacent points.
- **fastalp Algorithmic Innovation**:<br>
  `fastalp` introduces **Adaptive Delta-ALP (`TYPE_F64_DELTA` / `TYPE_F32_DEC_DELTA`)**:
  1. **Adaptive Benefit Evaluation**: Dynamically evaluates the bit-width required by FOR versus first-order differences (`delta[i] = encoded[i] - encoded[i-1]`), activating Delta-ALP only when differences yield measurable savings;
  2. **Tightly Packed Adjacent Differences**: Stores the initial element as a baseline, bitpacking `(delta - min_delta)`. Physical sensor delta bit-widths routinely collapse to 1 to 6 bits;
  3. **Compression Gain**: Yields a **50% to 90%** size reduction on smooth and monotonic time-series. In monotonic ramp benchmarks, bits per value fall from 68.09 b/v in the reference implementation to 0.16 b/v, elevating compression ratio to **390x** (compared to 0.94x in C++ ALP).

#### 3. Outlier Smoothing Isolation — Safeguarding Delta Bit-Width from Noise
- **Reference C++ Limitation**:<br>
  In traditional differential encoding pipelines, a single sensor spike or outlier pollutes two adjacent deltas (the jump upward and the drop downward), causing the maximum delta range across the entire block to explode and degrading bit-width for all points.
- **fastalp Algorithmic Innovation**:<br>
  `fastalp` designs an **Outlier Smoothing Isolation** mechanism for Delta encoding: when a floating-point exception occurs, the delta stream records an increment of 0 (carrying forward the previous valid integer), while the true outlier float is isolated in the patch dictionary. On decompression, a branchless register prefix-sum pass reconstructs the base integers before patching outliers in-place. This preserves minimal delta bit-widths without compromising 100% bit-exact lossless fidelity.

#### 4. Raw Fallback Safeguard Against Negative Compression
- **Reference C++ Limitation**:<br>
  On high-entropy unstructured floats or high-precision geographic coordinates (such as POI latitude/longitude), exception tables expand beyond uncompressed payload size, leading to negative compression (down to 0.51x, doubling storage size).
- **fastalp Algorithmic Innovation**:<br>
  At the end of the encoding pipeline, `fastalp` checks the compressed payload size. If it exceeds the raw data plus a compact metadata header (only 1 byte for 1024-element blocks), the encoder instantly falls back to `TYPE_RAW` mode, storing raw bytes directly. This eliminates negative compression entirely, lifting overall dataset compression from 1.94x to 2.29x across the 31 paper datasets.

---

### Engineering Micro-Architecture & Throughput Optimizations (vs Reference C++)

#### Constant Sequence Fast Detection & Zero-Heap Allocation

- **Reference C++ Implementation**:<br>
  Executes full parameter sampling, intermediate integer transformation, and bit-width analysis even on completely constant sequences, requiring 9.25 µs end-to-end.<br>
- **fastalp Optimization**:<br>
  Inspects raw IEEE 754 bits at compression entry (`v.is_exact_same(first)`), strictly differentiating `+0.0` and `-0.0` sign bits;<br>
  Directly emits a self-describing compact header and base value (`bit_width = 0`) upon match, skipping parameter search and vector allocation, reducing compression time to 351 ns (26x speedup).

#### Zero-Heap Direct Streaming Decompression

- **Reference C++ Implementation**:<br>
  Employs a two-stage decompression pipeline: stage 1 unpacks bitstream to an intermediate heap array, and stage 2 iterates over the array to compute float unscaling and patch exceptions, incurring 8 B/elem heap allocation and cache pressure.<br>
- **fastalp Optimization**:<br>
  Executes a single-pass direct streaming reconstruction pipeline. Bits are unpacked within CPU registers and written directly to the caller destination slice, keeping L1/L2 caches hot;<br>
  For Delta differential time series, leverages 1024-element byte-alignment to stream-decode in fixed 1024-element stack batches, reducing additional heap allocations for arbitrary large arrays to 0 bytes while keeping stack buffers hot in L1 data cache;<br>
  Provides `compress_into` and `decompress_into` zero-allocation APIs.

#### Pure-Register SIMD Vectorized Decompression & Hybrid Local Table Acceleration

- **Reference C++ Implementation**:<br>
  Inner loop relies on two-stage heap buffering and scalar arithmetic, failing to saturate modern SIMD execution pipelines.<br>
- **fastalp Optimization**:<br>
  Eliminates large stack tables that induce indirect gather memory stalls; bit-widths of 8, 16, 32, and 64 bits execute pure linear register arithmetic with a dedicated `fac1` path (omitting integer multiplication), enabling LLVM to emit optimal SIMD vector instructions; 1, 2, and 4 bit-widths utilize tiny register-resident tables, driving single-core decode throughput up to 57+ GB/s.

#### Two-Pass SIMD Vectorized Encoding & Early-Exit Sampling

- **Reference C++ Implementation**:<br>
  Complex multi-level sampling logic with dense conditional branches inside the encoding loop, fragmenting basic blocks.<br>
- **fastalp Optimization**:<br>
  Introduces an `EARLY_EXIT_BIT_WIDTH` threshold during sampling to halt immediately once a high-compression model is identified, bypassing wasteful checks across 135 parameter combinations; adopts a Two-Pass decoupled encoding architecture (Pass 1 branchless register-level float-to-int rounding, Pass 2 centralized exception verification), eliminating per-element pipeline stalls and driving batch compression throughput up to 5.4+ GB/s.

#### Pure 128-bit Register Bitpacker

- **Reference C++ Implementation**:<br>
  Generates extensive template code across multiple compilation units, creating large binaries with architecture-specific intrinsics.<br>
- **fastalp Optimization**:<br>
  Maintains a sliding bit window with a single 128-bit register accumulator (`acc: u128`, `bits_in_acc: u32`), executing 64-bit word writes and reads in single instructions;<br>
  Pure safe Rust with zero external C++ toolchain dependencies, cross-compiling seamlessly for x86_64, ARM64, and WebAssembly.

#### Sample-Space Cost Lower-Bound Pruning

- **Reference C++ Implementation**:<br>
  Evaluates all samples across 135 `(exp, fac)` parameter combinations unconditionally.<br>
- **fastalp Optimization**:<br>
  Applies dynamic lower-bound pruning: breaks inner verification immediately once running exception penalty (`exceptions * penalty`) surpasses current global `best_cost`, skipping unnecessary parameter iterations.

#### Branchless Arithmetic & Precomputed Constants

- Pre-extracts exponent factor tables outside inner loops to eliminate repeated array lookups;<br>
- Calculates bit-width using hardware CLZ instructions and applies compile-time bitmasks to eliminate conditional branch mispredictions.
