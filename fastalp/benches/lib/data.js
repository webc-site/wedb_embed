import { readdir } from "node:fs/promises";
import { join, resolve } from "node:path";

const BENCHES_DIR = resolve(import.meta.dirname, "..");
const JSON_DIR = join(BENCHES_DIR, "json");

import { loadCppAlpResult } from "./cpp_alp_loader.js";

import os from "node:os";
import { execSync } from "node:child_process";

export const getSystemEnv = (isZh = true) => {
  let cpuModel = "";
  try {
    cpuModel = execSync("sysctl -n machdep.cpu.brand_string 2>/dev/null || sysctl -n hw.model 2>/dev/null", { encoding: "utf8" }).trim();
  } catch {}
  if (!cpuModel) {
    cpuModel = os.cpus()[0]?.model || "Apple Silicon";
  }
  const coreCount = os.cpus().length || 12;

  let osDesc = "macOS";
  try {
    const osVer = execSync("sw_vers -productVersion 2>/dev/null", { encoding: "utf8" }).trim();
    if (osVer) osDesc = `macOS ${osVer}`;
  } catch {
    osDesc = `${os.type()} ${os.release()}`;
  }

  let rustVer = "Rust";
  try {
    const r = execSync("rustc --version 2>/dev/null", { encoding: "utf8" }).trim().split(" ")[1];
    if (r) rustVer = `Rust ${r}`;
  } catch {}

  const cpu = isZh 
    ? `芯片: ${cpuModel} (${coreCount} 核)` 
    : `CPU: ${cpuModel} (${coreCount} Cores)`;

  const toolchain = isZh
    ? `环境: ${osDesc} ｜ 工具链: ${rustVer} / Clang (-O3)`
    : `OS: ${osDesc} ｜ Toolchain: ${rustVer} / Clang (-O3)`;

  return { cpu, toolchain, cpuModel };
};

export const geomean = (arr) => {
  if (!arr || arr.length === 0) return 0;
  const valid = arr.filter((v) => typeof v === "number" && !isNaN(v) && v > 0);
  if (valid.length === 0) return 0;
  const sumLn = valid.reduce((acc, v) => acc + Math.log(v), 0);
  return Math.exp(sumLn / valid.length);
};

export const datasetMeta = {
  // 1. IoT & Environmental Sensors
  "air_sensor_f": { zh: "空气环境传感", en: "Air Sensor IoT", domain: "IoT", domainZh: "环境传感", domainEn: "IoT Sensor" },
  "neon_air_pressure": { zh: "生态大气气压", en: "Atmospheric Pressure", domain: "IoT", domainZh: "生态台网", domainEn: "NEON Geo" },
  "neon_bio_temp_c": { zh: "生态土壤温度", en: "Ecology Soil Temp", domain: "IoT", domainZh: "生态台网", domainEn: "NEON Geo" },
  "neon_dew_point_temp": { zh: "生态露点温度", en: "Ecology Dew Point", domain: "IoT", domainZh: "生态台网", domainEn: "NEON Geo" },
  "neon_pm10_dust": { zh: "生态粉尘微粒", en: "Ecology Aerosol Dust", domain: "IoT", domainZh: "生态台网", domainEn: "NEON Geo" },
  "neon_wind_dir": { zh: "生态连续风向", en: "Ecology Wind Dir", domain: "IoT", domainZh: "生态台网", domainEn: "NEON Geo" },
  "scene_sensor": { zh: "十进制物理传感", en: "Industrial IoT Sensor", domain: "IoT", domainZh: "工业微标", domainEn: "Industrial" },

  // 2. Meteorology, Hydrology & Geospatial
  "arade4": { zh: "水文河流径流", en: "River Runoff Flow", domain: "气象", domainZh: "水文观测", domainEn: "Hydrology" },
  "basel_temp_f": { zh: "百年欧洲气温", en: "Basel Climate Temp", domain: "气象", domainZh: "气象气候", domainEn: "Climate" },
  "basel_wind_f": { zh: "连续风速监测", en: "Basel Wind Speed", domain: "气象", domainZh: "气象气候", domainEn: "Climate" },
  "city_temperature_f": { zh: "全球城市气温", en: "Urban Climate Temp", domain: "气象", domainZh: "气象气候", domainEn: "Climate" },
  "bird_migration_f": { zh: "候鸟高程轨迹", en: "Avian Telemetry GPS", domain: "地理", domainZh: "空间遥测", domainEn: "Telemetry" },
  "nyc29": { zh: "出租运营轨迹", en: "NYC Taxi Trajectory", domain: "地理", domainZh: "城市交通", domainEn: "Mobility" },
  "poi_lat": { zh: "高精测绘纬度", en: "Geospatial POI Lat", domain: "地理", domainZh: "高精测绘", domainEn: "Geospatial" },
  "poi_lon": { zh: "高精测绘经度", en: "Geospatial POI Lon", domain: "地理", domainZh: "高精测绘", domainEn: "Geospatial" },

  // 3. Quantitative Finance & Blockchain
  "stocks_usa_c": { zh: "美股纳指高频", en: "Nasdaq Equities HFT", domain: "金融", domainZh: "证券撮合", domainEn: "Securities" },
  "stocks_uk": { zh: "英股伦敦高频", en: "FTSE Equities HFT", domain: "金融", domainZh: "证券撮合", domainEn: "Securities" },
  "stocks_de": { zh: "德股法兰克福", en: "DAX Equities HFT", domain: "金融", domainZh: "证券撮合", domainEn: "Securities" },
  "bitcoin_f": { zh: "加密现货成交", en: "Crypto Trade Quotes", domain: "金融", domainZh: "加密资产", domainEn: "Crypto" },
  "bitcoin_transactions_f": { zh: "链上交易流水", en: "Blockchain Tx Vol", domain: "金融", domainZh: "区块链", domainEn: "Blockchain" },
  "food_prices": { zh: "粮农物价指数", en: "FAO Food Price Index", domain: "金融", domainZh: "宏观物价", domainEn: "Economics" },
  "scene_finance": { zh: "量化金融行情", en: "Quantitative Quotes", domain: "金融", domainZh: "量化微标", domainEn: "Fintech" },

  // 4. Healthcare & Public Health
  "cms1": { zh: "门诊医疗结算", en: "Medicare Claims", domain: "医疗", domainZh: "医保结算", domainEn: "Healthcare" },
  "cms25": { zh: "住院诊断总额", en: "Inpatient Charges", domain: "医疗", domainZh: "医保结算", domainEn: "Healthcare" },
  "cms9": { zh: "药品处方定价", en: "Pharma Drug Prices", domain: "医疗", domainZh: "医疗药品", domainEn: "Pharma" },
  "medicare1": { zh: "门诊理赔流水", en: "Outpatient Billing", domain: "医疗", domainZh: "公共卫生", domainEn: "Public Health" },
  "medicare9": { zh: "专科医疗津贴", en: "Specialty Grants", domain: "医疗", domainZh: "公共卫生", domainEn: "Public Health" },

  // 5. Government Fiscal & Demographics
  "gov10": { zh: "财政公共支出", en: "Fiscal Expenditure", domain: "政务", domainZh: "财政统计", domainEn: "Fiscal" },
  "gov26": { zh: "人口普查常数", en: "Census Population", domain: "政务", domainZh: "人口普查", domainEn: "Demographics" },
  "gov30": { zh: "宏观运行指标", en: "Macroeconomic Index", domain: "政务", domainZh: "宏观指标", domainEn: "Macro" },
  "gov31": { zh: "财政转移支付", en: "Fiscal Transfer", domain: "政务", domainZh: "财政统计", domainEn: "Fiscal" },
  "gov40": { zh: "市政管网测绘", en: "Infrastructure Survey", domain: "政务", domainZh: "市政设施", domainEn: "Civic" },

  // 6. Industrial Waveforms & Hardware
  "scene_ramp": { zh: "单调趋势波形", en: "Monotonic Ramp Wave", domain: "工业", domainZh: "波形趋势", domainEn: "Waveform" },
  "scene_steady": { zh: "稳态常数监控", en: "Steady Heartbeat", domain: "工业", domainZh: "稳态监控", domainEn: "Monitoring" },
  "ssd_hdd_benchmarks_f": { zh: "存储设备吞吐", en: "Storage I/O Speed", domain: "工业", domainZh: "硬件指标", domainEn: "Hardware" },
  "scene_geo": { zh: "高精测绘切片", en: "Geospatial Benchmark", domain: "地理", domainZh: "空间遥测", domainEn: "Geospatial" },
  "scene_macro": { zh: "宏观普查切片", en: "Demographics Benchmark", domain: "政务", domainZh: "宏观指标", domainEn: "Macro" },
};

export const scenarioDatasetMap = {
  scene_sensor: [
    "neon_pm10_dust", "neon_air_pressure", "neon_bio_temp_c", "neon_dew_point_temp",
    "neon_wind_dir", "air_sensor_f", "scene_sensor", "basel_temp_f", "basel_wind_f",
    "city_temperature_f", "arade4"
  ],
  scene_finance: [
    "stocks_usa_c", "stocks_de", "stocks_uk", "bitcoin_f",
    "bitcoin_transactions_f", "food_prices", "scene_finance"
  ],
  scene_geo: [
    "bird_migration_f", "nyc29", "poi_lat", "poi_lon", "scene_geo"
  ],
  scene_health: [
    "cms1", "cms25", "cms9", "medicare1", "medicare9"
  ],
  scene_macro: [
    "gov10", "gov26", "gov30", "gov31", "gov40", "scene_macro"
  ],
  scene_waveform: [
    "scene_ramp", "scene_steady", "ssd_hdd_benchmarks_f"
  ]
};

export const computeScenarioMetrics = (algo, sceneKey) => {
  if (!algo) throw new Error(`Algorithm object missing for scenario ${sceneKey}`);
  const ds = algo.paper_31?.datasets;
  if (!ds || ds.length === 0) throw new Error(`Datasets missing for algorithm ${algo.algorithm}`);

  const targetNames = scenarioDatasetMap[sceneKey];
  if (targetNames && targetNames.length > 0) {
    const list = ds.filter((d) => targetNames.includes(d.name));
    if (list.length > 0) {
      const avgDec = list.reduce((acc, d) => acc + d.dec_gb_s, 0) / list.length;
      const avgEnc = list.reduce((acc, d) => acc + d.enc_gb_s, 0) / list.length;
      const totalRaw = list.reduce((acc, d) => acc + d.raw_bytes, 0);
      const totalComp = list.reduce((acc, d) => acc + d.compressed_bytes, 0);
      return {
        dec_gb_s: avgDec,
        enc_gb_s: avgEnc,
        ratio: totalRaw / totalComp,
        count: list.length,
        totalPts: list.length * 1024
      };
    }
  }

  const exact = ds.find((d) => d.name === sceneKey);
  if (exact) {
    return { dec_gb_s: exact.dec_gb_s, enc_gb_s: exact.enc_gb_s, ratio: exact.ratio, count: 1, totalPts: 1024 };
  }

  throw new Error(`Scenario ${sceneKey} not found in datasets for algorithm ${algo.algorithm}`);
};

export const loadBenchData = async () => {
  const files = await readdir(JSON_DIR);
  const jsonFiles = files.filter((f) => f.endsWith(".json"));

  const algorithms = [];
  const measuredCpp = await loadCppAlpResult();
  const scenarioKeys = ["scene_sensor", "scene_finance", "scene_geo", "scene_health", "scene_macro", "scene_waveform"];

  for (const f of jsonFiles) {
    const filePath = join(JSON_DIR, f);
    const content = await Bun.file(filePath).json();

    // If measured C++ ALP CSV exists, use real machine measured numbers
    if (content.algorithm === "cpp_alp" && measuredCpp) {
      content.paper_31 = measuredCpp.paper_31;
    }

    const baseDatasets = content.paper_31.datasets;
    const allDatasets = [...baseDatasets];

    // 1. Calculate Comprehensive Geometric Mean strictly across the real benchmark datasets
    const decs = baseDatasets.map((d) => d.dec_gb_s);
    const encs = baseDatasets.map((d) => d.enc_gb_s);
    const encKerns = baseDatasets.map((d) => d.enc_kernel_gb_s ?? d.enc_gb_s);
    const ratios = baseDatasets.map((d) => d.ratio);
    content.paper_31.geomean_dec_gb_s = geomean(decs);
    content.paper_31.geomean_enc_gb_s = geomean(encs);
    content.paper_31.geomean_enc_kernel_gb_s = geomean(encKerns);
    content.paper_31.geomean_ratio = geomean(ratios);

    const avg = (arr) => arr.reduce((a, b) => a + b, 0) / arr.length;
    content.paper_31.avg_dec_gb_s = avg(decs);
    content.paper_31.avg_enc_gb_s = avg(encs);
    content.paper_31.avg_enc_kernel_gb_s = avg(encKerns);

    // 2. Compute dynamic scenario metrics for Section 2 scenario cards
    for (const scKey of scenarioKeys) {
      if (!allDatasets.some((d) => d.name === scKey)) {
        const m = computeScenarioMetrics(content, scKey);
        allDatasets.push({
          name: scKey,
          dec_gb_s: m.dec_gb_s,
          enc_gb_s: m.enc_gb_s,
          ratio: m.ratio
        });
      }
    }
    content.paper_31.datasets = allDatasets;

    algorithms.push(content);
  }

  // Define sort order / preferred display order
  const priority = [
    "fastalp",
    "cpp_alp",
    "pco",
    "zstd",
    "lz4",
    "snappy",
    "chimp128",
    "gorilla",
  ];

  algorithms.sort((a, b) => {
    const ia = priority.indexOf(a.algorithm);
    const ib = priority.indexOf(b.algorithm);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
  });

  const fastalp = algorithms.find((a) => a.algorithm === "fastalp");
  const cppAlp = algorithms.find((a) => a.algorithm === "cpp_alp");
  const pco = algorithms.find((a) => a.algorithm === "pco");
  const zstd = algorithms.find((a) => a.algorithm === "zstd");
  const lz4 = algorithms.find((a) => a.algorithm === "lz4");
  const gorilla = algorithms.find((a) => a.algorithm === "gorilla");
  const chimp = algorithms.find((a) => a.algorithm === "chimp128");

  // Summary statistics
  const summary = {
    fastalp,
    cppAlp,
    pco,
    zstd,
    lz4,
    gorilla,
    chimp,
    // Comparisons
    encKernelSpeedupVsCpp: (
      fastalp.paper_31.geomean_enc_kernel_gb_s / cppAlp.paper_31.geomean_enc_kernel_gb_s
    ).toFixed(2),
    encSampledSpeedupVsCpp: (
      fastalp.paper_31.geomean_enc_gb_s / cppAlp.paper_31.geomean_enc_gb_s
    ).toFixed(1),
    decSpeedupVsCpp: (
      fastalp.paper_31.geomean_dec_gb_s / cppAlp.paper_31.geomean_dec_gb_s
    ).toFixed(2),
    decSpeedupVsPco: (fastalp.paper_31.geomean_dec_gb_s / pco.paper_31.geomean_dec_gb_s).toFixed(1),
    decSpeedupVsZstd: (fastalp.paper_31.geomean_dec_gb_s / zstd.paper_31.geomean_dec_gb_s).toFixed(1),
    decSpeedupVsGorilla: (fastalp.paper_31.geomean_dec_gb_s / gorilla.paper_31.geomean_dec_gb_s).toFixed(1),
    decSpeedupVsChimp: (fastalp.paper_31.geomean_dec_gb_s / chimp.paper_31.geomean_dec_gb_s).toFixed(1),
    rampRatioFastalp: fastalp.paper_31.datasets.find((d) => d.name === "scene_ramp").ratio.toFixed(1),
    spaceSavedVsCppPct: (
      ((cppAlp.paper_31.total_compressed_bytes - fastalp.paper_31.total_compressed_bytes) /
        cppAlp.paper_31.total_compressed_bytes) *
      100
    ).toFixed(2),
  };

  return {
    algorithms,
    summary,
  };
};

export default loadBenchData;
