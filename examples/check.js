#!/usr/bin/env -S bun

import { resolve } from "node:path";
import { $ } from "bun";
import Table from "cli-table3";

const ROOT_DIR = resolve(import.meta.dirname, "..");

/**
 * 模块与对应 Rust Trait / 源码定义文件映射
 */
const MODULE_CONFIG_LI = [
  {
    name: "string",
    trait_file: "embed/src/api/string/impl.rs",
    example_file: "examples/string.rs",
    trait_name: "String",
  },
  {
    name: "hash",
    trait_file: "embed/src/api/hash/impl.rs",
    example_file: "examples/hash.rs",
    trait_name: "Hash",
  },
  {
    name: "list",
    trait_file: "embed/src/api/list/impl.rs",
    example_file: "examples/list.rs",
    trait_name: "List",
  },
  {
    name: "set",
    trait_file: "embed/src/api/set/impl.rs",
    example_file: "examples/set.rs",
    trait_name: "Set",
  },
  {
    name: "zset",
    trait_file: "embed/src/api/zset/impl.rs",
    example_file: "examples/zset.rs",
    trait_name: "ZSet",
  },
  {
    name: "bitmap",
    trait_file: "embed/src/api/bitmap/impl.rs",
    example_file: "examples/bitmap.rs",
    trait_name: "Bitmap",
  },
  {
    name: "bloom",
    trait_file: "embed/src/api/bloom/impl.rs",
    example_file: "examples/bloom.rs",
    trait_name: "Bloom",
  },
  {
    name: "json",
    trait_file: "embed/src/api/json/impl.rs",
    example_file: "examples/json.rs",
    trait_name: "Json",
  },
  {
    name: "timeseries",
    trait_file: "embed/src/api/timeseries/impl.rs",
    example_file: "examples/timeseries.rs",
    trait_name: "TimeSeries",
  },
  {
    name: "geo",
    trait_file: "embed/src/api/geo/impl.rs",
    example_file: "examples/geo.rs",
    trait_name: "Geo",
  },
  {
    name: "hll",
    trait_file: "embed/src/api/hll/impl.rs",
    example_file: "examples/hll.rs",
    trait_name: "Hll",
  },
  {
    name: "tdigest",
    trait_file: "embed/src/api/tdigest/impl.rs",
    example_file: "examples/tdigest.rs",
    trait_name: "TDigest",
  },
  {
    name: "sortedint",
    trait_file: "embed/src/api/sortedint/impl.rs",
    example_file: "examples/sortedint.rs",
    trait_name: "SortedInt",
  },
  {
    name: "stream",
    trait_file: "embed/src/api/stream/impl.rs",
    example_file: "examples/stream.rs",
    trait_name: "Stream",
  },
  {
    name: "key",
    trait_file: "embed/src/api/key/impl.rs",
    example_file: "examples/key.rs",
    trait_name: "Key",
  },
  {
    name: "search",
    trait_file: "embed/src/api/search/manager.rs",
    example_file: "examples/search.rs",
    trait_name: "SearchIndexManager",
  },
];

/**
 * 提取 Rust 接口结构体中定义的所有公共方法/接口
 */
const rustTraitMethodsExtract = (source_code) => {
  const method_set = new Set();
  let in_target_impl = false;
  let brace_depth = 0;

  for (const line of source_code.split("\n")) {
    const trimmed = line.trim();
    if (/^impl(?:<[^>]+>)?\s+(?:Db<|SearchIndexManager)/.test(trimmed)) {
      in_target_impl = true;
    }

    if (in_target_impl) {
      if (trimmed.startsWith("pub fn ") || trimmed.startsWith("pub const fn ") || trimmed.startsWith("pub async fn ")) {
        const match = trimmed.match(/^pub\s+(?:const\s+)?(?:async\s+)?fn\s+([a-zA-Z0-9_]+)/);
        if (match) {
          const fn_name = match[1];
          if (fn_name !== "new" && fn_name !== "default") {
            method_set.add(fn_name);
          }
        }
      }
      for (const ch of line) {
        if (ch === "{") brace_depth++;
        else if (ch === "}") {
          brace_depth--;
          if (brace_depth <= 0) {
            in_target_impl = false;
            brace_depth = 0;
          }
        }
      }
    }
  }
  return Array.from(method_set);
};

/**
 * 检查示例代码是否调用了指定的方法
 */
const exampleMethodInCheck = (example_code, fn_name) => {
  const call_regex = new RegExp("\\." + fn_name + "\\s*(?:<[^>]*>)?\\s*\\(", "m");
  return call_regex.test(example_code);
};

const examplesCoverageCheck = async () => {
  console.log("==> Reading Cargo metadata...\n    读取 Cargo 项目元数据...");
  const metadata_proc = await $`cargo metadata --format-version 1 --no-deps`.cwd(ROOT_DIR).quiet(),
    metadata = JSON.parse(metadata_proc.stdout.toString()),
    pkg = metadata.packages.find((p) => p.name === "wedb_embed");
  if (!pkg) {
    console.error("未找到 wedb_embed 包元数据");
    process.exit(1);
  }
  console.log("    已加载包: " + pkg.name + " (版本: " + pkg.version + ")");

  console.log("\n==> Checking example compilation (cargo check --examples)...\n    检查示例编译状态...");
  try {
    await $`cargo check --examples`.cwd(ROOT_DIR);
    console.log("    所有 16 个示例文件编译通过！");
  } catch (err) {
    console.error("示例代码编译失败:", err);
    process.exit(1);
  }

  console.log("\n==> Verifying interface coverage across all modules...\n    对比接口与各模块示例覆盖率...\n");
  let total_fn_cnt = 0,
    total_covered_cnt = 0,
    has_missing = false;

  // 无边框且对中文排版优化的表格
  const table = new Table({
    head: ["模块", "Trait / 结构体", "函数总数", "覆盖数", "覆盖率"],
    chars: {
      top: "",
      "top-mid": "",
      "top-left": "",
      "top-right": "",
      bottom: "",
      "bottom-mid": "",
      "bottom-left": "",
      "bottom-right": "",
      left: "",
      "left-mid": "",
      mid: "",
      "mid-mid": "",
      right: "",
      "right-mid": "",
      middle: "   ",
    },
    style: {
      "padding-left": 0,
      "padding-right": 0,
      head: ["bold", "cyan"],
    },
  });

  for (const mod of MODULE_CONFIG_LI) {
    const trait_path = resolve(ROOT_DIR, mod.trait_file),
      example_path = resolve(ROOT_DIR, mod.example_file),
      trait_file = Bun.file(trait_path),
      example_file = Bun.file(example_path);

    if (!(await trait_file.exists())) {
      console.error("未找到接口定义文件: " + trait_path);
      process.exit(1);
    }
    if (!(await example_file.exists())) {
      console.error("未找到示例演示文件: " + example_path);
      process.exit(1);
    }

    const trait_code = await trait_file.text(),
      example_code = await example_file.text(),
      methods = rustTraitMethodsExtract(trait_code),
      missing_methods = [];

    for (const m of methods) {
      if (!exampleMethodInCheck(example_code, m)) {
        missing_methods.push(m);
      }
    }

    const covered_cnt = methods.length - missing_methods.length;
    total_fn_cnt += methods.length;
    total_covered_cnt += covered_cnt;

    const rate = methods.length > 0 ? ((covered_cnt / methods.length) * 100).toFixed(0) : "100";

    table.push([
      mod.name,
      mod.trait_name,
      methods.length,
      covered_cnt,
      rate + "%",
    ]);

    if (missing_methods.length > 0) {
      has_missing = true;
      console.error("  ⚠️ [" + mod.name + "] 缺少函数演示: " + missing_methods.join(", "));
    }
  }

  console.log(table.toString());

  const overall_rate = total_fn_cnt > 0 ? ((total_covered_cnt / total_fn_cnt) * 100).toFixed(1) : "100.0";
  console.log("\n==> Summary: 16 modules, " + total_fn_cnt + " interface methods, " + total_covered_cnt + " covered (" + overall_rate + "%)\n    全量汇总: 16 大模块，共 " + total_fn_cnt + " 个接口函数，已覆盖 " + total_covered_cnt + " 个 (覆盖率: " + overall_rate + "%)");

  if (has_missing) {
    console.error("❌ 存在未覆盖的接口函数，请完善对应模块的演示示例！");
    process.exit(1);
  } else {
    console.log("✅ 100% full interface coverage verified!\n    100% 全接口覆盖验证通过！");
  }
};

if (import.meta.main) {
  await examplesCoverageCheck();
}

export default examplesCoverageCheck;
