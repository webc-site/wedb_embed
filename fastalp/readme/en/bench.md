## Performance & Comparative Benchmarks

### Test Environment and Compiler Setup

All benchmarks were evaluated on identical hardware under equivalent conditions:

- **Processor**: Apple M2 Max (12 cores: 8 Performance @ 3.68 GHz + 4 Efficiency @ 2.42 GHz, ARMv8.6-A NEON)<br>
- **Operating System**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust Toolchain**: `rustc 1.98.0 / nightly` (flags: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`)<br>
- **C++ Toolchain**: Homebrew LLVM Clang 22.1.8 (`-O3 -std=c++17 -DNDEBUG -march=native`) / CMake 4.4.2<br>
- **Memory Allocator**: `mimalloc 0.1.52`<br>
- **Benchmark Suite**: Rust `divan 0.1.20` micro-benchmark harness vs C++ `std::chrono::high_resolution_clock` (median steady-state sampling)

### Cross-Algorithm Benchmark Comparison

Tested against standard floating-point and time-series codecs across all 37 datasets on identical hardware:

| Codec | Category | Decomp Throughput | vs C++ Decomp | Comp Throughput (End-to-End) | vs C++ Comp | GeoMean Ratio |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: |
| **fastalp (Rust)** | Specialized Float | **26.9 GB/s** | **1.4x vs C++** | **3.6 GB/s** | **4.5x vs C++** | **6.99x** |
| **C++ ALP** (Paper Reference) | Specialized Float | **19.9 GB/s** | Baseline (1.0x) | **0.8 GB/s** | Baseline (1.0x) | **5.93x** |
| **Pcodec (pco 1.0.3)** | Specialized Float | **1.8 GB/s** | 0.09x (14.9x slower) | **0.2 GB/s** | 0.29x (15.6x slower) | **6.16x** |
| **Zstandard (zstd lvl 3)** | General Stream | **1.2 GB/s** | 0.06x (22.4x slower) | **0.5 GB/s** | 0.58x (7.8x slower) | **4.83x** |
| **LZ4 (lz4_flex 0.14)** | General Byte | **4.4 GB/s** | 0.22x | **1.7 GB/s** | 2.14x | **3.26x** |
| **Snappy (snap 1.1)** | General Byte | **4.1 GB/s** | 0.21x | **2.2 GB/s** | 2.80x | **2.72x** |
| **Chimp128** (VLDB 2022) | Specialized Float | **0.5 GB/s** | 0.02x | **0.6 GB/s** | 0.76x | **2.47x** |
| **Gorilla** (VLDB 2015) | Specialized Float | **0.6 GB/s** | 0.03x | **0.9 GB/s** | 1.14x | **2.14x** |

### Industrial Scenario Micro-Benchmarks

| Business Scenario Slice | Dataset Scale | fastalp (Decomp / Comp / Ratio) | C++ ALP (Decomp / Comp / Ratio) | Pcodec (Decomp / Comp / Ratio) | Zstd (Decomp / Comp / Ratio) |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **IoT Environmental Sensing** | 11 sets (11,264 pts) | **26.6 GB/s** ｜ **5.7 GB/s** ｜ **7.92x** | 21.3 GB/s ｜ 0.8 GB/s ｜ 7.91x | 1.6 GB/s ｜ 0.2 GB/s ｜ 3.02x | 1.0 GB/s ｜ 0.4 GB/s ｜ 2.11x |
| **Quantitative Trading Quotes** | 7 sets (7,168 pts) | **19.6 GB/s** ｜ **5.9 GB/s** ｜ **7.04x** | 20.5 GB/s ｜ 0.8 GB/s ｜ 7.04x | 1.7 GB/s ｜ 0.2 GB/s ｜ 3.71x | 1.2 GB/s ｜ 0.4 GB/s ｜ 2.90x |
| **Geospatial & GPS Trajectory** | 5 sets (5,120 pts) | **19.9 GB/s** ｜ **5.2 GB/s** ｜ **6.35x** | 20.3 GB/s ｜ 0.8 GB/s ｜ 6.07x | 2.0 GB/s ｜ 0.2 GB/s ｜ 1.84x | 1.1 GB/s ｜ 0.4 GB/s ｜ 1.63x |
| **Healthcare Claims & Billing** | 5 sets (5,120 pts) | **36.3 GB/s** ｜ **2.1 GB/s** ｜ **1.66x** | 20.0 GB/s ｜ 0.8 GB/s ｜ 2.19x | 2.0 GB/s ｜ 0.2 GB/s ｜ 2.16x | 0.9 GB/s ｜ 0.4 GB/s ｜ 1.99x |
| **Public Demographics & Census** | 6 sets (6,144 pts) | **44.7 GB/s** ｜ **7.0 GB/s** ｜ **8.89x** | 21.7 GB/s ｜ 0.8 GB/s ｜ 4.64x | 3.0 GB/s ｜ 0.4 GB/s ｜ 3.79x | 3.0 GB/s ｜ 2.1 GB/s ｜ 4.15x |
| **Monotonic Ramp & Steady Streams** | 3 sets (3,072 pts) | **44.4 GB/s** ｜ **10.2 GB/s** ｜ **11.70x** | 19.8 GB/s ｜ 0.9 GB/s ｜ 2.90x | 1.0 GB/s ｜ 0.1 GB/s ｜ 8.58x | 1.4 GB/s ｜ 0.4 GB/s ｜ 6.84x |

### C++ ALP Benchmark Methodology & Calibration

- **C++ ALP Fork Repository**: [github.com/x-at-01/ALP](https://github.com/x-at-01/ALP)
- **Unified Methodology Notes**:
  - **100% Unaltered Core Logic**: The fork maintains the original core algorithm (`include/` directory) without modification, preserving the authors' SIMD and inverse mapping logic;
  - **End-to-End Pipeline vs Pure Kernel Throughput**:
    - **Pure Kernel (Paper methodology, ~5.3 GB/s)**: C++ ALP's official benchmark (`ALP/benchmarks/benchmark.cpp`) calls `alp::encoder<PT>::init` outside the measurement loop, assuming optimal exponents and factors are known beforehand. Tested on this machine, pure kernel geometric mean throughput is approximately **5.3 GB/s**;
    - **End-to-End Compression (Real-world metric, 0.8 GB/s)**: In real-world time-series ingestion, incoming blocks require adaptive parameter sampling. When `init` sampling is measured, C++ ALP's unpruned exhaustive search accounts for >80% of execution time, yielding an end-to-end throughput of **0.8 GB/s**;
    - **fastalp End-to-End (3.6 GB/s)**: fastalp performs complete end-to-end compression including adaptive parameter sampling from scratch. With 3-stage microarchitectural pruning, it achieves **3.6 GB/s** geometric mean end-to-end throughput (4.5x faster than C++ ALP end-to-end, up to 7.0x in specific datasets); when hitting stateful parameter cache, pure kernel throughput reaches **15~24+ GB/s**;
    - **Decompression Throughput (26.9 GB/s vs 19.9 GB/s)**: Utilizing branchless SIMD register pipelines and L1D stack LUTs, fastalp attains **26.9 GB/s** geometric mean decompression throughput, outperforming C++ ALP's **19.9 GB/s** (1.4x faster).
  - **Full 37 Dataset Coverage**:
    - Supplements 6 industrial scenarios into `ALP/data/samples/` and `your_own_dataset.csv`, enabling full 37-dataset evaluation (31 paper datasets + 6 industrial benchmarks);
    - Evaluates Geometric Mean across all 37 datasets without sampling bias. fastalp achieves an overall geometric mean compression ratio of **6.99x** (compared to C++ ALP's **5.93x**).

### Comprehensive Dataset Coverage & Sources

Evaluated on all 31 public datasets from the original ALP paper plus 6 representative industrial benchmarks across 6 domains:

- **IoT & Environmental Sensors (11 datasets)**: `neon_pm10_dust`, `neon_dew_point_temp`, `neon_air_pressure`, `neon_wind_dir`, `neon_bio_temp_c`, `basel_temp_f`, `basel_wind_f`, `city_temperature_f`, `air_sensor_f`, `arade4`, `scene_sensor`.
- **Quantitative Finance & Trading (7 datasets)**: `stocks_usa_c`, `stocks_de`, `stocks_uk`, `bitcoin_f`, `bitcoin_transactions_f`, `food_prices`, `scene_finance`.
- **Geographic Mapping & Trajectories (5 datasets)**: `poi_lat`, `poi_lon`, `bird_migration_f`, `nyc29`, `scene_geo`.
- **Healthcare & Public Assistance (5 datasets)**: `medicare1`, `medicare9`, `cms1`, `cms9`, `cms25`.
- **Government & Macroeconomics (6 datasets)**: `gov10`, `gov26`, `gov30`, `gov31`, `gov40`, `scene_macro`.
- **Hardware Storage & Physical Waveforms (3 datasets)**: `ssd_hdd_benchmarks_f`, `scene_ramp`, `scene_steady`.
