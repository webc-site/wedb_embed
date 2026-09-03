use fastalp::{
  Result, TYPE_F32_DEC_DELTA, TYPE_F32_DELTA, TYPE_F64_DEC, TYPE_F64_DEC_DELTA, TYPE_F64_DELTA,
  compress, compress_delta, decompress,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_linear_ramp_constant_delta() -> Result<()> {
  // 线性等差序列：常数差分，bit_width = 0
  let data: Vec<f64> = (0..1000).map(|i| 10.0 + (i as f64) * 0.25).collect();
  let compressed = compress(&data);
  // 必须自动识别并采用 Delta 模式
  let hdr = fastalp::read_header(&compressed)?;
  assert_eq!(hdr.type_byte, TYPE_F64_DELTA);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at index {i}");
  }
  // 1000 个浮点数仅需头部与基准值（< 35 字节），压缩比 > 220x
  assert!(
    compressed.len() < 35,
    "Compressed len = {}",
    compressed.len()
  );
  Ok(())
}

#[test]
fn test_smooth_time_series_weather_like() -> Result<()> {
  // 模拟气象温度序列（小范围连续平滑波动）
  let mut temp: f64 = 20.0;
  let mut data = Vec::with_capacity(500);
  for i in 0..500 {
    temp += ((i % 3) as f64) * 0.25;
    data.push(temp);
  }

  let compressed = compress(&data);
  let hdr = fastalp::read_header(&compressed)?;
  assert_eq!(hdr.type_byte, TYPE_F64_DELTA);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at index {i}");
  }
  // 验证压缩体积低于 1.5 字节/点
  let bytes_per_point = compressed.len() as f64 / data.len() as f64;
  assert!(bytes_per_point < 1.5, "bpp = {bytes_per_point}");
  Ok(())
}

#[test]
fn test_delta_with_exceptions_and_outliers() -> Result<()> {
  // 带有偶发异常离群值和特殊浮点数的平滑序列
  let mut data: Vec<f64> = (0..300).map(|i| (300 + i) as f64 / 20.0).collect();
  data[50] = f64::NAN;
  data[120] = f64::INFINITY;
  data[180] = -0.0;
  data[250] = 123456789.987654;

  let compressed = compress_delta(&data);
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());

  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    if orig.is_nan() {
      assert!(dec.is_nan(), "expected NaN at {i}");
    } else {
      assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at index {i}");
    }
  }
  Ok(())
}

#[test]
fn test_f32_delta_roundtrip() -> Result<()> {
  let data: Vec<f32> = (0..400).map(|i| (20 + i) as f32 / 10.0f32).collect();
  let compressed = compress_delta(&data);
  let hdr = fastalp::read_header(&compressed)?;
  assert!(
    hdr.type_byte == TYPE_F32_DELTA || hdr.type_byte == TYPE_F32_DEC_DELTA,
    "expected delta format but got {}",
    hdr.type_byte
  );
  let decompressed: Vec<f32> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at index {i}");
  }
  Ok(())
}

#[test]
fn test_decimal_division_mode_exact_match() -> Result<()> {
  // 模拟从文本解析的物理指标（如 123.456, 123.457 ...），在乘法浮点下无法精确匹配，但除法 100% 精确
  let data: Vec<f64> = (0..100).map(|i| (123450 + i) as f64 / 1000.0).collect();
  let compressed = compress(&data);
  let hdr = fastalp::read_header(&compressed)?;
  assert!(
    hdr.type_byte == TYPE_F64_DEC || hdr.type_byte == TYPE_F64_DEC_DELTA,
    "expected decimal division format, got {}",
    hdr.type_byte
  );
  let decompressed: Vec<f64> = decompress(&compressed)?;
  assert_eq!(decompressed.len(), data.len());
  for (i, (&orig, &dec)) in data.iter().zip(&decompressed).enumerate() {
    assert_eq!(orig.to_bits(), dec.to_bits(), "mismatch at index {i}");
  }
  Ok(())
}
