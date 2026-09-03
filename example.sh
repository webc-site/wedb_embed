#!/usr/bin/env bash

# Run all example tests and interface coverage checks
# 运行所有示例测试与接口覆盖率检查

set -e
DIR=$(realpath $0) && DIR=${DIR%/*}
cd $DIR

if [ -f "$DIR/sh/env.sh" ]; then
  . "$DIR/sh/env.sh"
fi

./examples/check.js

cargo build --examples

for f in examples/*.rs; do
  name=$(basename "$f" .rs)
  echo -e "\n==> Running example: $name\n    运行示例: $name"
  cargo run --example "$name"
done

echo -e "\n==> All 16 examples executed successfully!\n    全部 16 个示例执行成功！\n"
