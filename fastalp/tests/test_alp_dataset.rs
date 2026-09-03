use std::{
  fs,
  path::{Path, PathBuf},
  str::FromStr,
};

use fastalp::{compress, decompress};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

fn get_alp_data_dir() -> Option<PathBuf> {
  let paths = [
    PathBuf::from("/Users/z/git/db/ALP/data"),
    PathBuf::from("../../ALP/data"),
  ];
  paths.into_iter().find(|p| p.exists())
}

fn load_csv<T: FromStr>(path: &Path) -> Vec<T> {
  let content = fs::read_to_string(path).expect("Failed to read CSV");
  content
    .lines()
    .filter_map(|line| {
      let trimmed = line.trim();
      if trimmed.is_empty() {
        None
      } else {
        trimmed.parse::<T>().ok()
      }
    })
    .collect()
}

#[test]
fn test_alp_paper_datasets_roundtrip_and_ratio() -> aok::Result<()> {
  let data_dir = match get_alp_data_dir() {
    Some(d) => d,
    None => {
      println!("ALP dataset directory not found, skipping.");
      return Ok(());
    }
  };

  let samples_dir = data_dir.join("samples");
  if !samples_dir.exists() {
    return Ok(());
  }

  let mut entries: Vec<_> = fs::read_dir(&samples_dir)?
    .filter_map(|r| r.ok())
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "csv"))
    .collect();
  entries.sort_by_key(|a| a.file_name());

  println!("\n{:=<88}", "");
  println!(
    "{:<28} | {:>8} | {:>10} | {:>12} | {:>10}",
    "Dataset Name", "Count", "Raw (B)", "Comp (B)", "Ratio"
  );
  println!("{:-<88}", "");

  let mut total_raw = 0usize;
  let mut total_comp = 0usize;

  for entry in entries {
    let path = entry.path();
    let file_stem = path.file_stem().unwrap().to_string_lossy();
    let data = load_csv::<f64>(&path);
    if data.is_empty() {
      continue;
    }

    let raw_bytes = data.len() * 8;
    let compressed = compress(&data);
    let decompressed: Vec<f64> = decompress(&compressed)?;

    // 100% Exact bitwise validation
    assert_eq!(
      decompressed.len(),
      data.len(),
      "Length mismatch for {file_stem}"
    );
    for (i, (&orig, &dec)) in data.iter().zip(decompressed.iter()).enumerate() {
      if orig.is_nan() {
        assert!(dec.is_nan(), "{file_stem}[{i}]: expected NaN");
      } else {
        assert_eq!(
          orig.to_bits(),
          dec.to_bits(),
          "{file_stem}[{i}]: bit mismatch (orig={orig}, dec={dec})"
        );
      }
    }

    let comp_bytes = compressed.len();
    let ratio = raw_bytes as f64 / comp_bytes as f64;
    total_raw += raw_bytes;
    total_comp += comp_bytes;

    println!(
      "{:<28} | {:>8} | {:>10} | {:>12} | {:>9.2}x",
      file_stem,
      data.len(),
      raw_bytes,
      comp_bytes,
      ratio
    );
  }

  let total_ratio = total_raw as f64 / total_comp as f64;
  println!("{:-<88}", "");
  println!(
    "{:<28} | {:>8} | {:>10} | {:>12} | {:>9.2}x",
    "TOTAL / AVERAGE", "-", total_raw, total_comp, total_ratio
  );
  println!("{:=<88}\n", "");

  Ok(())
}

#[test]
fn test_alp_edge_case_and_float_datasets() -> aok::Result<()> {
  let data_dir = match get_alp_data_dir() {
    Some(d) => d,
    None => return Ok(()),
  };

  // 1. Edge cases
  let edge_dir = data_dir.join("edge_case");
  if edge_dir.exists() {
    for entry in fs::read_dir(edge_dir)?.filter_map(|r| r.ok()) {
      if entry.path().extension().is_some_and(|e| e == "csv") {
        let data = load_csv::<f64>(&entry.path());
        if !data.is_empty() {
          let compressed = compress(&data);
          let decompressed: Vec<f64> = decompress(&compressed)?;
          assert_eq!(decompressed.len(), data.len());
          for (orig, dec) in data.iter().zip(decompressed.iter()) {
            if orig.is_nan() {
              assert!(dec.is_nan());
            } else {
              assert_eq!(orig.to_bits(), dec.to_bits());
            }
          }
        }
      }
    }
  }

  // 2. Float datasets
  let float_dir = data_dir.join("float");
  if float_dir.exists() {
    for entry in fs::read_dir(float_dir)?.filter_map(|r| r.ok()) {
      if entry.path().extension().is_some_and(|e| e == "csv") {
        let data = load_csv::<f32>(&entry.path());
        if !data.is_empty() {
          let compressed = compress(&data);
          let decompressed: Vec<f32> = decompress(&compressed)?;
          assert_eq!(decompressed.len(), data.len());
          for (orig, dec) in data.iter().zip(decompressed.iter()) {
            if orig.is_nan() {
              assert!(dec.is_nan());
            } else {
              assert_eq!(orig.to_bits(), dec.to_bits());
            }
          }
        }
      }
    }
  }

  Ok(())
}
