use fastalp::{compress, compress_into, decompress, decompress_into};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_empty_and_single() -> aok::Result<()> {
  let empty: [f64; 0] = [];
  let compressed = compress(&empty);
  // 校验空序列紧凑截断：仅占用 3 字节 (MIN_HEADER_LEN)
  assert_eq!(compressed.len(), 3);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), 0);

  let single = [42.125f64];
  let compressed = compress(&single);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed, single);

  // 测试单精度浮点数
  let single_f32 = [42.125f32];
  let compressed = compress(&single_f32[..]);
  let decompressed: Vec<f32> = decompress(&compressed)?;
  assert_eq!(decompressed, single_f32);
  Ok(())
}

#[test]
fn test_header_and_trailer_elimination() -> aok::Result<()> {
  // 无异常序列：5 字节 Header + 8 字节 Base + 0 字节 payload (全等值 bit_width=0)
  let identical = [10.5f64; 16];
  let comp = compress(&identical);
  // HEADER_LEN (5) + BASE_SIZE (8) + packed_len (0) + 0 异常尾缀 = 13 字节
  assert_eq!(comp.len(), 13);
  let dec: Vec<f64> = decompress(&comp)?;
  assert_eq!(dec, identical);

  // f32 无异常全等序列：HEADER_LEN (5) + BASE_SIZE (4) + 0 = 9 字节
  let identical_f32 = [10.5f32; 16];
  let comp_f32 = compress(&identical_f32);
  assert_eq!(comp_f32.len(), 9);
  let dec_f32: Vec<f32> = decompress(&comp_f32)?;
  assert_eq!(dec_f32, identical_f32);
  Ok(())
}

#[test]
fn test_bitpack_u64_all_widths() -> aok::Result<()> {
  use fastalp::{bitpack_u64, bitunpack_u64};

  for bit_width in 1..=64 {
    let mask = if bit_width == 64 {
      u64::MAX
    } else {
      (1u64 << bit_width) - 1
    };

    for count in [0, 1, 2, 3, 4, 7, 8, 9, 15, 16, 17, 31, 32, 33, 65, 100] {
      let values: Vec<u64> = (0..count).map(|i| (i as u64 * 37 + 13) & mask).collect();
      let mut packed = Vec::new();
      bitpack_u64(&values, bit_width, &mut packed);

      let mut unpacked = Vec::new();
      bitunpack_u64(&packed, count, bit_width, &mut unpacked)?;
      assert_eq!(
        unpacked, values,
        "Mismatch at bit_width={bit_width}, count={count}"
      );
    }
  }
  Ok(())
}

#[test]
fn test_real_world_decimals_compression_ratio() -> aok::Result<()> {
  let mut data = Vec::with_capacity(1024);
  for i in 0..1024 {
    // 模拟真实十进制传感器温度 20.0 ~ 34.9 ℃ (例如从字符串或单次乘法生成)
    let val = (200 + (i % 150)) as f64 * 0.1;
    data.push(val);
  }

  let compressed = compress(&data);
  let decompressed: Vec<f64> = decompress(&compressed)?;

  assert_eq!(decompressed.len(), data.len());
  for (a, b) in decompressed.iter().zip(data.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }

  let raw_size = data.len() * 8; // 8192 B
  let comp_size = compressed.len();
  println!(
    "Real-world decimals: raw={} B, compressed={} B, ratio={:.2}x",
    raw_size,
    comp_size,
    raw_size as f64 / comp_size as f64
  );
  // 预期压缩比超过 6x (8192 B -> ~1040 B, 7.8x 压缩比)
  assert!(comp_size < raw_size / 6);
  Ok(())
}

#[test]
fn test_identical_floats_super_compression() -> aok::Result<()> {
  let data = vec![98.6f64; 1000];
  let mut compressed = Vec::new();
  compress_into(&data, &mut compressed);
  let mut decompressed: Vec<f64> = Vec::new();
  decompress_into(&compressed, &mut decompressed)?;

  assert_eq!(decompressed, data);
  println!(
    "Identical floats: raw={} B, compressed={} B, ratio={:.2}x",
    data.len() * 8,
    compressed.len(),
    (data.len() * 8) as f64 / compressed.len() as f64
  );
  assert!(compressed.len() < 30);
  Ok(())
}

#[test]
fn test_mixed_exceptions_and_special_values() -> aok::Result<()> {
  let mut data = vec![1.23, 4.56, 7.89, 10.11];
  data.push(f64::NAN);
  data.push(f64::INFINITY);
  data.push(f64::NEG_INFINITY);
  data.push(-0.0);
  data.push(1e30);
  data.push(-1e30);
  data.push(1e-25);

  let compressed = compress(&data);
  let decompressed: Vec<f64> = decompress(&compressed)?;

  assert_eq!(decompressed.len(), data.len());
  for (i, (&a, &b)) in decompressed.iter().zip(data.iter()).enumerate() {
    if a.is_nan() {
      assert!(b.is_nan(), "index {i} expected NaN");
    } else {
      assert_eq!(
        a.to_bits(),
        b.to_bits(),
        "index {i} bits mismatch: {a} vs {b}"
      );
    }
  }
  Ok(())
}

#[test]
fn test_f32_roundtrip() -> aok::Result<()> {
  let mut data = Vec::with_capacity(500);
  for i in 0..500 {
    data.push((100 + (i % 50)) as f32 * 0.25f32);
  }
  data.push(f32::NAN);
  data.push(f32::INFINITY);
  data.push(-0.0f32);

  let mut compressed = Vec::new();
  compress_into(&data, &mut compressed);
  let mut decompressed: Vec<f32> = Vec::new();
  decompress_into(&compressed, &mut decompressed)?;

  assert_eq!(decompressed.len(), data.len());
  for (i, (&a, &b)) in decompressed.iter().zip(data.iter()).enumerate() {
    if a.is_nan() {
      assert!(b.is_nan(), "index {i} expected NaN");
    } else {
      assert_eq!(a.to_bits(), b.to_bits(), "index {i} bits mismatch");
    }
  }
  Ok(())
}

#[test]
fn test_random_floats_stress() -> aok::Result<()> {
  fastrand::seed(12345);
  let mut data = Vec::with_capacity(2000);
  for _ in 0..2000 {
    let base = fastrand::i32(-1000..1000) as f64;
    let decimals = (fastrand::u32(0..1000) as f64) * 0.01;
    data.push(base + decimals);
  }

  let compressed = compress(&data);
  let decompressed: Vec<f64> = decompress(&compressed)?;

  assert_eq!(decompressed.len(), data.len());
  for (a, b) in decompressed.iter().zip(data.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }
  Ok(())
}

#[test]
fn test_negative_decimals_and_various_sizes() -> aok::Result<()> {
  for size in [1, 2, 3, 7, 15, 16, 31, 32, 63, 64, 127, 255, 1000] {
    let mut data = Vec::with_capacity(size);
    for i in 0..size {
      let val = -((i as f64) * 0.125 + 50.0);
      data.push(val);
    }
    let compressed = compress(&data);
    let decompressed: Vec<f64> = decompress(&compressed)?;
    assert_eq!(decompressed.len(), data.len());
    for (a, b) in decompressed.iter().zip(data.iter()) {
      assert_eq!(a.to_bits(), b.to_bits());
    }
  }
  Ok(())
}

#[test]
fn test_large_vector_stress() -> aok::Result<()> {
  fastrand::seed(999);
  let mut data = Vec::with_capacity(10000);
  for i in 0..10000 {
    let sign = if fastrand::bool() { 1.0 } else { -1.0 };
    let val = sign * ((i % 500) as f64 * 0.001);
    data.push(val);
  }

  let compressed = compress(&data);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (a, b) in decompressed.iter().zip(data.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }
  Ok(())
}

#[test]
fn test_negative_zero_preservation() -> aok::Result<()> {
  let data = [0.0f64, -0.0f64, 0.0f64, -0.0f64];
  let compressed = compress(&data);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), 4);
  for (a, b) in decompressed.iter().zip(data.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }

  let data_f32 = [0.0f32, -0.0f32, 0.0f32, -0.0f32];
  let compressed_f32 = compress(&data_f32);
  let decompressed_f32: Vec<f32> = decompress(&compressed_f32)?;
  assert_eq!(decompressed_f32.len(), 4);
  for (a, b) in decompressed_f32.iter().zip(data_f32.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }
  Ok(())
}

#[test]
fn test_raw_fallback_incompressible_data() -> aok::Result<()> {
  fastrand::seed(8888);
  let random_bits_data: Vec<f64> = (0..1024)
    .map(|_| f64::from_bits(fastrand::u64(..)))
    .collect();
  let compressed = compress(&random_bits_data);
  // 断言触发 RAW 模式：首字节为 TYPE_F64_RAW (3)
  assert_eq!(compressed[0], fastalp::TYPE_F64_RAW);
  assert_eq!(
    compressed.len(),
    fastalp::MIN_HEADER_LEN + random_bits_data.len() * 8
  );
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), random_bits_data.len());
  for (a, b) in decompressed.iter().zip(random_bits_data.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }

  let random_bits_f32: Vec<f32> = (0..1024)
    .map(|_| f32::from_bits(fastrand::u32(..)))
    .collect();
  let compressed_f32 = compress(&random_bits_f32);
  // 断言触发 RAW 模式：首字节为 TYPE_F32_RAW (4)
  assert_eq!(compressed_f32[0], fastalp::TYPE_F32_RAW);
  assert_eq!(
    compressed_f32.len(),
    fastalp::MIN_HEADER_LEN + random_bits_f32.len() * 4
  );
  let decompressed_f32: Vec<f32> = decompress(&compressed_f32)?;
  assert_eq!(decompressed_f32.len(), random_bits_f32.len());
  for (a, b) in decompressed_f32.iter().zip(random_bits_f32.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }
  Ok(())
}
