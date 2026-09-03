#!/usr/bin/env -S bun

import benchRun from "./benchRun.js";
import benchSvgGen from "./benchSvgGen.js";

// 主执行逻辑：串联执行基准测试数据产出与 SVG/文档生成
const benchTableGen = async () => {
  const data = await benchRun();
  if (data) {
    await benchSvgGen(data);
  }
};

if (import.meta.main) {
  await benchTableGen();
}

export default benchTableGen;
