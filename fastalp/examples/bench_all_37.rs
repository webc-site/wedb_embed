use std::{
  env, fs,
  path::{Path, PathBuf},
  time::{Duration, Instant},
};

use fastalp::{Encoder, compress_into, decompress_into};

fn load_csv(path: &Path) -> Vec<f64> {
  let content = fs::read_to_string(path).expect("Failed to read CSV");
  content
    .lines()
    .filter_map(|line| {
      let trimmed = line.trim();
      if trimmed.is_empty() {
        None
      } else {
        trimmed.parse::<f64>().ok()
      }
    })
    .collect()
}

fn main() {
  let alp_dir = env::var("ALP_DIR")
    .map(PathBuf::from)
    .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ALP"));
  let samples_dir = alp_dir.join("data/samples");
  let mut entries: Vec<_> = fs::read_dir(&samples_dir)
    .expect("samples dir not found")
    .filter_map(|r| r.ok())
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "csv"))
    .collect();
  entries.sort_by_key(|a| a.file_name());

  println!(
    "Running fastalp benchmark across all {} datasets (fair zero-alloc pipeline)...",
    entries.len()
  );

  let mut datasets_json = Vec::new();
  let mut total_raw_bytes = 0;
  let mut total_comp_bytes = 0;
  let mut sum_enc = 0.0;
  let mut sum_enc_kern = 0.0;
  let mut sum_dec = 0.0;

  let mut comp_buf = Vec::with_capacity(65536);
  let mut dec_buf = Vec::with_capacity(1024);
  let mut encoder = Encoder::<f64>::with_capacity(1024);

  for entry in &entries {
    let path = entry.path();
    let name = path.file_stem().unwrap().to_string_lossy().to_string();
    let data = load_csv(&path);
    if data.is_empty() {
      continue;
    }

    let raw_bytes = data.len() * 8;
    total_raw_bytes += raw_bytes;

    // Warmup
    encoder.reset();
    for _ in 0..100 {
      comp_buf.clear();
      compress_into(&data, &mut comp_buf);
      dec_buf.clear();
      let _ = decompress_into::<f64>(&comp_buf, &mut dec_buf);
    }

    // Measure End-to-End Compression with Dynamic Sampling (min of 3 rounds of 1000 iters)
    let comp_iters = 1000;
    let mut best_enc_dur = Duration::MAX;
    for _ in 0..3 {
      let start_enc = Instant::now();
      for _ in 0..comp_iters {
        comp_buf.clear();
        compress_into(&data, &mut comp_buf);
      }
      best_enc_dur = best_enc_dur.min(start_enc.elapsed());
    }
    let enc_gb_s = (raw_bytes as f64 * comp_iters as f64) / (best_enc_dur.as_secs_f64() * 1e9);

    // Measure Pure Encoding Kernel Without Sampling (Reusing Cached Parameters, min of 3 rounds of 1000 iters)
    encoder.reset();
    comp_buf.clear();
    encoder.compress_into(&data, &mut comp_buf); // First pass establishes cached parameters
    let mut best_enc_kern_dur = Duration::MAX;
    for _ in 0..3 {
      let start_enc_kern = Instant::now();
      for _ in 0..comp_iters {
        comp_buf.clear();
        encoder.compress_into(&data, &mut comp_buf);
      }
      best_enc_kern_dur = best_enc_kern_dur.min(start_enc_kern.elapsed());
    }
    let enc_kernel_gb_s =
      (raw_bytes as f64 * comp_iters as f64) / (best_enc_kern_dur.as_secs_f64() * 1e9);

    // Measure Decompression (min of 3 rounds of 1000 iters - exact match with C++ ALP benchmark)
    let dec_iters = 1000;
    let mut best_dec_dur = Duration::MAX;
    for _ in 0..3 {
      let start_dec = Instant::now();
      for _ in 0..dec_iters {
        dec_buf.clear();
        let _ = decompress_into::<f64>(&comp_buf, &mut dec_buf);
      }
      best_dec_dur = best_dec_dur.min(start_dec.elapsed());
    }
    let dec_gb_s = (raw_bytes as f64 * dec_iters as f64) / (best_dec_dur.as_secs_f64() * 1e9);

    let comp_bytes = comp_buf.len();
    total_comp_bytes += comp_bytes;
    let ratio = raw_bytes as f64 / comp_bytes as f64;
    let bits_per_val = (comp_bytes * 8) as f64 / data.len() as f64;

    sum_enc += enc_gb_s;
    sum_enc_kern += enc_kernel_gb_s;
    sum_dec += dec_gb_s;

    let header = fastalp::read_header(&comp_buf).unwrap();
    let chunk_type = header.chunk_type().unwrap_or(fastalp::ChunkType::F64);
    let bw = header.params.map(|p| p.bit_width).unwrap_or(0);
    println!(
      "{:<24} | {:<12?} (bw={:>2}, rep={}) | Ratio: {:>6.2}x | Enc(samp): {:>5.2} GB/s | Enc(kern): {:>5.2} GB/s | Dec: {:>5.2} GB/s",
      name, chunk_type, bw, header.has_repeat, ratio, enc_gb_s, enc_kernel_gb_s, dec_gb_s
    );

    datasets_json.push(format!(
      r#"      {{
        "name": "{name}",
        "raw_bytes": {raw_bytes},
        "compressed_bytes": {comp_bytes},
        "ratio": {ratio:.4},
        "bits_per_val": {bits_per_val:.2},
        "enc_gb_s": {enc_gb_s:.2},
        "enc_sampled_gb_s": {enc_gb_s:.2},
        "enc_kernel_gb_s": {enc_kernel_gb_s:.2},
        "dec_gb_s": {dec_gb_s:.2}
      }}"#
    ));
  }

  let n = entries.len() as f64;
  let avg_enc = sum_enc / n;
  let avg_enc_kern = sum_enc_kern / n;
  let avg_dec = sum_dec / n;
  let total_ratio = total_raw_bytes as f64 / total_comp_bytes as f64;
  let total_bpv = (total_comp_bytes * 8) as f64 / (total_raw_bytes as f64 / 8.0);

  let full_json = format!(
    r#"{{
  "algorithm": "fastalp",
  "display_name": "fastalp (Rust)",
  "category": "specialized_float",
  "paper_31": {{
    "total_raw_bytes": {total_raw_bytes},
    "total_compressed_bytes": {total_comp_bytes},
    "ratio": {total_ratio:.4},
    "bits_per_val": {total_bpv:.2},
    "avg_enc_gb_s": {avg_enc:.2},
    "avg_enc_sampled_gb_s": {avg_enc:.2},
    "avg_enc_kernel_gb_s": {avg_enc_kern:.2},
    "avg_dec_gb_s": {avg_dec:.2},
    "datasets": [
{}
    ]
  }}
}}
"#,
    datasets_json.join(",\n")
  );

  let json_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/json/fastalp.json");
  fs::write(&json_path, full_json).expect("Failed to write fastalp.json");
  println!("\nSuccessfully updated {}", json_path.display());
}
