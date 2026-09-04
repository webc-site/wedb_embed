import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

export const loadCppAlpResult = async () => {
  const alpDir = process.env.ALP_DIR || resolve(import.meta.dirname, "../../../../ALP");
  const csvPath = resolve(alpDir, "benchmarks/your_own_dataset_result.csv");
  const file = Bun.file(csvPath);
  if (!(await file.exists())) return null;

  const content = await file.text();
  const lines = content.trim().split("\n");
  if (lines.length <= 1) return null;

  const datasets = [];
  let totalRaw = 0;
  let totalCompressed = 0;
  let sumDec = 0;
  let sumEnc = 0;
  let sumEncKern = 0;

  for (let i = 1; i < lines.length; i++) {
    const parts = lines[i].split(",");
    if (parts.length < 10) continue;
    const name = parts[1];
    const bitsPerVal = parseFloat(parts[3]);
    const encSampledGbS = parseFloat(parts[5]);
    const encKernelGbS = parseFloat(parts[7]);
    const decGbS = parseFloat(parts[9]);

    const rawBytes = 1024 * 8;
    const compBytes = Math.round((1024 * bitsPerVal) / 8);
    const ratio = rawBytes / compBytes;

    totalRaw += rawBytes;
    totalCompressed += compBytes;
    sumDec += decGbS;
    sumEnc += encSampledGbS;
    sumEncKern += encKernelGbS;

    datasets.push({
      name,
      raw_bytes: rawBytes,
      compressed_bytes: compBytes,
      ratio,
      bits_per_val: bitsPerVal,
      enc_gb_s: encSampledGbS,
      enc_sampled_gb_s: encSampledGbS,
      enc_kernel_gb_s: encKernelGbS,
      dec_gb_s: decGbS,
    });
  }

  const n = datasets.length;
  const sensorSc = datasets.find((d) => d.name === "scene_sensor");
  const rampSc = datasets.find((d) => d.name === "scene_ramp");
  const steadySc = datasets.find((d) => d.name === "scene_steady");

  const resultObj = {
    algorithm: "cpp_alp",
    display_name: "C++ ALP",
    category: "specialized_float",
    paper_31: {
      total_raw_bytes: totalRaw,
      total_compressed_bytes: totalCompressed,
      ratio: totalRaw / totalCompressed,
      bits_per_val: (totalCompressed * 8) / (totalRaw / 8),
      avg_enc_gb_s: sumEnc / n,
      avg_enc_kernel_gb_s: sumEncKern / n,
      avg_dec_gb_s: sumDec / n,
      datasets,
    },
    micro_benchmarks: {
      sensor_1024: sensorSc
        ? {
            raw_bytes: 8192,
            compressed_bytes: sensorSc.compressed_bytes,
            ratio: sensorSc.ratio,
            bits_per_val: sensorSc.bits_per_val,
            enc_gb_s: sensorSc.enc_gb_s,
            enc_kernel_gb_s: sensorSc.enc_kernel_gb_s,
            dec_gb_s: sensorSc.dec_gb_s,
          }
        : null,
      ramp_1024: rampSc
        ? {
            raw_bytes: 8192,
            compressed_bytes: rampSc.compressed_bytes,
            ratio: rampSc.ratio,
            bits_per_val: rampSc.bits_per_val,
            enc_gb_s: rampSc.enc_gb_s,
            enc_kernel_gb_s: rampSc.enc_kernel_gb_s,
            dec_gb_s: rampSc.dec_gb_s,
          }
        : null,
      constant_1024: steadySc
        ? {
            raw_bytes: 8192,
            compressed_bytes: steadySc.compressed_bytes,
            ratio: steadySc.ratio,
            bits_per_val: steadySc.bits_per_val,
            enc_gb_s: steadySc.enc_gb_s,
            enc_kernel_gb_s: steadySc.enc_kernel_gb_s,
            dec_gb_s: steadySc.dec_gb_s,
          }
        : null,
    },
  };

  // 自动将 C++ 官方 Fork 跑出的真实结果同步固化到 benches/json/cpp_alp.json
  const jsonDst = resolve(import.meta.dir, "../json/cpp_alp.json");
  await Bun.write(jsonDst, JSON.stringify(resultObj, null, 2) + "\n");

  return resultObj;
};

export default loadCppAlpResult;
