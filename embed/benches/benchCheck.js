#!/usr/bin/env -S bun

import { resolve } from "node:path";
import { OP_NAME_MAP } from "./benchRun.js";

const ROOT_DIR = resolve(import.meta.dirname, "../.."),
  BENCH_RS = resolve(ROOT_DIR, "embed/benches/bench.rs"),
  REDIS_CMP_RS = resolve(ROOT_DIR, "embed/benches/redis_vs_wedb.rs");

const REQUIRED_MODULE_LI = [
  "str",
  "hash",
  "list",
  "set",
  "zset",
  "bitmap",
  "json",
  "bloom",
  "timeseries",
  "geo",
  "hll",
  "tdigest",
  "sortedint",
  "stream",
  "db",
  "search",
  "vector",
];

const REQUIRED_CMP_CMD_LI = [
  "str_set",
  "str_get",
  "str_mset",
  "str_mget",
  "str_incrby",
  "str_decrby",
  "str_append",
  "str_strlen",
  "str_getdel",
  "str_getrange",
  "str_setrange",
  "hash_hset",
  "hash_hget",
  "hash_hmget",
  "hash_hexists",
  "hash_hlen",
  "hash_hdel",
  "hash_hgetall",
  "hash_hkeys",
  "hash_hvals",
  "hash_hincrby",
  "list_lpush",
  "list_rpush",
  "list_lpop",
  "list_rpop",
  "list_llen",
  "list_lrange",
  "list_lindex",
  "list_lset",
  "list_lrem",
  "list_ltrim",
  "set_sadd",
  "set_srem",
  "set_sismember",
  "set_scard",
  "set_smembers",
  "set_spop",
  "set_srandmember",
  "zset_zadd",
  "zset_zscore",
  "zset_zrange",
  "zset_zcard",
  "zset_zcount",
  "zset_zincrby",
  "zset_zrank",
  "zset_zrevrange",
  "zset_zpopmin",
  "zset_zrem",
  "bitmap_setbit",
  "bitmap_getbit",
  "bitmap_bitcount",
  "bitmap_bitpos",
  "hll_pfadd",
  "hll_pfcount",
  "geo_geoadd",
  "geo_geodist",
  "geo_geopos",
  "geo_geohash",
  "stream_xadd",
  "stream_xlen",
  "stream_xrange",
  "stream_xread",
  "stream_xdel",
  "db_del",
  "db_exists",
  "db_expire",
  "db_ttl",
  "json_set",
  "json_get",
  "json_del",
  "json_numincrby",
  "json_arrlen",
  "json_type",
  "bloom_bf_add",
  "bloom_bf_exists",
  "bloom_bf_info",
  "bloom_cf_add",
  "bloom_cf_exists",
  "bloom_cf_del",
  "tdigest_add",
  "tdigest_quantile",
  "tdigest_byrank",
  "tdigest_cdf",
  "timeseries_ts_add",
  "timeseries_ts_get",
  "timeseries_ts_range",
  "timeseries_ts_incrby",
  "search_ft_search",
  "search_tag",
  "vector_knn",
];

const benchCoverageVerify = async () => {
  const bench_file = Bun.file(BENCH_RS),
    redis_cmp_file = Bun.file(REDIS_CMP_RS);

  if (!(await bench_file.exists())) {
    console.error("未找到基准测试文件: " + BENCH_RS);
    process.exit(1);
  }

  if (!(await redis_cmp_file.exists())) {
    console.error("未找到 Redis 对比测试文件: " + REDIS_CMP_RS);
    process.exit(1);
  }

  const bench_code = await bench_file.text(),
    redis_cmp_code = await redis_cmp_file.text(),
    bench_name_li = [
      ...bench_code.matchAll(/fn\s+(bench_[a-zA-Z0-9_]+)\s*\(/g),
    ].map((m) => m[1]);

  console.log("==> 基准测试覆盖率检查开始...");
  console.log("==> 已发现 " + bench_name_li.length + " 项独立命令基准测试");

  // 2. 检查是否有混合命令测试（拒绝复合命名）
  const mixed_cmd_li = bench_name_li.filter(
    (name) => name.includes("_and_") || name.includes("_plus_"),
  );
  if (mixed_cmd_li.length > 0) {
    console.error("发现混合测试命令（要求单一命令独立测试）:", mixed_cmd_li);
    process.exit(1);
  }

  // 3. 检查 15 大核心模块覆盖情况
  const module_map = Object.fromEntries(
    REQUIRED_MODULE_LI.map((mod) => [
      mod,
      bench_name_li.filter(
        (name) =>
          name.startsWith("bench_" + mod + "_") || name === "bench_" + mod,
      ),
    ]),
  );

  let missing_module_cnt = 0;
  REQUIRED_MODULE_LI.forEach((mod) => {
    const mod_bench_li = module_map[mod];
    if (!mod_bench_li || mod_bench_li.length === 0) {
      console.error("模块缺少基准测试覆盖: " + mod);
      missing_module_cnt += 1;
    } else {
      console.log(
        "  - 模块 [" +
          mod.padEnd(10) +
          "] : 覆盖 " +
          mod_bench_li.length +
          " 个独立命令 (" +
          mod_bench_li
            .map((n) => n.replace("bench_" + mod + "_", ""))
            .join(", ") +
          ")",
      );
    }
  });

  if (missing_module_cnt > 0) {
    console.error("存在 " + missing_module_cnt + " 个未覆盖模块！");
    process.exit(1);
  }

  // 4. 检查 WeDb vs Redis 对比基准测试覆盖情况
  console.log("==> 检查 WeDb vs Redis 对比测试覆盖情况...");
  let missing_cmp_cnt = 0;
  REQUIRED_CMP_CMD_LI.forEach((cmd) => {
    const wedb_fn = "bench_cmp_" + cmd + "_wedb",
      redis_fn = "bench_cmp_" + cmd + "_redis";

    if (
      !redis_cmp_code.includes("fn " + wedb_fn) ||
      !redis_cmp_code.includes("fn " + redis_fn)
    ) {
      console.error(
        "Redis 对比测试缺少命令: " +
          cmd +
          " (" +
          wedb_fn +
          " / " +
          redis_fn +
          ")",
      );
      missing_cmp_cnt += 1;
    }
  });

  if (missing_cmp_cnt > 0) {
    console.error("缺少 " + missing_cmp_cnt + " 项 Redis 对比测试命令！");
    process.exit(1);
  }
  console.log(
    "  - Redis 对比测试: 覆盖全部 " +
      REQUIRED_CMP_CMD_LI.length +
      " 项核心代表性命令",
  );

  // 5. 检查 OP_NAME_MAP 命令名映射配置完整性
  let missing_dict_cnt = 0;
  bench_name_li.forEach((name) => {
    if (!OP_NAME_MAP[name]) {
      console.warn("OP_NAME_MAP 缺少命令名称映射: " + name);
      missing_dict_cnt += 1;
    }
  });

  if (missing_dict_cnt > 0) {
    console.error("缺少 " + missing_dict_cnt + " 项命令的名称映射！");
    process.exit(1);
  }

  console.log(
    "==> 100% 命令全覆盖验证通过！无混合测试，全部 15 大模块与 15 项 Redis 对比测试均已覆盖。",
  );
};

if (import.meta.main) {
  await benchCoverageVerify();
}

export default benchCoverageVerify;
