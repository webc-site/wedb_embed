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

### Stateful Encoder & Parameter Caching

For streaming time-series pipelines, use `Encoder` to cache model parameters across consecutive chunks and reuse buffers:

```rust
use fastalp::{decompress, Encoder, Result};

fn main() -> Result<()> {
  let mut encoder = Encoder::<f64>::with_capacity(1024);

  let chunk1: Vec<f64> = (0..1024).map(|i| 25.0 + (i as f64) * 0.25).collect();
  let chunk2: Vec<f64> = (1024..2048).map(|i| 25.0 + (i as f64) * 0.25).collect();

  let mut compressed = Vec::new();

  // First chunk: detects and caches optimal parameters
  encoder.compress_into(&chunk1, &mut compressed);

  // Second chunk: cache hit, skips full parameter search for ultra-high throughput
  compressed.clear();
  encoder.compress_into(&chunk2, &mut compressed);

  let restored: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored, chunk2);

  // Reset when switching to a different data stream
  encoder.reset();
  Ok(())
}
```

### Single-Precision Floating-Point Processing

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

### High-Performance Engineering Tips & Best Practices

#### Enable Parameter Caching for Streaming Pipelines
In time-series databases and metrics ingestion engines, the physical scale and precision of consecutive blocks on the same metric (e.g., temperature sensor, trade prices) remain highly uniform.<br>
While `compress` performs a 32-sample exploration on every invocation, reusing a stateful `Encoder` instance hits cached model parameters across subsequent blocks, skipping exploration entirely and elevating throughput to **15~24+ GB/s**:

```rust
use fastalp::Encoder;

// Maintain an Encoder per metric column or ingestion stream
let mut encoder = Encoder::<f64>::with_capacity(1024);
let mut buf = Vec::with_capacity(1024 * 8);

for chunk in incoming_stream {
  buf.clear();
  // Hits parameter cache, executing pure kernel at 15~24+ GB/s
  encoder.compress_into(&chunk, &mut buf);
  write_to_storage(&buf);
}
```

#### In-Place Buffer Reuse to Eliminate Allocation Jitter
Frequent allocations and deallocations in hot loops cause heap fragmentation and lock contention. Use `_into` function variants to write directly into long-lived memory buffers:

```rust
use fastalp::{compress_into, decompress_into};

let mut comp_buf = Vec::with_capacity(8192);
let mut decomp_buf = Vec::with_capacity(1024);

// Zero heap allocations inside the loop
for batch in batches {
  comp_buf.clear();
  compress_into(&batch, &mut comp_buf);

  decomp_buf.clear();
  decompress_into(&comp_buf, &mut decomp_buf)?;
}
```

#### Low-Entropy and Monotonic Waveform Acceleration
- **Constant Streams & Heartbeats**: On standby sensors or heartbeat streams, `fastalp` verifies equality in 1 CPU cycle, encoding 1024 items into 11 bytes (**744x ratio**).
- **Linear Ramps & Physical Steps**: For monotonic waveforms (industrial PID, hydrological levels), `fastalp` automatically engages first-order Delta difference encoding to eliminate large span offsets, achieving **430x+** compression.
