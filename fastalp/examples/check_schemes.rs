use std::{fs, path::PathBuf};

use fastalp::Encoder;

fn main() {
  let alp_dir = PathBuf::from("/Users/z/git/db/ALP");
  let test_cases = [
    "gov26",
    "scene_ramp",
    "scene_sensor",
    "scene_geo",
    "scene_steady",
  ];
  let mut enc = Encoder::<f64>::with_capacity(1024);
  let mut dst = Vec::new();
  for name in test_cases {
    let content = fs::read_to_string(alp_dir.join(format!("data/samples/{name}.csv"))).unwrap();
    let data: Vec<f64> = content
      .lines()
      .filter_map(|l| l.trim().parse().ok())
      .collect();
    enc.reset();
    dst.clear();
    enc.compress_into(&data, &mut dst);
    println!("{}: scheme={:?}", name, enc.cached_scheme);
  }
}
