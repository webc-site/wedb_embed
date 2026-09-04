## Changelog

### v0.1.35

- **Raw Pointer Decompression Kernel & Soundness Guarantee**:
  Introduced `decompress_into_raw`, `decode_standard_raw`, and `decode_delta_raw` to write directly into target raw pointers, avoiding constructing slice references over uninitialized memory; seamlessly supports uninitialized buffers from C callers via C-API.
- **Single-Pass Exception Patching**:
  Refactored `patch_exceptions` using `chunks_exact` to eliminate repeated slice recalculation and bounds checks in the inner loop.
- **Dead Code Elimination & Hardware-Accelerated Rounding**:
  Removed legacy `MAGIC_NUMBER` simulation constants, adopting `round_ties_even()` with direct mapping to SSE4.1/AVX and ARM64 instructions, ensuring 100% bit-exact lossless roundtrip.

### v0.1.34

- **Strict Code Standards & Zero Compiler Warnings**:
  Completely eliminated all `#[allow(...)]` attributes across the entire codebase (`src/`), addressing all Clippy warnings and dead code to enforce strict code quality.

- **Struct Encapsulation & Architectural Decoupling**:
  Encapsulated compression parameters (exponent, factor, exception threshold, bit-width, etc.) into `AlpParams`, eliminating raw tuple arguments. Encapsulated `AlpHeader` decoder to remove scattered magic numbers and manual bit offsets.

- **Bitpack Kernel Refactoring & Code Reuse**:
  Abstracted and unified the 8-element loop packing kernel `pack_chunk_8`, removing duplicated loop unrolls. Streamlined the Delta first-order difference decoder with tree-reduction to eliminate scalar dependency chains and improve instruction-level parallelism (ILP).

- **Accurate Benchmark Calibration & Branch Isolation**:
  Refined C++ ALP benchmark metrics extraction, clearly distinguishing between sampled compression throughput (~0.85 GB/s) and raw kernel throughput (~5.9 GB/s), while accurately recording decompression throughput (~20.3 GB/s). Decoupled the official PR branch from self-use evaluation branches.

- **Documentation Architecture Restructuring**:
  Reorganized documentation into dedicated `readme/zh/` and `readme/en/` directories with integrated version changelogs and automatic multilingual README aggregation.

### v0.1.33

- Code architecture optimization and performance fine-tuning.

### v0.1.32

- Refined stateful `Encoder` documentation and buffer reuse API ergonomics.

### v0.1.31

- Added optional `capi` feature with bilingual C-API documentation and header files for cross-language (C/C++/Python) integration.

### v0.1.30

- Clarified standard ALP baseline vs custom compression ratio optimizations; enhanced floating-point precision stability.
