# fastalp : Lossless Floating-Point Compression in Pure Rust

A pure Rust implementation of adaptive lossless floating-point compression based on the ALP algorithm, providing generic interfaces for both `f64` and `f32` data streams.

<p align="center">
  <img src="https://fastly.jsdelivr.net/gh/webc-fs/-@em/CqukPak3hYNl8GkgfRhQ.svg" alt="fastalp Floating-Point Compression Performance & Ratio Benchmark" width="100%">
  <br>
  <sub><b>Benchmark Environment</b>: CPU: Apple M2 Max (12 Cores) ｜ OS: macOS 26.5.1 ｜ Toolchain: Rust 1.98.0 / Clang (-O3)</sub>
</p>

---

## Features

In IoT sensing, quantitative finance, GPS telemetry, and observability monitoring, floating-point measurements naturally originate from decimal scales.<br>
Due to the IEEE 754 layout of exponents and mantissas, general-purpose byte compressors and integer bitpackers often perform poorly on raw floating-point streams.

`fastalp` delivers lossless compression tailored to decimal float patterns:

- **Adaptive Parameter Estimation**:<br>
  Samples input streams and evaluates a cost model to discover optimal decimal scaling factors `(exp, fac)` that minimize combined bit-width and exception overhead.

- **Lossless Integer Mapping**:<br>
  Multiplies floats by decimal factors to project them into integers, validating reversibility via inverse scaling to ensure bit-exact fidelity.

- **Frame-of-Reference & Dense Bitpacking**:<br>
  Subtracts the frame-wide minimum value to shift integers into non-negative offsets, packed at dynamic bit-widths (1 to 64 bits).

- **Isolated Exception Stream**:<br>
  Special floats (`NaN`, `+Inf`, `-Inf`, `-0.0`) and values that cannot be encoded losslessly are recorded separately with their original IEEE 754 bit representations.

- **Strict Bit-Exact Roundtripping**:<br>
  Guarantees decoded floats match the original binary representation bit-for-bit (`a.to_bits() == b.to_bits()`).

- **Unified Generic Support**:<br>
  Zero-cost abstractions for both `f64` and `f32` streams, handling high-precision scientific computing and lightweight sensor telemetry alike.

- **Zero-Allocation APIs**:<br>
  Provides `_into` function variants to write directly into caller-managed, preallocated buffers without runtime heap allocations.

Key algorithmic improvements over original C++ ALP:

- **Adaptive Delta Encoding**:<br>
  First-order differences and prefix-sum recurrence with a 16-sample early-exit filter to narrow dynamic bit-widths by 15% ~ 38%.

- **Decimal Exact Division Reconstruction**:<br>
  Eliminates spurious exception points caused by IEEE 754 binary truncation in float multiplication, reducing footprint by 20% ~ 38%.

- **Outlier Pruning for Sparse Constants**:<br>
  Isolates sparse impulse spikes to the exception dictionary, allowing base streams to drop to 0-bit width and delivering 150x ~ 744x compression ratios on constant-heavy series.

- **Previous-Value Exception Backfilling**:<br>
  Backfills exception slots with preceding integers to prevent artificial gradient spikes in difference encoding.

- **2-bit Self-Describing Headers**:<br>
  Compact 3-byte headers for full 1024-element blocks and 1-byte headers for raw fallbacks, automatically scaling to 32-bit counts for large slices.

- **12.5% Exception Ceiling & RAW Fallback**:<br>
  Enforces a 12.5% exception limit to guard against negative compression, reverting gracefully to raw byte storage on incompressible random data.

- **Single-Comparison Fast Path**:<br>
  Detects uniform arrays in a single comparison cycle, emitting 1024 uniform items in 11 bytes within 1 clock cycle.
