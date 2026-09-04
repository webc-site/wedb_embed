use fastalp::{
  CHUNK_SIZE, CHUNK_SIZE_1024, Error, compress, compress_into, count, decompress, decompress_into,
  decompress_into_raw, decompress_into_slice,
  header::{LEN_TAG_1024, LEN_TAG_U32, TYPE_F32_RAW, TYPE_F64_RAW, raw_header_len, read_count},
  max_compressed_size, read_header,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_empty_and_single() -> aok::Result<()> {
  let empty: [f64; 0] = [];
  let compressed = compress(&empty);
  // 校验空序列紧凑截断：仅占用 2 字节 (1B 描述符 + 1B count)
  assert_eq!(compressed.len(), 2);
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
  // count=16: 4 字节 Header (1B desc + 1B count + 2B params) + 8 字节 Base + 0 字节 payload = 12 字节
  let identical = [10.5f64; 16];
  let comp = compress(&identical);
  assert_eq!(comp.len(), 12);
  let dec: Vec<f64> = decompress(&comp)?;
  assert_eq!(dec, identical);

  // f32 无异常全等序列：4 字节 Header + 4 字节 Base + 0 = 8 字节
  let identical_f32 = [10.5f32; 16];
  let comp_f32 = compress(&identical_f32);
  assert_eq!(comp_f32.len(), 8);
  let dec_f32: Vec<f32> = decompress(&comp_f32)?;
  assert_eq!(dec_f32, identical_f32);
  Ok(())
}

#[test]
fn test_bitpack_u64_all_widths() -> aok::Result<()> {
  use fastalp::bitpack::{bitpack_u64, bitunpack_u64};

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
  // 断言触发 RAW 模式：首字节类型为 TYPE_F64_RAW (3)，长度档位为 1024 预设块
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.type_byte, TYPE_F64_RAW);
  assert_eq!(hdr.len_tag, LEN_TAG_1024);
  assert_eq!(
    compressed.len(),
    raw_header_len(1024) + random_bits_data.len() * 8
  );
  assert_eq!(raw_header_len(1024), 1);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), random_bits_data.len());
  for (a, b) in decompressed.iter().zip(random_bits_data.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }

  let random_bits_f32: Vec<f32> = (0..1024)
    .map(|_| f32::from_bits(fastrand::u32(..)))
    .collect();
  let compressed_f32 = compress(&random_bits_f32);
  let hdr_f32 = read_header(&compressed_f32)?;
  assert_eq!(hdr_f32.type_byte, TYPE_F32_RAW);
  assert_eq!(hdr_f32.len_tag, LEN_TAG_1024);
  assert_eq!(
    compressed_f32.len(),
    raw_header_len(1024) + random_bits_f32.len() * 4
  );
  assert_eq!(raw_header_len(1024), 1);
  let decompressed_f32: Vec<f32> = decompress(&compressed_f32)?;
  assert_eq!(decompressed_f32.len(), random_bits_f32.len());
  for (a, b) in decompressed_f32.iter().zip(random_bits_f32.iter()) {
    assert_eq!(a.to_bits(), b.to_bits());
  }
  Ok(())
}

#[test]
fn test_large_array_u32_roundtrip() -> aok::Result<()> {
  // 超过 65535 元素的大数组测试，验证 u32 长度标签与 u32 异常索引无损编解码
  let size = 70_000usize;
  let mut data: Vec<f64> = (0..size).map(|i| (i as f64) * 0.125).collect();
  // 注入偶发异常值
  data[100] = f64::NAN;
  data[66_000] = 99999999.12345;

  let compressed = compress(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.count, size);
  assert_eq!(hdr.len_tag, LEN_TAG_U32);

  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), size);
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    if orig.is_nan() {
      assert!(dec.is_nan(), "expected NaN at {i}");
    } else {
      assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at {i}");
    }
  }
  Ok(())
}

#[test]
fn test_large_array_f32_u32_roundtrip() -> aok::Result<()> {
  // 单精度 f32 超过 65535 元素的大数组测试，验证 u32 长度标签与 u32 异常索引无损编解码
  let size = 68_000usize;
  let mut data: Vec<f32> = (0..size).map(|i| (i as f32) * 0.25f32).collect();
  data[50] = f32::NAN;
  data[67_000] = 88888.5f32;

  let compressed = compress(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.count, size);
  assert_eq!(hdr.len_tag, LEN_TAG_U32);

  let decompressed: Vec<f32> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), size);
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    if orig.is_nan() {
      assert!(dec.is_nan(), "expected NaN at {i}");
    } else {
      assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at {i}");
    }
  }
  Ok(())
}

#[test]
fn test_large_array_delta_u32_roundtrip() -> aok::Result<()> {
  use fastalp::compress_delta;

  // 超过 65535 元素的强制 Delta 一阶差分测试，验证 1024 栈分批流式解包与 u32 异常无损恢复
  let size = 75_000usize;
  let mut data: Vec<f64> = (0..size)
    .map(|i| 100.0 + (i as f64) * 0.5 + ((i % 10) as f64) * 0.05)
    .collect();
  data[10] = f64::NAN;
  data[70_000] = 999999.99;

  let compressed = compress_delta(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.count, size);
  assert_eq!(hdr.len_tag, LEN_TAG_U32);

  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), size);
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    if orig.is_nan() {
      assert!(dec.is_nan(), "expected NaN at {i}");
    } else {
      assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at {i}");
    }
  }
  Ok(())
}

#[test]
fn test_stateful_encoder_roundtrip_and_invalidation() -> aok::Result<()> {
  use fastalp::Encoder;

  let mut encoder = Encoder::<f64>::new();
  assert!(encoder.cached_params.is_none());

  // 1. 第一个块：常规 2 位小数时间序列
  let block1: Vec<f64> = (0..1024).map(|i| (i as f64) * 0.25 + 10.0).collect();
  let mut c1 = Vec::new();
  encoder.compress_into(&block1, &mut c1);
  assert!(encoder.cached_params.is_some());
  let p1 = encoder.cached_params.unwrap();
  assert_eq!(p1.exp, 2);

  let d1: Vec<f64> = decompress(&c1)?;
  assert_eq!(d1.len(), block1.len());
  for (orig, dec) in block1.iter().zip(&d1) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  // 2. 第二个块：同类型数据，成功复用 cached_params
  let block2: Vec<f64> = (1024..2048).map(|i| (i as f64) * 0.25 + 10.0).collect();
  let mut c2 = Vec::new();
  encoder.compress_into(&block2, &mut c2);
  assert_eq!(encoder.cached_params, Some(p1));

  let d2: Vec<f64> = decompress(&c2)?;
  assert_eq!(d2.len(), block2.len());
  for (orig, dec) in block2.iter().zip(&d2) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  // 3. 第三个块：数据分布突变（6 位小数），但前 4 个元素碰巧为整数
  // 验证重新探测挽救机制：能自动识别缓存失效并成功以新参数压缩
  let mut block3: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0];
  for i in 4..1024 {
    block3.push(100.0 + (i as f64) * 0.000001);
  }
  let mut c3 = Vec::new();
  encoder.compress_into(&block3, &mut c3);
  assert!(encoder.cached_params.is_some());
  let p3 = encoder.cached_params.unwrap();
  assert_eq!(p3.exp, 6);

  let d3: Vec<f64> = decompress(&c3)?;
  assert_eq!(d3.len(), block3.len());
  for (orig, dec) in block3.iter().zip(&d3) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  // 4. 重置编码器缓存
  encoder.reset();
  assert!(encoder.cached_params.is_none());

  // 5. 不可压缩随机数据块：验证超过 128 异常保底为 RAW 且清空缓存参数
  let raw_block: Vec<f64> = (0..1024)
    .map(|i| f64::from_bits(0x3FF0000000000000u64 | (i as u64).wrapping_mul(0x123456789ABCDEF)))
    .collect();
  let mut c_raw = Vec::new();
  encoder.compress_into(&raw_block, &mut c_raw);
  assert!(
    encoder.cached_params.is_none(),
    "RAW 块不应保留有效缓存参数"
  );

  let d_raw: Vec<f64> = decompress(&c_raw)?;
  assert_eq!(d_raw.len(), raw_block.len());
  for (orig, dec) in raw_block.iter().zip(&d_raw) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  Ok(())
}

#[test]
fn test_stateful_encoder_capacity_and_delta() -> aok::Result<()> {
  use fastalp::Encoder;

  let mut encoder = Encoder::<f64>::with_capacity(2048);
  assert!(encoder.cached_params.is_none());

  // 1. 测试差分单调递增时序数据
  let data: Vec<f64> = (0..1024).map(|i| 1000.0 + (i as f64) * 0.1).collect();
  let mut compressed = Vec::new();
  encoder.compress_delta_into(&data, &mut compressed);
  assert!(encoder.cached_params.is_some());

  let restored: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored.len(), data.len());
  for (a, b) in data.iter().zip(&restored) {
    assert_eq!(a.to_bits(), b.to_bits());
  }

  // 2. 第二个块继续复用
  let data2: Vec<f64> = (1024..2048).map(|i| 1000.0 + (i as f64) * 0.1).collect();
  compressed.clear();
  encoder.compress_delta_into(&data2, &mut compressed);
  let restored2: Vec<f64> = decompress(&compressed)?;
  assert_eq!(restored2.len(), data2.len());
  for (a, b) in data2.iter().zip(&restored2) {
    assert_eq!(a.to_bits(), b.to_bits());
  }

  Ok(())
}

#[cfg(feature = "capi")]
#[test]
fn test_capi_roundtrip() {
  use fastalp::{
    fastalp_compress_f32, fastalp_compress_f64, fastalp_decompress_f32, fastalp_decompress_f64,
  };

  let data_f64: Vec<f64> = (0..1024).map(|i| (i as f64) * 0.125).collect();
  let mut comp_buf = vec![0u8; 65536];
  let written_f64 = unsafe {
    fastalp_compress_f64(
      data_f64.as_ptr(),
      data_f64.len(),
      comp_buf.as_mut_ptr(),
      comp_buf.len(),
    )
  };
  assert!(written_f64 > 0);

  let mut dec_f64 = vec![0.0f64; 1024];
  let dec_count_f64 = unsafe {
    fastalp_decompress_f64(
      comp_buf.as_ptr(),
      written_f64,
      dec_f64.as_mut_ptr(),
      dec_f64.len(),
    )
  };
  assert_eq!(dec_count_f64, 1024);
  for i in 0..1024 {
    assert_eq!(data_f64[i].to_bits(), dec_f64[i].to_bits());
  }

  let data_f32: Vec<f32> = (0..1024).map(|i| (i as f32) * 0.25).collect();
  let written_f32 = unsafe {
    fastalp_compress_f32(
      data_f32.as_ptr(),
      data_f32.len(),
      comp_buf.as_mut_ptr(),
      comp_buf.len(),
    )
  };
  assert!(written_f32 > 0);

  let mut dec_f32 = vec![0.0f32; 1024];
  let dec_count_f32 = unsafe {
    fastalp_decompress_f32(
      comp_buf.as_ptr(),
      written_f32,
      dec_f32.as_mut_ptr(),
      dec_f32.len(),
    )
  };
  assert_eq!(dec_count_f32, 1024);
  for i in 0..1024 {
    assert_eq!(data_f32[i].to_bits(), dec_f32[i].to_bits());
  }

  // Test max compressed size helpers
  let max_f64 = fastalp::fastalp_max_compressed_size_f64(1024);
  assert!(max_f64 >= 1024 * 8);
  let max_f32 = fastalp::fastalp_max_compressed_size_f32(1024);
  assert!(max_f32 >= 1024 * 4);

  // Test handle-based stateful encoders
  let enc_f64 = fastalp::fastalp_encoder_f64_new();
  assert!(!enc_f64.is_null());
  let h_written_f64 = unsafe {
    fastalp::fastalp_encoder_f64_compress(
      enc_f64,
      data_f64.as_ptr(),
      data_f64.len(),
      comp_buf.as_mut_ptr(),
      comp_buf.len(),
    )
  };
  assert!(h_written_f64 > 0);
  unsafe {
    fastalp::fastalp_encoder_f64_reset(enc_f64);
    fastalp::fastalp_encoder_f64_free(enc_f64);
  }

  let enc_f32 = fastalp::fastalp_encoder_f32_new();
  assert!(!enc_f32.is_null());
  let h_written_f32 = unsafe {
    fastalp::fastalp_encoder_f32_compress(
      enc_f32,
      data_f32.as_ptr(),
      data_f32.len(),
      comp_buf.as_mut_ptr(),
      comp_buf.len(),
    )
  };
  assert!(h_written_f32 > 0);
  unsafe {
    fastalp::fastalp_encoder_f32_reset(enc_f32);
    fastalp::fastalp_encoder_f32_free(enc_f32);
  }
}

#[test]
fn test_exposed_utilities() -> aok::Result<()> {
  assert_eq!(CHUNK_SIZE, 1024);
  assert_eq!(CHUNK_SIZE_1024, 1024);

  // 1. 测试 count() / read_count() 函数在不同大小数据上的 O(1) 解析准确性与上限推导
  let sizes = [0usize, 1, 10, 255, 256, 1024, 2048, 70_000];
  for &sz in &sizes {
    let data: Vec<f64> = (0..sz).map(|i| (i as f64) * 0.125).collect();
    let comp = compress(&data);

    let counted = count(&comp)?;
    assert_eq!(counted, sz, "count mismatch for size {sz}");

    let counted_hdr = read_count(&comp)?;
    assert_eq!(counted_hdr, sz, "read_count mismatch for size {sz}");

    #[cfg(feature = "capi")]
    unsafe {
      let c_count = fastalp::fastalp_count(comp.as_ptr(), comp.len());
      assert_eq!(c_count, sz, "capi fastalp_count mismatch for size {sz}");
    }

    let max_sz_f64 = max_compressed_size::<f64>(sz);
    assert!(
      comp.len() <= max_sz_f64,
      "compressed len {} exceeds max_compressed_size {}",
      comp.len(),
      max_sz_f64
    );

    // 2. 测试 decompress_into_slice 切片直接解压（零堆分配）
    let mut slice_dst = vec![0.0f64; sz];
    let written = decompress_into_slice(&comp, &mut slice_dst)?;
    assert_eq!(written, sz);
    assert_eq!(&slice_dst[..], &data[..]);

    // 3. 测试 decompress_into_raw 底层裸指针解压
    if sz > 0 {
      let mut raw_dst = vec![0.0f64; sz];
      unsafe {
        let raw_written = decompress_into_raw(&comp, raw_dst.as_mut_ptr(), sz)?;
        assert_eq!(raw_written, sz);
      }
      assert_eq!(&raw_dst[..], &data[..]);
    }

    // 4. 测试单精度 f32
    let data_f32: Vec<f32> = (0..sz).map(|i| (i as f32) * 0.25).collect();
    let comp_f32 = compress(&data_f32);
    let counted_f32 = count(&comp_f32)?;
    assert_eq!(counted_f32, sz, "count mismatch for f32 size {sz}");

    let max_sz_f32 = max_compressed_size::<f32>(sz);
    assert!(
      comp_f32.len() <= max_sz_f32,
      "compressed f32 len {} exceeds max_compressed_size {}",
      comp_f32.len(),
      max_sz_f32
    );

    let mut slice_f32 = vec![0.0f32; sz];
    let written_f32 = decompress_into_slice(&comp_f32, &mut slice_f32)?;
    assert_eq!(written_f32, sz);
    assert_eq!(&slice_f32[..], &data_f32[..]);
  }

  // 5. 校验异常与边界情况
  // 5.1 空切片提取 count 报 UnexpectedEof
  assert!(matches!(count(&[]), Err(Error::UnexpectedEof { .. })));

  // 5.2 目标切片容量不足时 decompress_into_slice 报 BufferTooSmall
  let sample_data = vec![1.0f64, 2.0, 3.0, 4.0];
  let comp_sample = compress(&sample_data);
  let mut small_buf = [0.0f64; 2];
  assert!(matches!(
    decompress_into_slice(&comp_sample, &mut small_buf),
    Err(Error::BufferTooSmall {
      needed: 4,
      available: 2
    })
  ));

  // 5.3 非法类型标识字节报 InvalidHeader
  let mut invalid_comp = comp_sample.clone();
  invalid_comp[0] &= 0xF0; // type_byte = 0
  assert!(matches!(count(&invalid_comp), Err(Error::InvalidHeader)));
  assert!(matches!(
    read_header(&invalid_comp),
    Err(Error::InvalidHeader)
  ));

  let mut invalid_comp2 = comp_sample.clone();
  invalid_comp2[0] = (invalid_comp2[0] & 0xF0) | 15; // type_byte = 15
  assert!(matches!(count(&invalid_comp2), Err(Error::InvalidHeader)));
  assert!(matches!(
    read_header(&invalid_comp2),
    Err(Error::InvalidHeader)
  ));

  Ok(())
}

#[test]
fn test_dictionary_mode_roundtrip() -> fastalp::Result<()> {
  use fastalp::{
    compress, decompress,
    header::{TYPE_F64_DICT, read_header},
  };

  // 1. 20 个离散值随机分布（模拟 scene_macro），验证字典压缩模式与完全无损解压
  let dict_vals = [
    1250.0f64, 1265.5, 1281.0, 1296.5, 1312.0, 1327.5, 1343.0, 1358.5, 1374.0, 1389.5, 1405.0,
    1420.5, 1436.0, 1451.5, 1467.0, 1482.5, 1498.0, 1513.5, 1529.0, 1544.5,
  ];
  let mut data = Vec::with_capacity(1024);
  for i in 0..1024 {
    data.push(dict_vals[(i * 7 + 13) % dict_vals.len()]);
  }

  let compressed = compress(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(
    hdr.type_byte, TYPE_F64_DICT,
    "低基数离散数据应优先选用字典压缩"
  );
  assert!(
    compressed.len() <= 850,
    "1024 个 20 离散浮点数字典压缩大小应 <= 850B，实际：{}B",
    compressed.len()
  );

  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (orig, dec) in data.iter().zip(&decompressed) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  // 2. 两个交替浮点数（0.0 与 1.0），无连续重复，字典压缩位宽应为 1
  let mut alt_data = Vec::with_capacity(1024);
  for i in 0..1024 {
    alt_data.push(if i % 2 == 0 { 0.0f64 } else { 1.0f64 });
  }
  let comp_alt = compress(&alt_data);
  let hdr_alt = read_header(&comp_alt)?;
  assert_eq!(hdr_alt.type_byte, TYPE_F64_DICT);
  // 1 字节头部 + 2 字节元数据 + 16 字节字典 + 1024/8 = 128 字节位打包 = 147 字节
  assert!(comp_alt.len() <= 160);
  let dec_alt: Vec<f64> = decompress(&comp_alt)?;
  assert_eq!(dec_alt.len(), alt_data.len());
  for (orig, dec) in alt_data.iter().zip(&dec_alt) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  // 3. 所有元素全部相同的单值字典压缩
  let single_data = vec![42.5f64; 1024];
  let comp_single = compress(&single_data);
  let hdr_single = read_header(&comp_single)?;
  assert_eq!(hdr_single.type_byte, TYPE_F64_DICT);
  assert!(comp_single.len() <= 15);
  let dec_single: Vec<f64> = decompress(&comp_single)?;
  assert_eq!(dec_single, single_data);

  Ok(())
}

#[test]
fn test_dictionary_mode_f32_roundtrip() -> fastalp::Result<()> {
  use fastalp::{
    compress, decompress,
    header::{TYPE_F32_DICT, read_header},
  };

  let dict_vals = [10.5f32, -20.25, 0.0, -0.0, 100.125, 999.0];
  let mut data = Vec::with_capacity(1024);
  for i in 0..1024 {
    data.push(dict_vals[i % dict_vals.len()]);
  }

  let compressed = compress(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.type_byte, TYPE_F32_DICT);

  let decompressed: Vec<f32> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (orig, dec) in data.iter().zip(&decompressed) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  Ok(())
}

#[test]
fn test_rd_mode_roundtrip() -> fastalp::Result<()> {
  use fastalp::{
    compress, decompress,
    header::{TYPE_F64_RD, read_header},
  };

  // 构造真实双精度高熵尾数数据（指数集中在极少离散值，低位尾数连续高熵分布，无法十进制化）
  // 此时标准 ALP 回退，ALP-RD 高低位解耦可自动触发并高吞吐无损编解码
  let mut data = Vec::with_capacity(1024);
  for i in 0..1024 {
    let exp_bias = ((i % 3) as u64 + 1023) << 52;
    let mantissa = (i as u64)
      .wrapping_mul(6364136223846793005)
      .wrapping_add(1442695040888963407)
      & 0x000F_FFFF_FFFF_FFFF;
    data.push(f64::from_bits(exp_bias | mantissa));
  }

  let compressed = compress(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.type_byte, TYPE_F64_RD, "高位聚集浮点数据应触发 ALP-RD 压缩");
  assert!(
    compressed.len() < data.len() * 8,
    "ALP-RD 压缩体积应小于未压缩原始体积"
  );

  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (orig, dec) in data.iter().zip(&decompressed) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  Ok(())
}

#[test]
fn test_rd_mode_f32_roundtrip() -> fastalp::Result<()> {
  use fastalp::{
    compress, decompress,
    header::{TYPE_F32_RD, read_header},
  };

  let mut data = Vec::with_capacity(1024);
  for i in 0..1024 {
    let exp_bias = ((i % 2) as u32 + 127) << 23;
    let mantissa = (i as u32)
      .wrapping_mul(1103515245)
      .wrapping_add(12345)
      & 0x007F_FFFF;
    data.push(f32::from_bits(exp_bias | mantissa));
  }

  let compressed = compress(&data);
  let hdr = read_header(&compressed)?;
  assert_eq!(hdr.type_byte, TYPE_F32_RD, "f32 高位聚集浮点数据应触发 ALP-RD 压缩");
  assert!(
    compressed.len() < data.len() * 4,
    "f32 ALP-RD 压缩体积应小于原始体积"
  );

  let decompressed: Vec<f32> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (orig, dec) in data.iter().zip(&decompressed) {
    assert_eq!(orig.to_bits(), dec.to_bits());
  }

  Ok(())
}
