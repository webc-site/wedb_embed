import { readdir } from "node:fs/promises";
import { join, resolve } from "node:path";

const BENCHES_DIR = resolve(import.meta.dirname, "..");
const JSON_DIR = join(BENCHES_DIR, "json");

export const loadBenchData = async () => {
  const files = await readdir(JSON_DIR);
  const jsonFiles = files.filter((f) => f.endsWith(".json"));

  const algorithms = [];
  for (const f of jsonFiles) {
    const filePath = join(JSON_DIR, f);
    const content = await Bun.file(filePath).json();
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
    decSpeedupVsPco: (fastalp.paper_31.avg_dec_gb_s / pco.paper_31.avg_dec_gb_s).toFixed(1),
    decSpeedupVsZstd: (fastalp.paper_31.avg_dec_gb_s / zstd.paper_31.avg_dec_gb_s).toFixed(1),
    decSpeedupVsGorilla: (fastalp.paper_31.avg_dec_gb_s / gorilla.paper_31.avg_dec_gb_s).toFixed(1),
    decSpeedupVsChimp: (fastalp.paper_31.avg_dec_gb_s / chimp.paper_31.avg_dec_gb_s).toFixed(1),
    decSpeedupVsCpp: (fastalp.micro_benchmarks.constant_1024.dec_gb_s / cppAlp.micro_benchmarks.constant_1024.dec_gb_s).toFixed(1),
    rampRatioFastalp: fastalp.micro_benchmarks.ramp_1024.ratio.toFixed(1),
    rampRatioVsPco: (fastalp.micro_benchmarks.ramp_1024.ratio / pco.micro_benchmarks.ramp_1024.ratio).toFixed(1),
    rampRatioVsZstd: (fastalp.micro_benchmarks.ramp_1024.ratio / zstd.micro_benchmarks.ramp_1024.ratio).toFixed(1),
    rampRatioVsCpp: (fastalp.micro_benchmarks.ramp_1024.ratio / cppAlp.micro_benchmarks.ramp_1024.ratio).toFixed(1),
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
