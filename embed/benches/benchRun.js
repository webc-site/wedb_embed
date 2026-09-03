#!/usr/bin/env -S bun

import { resolve } from "node:path";
import { mkdir } from "node:fs/promises";
import {
  arch as osArch,
  cpus,
  platform as osPlatform,
  release as osRelease,
  totalmem,
  type as osType,
} from "node:os";
import { $ } from "bun";

// 常量定义
const PREFIX_BENCH = "bench_",
  PREFIX_BENCH_CMP = "bench_cmp_",
  REDIS_SOCK = "/tmp/wedb_redis_bench.sock",
  REDIS_DATA_DIR = "/tmp/wedb_redis_bench_data",
  ROOT_DIR = resolve(import.meta.dirname, "../.."),
  BASELINE_DIR = resolve(ROOT_DIR, "embed/benches/baseline"),
  BENCH_DATA_JSON = resolve(ROOT_DIR, "embed/benches/benchData.json");

// 1. 原始操作定义表（领域模块分类）
const RAW_OP_DEFS = {
  str: ["SET", "GET", "MSET", "MGET", "INCRBY", "DECRBY", "APPEND", "STRLEN", "GETDEL", "GETRANGE", "SETRANGE"],
  hash: ["HSET", "HGET", "HDEL", "HEXISTS", "HLEN", "HMGET", "HGETALL", "HKEYS", "HVALS", "HINCRBY"],
  list: ["LPUSH", "RPUSH", "LPOP", "RPOP", "LLEN", "LRANGE", "LINDEX", "LSET", "LREM", "LTRIM"],
  set: ["SADD", "SREM", "SISMEMBER", "SMEMBERS", "SCARD", "SPOP", "SRANDMEMBER"],
  zset: ["ZADD", "ZREM", "ZSCORE", "ZCARD", "ZCOUNT", "ZINCRBY", "ZRANK", "ZRANGE", "ZREVRANGE", "ZPOPMIN"],
  bitmap: ["SETBIT", "GETBIT", "BITCOUNT", "BITPOS"],
  json: {
    set: "JSON.SET",
    get: "JSON.GET",
    del: "JSON.DEL",
    type: "JSON.TYPE",
    numincrby: "JSON.NUMINCRBY",
    arrlen: "JSON.ARRLEN",
  },
  vector: {
    knn: "VECTOR.KNN",
  },
  search: {
    ft_search: "FT.SEARCH",
    tag: "FT.TAG",
  },
  bloom: {
    bf_add: "BF.ADD",
    bf_exists: "BF.EXISTS",
    bf_info: "BF.INFO",
    cf_add: "CF.ADD",
    cf_exists: "CF.EXISTS",
    cf_del: "CF.DEL",
  },
  timeseries: {
    ts_add: "TS.ADD",
    ts_get: "TS.GET",
    ts_range: "TS.RANGE",
    ts_incrby: "TS.INCRBY",
  },
  geo: ["GEOADD", "GEODIST", "GEOPOS", "GEOHASH"],
  hll: ["PFADD", "PFCOUNT", "PFMERGE"],
  tdigest: {
    add: "TDIGEST.ADD",
    quantile: "TDIGEST.QUANTILE",
    byrank: "TDIGEST.BYRANK",
    cdf: "TDIGEST.CDF",
  },
  sortedint: {
    si_add: "SI.ADD",
    si_rem: "SI.REM",
    si_card: "SI.CARD",
    si_exists: "SI.EXISTS",
    si_range: "SI.RANGE",
  },
  stream: ["XADD", "XLEN", "XRANGE", "XREAD", "XDEL"],
  db: ["EXISTS", "DEL", "EXPIRE", "TTL", "NAMESPACE", "BATCH_COMMIT"],
};

// 86 项全类型核心对比命令定义表（涵盖全部 Redis 核心数据结构与官方插件：JSON、向量检索、布隆/布谷鸟、TDigest、时序）
const RAW_CMP_DEFS = {
  str: ["SET", "GET", "MSET", "MGET", "INCRBY", "DECRBY", "APPEND", "STRLEN", "GETDEL", "GETRANGE", "SETRANGE"],
  hash: ["HSET", "HGET", "HMGET", "HEXISTS", "HLEN", "HDEL", "HGETALL", "HKEYS", "HVALS", "HINCRBY"],
  list: ["LPUSH", "RPUSH", "LPOP", "RPOP", "LLEN", "LRANGE", "LINDEX", "LSET", "LREM", "LTRIM"],
  set: ["SADD", "SREM", "SISMEMBER", "SCARD", "SMEMBERS", "SPOP", "SRANDMEMBER"],
  zset: ["ZADD", "ZSCORE", "ZRANGE", "ZCARD", "ZCOUNT", "ZINCRBY", "ZRANK", "ZREVRANGE", "ZPOPMIN", "ZREM"],
  bitmap: ["SETBIT", "GETBIT", "BITCOUNT", "BITPOS"],
  hll: ["PFADD", "PFCOUNT"],
  geo: ["GEOADD", "GEODIST", "GEOPOS", "GEOHASH"],
  stream: ["XADD", "XLEN", "XRANGE", "XREAD", "XDEL"],
  db: ["DEL", "EXISTS", "EXPIRE", "TTL"],
  json: {
    set: "JSON.SET",
    get: "JSON.GET",
    del: "JSON.DEL",
    numincrby: "JSON.NUMINCRBY",
    arrlen: "JSON.ARRLEN",
    type: "JSON.TYPE",
  },
  bloom: {
    bf_add: "BF.ADD",
    bf_exists: "BF.EXISTS",
    bf_info: "BF.INFO",
    cf_add: "CF.ADD",
    cf_exists: "CF.EXISTS",
    cf_del: "CF.DEL",
  },
  tdigest: {
    add: "TDIGEST.ADD",
    quantile: "TDIGEST.QUANTILE",
    byrank: "TDIGEST.BYRANK",
    cdf: "TDIGEST.CDF",
  },
  timeseries: {
    ts_add: "TS.ADD",
    ts_get: "TS.GET",
    ts_range: "TS.RANGE",
    ts_incrby: "TS.INCRBY",
  },
  search: {
    ft_search: "FT.SEARCH",
    tag: "FT.TAG",
  },
  vector: {
    knn: "VECTOR.KNN",
  },
};

// 映射表构建工具函数（消除重复前缀与样板代码）
const opNameMapBuild = (defs, prefix) => {
  const map = {};
  for (const [mod, items] of Object.entries(defs)) {
    if (Array.isArray(items)) {
      items.forEach((cmd) => {
        map[prefix + mod + "_" + cmd.toLowerCase()] = cmd;
      });
    } else {
      for (const [fn, cmd] of Object.entries(items)) {
        map[prefix + mod + "_" + fn] = cmd;
      }
    }
  }
  return map;
};

// 接口命令映射表（纯原生大写命令名称）与 39 项全类型核心对比命令映射表
const OP_NAME_MAP = opNameMapBuild(RAW_OP_DEFS, PREFIX_BENCH),
  CMP_NAME_MAP = opNameMapBuild(RAW_CMP_DEFS, PREFIX_BENCH_CMP);

// ARM 处理器厂商与型号识别映射表
const ARM_PART_MAP = {
  "0x41": {
    "0xd49": "ARM Neoverse-N2",
    "0xd4f": "ARM Neoverse-V2",
    "0xd40": "ARM Neoverse-V1",
    "0xd0c": "ARM Neoverse-N1",
    "0xd4a": "ARM Neoverse-E1",
    "0xd08": "ARM Cortex-A72",
    "0xd07": "ARM Cortex-A57",
    "0xd03": "ARM Cortex-A53",
    "0xd04": "ARM Cortex-A35",
    "0xd05": "ARM Cortex-A55",
    "0xd09": "ARM Cortex-A73",
    "0xd0a": "ARM Cortex-A75",
    "0xd0b": "ARM Cortex-A76",
    "0xd0d": "ARM Cortex-A77",
    "0xd41": "ARM Cortex-A78",
    "0xd44": "ARM Cortex-X1",
    "0xd46": "ARM Cortex-A510",
    "0xd47": "ARM Cortex-A710",
    "0xd48": "ARM Cortex-X2",
    "0xd4b": "ARM Cortex-A78AE",
    "0xd4c": "ARM Cortex-A78C",
    "0xd4d": "ARM Cortex-A715",
    "0xd4e": "ARM Cortex-X3",
    "0xd80": "ARM Cortex-A520",
    "0xd81": "ARM Cortex-A720",
    "0xd82": "ARM Cortex-X4",
  },
  "0xc0": {
    "0xac3": "Ampere Altra",
    "0x0a1": "Ampere-1",
    "0x0a2": "Ampere-1A",
    "0x0a3": "Ampere-1B",
  },
  "0x51": {
    "0x001": "Qualcomm Oryon",
  },
  "0x48": {
    "0xd01": "HiSilicon Kunpeng 920",
    "0xd02": "HiSilicon Kunpeng 930",
  },
  "0x61": {
    "0x022": "Apple M1",
    "0x023": "Apple M1 Pro",
    "0x024": "Apple M1 Max",
    "0x025": "Apple M1 Ultra",
    "0x028": "Apple M2",
    "0x029": "Apple M2 Pro",
    "0x02a": "Apple M2 Max",
    "0x02b": "Apple M2 Ultra",
    "0x030": "Apple M3",
    "0x031": "Apple M3 Pro",
    "0x032": "Apple M3 Max",
    "0x033": "Apple M3 Ultra",
    "0x038": "Apple M4",
    "0x039": "Apple M4 Pro",
    "0x03a": "Apple M4 Max",
  },
};

// 提取并校准 CPU 具体型号（解决 Linux ARM 环境下 os.cpus() 返回 unknown 的问题）
const cpuModelDetect = async (platform, arch, raw_model) => {
  const is_unknown =
    !raw_model ||
    raw_model.toLowerCase() === "unknown" ||
    raw_model.toLowerCase() === "unknown cpu" ||
    raw_model === "-";

  if (!is_unknown) {
    return raw_model;
  }

  if (platform === "linux") {
    // 1. 优先尝试 lscpu（util-linux 自带完善的 ARM/x86 架构识别）
    try {
      const lscpu_txt = await $`lscpu`.quiet().text(),
        model_m = lscpu_txt.match(/^\s*(?:Model name|BIOS Model name)\s*:\s*(.+)$/im);
      if (model_m && model_m[1].trim() && model_m[1].trim().toLowerCase() !== "unknown") {
        return model_m[1].trim();
      }
    } catch {}

    // 2. 尝试读取 /proc/cpuinfo
    try {
      const cpuinfo_file = Bun.file("/proc/cpuinfo");
      if (await cpuinfo_file.exists()) {
        const txt = await cpuinfo_file.text(),
          model_m = txt.match(/^(?:model name|Hardware|Processor)\s*:\s*(.+)$/im);
        if (model_m && model_m[1].trim() && model_m[1].trim().toLowerCase() !== "unknown") {
          return model_m[1].trim();
        }

        const imp_m = txt.match(/^CPU implementer\s*:\s*(0x[0-9a-fA-F]+|[0-9]+)/im),
          part_m = txt.match(/^CPU part\s*:\s*(0x[0-9a-fA-F]+|[0-9]+)/im);
        if (imp_m && part_m) {
          const imp_hex = imp_m[1].startsWith("0x")
              ? imp_m[1].toLowerCase()
              : "0x" + parseInt(imp_m[1], 10).toString(16),
            part_hex = part_m[1].startsWith("0x")
              ? part_m[1].toLowerCase()
              : "0x" + parseInt(part_m[1], 10).toString(16),
            mapped = ARM_PART_MAP[imp_hex]?.[part_hex];
          if (mapped) {
            return mapped;
          }
        }
      }
    } catch {}
  } else if (platform === "darwin") {
    try {
      const brand = await $`sysctl -n machdep.cpu.brand_string`.quiet().text();
      if (brand.trim()) {
        return brand.trim();
      }
    } catch {}
  }

  return arch === "aarch64" ? "ARM64 Processor" : arch === "x86_64" ? "x86_64 Processor" : "Unknown CPU";
};

// 2. 自动检测物理硬盘类型与配置
const diskInfoDetect = async (platform) => {
  if (platform === "darwin") {
    try {
      const sp_out = await $`system_profiler SPStorageDataType 2>/dev/null`.quiet().text(),
        name_m = sp_out.match(/Device Name:\s*([^\n]+)/),
        medium_m = sp_out.match(/Medium Type:\s*([^\n]+)/);
      if (name_m) {
        const dev_name = name_m[1].trim(),
          med = medium_m ? medium_m[1].trim() : "SSD";
        return `${dev_name} (${med} / Apple Fabric NVMe)`;
      }
      return "Apple PCIe NVMe SSD";
    } catch {
      return "Apple PCIe NVMe SSD";
    }
  } else if (platform === "linux") {
    try {
      const is_ci = process.env.CI === "true" || process.env.GITHUB_ACTIONS === "true";
      if (is_ci) {
        return "Azure Managed Virtual Disk (Cloud Standard SSD)";
      }
      const lsblk_out = await $`lsblk -d -n -o NAME,MODEL,ROTA 2>/dev/null`.quiet().text();
      const line = lsblk_out.trim().split("\n").filter(Boolean)[0];
      if (line) {
        const parts = line.split(/\s+/);
        const name = parts[0],
          rota = parts.find((p) => p === "0" || p === "1"),
          type_str = rota === "0" ? "NVMe/SSD" : "HDD";
        return `${name} (${type_str})`;
      }
      return "Linux Block Device (SSD)";
    } catch {
      return process.env.CI === "true" ? "Azure Managed Virtual Disk (Cloud Standard SSD)" : "Local SSD";
    }
  }
  return "Standard Disk Storage";
};

// 3. 自动采集硬件与系统测试环境
const envDetect = async () => {
  const platform = osPlatform(),
    raw_arch = osArch(),
    arch = raw_arch === "arm64" ? "aarch64" : raw_arch === "x64" ? "x86_64" : raw_arch,
    cpu_li = cpus(),
    raw_cpu_model = cpu_li[0]?.model?.trim() || "",
    cpu_model = await cpuModelDetect(platform, arch, raw_cpu_model),
    cpu_cores = cpu_li.length,
    total_mem_gb = (totalmem() / (1024 * 1024 * 1024)).toFixed(1) + " GB",
    disk_info = await diskInfoDetect(platform);

  let os_name = osType() + " " + osRelease();
  if (platform === "darwin") {
    try {
      const sw_vers = await $`sw_vers -productVersion`.text();
      os_name = "macOS " + sw_vers.trim() + " (Darwin " + osRelease() + ")";
    } catch {
      os_name = "macOS (Darwin " + osRelease() + ")";
    }
  } else if (platform === "linux") {
    try {
      const os_file = Bun.file("/etc/os-release");
      if (await os_file.exists()) {
        const content = await os_file.text(),
          pretty_match = content.match(/PRETTY_NAME="?([^"\n]+)"?/);
        if (pretty_match) {
          os_name = pretty_match[1] + " (Linux " + osRelease() + ")";
        }
      }
    } catch {
      os_name = "Linux " + osRelease();
    }
  }

  let rust_ver = "unknown";
  try {
    const raw_rustc = await $`rustc --version`.text();
    rust_ver = raw_rustc.replace("rustc ", "").trim();
  } catch {}

  const is_ci = process.env.CI === "true" || process.env.GITHUB_ACTIONS === "true";
  let slug = is_ci ? "ubuntu_" + arch : (platform === "darwin" ? "macos" : platform) + "_" + arch;

  const env_arg_idx = process.argv.indexOf("--env");
  if (env_arg_idx !== -1 && process.argv[env_arg_idx + 1]) {
    slug = process.argv[env_arg_idx + 1]
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_]/g, "_");
  }

  const title = is_ci
    ? "Ubuntu CI (GitHub Actions Runner)"
    : (platform === "darwin" ? "macOS" : platform) + " (" + cpu_model + ")";

  return {
    slug,
    is_ci,
    cpu_model,
    cpu_cores,
    total_mem_gb,
    disk_info,
    os_name,
    arch,
    rust_ver,
    title_zh: title,
    title_en: title,
  };
};

// 3. Redis 清理、插件自动发现与守护进程管理
const redisModulesFind = async () => {
  const candidate_dir_li = [
    "/opt/homebrew/Caskroom/redis-stack-server",
    "/usr/local/Caskroom/redis-stack-server",
    "/opt/redis-stack/lib",
    "/usr/lib/redis/modules",
    "/usr/local/lib",
    "/opt/homebrew/lib",
  ];

  let module_dir = null;
  for (const base of candidate_dir_li) {
    try {
      if (base.includes("Caskroom")) {
        const check = await $`ls -d ${base}/*/lib 2>/dev/null`.quiet().text(),
          found = check.trim().split("\n").filter(Boolean)[0];
        if (found && (await Bun.file(found + "/rejson.so").exists())) {
          module_dir = found;
          break;
        }
      } else {
        const f = Bun.file(base + "/rejson.so");
        if (await f.exists()) {
          module_dir = base;
          break;
        }
      }
    } catch {}
  }

  if (!module_dir) {
    try {
      const which_stack = await $`which redis-stack-server 2>/dev/null`.quiet().text();
      if (which_stack.trim()) {
        const stack_path = which_stack.trim(),
          lib_dir = resolve(stack_path, "../../lib");
        if (await Bun.file(lib_dir + "/rejson.so").exists()) {
          module_dir = lib_dir;
        }
      }
    } catch {}
  }

  if (!module_dir) {
    return [];
  }

  const module_file_li = [
    "rediscompat.so",
    "redisearch.so",
    "redistimeseries.so",
    "rejson.so",
    "redisbloom.so",
  ],
    loadmodule_arg_li = [];

  for (const mod of module_file_li) {
    const p = module_dir + "/" + mod;
    if (await Bun.file(p).exists()) {
      if (mod === "redisearch.so") {
        loadmodule_arg_li.push("--loadmodule", p, "MAXSEARCHRESULTS", "10000", "MAXAGGREGATERESULTS", "10000");
      } else {
        loadmodule_arg_li.push("--loadmodule", p);
      }
    }
  }

  return loadmodule_arg_li;
};

const redisCleanup = async () => {
  try {
    await $`rm -f ${REDIS_SOCK}`.quiet();
    await $`rm -rf ${REDIS_DATA_DIR}`.quiet();
  } catch {}
};

const redisServerEnsure = async () => {
  try {
    const which_res = await $`which redis-server`.quiet().text();
    if (!which_res.trim()) {
      console.warn("本地未检测到 redis-server，跳过 Redis 对比基准测试");
      return null;
    }

    const raw_ver = await $`redis-server --version`.text(),
      ver_match = raw_ver.match(/v=([0-9.]+)/),
      redis_ver = ver_match ? "v" + ver_match[1] : "latest",
      loadmodule_arg_li = await redisModulesFind();

    if (loadmodule_arg_li.length > 0) {
      const active_mods = loadmodule_arg_li
        .filter((a) => a.endsWith(".so"))
        .map((a) => a.split("/").pop().replace(".so", ""))
        .join(", ");
      console.log(`==> 成功加载 Redis 扩展插件 (${active_mods})`);
    } else {
      console.log("==> 未检测到 Redis 扩展插件，以标准模式启动");
    }

    await redisCleanup();
    await $`mkdir -p ${REDIS_DATA_DIR}`.quiet();
    if (loadmodule_arg_li.length > 0) {
      await $`redis-server --port 0 --unixsocket ${REDIS_SOCK} --unixsocketperm 777 --dir ${REDIS_DATA_DIR} --save "" --appendonly yes --appendfsync everysec --daemonize yes ${loadmodule_arg_li}`.quiet();
    } else {
      await $`redis-server --port 0 --unixsocket ${REDIS_SOCK} --unixsocketperm 777 --dir ${REDIS_DATA_DIR} --save "" --appendonly yes --appendfsync everysec --daemonize yes`.quiet();
    }

    let ready = false;
    for (let i = 0; i < 20; ++i) {
      try {
        const ping = await $`redis-cli -s ${REDIS_SOCK} PING`.quiet().text();
        if (ping.trim() === "PONG") {
          ready = true;
          break;
        }
      } catch {}
      await Bun.sleep(50);
    }

    if (!ready) {
      console.warn("Redis Server 启动未能成功建立通信");
      return null;
    }

    console.log(`==> 成功启动 Redis Server (${redis_ver} AOF 模式) 于 ${REDIS_SOCK}`);
    return redis_ver;
  } catch (err) {
    console.warn("Redis Server 启动异常: " + (err?.message || err));
    return null;
  }
};

const redisServerStop = async () => {
  try {
    await $`redis-cli -s ${REDIS_SOCK} shutdown nosave`.quiet();
  } catch {}
  await redisCleanup();
};

// 4. 5GB 全格式数据灌入与磁盘/内存物理开销实测
const footprintBenchRun = async () => {
  try {
    console.log("==> 正在灌入 5GB 全格式真实数据并测量磁盘与内存物理占用 (5,000,000 条结构化数据)...");
    const foot_txt = await $`cargo bench -p wedb_embed --features bench --bench footprint -- --nocapture`.cwd(ROOT_DIR).text(),
      match = foot_txt.match(/FOOTPRINT_RESULT_START\s*([\s\S]*?)\s*FOOTPRINT_RESULT_END/);

    if (match) {
      const data = JSON.parse(match[1].trim()),
        { disk_mb: wedb_mb } = data.wedb,
        { disk_mb: redis_mb } = data.redis,
        saved_pct = (((redis_mb - wedb_mb) / (redis_mb || 1)) * 100).toFixed(1);
      console.log(
        `==> 5GB 全格式数据灌入与资源测量完成: WeDb 物理落盘 ${wedb_mb.toFixed(2)} MB vs Redis AOF ${redis_mb.toFixed(2)} MB (节省 ${saved_pct}%)`
      );
      return data;
    }
  } catch (err) {
    console.warn("⚠️ 资源占用测试跳过或异常: " + (err?.message || err));
  }
  return null;
};

// 5. 解析时间字符串为纳秒
const divanTimeParse = (val_str) => {
  const match = val_str.trim().match(/^([0-9.]+)\s*([a-zA-Zµμ]+)$/);
  if (!match) return 0;
  const num = parseFloat(match[1]),
    unit = match[2].toLowerCase();
  if (unit === "ns") return num;
  if (unit === "µs" || unit === "us" || unit === "μs") return num * 1000;
  if (unit === "ms") return num * 1000 * 1000;
  if (unit === "s") return num * 1000 * 1000 * 1000;
  return num;
};

// 6. 通用 Divan 性能测试表格输出行解析器（复用重复的正则与字符串拆分逻辑）
const divanTableRowsParse = (txt) => {
  const row_li = [],
    clean_line_li = txt.split("\n").map((l) => l.replace(/\x1b\[[0-9;]*m/g, ""));

  for (const line of clean_line_li) {
    const parts = line.split("│").map((p) => p.trim());
    if (parts.length < 4) continue;

    const match = parts[0].match(/([a-zA-Z0-9_]+)\s+([0-9.]+\s*[a-zA-Zµμ]+)$/);
    if (!match) continue;

    const name = match[1],
      fastest = match[2],
      slowest = parts[1],
      median = parts[2] || slowest,
      mean = parts[3] || median;

    row_li.push({
      name,
      fastest,
      slowest,
      median,
      mean,
      fastest_ns: divanTimeParse(fastest),
      slowest_ns: divanTimeParse(slowest),
      median_ns: divanTimeParse(median),
      mean_ns: divanTimeParse(mean),
    });
  }
  return row_li;
};

// 7. 解析 WeDb vs Redis 对比输出
const cmpOutputParse = (txt) => {
  const raw_map = {};
  for (const row of divanTableRowsParse(txt)) {
    if (row.name.startsWith(PREFIX_BENCH_CMP)) {
      raw_map[row.name] = row;
    }
  }

  const result_li = [];
  for (const [prefix, cmd_name] of Object.entries(CMP_NAME_MAP)) {
    const wedb_data = raw_map[prefix + "_wedb"],
      redis_data = raw_map[prefix + "_redis"];

    if (wedb_data && redis_data) {
      const wedb_ns = wedb_data.median_ns || wedb_data.fastest_ns || 1,
        redis_ns = redis_data.median_ns || redis_data.fastest_ns || 1,
        speedup = (redis_ns / wedb_ns).toFixed(1) + "x";

      result_li.push({
        cmd: cmd_name,
        wedb_fastest: wedb_data.fastest,
        wedb_slowest: wedb_data.slowest,
        wedb_median: wedb_data.median,
        wedb_ns,
        wedb_slowest_ns: wedb_data.slowest_ns,
        redis_fastest: redis_data.fastest,
        redis_slowest: redis_data.slowest,
        redis_median: redis_data.median,
        redis_ns,
        redis_slowest_ns: redis_data.slowest_ns,
        speedup,
      });
    }
  }

  return result_li;
};

// 8. 解析 Divan 93 项命令独立输出
const benchOutputParse = (txt) => {
  const result_li = [];
  for (const row of divanTableRowsParse(txt)) {
    const { name, fastest, slowest, median, fastest_ns, slowest_ns, median_ns } = row;
    if (name.startsWith(PREFIX_BENCH) && !name.startsWith(PREFIX_BENCH_CMP)) {
      const ops_per_sec = median_ns > 0 ? 1000000000 / median_ns : 0,
        throughput =
          ops_per_sec >= 1000000
            ? (ops_per_sec / 1000000).toFixed(2) + " M ops/s"
            : ops_per_sec >= 1000
              ? (ops_per_sec / 1000).toFixed(1) + " k ops/s"
              : Math.round(ops_per_sec) + " ops/s";

      result_li.push({
        raw_name: name,
        cmd: OP_NAME_MAP[name] || name.replace(/^bench_[a-z0-9]+_/, "").toUpperCase(),
        fastest,
        slowest,
        median,
        fastest_ns,
        slowest_ns,
        median_ns,
        throughput,
      });
    }
  }

  return result_li;
};

// 9. 主执行函数：执行 5GB 灌入、对比测试与 93 项全命令基准测试，并写入 JSON
export const benchRun = async () => {
  const env_info = await envDetect();
  let extra_arg_li = process.argv
    .slice(2)
    .filter((arg, i, arr) => arg !== "--env" && arr[i - 1] !== "--env");

  if (extra_arg_li.length > 0 && extra_arg_li[0] === "--") {
    extra_arg_li = extra_arg_li.slice(1);
  }

  console.log(`==> 识别到测试环境: ${env_info.slug} (${env_info.title_zh})`);
  console.log(
    `==> 硬件信息: CPU: ${env_info.cpu_model} (${env_info.cpu_cores}核), 内存: ${env_info.total_mem_gb}, 硬盘: ${env_info.disk_info}, 系统: ${env_info.os_name}`
  );

  const redis_ver = await redisServerEnsure();
  env_info.redis_ver = redis_ver;

  let cmp_data_li = [],
    footprint_data = null;

  try {
    if (redis_ver) {
      // 阶段 1: 执行 WeDb vs Redis 公有命令对比基准测试
      console.log("==> [阶段 1/3] 正在执行 WeDb vs Redis 90 项核心公有命令对比基准测试...");
      try {
        const cmp_txt = await $`cargo bench -p wedb_embed --features bench --bench redis_vs_wedb -- --sample-count 20`.cwd(ROOT_DIR).text();
        cmp_data_li = cmpOutputParse(cmp_txt);
        console.log(`==> 成功解析 ${cmp_data_li.length} 项核心公有命令 WeDb vs Redis 对比数据`);
      } catch (err) {
        console.error("WeDb vs Redis 基准测试执行异常:", err.message || err);
      }

      // 阶段 2: 在测试特有命令前，先精确测量双方基于相同公有数据结构的 5GB 数据集物理落盘与常驻内存 (RSS)
      console.log("==> [阶段 2/3] 正在测量公有数据结构 5GB 真实物理落盘与内存开销 (严格公允对比)...");
      footprint_data = await footprintBenchRun();
    }

    // 阶段 3: 执行 WeDb 全量指令（含 WeDb 专有数据结构与特有命令）独立基准测试
    console.log("==> [阶段 3/3] 正在执行 WeDb 全量指令（含专有命令如 SortedInt/Engine 等）独立基准测试...");
    const bench_txt = await (extra_arg_li.length > 0
      ? $`cargo bench -p wedb_embed --features bench --bench bench -- ${extra_arg_li}`.cwd(ROOT_DIR)
      : $`cargo bench -p wedb_embed --features bench --bench bench`.cwd(ROOT_DIR)
    ).text();
    const record_li = benchOutputParse(bench_txt);

    if (record_li.length === 0) {
      console.warn("⚠️ 未能从输出中解析到基准测试数据");
      return null;
    }

    console.log(`==> 成功解析 ${record_li.length} 项 WeDb 独立命令基准测试数据`);

    // 阶段 4: 整体性能对比严格仅基于双方公有命令（cmp_li）计算总吞吐量比值
    const total_wedb_qps = cmp_data_li.reduce((acc, c) => acc + (1000000000 / c.wedb_ns), 0),
      total_redis_qps = cmp_data_li.reduce((acc, c) => acc + (1000000000 / c.redis_ns), 0),
      overall_speedup_str = total_redis_qps > 0 ? (total_wedb_qps / total_redis_qps).toFixed(1) + "x" : "N/A";

    const payload_data = {
      timestamp: Math.floor(Date.now() / 1000),
      env: env_info,
      footprint: footprint_data,
      cmp_li: cmp_data_li,
      overall_speedup: overall_speedup_str,
      record_li,
    };

    await mkdir(BASELINE_DIR, { recursive: true });
    await Promise.all([
      Bun.write(BENCH_DATA_JSON, JSON.stringify(payload_data, null, 2)),
      Bun.write(resolve(BASELINE_DIR, env_info.slug + "_data.json"), JSON.stringify(payload_data, null, 2)),
    ]);

    console.log(`==> 成功保存基准测试全量数据至: ${BENCH_DATA_JSON}`);

    // 阶段 5: 自动触发双语卡片 SVG/JPG 渲染与 Markdown 报告同步
    try {
      const { benchSvgGen } = await import("./benchSvgGen.js");
      await benchSvgGen(payload_data);
    } catch (err) {
      console.warn("SVG & 图表自动生成跳过或异常:", err.message || err);
    }

    return payload_data;
  } finally {
    await redisServerStop();
  }
};

if (import.meta.main) {
  await benchRun();
}

export { OP_NAME_MAP, CMP_NAME_MAP, envDetect, cpuModelDetect };
export default benchRun;
