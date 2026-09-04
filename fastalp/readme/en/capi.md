## C-Compatible API & Cross-Language Integration

`fastalp` provides an optional, disabled-by-default C-compatible FFI layer for integration into C, C++, Python, Go, and other language runtimes.<br>
When the `capi` feature is not enabled, standard Rust builds incur zero exported symbol overhead.

Enable the feature in `Cargo.toml`:

```toml
[dependencies]
fastalp = { version = "0.1.36", features = ["capi"] }
```

Build standalone static libraries (`libfastalp.a`) or shared libraries (`libfastalp.so` / `libfastalp.dylib`):

```bash
cargo build --release --features capi
```

### Buffer Capacity Estimation

Callers can calculate worst-case buffer bounds:

- `fastalp_max_compressed_size_f64(len)`: Computes maximum compressed byte bound for `len` `f64` floats.<br>
- `fastalp_max_compressed_size_f32(len)`: Computes maximum compressed byte bound for `len` `f32` floats.

### Thread-Local Streaming Interface

Stateless streaming functions reusing thread-local buffers to eliminate per-call allocation overhead:

- `fastalp_compress_f64(src, len, dst, dst_cap)`: Compresses an `f64` array with full parameter exploration.<br>
- `fastalp_compress_cached_f64(src, len, dst, dst_cap)`: Reuses cached parameters, bypassing the sampling phase.<br>
- `fastalp_decompress_f64(src, src_len, dst, dst_cap)`: Decompresses bytes into an `f64` target buffer.<br>
- `fastalp_reset_encoder_f64()`: Clears cached parameters in the current thread-local `f64` encoder.<br>
- Single-precision equivalents: `fastalp_compress_f32`, `fastalp_compress_cached_f32`, `fastalp_decompress_f32`, and `fastalp_reset_encoder_f32`.

### Explicit Instance Handle Interface

Designed for worker-pool architectures and per-column isolated states:

- `fastalp_encoder_f64_new()`: Allocates a heap-backed stateful `f64` encoder instance.<br>
- `fastalp_encoder_f64_free(enc)`: Frees the specified encoder instance.<br>
- `fastalp_encoder_f64_reset(enc)`: Clears cached model parameters in the handle.<br>
- `fastalp_encoder_f64_compress(enc, src, len, dst, dst_cap)`: Compresses data using the specified encoder handle.<br>
- Single-precision equivalents: `FastAlpEncoderF32`, `fastalp_encoder_f32_new`, `fastalp_encoder_f32_free`, `fastalp_encoder_f32_reset`, and `fastalp_encoder_f32_compress`.
