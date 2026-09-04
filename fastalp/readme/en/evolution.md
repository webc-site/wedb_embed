## Architectural Evolution & Novel Optimizations

`fastalp` is an engineered reimagining of the ALP paradigm for modern superscalar architectures and columnar time-series storage engines.

### Foundations Inherited from Original ALP

- **Two-Level Adaptive Sampling**:
  Derives optimal decimal scaling parameters `(exp, fac)` that minimize combined bit-width and exception penalties through two-phase coarse and fine sampling.

- **Hardware-Native Round-Ties-Even (Upgraded from Magic Number)**:
  Original ALP utilized IEEE 754 bias constants (`0x0018000000000000` / `12582912.0`) inside floating-point units. `fastalp` investigates its $[-2^{51}, 2^{51}]$ range boundary limitations and upgrades it to hardware-native round-to-nearest-even instructions (x86 `ROUNDSD` / ARM64 `FRINTN`), eliminating range overflow risks while maintaining branchless latency.

- **FOR Frame-of-Reference Subtraction**:
  Subtracts the frame-wide minimum value to shift signed ranges into compact non-negative domains, reducing encoded bit-widths.

- **Stateful Encoder & Parameter Caching**:
  Enables caching of derived `(exp, fac)` models across consecutive 1024-element blocks in continuous streams, boosting steady-state throughput from `4-5 GB/s` to `15-24+ GB/s`.

---

### Proprietary Algorithmic & Performance Breakthroughs

- **Adaptive Delta-ALP**:
  Smooth sensor physical waveforms often have large absolute spans but tiny step differences. `fastalp` implements first-order difference encoding with 16-sample mathematical short-circuit pruning, narrowing bit-widths by 15% ~ 38%.

- **Decimal Exact Division Reconstruction (`use_div`)**:
  Eliminates spurious exception inflation caused by IEEE 754 binary truncation in multiplication (e.g. `* 0.1`). Reduces stored byte volume by 20% ~ 38%.

- **Intelligent Outlier Pruning & 0-bit Sparse Encoding**:
  For datasets where 99% of values are constant with rare isolated pulses, `fastalp` strips outliers into the exception dictionary, allowing the main bitstream to drop to 0-bit. Delivers compression ratios exceeding 150x ~ 744x.

- **Exception Previous-Value Backfill**:
  Backfills exceptions with previous integer values to prevent artificial gradient steps that corrupt delta difference bit-widths.

- **2-bit Self-Describing Headers & Arbitrary Length Support**:
  Employs a 2-bit length tag: standard 1024-element frames require only 3 bytes of header, while RAW fallback frames require 1 byte. Automatically scales to 32-bit offsets for arrays exceeding 65,535 elements.

- **12.5% Exception Bound & Single-Byte RAW Fallback**:
  Guarantees zero negative compression inflation on high-entropy data by falling back to a 1-byte header RAW stream whenever exceptions exceed 12.5% or encoded bytes exceed raw size.

- **Single-Comparison Fast Path for Equi-Value Sequences**:
  Checks `slice[1] == slice[0]` on block entry; non-constant streams exit in 1 CPU cycle, while constant sequences encode 1024 elements into 11 bytes (744x ratio).

- **Three-Stage Microarchitectural Pruning Pipeline**:
  Replaces unpruned parameter searches with a 3-tier cascade (pure decimal early return, 4/16-sample short-circuiting, and non-decimal abort), boosting end-to-end compression throughput from 0.80 GB/s to **3.7 GB/s** (4.6x geometric mean speedup, up to 7.0x in specific datasets); pure encoding kernel throughput reaches **6.0 GB/s (1.10x faster than C++ ALP)**; streaming throughput reaches **15~24+ GB/s** with cached parameters.

- **Pure Register SIMD Decompression**:
  Vectorizes common bit-widths (8, 16, 32, 64) into branchless register pipelines, achieving **27.0 GB/s** geometric mean decompression throughput (surpassing C++ ALP's 20.0 GB/s, 1.35x faster).

- **256-Entry L1D Stack-Allocated Lookup Tables**:
  Eliminates costly division latency by maintaining stack-resident tables that fit entirely in L1D cache.

- **Fused 8-Way Register-Level Delta Bitpacker**:
  Merges difference calculation, base subtraction, and bitpacking into a unified single-pass 128-bit register pipeline, eliminating intermediate memory roundtrips.

- **Mathematical Short-Circuit Delta Filter**:
  Proves mathematically that if the first 16 samples' delta range exceeds the FOR span, full delta encoding cannot be optimal, avoiding redundant scans for 90% of irregular series.

- **Branchless 4-Way Unrolled Encoding Loop**:
  Unrolls core scalar loops into 4-way parallel ALU streams, reaching **4.4~6.8 GB/s** encoding speeds.

- **Zero-Allocation Streaming Pipeline**:
  Provides `compress_into` and `decompress_into` interfaces, allowing applications to reuse buffers without GC or heap allocation churn.

- **Zero-Cost Generic Trait Abstraction**:
  Unifies `f64` and `f32` operations under `AlpFloat` with precomputed static power tables and compile-time inlining.
