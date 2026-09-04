## C 兼容接口与跨语言集成

`fastalp` 提供默认不启用的可选 C 兼容接口（FFI），便于集成到 C、C++、Python、Go 等多语言运行环境中。<br>
在未开启 `capi` 特性时，纯 Rust 构建不引入任何额外导出符号或运行时开销。

在 `Cargo.toml` 中按需启用特性：

```toml
[dependencies]
fastalp = { version = "0.1.38", features = ["capi"] }
```

构建独立的静态库（`libfastalp.a`）或动态库（`libfastalp.so` / `libfastalp.dylib`）：

```bash
cargo build --release --features capi
```

### 缓冲区容量预估与元素提取

调用方可预先计算最差情况下的缓冲区需求或提取压缩块元素数，确保不发生容量不足异常：

- `fastalp_count(src, src_len)`：从压缩字节流自描述头部以 O(1) 复杂度快速解析出包含的浮点元素总数，便于调用方按需预分配解压目标缓冲区。<br>
- `fastalp_max_compressed_size_f64(len)`：计算 `len` 个 `f64` 浮点数所需的最大目标缓冲区字节容量。<br>
- `fastalp_max_compressed_size_f32(len)`：计算 `len` 个 `f32` 浮点数所需的最大目标缓冲区字节容量。

### 线程局部流式接口

针对高吞吐时序场景提供的无状态流式接口，内部复用线程局部工作缓冲区，避免每次调用的堆内存分配：

- `fastalp_compress_f64(src, len, dst, dst_cap)`：压缩 `f64` 浮点数组（包含动态模型参数采样探测）。<br>
- `fastalp_compress_cached_f64(src, len, dst, dst_cap)`：复用已缓存模型参数执行纯编码内核，跳过采样开销。<br>
- `fastalp_decompress_f64(src, src_len, dst, dst_cap)`：解压字节流至 `f64` 浮点数组。<br>
- `fastalp_reset_encoder_f64()`：重置当前线程局部的 `f64` 编码器模型参数缓存。<br>
- 单精度浮点对应接口：`fastalp_compress_f32`、`fastalp_compress_cached_f32`、`fastalp_decompress_f32` 以及 `fastalp_reset_encoder_f32`。

### 独立实例句柄接口

适用于多线程工作池、按列维护独立编码状态的复杂系统集成：

- `fastalp_encoder_f64_new()`：在堆上创建新的 `f64` 状态化独立编码器实例。<br>
- `fastalp_encoder_f64_free(enc)`：释放由 `fastalp_encoder_f64_new` 分配的编码器实例。<br>
- `fastalp_encoder_f64_reset(enc)`：重置指定编码器句柄中的已缓存模型参数。<br>
- `fastalp_encoder_f64_compress(enc, src, len, dst, dst_cap)`：使用指定编码器句柄压缩 `f64` 浮点数组。<br>
- 单精度浮点对应句柄接口：`FastAlpEncoderF32`、`fastalp_encoder_f32_new`、`fastalp_encoder_f32_free`、`fastalp_encoder_f32_reset` 以及 `fastalp_encoder_f32_compress`。
