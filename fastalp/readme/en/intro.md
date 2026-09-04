# fastalp : Lossless Floating-Point Compression in Pure Rust

A pure Rust implementation of adaptive lossless floating-point compression, deeply absorbing and extending the theoretical foundation of the ACM SIGMOD 2024 Best Artifact paper [ALP](https://dl.acm.org/doi/10.1145/3626717), providing high-performance unified generic interfaces for both `f64` and `f32` streams.

<p align="center">
  <img src="https://fastly.jsdelivr.net/gh/webc-fs/-@PQ/wZlYnSCRgaBfHf3Zo3WQ.svg" alt="fastalp Floating-Point Compression Performance & Ratio Benchmark" width="100%">
  <br>
  <sub><b>Benchmark Environment</b>: CPU: Apple M2 Max (12 Cores) ｜ OS: macOS 26.5.1 ｜ Toolchain: Rust 1.98.0 / Clang (-O3)</sub>
</p>

---

## Theoretical Background & Official Paper

ALP (Adaptive Lossless Floating-Point Compression) was introduced at **ACM SIGMOD 2024** by the database research team at CWI (Azim Afroozeh, Leonardo Kuffó, Peter Boncz) and won the **SIGMOD 2024 Best Artifact Award**. It is integrated into modern columnar database engines such as **DuckDB**, **FastLanes**, and **KuzuDB**:

- **Official Paper**: _ALP: Adaptive Lossless Floating-Point Compression_, ACM SIGMOD 2024 · [DOI: 10.1145/3626717](https://dl.acm.org/doi/10.1145/3626717)
- **Official C++ Implementation**: [github.com/cwida/ALP](https://github.com/cwida/ALP)
- **Core Theoretical Insight**: Most floating-point values in real-world time series (IoT, finance, telemetry) originate from decimal readings with fixed decimal places. By adaptively projecting floats onto integers, combined with Frame-of-Reference (FOR) and SIMD bitpacking, ALP delivers compression ratios and speeds far exceeding general-purpose compressors.

`fastalp` fully retains and rigorously validates the official ALP foundations while re-engineering the encoding/decoding execution pipelines to overcome limitations in dynamic range, multiplication truncation errors, self-describing framing, and unpruned sampling overhead.

---

## Features

In IoT sensing, quantitative finance, GPS telemetry, and observability monitoring, floating-point measurements naturally originate from decimal scales.<br>
Due to the IEEE 754 layout of exponents and mantissas, general-purpose byte compressors and integer bitpackers often perform poorly on raw floating-point streams.

`fastalp` delivers lossless compression tailored to decimal float patterns:

- **Adaptive Parameter Estimation**:<br>
  Samples input streams and evaluates a cost model to discover optimal decimal scaling factors `(exp, fac)` that minimize combined bit-width and exception overhead.

- **Lossless Integer Mapping**:<br>
  Multiplies floats by decimal factors to project them into integers, validating reversibility via inverse scaling to ensure bit-exact fidelity (`a.to_bits() == b.to_bits()`).

- **Frame-of-Reference & Dense Bitpacking**:<br>
  Subtracts the frame-wide minimum value to shift integers into non-negative offsets, packed at dynamic bit-widths (1 to 64 bits).

- **Isolated Exception Stream**:<br>
  Special floats (`NaN`, `+Inf`, `-Inf`, `-0.0`) and values that cannot be encoded losslessly are recorded separately with their original IEEE 754 bit representations.

- **Strict Bit-Exact Roundtripping**:<br>
  Guarantees decoded floats match the original binary representation bit-for-bit.

- **Unified Generic Support**:<br>
  Zero-cost abstractions for both `f64` and `f32` streams, handling high-precision scientific computing and lightweight sensor telemetry alike.

- **Zero-Allocation APIs**:<br>
  Provides `_into` function variants to write directly into caller-managed, preallocated buffers without runtime heap allocations.

### Key Algorithmic & Architectural Breakthroughs over C++ ALP

- **Adaptive Delta-ALP**:<br>
  First-order differences and prefix-sum recurrence with a 16-sample early-exit filter to narrow dynamic bit-widths by 15% ~ 38%.

- **Decimal Exact Division Reconstruction (`use_div`)**:<br>
  Eliminates spurious exception points caused by IEEE 754 binary truncation in float multiplication, reducing footprint by 20% ~ 38%.

- **Intelligent Outlier Pruning for Sparse Constants (0-bit Encoding)**:<br>
  Isolates sparse impulse spikes to the exception dictionary, allowing base streams to drop to 0-bit width and delivering 150x ~ 744x compression ratios on constant-heavy series.

- **Previous-Value Exception Backfilling**:<br>
  Backfills exception slots with preceding integers to prevent artificial gradient spikes in difference encoding.

- **Hardware-Native Round-Ties-Even (`round_ties_even`)**:<br>
  Replaces the legacy IEEE 754 magic number offset (`0x0018000000000000`, limited to $[-2^{51}, 2^{51}]$) with direct hardware round-to-nearest-even instructions (x86 `ROUNDSD` / ARM64 `FRINTN`), guaranteeing full-range fidelity.

- **2-bit Self-Describing Headers & Arbitrary Array Slicing**:<br>
  Compact 3-byte headers for full 1024-element blocks and 1-byte headers for raw fallbacks, automatically scaling to 32-bit counts for large slices.

- **12.5% Exception Ceiling & RAW Fallback**:<br>
  Enforces a 12.5% exception limit to guard against negative compression, reverting gracefully to raw byte storage on incompressible random data.

- **Single-Comparison Fast Path**:<br>
  Detects uniform arrays in a single comparison cycle, emitting 1024 uniform items in 11 bytes within 1 clock cycle (744x ratio).

- **Three-Stage Microarchitectural Sampling Pruning**:<br>
  Replaces unpruned parameter searches with a 3-tier cascade (pure decimal early return, 4/16-sample short-circuiting, and non-decimal abort), boosting end-to-end compression throughput to **3.7 GB/s (4.6x faster than C++ ALP)**; pure encoding kernel throughput reaches **6.0 GB/s (1.10x faster than C++ ALP)**; streaming throughput reaches **15~24+ GB/s** with cached parameters.
