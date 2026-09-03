#!/usr/bin/env bash

set -e
DIR=$(realpath $0) && DIR=${DIR%/*}
cd $DIR
set -x

# 1. 检查并准备 C++ ALP 环境 (相对路径，不存在则自动 clone depth=1 并编译)
ALP_DIR="$(cd "${DIR}/../../" && pwd)/ALP"
export ALP_DIR

if [ ! -d "$ALP_DIR" ]; then
  echo "=== 1. Cloning C++ ALP repo (depth=1) ==="
  git clone --depth 1 https://github.com/x-at-01/ALP.git "$ALP_DIR"
fi

echo "=== 1. Building and Running C++ ALP Benchmark (All 37 Datasets) ==="
if [ ! -d "$ALP_DIR/build" ]; then
  cmake -B "$ALP_DIR/build" -S "$ALP_DIR" -DCMAKE_BUILD_TYPE=Release
fi
cmake --build "$ALP_DIR/build" --target bench_your_dataset -j
(cd "$ALP_DIR" && ./build/benchmarks/bench_your_dataset)
bun -e 'import { loadCppAlpResult } from "./benches/lib/cpp_alp_loader.js"; await loadCppAlpResult();'

# 2. 运行 Rust fastalp 全量 37 项公开测试集端到端基准测试
echo "=== 2. Running Rust fastalp Benchmark (All 37 Datasets) ==="
cargo run --release --example bench_all_37

# 3. 生成最新手机长图 SVG 与超清 JPG
echo "=== 3. Generating Benchmark SVG and JPG Images ==="
bun run benches/benchSvgGen.js

# 4. 上传 CDN 并自动更新 readme 与编译 README.md
echo "=== 4. Uploading CDN & Syncing Markdown README ==="
bun run benches/benchCdnPublish.js

echo "=== All benchmark steps complete! ==="
