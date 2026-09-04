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

### 零堆分配切片解压与 O(1) 元素计数

针对数据库执行器、嵌入式环境或预分配内存池等极致低延迟场景，`fastalp` 提供 O(1) 紧凑头部元素计数与切片原地解压接口，全程零堆内存分配：

```rust
use fastalp::{
  compress, count, decompress_into_slice, max_compressed_size, Result,
};

fn main() -> Result<()> {
  let sensor_data = [20.5, 20.6, 20.8, 21.0, 20.9, 21.2];
  let compressed = compress(&sensor_data);

  // 1. O(1) 零堆分配快速提取压缩块中的元素总数
  let num_items = count(&compressed)?;
  assert_eq!(num_items, 6);

  // 2. 预估最坏情况下（含保底回退）所需最大压缩缓冲区大小，防止越界
  let max_cap = max_compressed_size::<f64>(num_items);
  assert!(compressed.len() <= max_cap);

  // 3. 解压至栈数组或既有切片，实现真正的零堆内存分配与零拷贝
  let mut dst = [0.0f64; 6];
  let written = decompress_into_slice(&compressed, &mut dst)?;
  assert_eq!(written, 6);
  assert_eq!(&dst[..], &sensor_data[..]);

  Ok(())
}
```

### 状态化编码与参数缓存

针对连续数据块流式压缩场景，使用 `Encoder` 缓存采样参数并复用内部工作内存，消除重复采样开销：

```rust
use fastalp::{decompress, Encoder, Result};

fn main() -> Result<()> {
  let mut encoder = Encoder::<f64>::with_capacity(1024);

  let chunk1: Vec<f64> = (0..1024).map(|i| 25.0 + (i as f64) * 0.25).collect();
  let chunk2: Vec<f64> = (1024..2048).map(|i| 25.0 + (i as f64) * 0.25).collect();

  let mut compressed = Vec::new();

  // 第一个块：采样探测最优参数并缓存
  encoder.compress_into(&chunk1, &mut compressed);

  // 第二个块：命中参数缓存，跳过全量采样，吞吐大幅提升
  compressed.clear();
  encoder.compress_into(&chunk2, &mut compressed);

  let restored: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored, chunk2);

  // 切换不同数据流时重置缓存
  encoder.reset();
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

### 高性能工程技巧与最佳实践

#### 连续时序流启用参数缓存
在时序数据库或流式管道中，同一指标列（如温度传感器、订单簿成交价）的量纲与精度往往随时间保持高度平稳。<br>
直接使用 `compress` 每次都会执行 32 点轻量采样。而通过复用 `Encoder` 实例，连续数据块将命中已缓存的 `(exp, fac)` 最优参数，直接执行纯向量化编码内核，吞吐可提升至 **15~24+ GB/s**：

```rust
use fastalp::Encoder;

// 推荐为每个时间序列或写入通道保持一个 Encoder 实例
let mut encoder = Encoder::<f64>::with_capacity(1024);
let mut buf = Vec::with_capacity(1024 * 8);

for chunk in incoming_stream {
  buf.clear();
  // 跨块复用模型参数，吞吐达 15~24+ GB/s
  encoder.compress_into(&chunk, &mut buf);
  write_to_storage(&buf);
}
```

#### 就地复用缓冲区消除堆分配与内存抖动
高吞吐场景下频繁分配和丢弃 `Vec<u8>` 会导致内存碎片与 CPU 分配器锁争用。使用 `_into` 系列接口直接就地写入持久化缓冲区：

```rust
use fastalp::{compress_into, decompress_into};

let mut comp_buf = Vec::with_capacity(8192);
let mut decomp_buf = Vec::with_capacity(1024);

// 循环内零堆内存分配
for batch in batches {
  comp_buf.clear();
  compress_into(&batch, &mut comp_buf);

  decomp_buf.clear();
  decompress_into(&comp_buf, &mut decomp_buf)?;
}
```

#### 极低熵与单调波形自适应增益
- **常数流与设备心跳**：当遇到设备断线、待机或心跳常数时，`fastalp` 入口仅需 1 个 CPU 时钟周期识别全等流，1024 元素以 11 字节高速输出（压缩比达 **744x**）。
- **线性升降波形与步进计数**：针对工业 PID 调节、水文流量与连续计数器，`fastalp` 自动激活 Delta 一阶差分编码，动态消除波形大跨度基准，压缩比突破 **430x+**。
