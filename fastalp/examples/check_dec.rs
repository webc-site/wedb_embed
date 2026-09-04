use std::{fs, path::PathBuf, time::Instant};

use fastalp::{Encoder, decompress_into};

fn main() {
  let alp_dir = PathBuf::from("/Users/z/git/db/ALP");
  let content = fs::read_to_string(alp_dir.join("data/samples/neon_air_pressure.csv")).unwrap();
  let data: Vec<f64> = content
    .lines()
    .filter_map(|l| l.trim().parse().ok())
    .collect();

  let mut enc = Encoder::<f64>::with_capacity(1024);
  let mut comp_buf = Vec::new();
  enc.compress_into(&data, &mut comp_buf);

  println!("Comp buf len: {}", comp_buf.len());
  let mut dec_buf = Vec::with_capacity(1024);

  // Measure decompression
  let iters = 10000;
  let start = Instant::now();
  for _ in 0..iters {
    dec_buf.clear();
    let _ = decompress_into::<f64>(&comp_buf, &mut dec_buf);
  }
  let dur = start.elapsed();
  let gb_s = (data.len() * 8 * iters) as f64 / (dur.as_secs_f64() * 1e9);
  println!("Decomp speed: {:.2} GB/s", gb_s);
}
