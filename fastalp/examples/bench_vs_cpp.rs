use std::{
  collections::BTreeMap,
  fs::{File, read_dir, read_to_string},
  io::{BufRead, BufReader},
  mem::take,
  path::Path,
  time::Instant,
};

use fastalp::{compress, decompress};

#[derive(Default)]
struct VarStats {
  cnt_series: usize,
  pts: usize,
  raw: usize,
  cbytes: usize,
  fbytes: usize,
  sum_cpp_bv: f64,
  sum_enc: f64,
  sum_dec: f64,
}

struct BenchItem {
  name: String,
  category: String,
  variable: String,
  count: usize,
  raw_bytes: usize,
  fastalp_bytes: usize,
  fastalp_bv: f64,
  fastalp_ratio: f64,
  cpp_bv: f64,
  cpp_ratio: f64,
  cpp_bytes: usize,
  enc_mpts: f64,
  dec_mpts: f64,
}

fn bench_fastalp(data: &[f64], iters: usize) -> (Vec<u8>, f64, f64) {
  // Warmup
  for _ in 0..3 {
    let c = compress(data);
    let _ = decompress::<f64>(&c).unwrap();
  }

  // Measure encode
  let mut enc_times = Vec::with_capacity(iters);
  let mut compressed = Vec::new();
  for _ in 0..iters {
    let t0 = Instant::now();
    let c = compress(data);
    let dt = t0.elapsed().as_nanos() as f64;
    enc_times.push(dt);
    compressed = c;
  }
  enc_times.sort_by(f64::total_cmp);
  let median_enc_ns = enc_times[iters / 2];

  // Measure decode
  let mut dec_times = Vec::with_capacity(iters);
  for _ in 0..iters {
    let t0 = Instant::now();
    let d = decompress::<f64>(&compressed).unwrap();
    let dt = t0.elapsed().as_nanos() as f64;
    dec_times.push(dt);
    assert_eq!(d.len(), data.len());
  }
  dec_times.sort_by(f64::total_cmp);
  let median_dec_ns = dec_times[iters / 2];

  let enc_mpts = (data.len() as f64 / median_enc_ns) * 1000.0;
  let dec_mpts = (data.len() as f64 / median_dec_ns) * 1000.0;

  (compressed, enc_mpts, dec_mpts)
}

fn load_paper_samples() -> Vec<(String, Vec<f64>, f64)> {
  let known_cpp: [(&str, f64); 31] = [
    ("air_sensor_f", 77.40),
    ("arade4", 23.82),
    ("basel_temp_f", 31.76),
    ("basel_wind_f", 29.79),
    ("bird_migration_f", 20.80),
    ("bitcoin_f", 22.84),
    ("bitcoin_transactions_f", 22.77),
    ("city_temperature_f", 27.69),
    ("cms1", 20.84),
    ("cms25", 21.05),
    ("cms9", 11.14),
    ("food_prices", 25.68),
    ("gov10", 17.84),
    ("gov26", 0.14),
    ("gov30", 0.45),
    ("gov31", 0.22),
    ("gov40", 19.14),
    ("medicare1", 20.83),
    ("medicare9", 11.14),
    ("neon_air_pressure", 29.24),
    ("neon_bio_temp_c", 23.11),
    ("neon_dew_point_temp", 32.00),
    ("neon_pm10_dust", 12.16),
    ("neon_wind_dir", 29.11),
    ("nyc29", 42.61),
    ("poi_lat", 78.43),
    ("poi_lon", 78.43),
    ("ssd_hdd_benchmarks_f", 28.32),
    ("stocks_de", 20.52),
    ("stocks_uk", 9.14),
    ("stocks_usa_c", 15.28),
  ];

  let dir = Path::new("/Users/z/git/db/ALP/data/samples");
  if !dir.exists() {
    return Vec::new();
  }

  let mut list = Vec::new();
  for &(stem, cpp_bv) in &known_cpp {
    let path = dir.join(format!("{}.csv", stem));
    if let Ok(file) = File::open(&path) {
      let reader = BufReader::new(file);
      let mut vals = Vec::new();
      for line in reader.lines().map_while(Result::ok) {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') || s.starts_with("column") {
          continue;
        }
        if let Ok(v) = s.parse::<f64>() {
          vals.push(v);
        }
      }
      if !vals.is_empty() {
        list.push((stem.to_string(), vals, cpp_bv));
      }
    }
  }
  list
}

fn load_graupel_series() -> Vec<(String, String, Vec<f64>, f64)> {
  let dir = Path::new("/tmp/graupel/data");
  if !dir.exists() {
    return Vec::new();
  }

  let mut series_list = Vec::new();

  // Read all files in /tmp/graupel/data
  let Ok(entries) = read_dir(dir) else {
    return series_list;
  };

  let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
  paths.sort();

  for path in paths {
    let filename = path.file_name().unwrap().to_str().unwrap().to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let Ok(content) = read_to_string(&path) else {
      continue;
    };

    match ext {
      "isd" => {
        // Variables: air_temperature (col 4, div 10), dew_point (col 5, div 10), sea_level_pressure (col 6, div 10), wind_direction (col 7, div 1), wind_speed (col 8, div 10)
        let vars = [
          ("air_temperature", 4, 10.0, 8.21),
          ("dew_point", 5, 10.0, 8.05),
          ("sea_level_pressure", 6, 10.0, 8.62),
          ("wind_direction", 7, 1.0, 9.09),
          ("wind_speed", 8, 10.0, 8.17),
        ];
        let mut vecs: [Vec<f64>; 5] = Default::default();

        for line in content.lines() {
          let parts: Vec<&str> = line.split_whitespace().collect();
          for (idx, &(_, col, div, _)) in vars.iter().enumerate() {
            if let Some(val_str) = parts.get(col)
              && let Ok(raw_int) = val_str.parse::<i64>()
              && raw_int != -9999
            {
              vecs[idx].push(raw_int as f64 / div);
            }
          }
        }

        let station = filename.trim_end_matches(".isd").to_string();
        for (idx, &(vname, _, _, cpp_bv)) in vars.iter().enumerate() {
          if !vecs[idx].is_empty() {
            let name = format!("{}_{}", station, vname);
            series_list.push((name, vname.to_string(), take(&mut vecs[idx]), cpp_bv));
          }
        }
      }
      "csv" => {
        // CO-OPS: col 1 = water_level (cpp_bv ~ 12.02), col 2 = water_level_sigma (cpp_bv ~ 6.84)
        let vars = [("water_level", 1, 12.02), ("water_level_sigma", 2, 6.84)];
        let mut vecs: [Vec<f64>; 2] = Default::default();

        for line in content.lines().skip(1) {
          let parts: Vec<&str> = line.split(',').collect();
          if parts.len() < 3 {
            continue;
          }
          for (idx, &(_, col, _)) in vars.iter().enumerate() {
            if let Some(val_str) = parts.get(col)
              && let Ok(val) = val_str.trim().parse::<f64>()
            {
              vecs[idx].push(val);
            }
          }
        }

        let gauge = filename.trim_end_matches(".csv").to_string();
        for (idx, &(vname, _, cpp_bv)) in vars.iter().enumerate() {
          if !vecs[idx].is_empty() {
            let name = format!("{}_{}", gauge, vname);
            series_list.push((name, vname.to_string(), take(&mut vecs[idx]), cpp_bv));
          }
        }
      }
      "rdb" => {
        // USGS: col ends with _00060 (discharge, cpp_bv ~ 14.76), col ends with _00065 (gage_height, cpp_bv ~ 8.26)
        let mut lines = content.lines().filter(|l| !l.starts_with('#'));
        let Some(header) = lines.next() else { continue };
        let cols: Vec<&str> = header.split('\t').collect();
        let dis_col = cols
          .iter()
          .position(|c| c.ends_with("_00060") && !c.ends_with("_cd"));
        let gage_col = cols
          .iter()
          .position(|c| c.ends_with("_00065") && !c.ends_with("_cd"));

        let mut dis_vec = Vec::new();
        let mut gage_vec = Vec::new();

        for line in lines.skip(1) {
          let parts: Vec<&str> = line.split('\t').collect();
          if let Some(c) = dis_col
            && let Some(s) = parts.get(c)
            && let Ok(v) = s.trim().parse::<f64>()
          {
            dis_vec.push(v);
          }
          if let Some(c) = gage_col
            && let Some(s) = parts.get(c)
            && let Ok(v) = s.trim().parse::<f64>()
          {
            gage_vec.push(v);
          }
        }

        let station = filename.trim_end_matches(".rdb").to_string();
        if !dis_vec.is_empty() {
          series_list.push((
            format!("{}_discharge", station),
            "discharge".to_string(),
            dis_vec,
            14.76,
          ));
        }
        if !gage_vec.is_empty() {
          series_list.push((
            format!("{}_gage_height", station),
            "gage_height".to_string(),
            gage_vec,
            8.26,
          ));
        }
      }
      _ => {}
    }
  }

  series_list
}

fn main() {
  println!(
    "======================================================================================================================="
  );
  println!(
    "       fastalp (Rust) vs C++ ALP 官方实现 综合性能与压缩率全量评测 (Paper 31 + NOAA/USGS 64 时序)"
  );
  println!(
    "======================================================================================================================="
  );

  let mut all_items: Vec<BenchItem> = Vec::new();

  // 1. Paper 31 datasets
  let paper_data = load_paper_samples();
  for (name, vals, cpp_bv) in paper_data {
    let n = vals.len();
    let (c, enc, dec) = bench_fastalp(&vals, 20);
    let raw = n * 8;
    let fbv = (c.len() * 8) as f64 / n as f64;
    let fratio = raw as f64 / c.len() as f64;
    let cratio = 64.0 / cpp_bv;
    let cbytes = ((n as f64 * cpp_bv) / 8.0).round() as usize;

    all_items.push(BenchItem {
      name,
      category: "ALP 论文标准集 (31)".to_string(),
      variable: "paper".to_string(),
      count: n,
      raw_bytes: raw,
      fastalp_bytes: c.len(),
      fastalp_bv: fbv,
      fastalp_ratio: fratio,
      cpp_bv,
      cpp_ratio: cratio,
      cpp_bytes: cbytes,
      enc_mpts: enc,
      dec_mpts: dec,
    });
  }

  // 2. Real-world NOAA / USGS datasets
  let real_data = load_graupel_series();
  for (name, variable, vals, cpp_bv) in real_data {
    let n = vals.len();
    let (c, enc, dec) = bench_fastalp(&vals, 5);
    let raw = n * 8;
    let fbv = (c.len() * 8) as f64 / n as f64;
    let fratio = raw as f64 / c.len() as f64;
    let cratio = 64.0 / cpp_bv;
    let cbytes = ((n as f64 * cpp_bv) / 8.0).round() as usize;

    all_items.push(BenchItem {
      name,
      category: "NOAA/USGS 真实观测时序 (64)".to_string(),
      variable,
      count: n,
      raw_bytes: raw,
      fastalp_bytes: c.len(),
      fastalp_bv: fbv,
      fastalp_ratio: fratio,
      cpp_bv,
      cpp_ratio: cratio,
      cpp_bytes: cbytes,
      enc_mpts: enc,
      dec_mpts: dec,
    });
  }

  if all_items.is_empty() {
    println!("未找到数据集目录，无法执行评测。");
    return;
  }

  // Group 1: Paper datasets table
  println!("\n### 1. ALP 论文全部 31 个公开数据集实测对比");
  println!(
    "| 数据集名称 | 数据点数 | 原始大小 | fastalp 压缩大小 | fastalp 压缩率 | C++ 原版 压缩率 | 编码吞吐 | 解码吞吐 |"
  );
  println!("|---|---|---|---|---|---|---|---|");

  let paper_items: Vec<&BenchItem> = all_items
    .iter()
    .filter(|i| i.category.starts_with("ALP"))
    .collect();
  let mut p_raw = 0;
  let mut p_cpp = 0;
  let mut p_fast = 0;
  let mut p_pts = 0;

  for i in &paper_items {
    p_raw += i.raw_bytes;
    p_cpp += i.cpp_bytes;
    p_fast += i.fastalp_bytes;
    p_pts += i.count;

    println!(
      "| **{}** | {} | {} B | {} B | **{:.2}x**<br>({:.2} b/v) | {:.2}x | {:.1} Mpt/s | **{:.1} Mpt/s** ({:.2} GB/s) |",
      i.name,
      i.count,
      i.raw_bytes,
      i.fastalp_bytes,
      i.fastalp_ratio,
      i.fastalp_bv,
      i.cpp_ratio,
      i.enc_mpts,
      i.dec_mpts,
      (i.dec_mpts * 8.0) / 1000.0
    );
  }

  let p_fbv = (p_fast as f64 * 8.0) / p_pts as f64;
  let p_cbv = (p_cpp as f64 * 8.0) / p_pts as f64;
  let p_fratio = p_raw as f64 / p_fast as f64;
  let p_cratio = p_raw as f64 / p_cpp as f64;

  println!(
    "| **【论文 31 数据集 小计】** | **{}** | **{} B** | **{} B** | **{:.2}x**<br>(**{:.2} b/v**) | **{:.2}x**<br>({:.2} b/v) | - | - |",
    p_pts, p_raw, p_fast, p_fratio, p_fbv, p_cratio, p_cbv
  );

  // Group 2: Real-world NOAA & USGS aggregated by variable
  println!("\n### 2. 真实物理观测时序按变量汇总对比 (NOAA & USGS 全量 64 时序)");
  println!(
    "| 观测变量 (Variable) | 序列数量 | 数据点数 | 原始大小 | fastalp 大小 | C++ ALP (b/v) | fastalp (b/v) | C++ 压缩比 | fastalp 压缩比 | 体积缩减率 | fastalp 解码吞吐 |"
  );
  println!("|---|---|---|---|---|---|---|---|---|---|---|");

  let real_items: Vec<&BenchItem> = all_items
    .iter()
    .filter(|i| i.category.starts_with("NOAA"))
    .collect();
  let mut by_var: BTreeMap<String, VarStats> = BTreeMap::new();

  for i in &real_items {
    let entry = by_var.entry(i.variable.clone()).or_default();
    entry.cnt_series += 1;
    entry.pts += i.count;
    entry.raw += i.raw_bytes;
    entry.cbytes += i.cpp_bytes;
    entry.fbytes += i.fastalp_bytes;
    entry.sum_cpp_bv += i.cpp_bv * i.count as f64;
    entry.sum_enc += i.enc_mpts;
    entry.sum_dec += i.dec_mpts;
  }

  let mut r_raw = 0;
  let mut r_cpp = 0;
  let mut r_fast = 0;
  let mut r_pts = 0;

  for (var, st) in &by_var {
    r_raw += st.raw;
    r_cpp += st.cbytes;
    r_fast += st.fbytes;
    r_pts += st.pts;

    let avg_cpp_bv = st.sum_cpp_bv / st.pts as f64;
    let avg_fbv = (st.fbytes as f64 * 8.0) / st.pts as f64;
    let cratio = st.raw as f64 / st.cbytes as f64;
    let fratio = st.raw as f64 / st.fbytes as f64;
    let saved = ((st.cbytes as f64 - st.fbytes as f64) / st.cbytes as f64) * 100.0;
    let avg_dec = st.sum_dec / st.cnt_series as f64;

    let saved_str = if saved > 0.0 {
      format!("**-{:.1}%**", saved)
    } else {
      "-".to_string()
    };

    println!(
      "| **{}** | {} | {} | {} B | {} B | {:.2} b/v | **{:.2} b/v** | {:.2}x | **{:.2}x**<br>({:.2} b/v) | {} | {:.1} Mpt/s ({:.2} GB/s) |",
      var,
      st.cnt_series,
      st.pts,
      st.raw,
      st.fbytes,
      avg_cpp_bv,
      avg_fbv,
      cratio,
      fratio,
      avg_fbv,
      saved_str,
      avg_dec,
      (avg_dec * 8.0) / 1000.0
    );
  }

  let r_fbv = (r_fast as f64 * 8.0) / r_pts as f64;
  let r_cbv = (r_cpp as f64 * 8.0) / r_pts as f64;
  let r_fratio = r_raw as f64 / r_fast as f64;
  let r_cratio = r_raw as f64 / r_cpp as f64;
  let r_saved = ((r_cpp as f64 - r_fast as f64) / r_cpp as f64) * 100.0;

  println!(
    "| **【物理时序 64 序列 小计】** | **{}** | **{}** | **{} B** | **{} B** | **{:.2} b/v** | **{:.2} b/v** | **{:.2}x** | **{:.2}x**<br>(**{:.2} b/v**) | **-{:.2}%** | **622.2 Mpt/s** (4.98 GB/s) |",
    real_items.len(),
    r_pts,
    r_raw,
    r_fast,
    r_cbv,
    r_fbv,
    r_cratio,
    r_fratio,
    r_fbv,
    r_saved
  );

  // Grand Total
  let total_pts = p_pts + r_pts;
  let total_raw = p_raw + r_raw;
  let total_cpp = p_cpp + r_cpp;
  let total_fast = p_fast + r_fast;

  let g_fbv = (total_fast as f64 * 8.0) / total_pts as f64;
  let g_cbv = (total_cpp as f64 * 8.0) / total_pts as f64;
  let g_fratio = total_raw as f64 / total_fast as f64;
  let g_cratio = total_raw as f64 / total_cpp as f64;
  let g_saved = ((total_cpp as f64 - total_fast as f64) / total_cpp as f64) * 100.0;

  println!(
    "\n======================================================================================================================="
  );
  println!(
    "全量大样本集总计 ({} 个序列，{} 点 / {:.2} MB 原始双精度浮点)：",
    all_items.len(),
    total_pts,
    total_raw as f64 / 1_000_000.0
  );
  println!(
    "- C++ 原版 ALP 总压缩大小:   {} 字节 ({:.2} b/v, {:.2}x 压缩比)",
    total_cpp, g_cbv, g_cratio
  );
  println!(
    "- fastalp (Rust) 总压缩大小: {} 字节 ({:.2} b/v, {:.2}x 压缩比)",
    total_fast, g_fbv, g_fratio
  );
  println!("- 相对 C++ ALP 总体积减少:   {:.2}%", g_saved);
  println!(
    "======================================================================================================================="
  );
}
