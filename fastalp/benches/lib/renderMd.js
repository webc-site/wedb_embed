import { computeScenarioMetrics } from "./data.js";

export const renderMd = (benchData, lang = "zh") => {
  const { algorithms } = benchData;
  const isZh = lang === "zh";

  const fastalp = algorithms.find((a) => a.algorithm === "fastalp") || algorithms[0];
  const cppAlp = algorithms.find((a) => a.algorithm === "cpp_alp") || algorithms[1];
  const pco = algorithms.find((a) => a.algorithm === "pco");
  const zstd = algorithms.find((a) => a.algorithm === "zstd");
  const lz4 = algorithms.find((a) => a.algorithm === "lz4");
  const snappy = algorithms.find((a) => a.algorithm === "snappy");
  const chimp = algorithms.find((a) => a.algorithm === "chimp128");
  const gorilla = algorithms.find((a) => a.algorithm === "gorilla");

  // Metrics for fastalp dynamically read from JSON
  const faDecGeo = fastalp.paper_31.geomean_dec_gb_s;
  const faDecAvg = fastalp.paper_31.avg_dec_gb_s;
  const faEncGeo = fastalp.paper_31.geomean_enc_gb_s;
  const faEncAvg = fastalp.paper_31.avg_enc_gb_s;
  const faKernGeo = fastalp.paper_31.geomean_enc_kernel_gb_s;
  const faKernAvg = fastalp.paper_31.avg_enc_kernel_gb_s;
  const faRatioGeo = fastalp.paper_31.geomean_ratio;
  const faRatioTotal = fastalp.paper_31.total_raw_bytes / fastalp.paper_31.total_compressed_bytes;

  // Metrics for C++ ALP dynamically read from JSON
  const cppDecGeo = cppAlp.paper_31.geomean_dec_gb_s;
  const cppDecAvg = cppAlp.paper_31.avg_dec_gb_s;
  const cppEncGeo = cppAlp.paper_31.geomean_enc_gb_s;
  const cppEncAvg = cppAlp.paper_31.avg_enc_gb_s;
  const cppKernGeo = cppAlp.paper_31.geomean_enc_kernel_gb_s;
  const cppKernAvg = cppAlp.paper_31.avg_enc_kernel_gb_s;
  const cppRatioGeo = cppAlp.paper_31.geomean_ratio;
  const cppRatioTotal = cppAlp.paper_31.total_raw_bytes / cppAlp.paper_31.total_compressed_bytes;

  // Speedup calculations
  const decSpeedupGeo = (faDecGeo / cppDecGeo).toFixed(2);
  const decSpeedupAvg = (faDecAvg / cppDecAvg).toFixed(2);
  const encSpeedupGeo = (faEncGeo / cppEncGeo).toFixed(2);
  const encSpeedupAvg = (faEncAvg / cppEncAvg).toFixed(2);
  const kernSpeedupGeo = (faKernGeo / cppKernGeo).toFixed(2);
  const kernSpeedupAvg = (faKernAvg / cppKernAvg).toFixed(2);
  const ratioLeadGeo = (((faRatioGeo - cppRatioGeo) / cppRatioGeo) * 100).toFixed(0);
  const ratioLeadTotal = (((faRatioTotal - cppRatioTotal) / cppRatioTotal) * 100).toFixed(0);

  // Table 1 Rows
  const renderTable1Row = (algo, isLeader = false, isBase = false) => {
    const isSpecialized = algo.category === "specialized_float";
    const cat = isZh
      ? (isSpecialized ? "浮点专用" : "通用字节")
      : (isSpecialized ? "Specialized Float" : "General Byte");

    const name = isLeader
      ? `**${algo.display_name}**`
      : isBase
      ? (isZh ? `**C++ ALP** (原版实现)` : `**C++ ALP** (Paper Reference)`)
      : algo.display_name;

    const dec = algo.paper_31.geomean_dec_gb_s;
    const enc = algo.paper_31.geomean_enc_gb_s;
    const kern = algo.paper_31.geomean_enc_kernel_gb_s;
    const ratio = algo.paper_31.geomean_ratio;

    let decVs = "";
    if (isLeader) {
      decVs = isZh ? `**较 C++ 快 ${decSpeedupGeo}x**` : `**${decSpeedupGeo}x vs C++**`;
    } else if (isBase) {
      decVs = isZh ? "基准 (1.0x)" : "Baseline (1.0x)";
    } else {
      const rel = (dec / cppDecGeo).toFixed(2);
      const timesSlower = (cppDecGeo / dec).toFixed(1);
      decVs = isZh ? `${rel}x (慢 ${timesSlower}x)` : `${rel}x (${timesSlower}x slower)`;
    }

    let encStr = "";
    if (isLeader) {
      encStr = isZh
        ? `**${enc.toFixed(1)} GB/s (快 ${encSpeedupGeo}x)**`
        : `**${enc.toFixed(1)} GB/s (${encSpeedupGeo}x faster)**`;
    } else if (isBase) {
      encStr = `**${enc.toFixed(1)} GB/s**`;
    } else {
      encStr = `**${enc.toFixed(1)} GB/s**`;
    }

    let kernStr = "—";
    let kernVs = "—";
    if (isLeader) {
      kernStr = `**${kern.toFixed(1)} GB/s**`;
      kernVs = isZh ? `**较 C++ 快 ${kernSpeedupGeo}x**` : `**${kernSpeedupGeo}x vs C++**`;
    } else if (isBase) {
      kernStr = `**${kern.toFixed(1)} GB/s**`;
      kernVs = isZh ? "基准 (1.0x)" : "Baseline (1.0x)";
    }

    const decFmt = isLeader ? `**${dec.toFixed(1)} GB/s**` : `**${dec.toFixed(1)} GB/s**`;
    const ratioFmt = isLeader ? `**${ratio.toFixed(2)}x**` : `**${ratio.toFixed(2)}x**`;

    return `| ${name} | ${cat} | ${decFmt} | ${decVs} | ${encStr} | ${kernStr} | ${kernVs} | ${ratioFmt} |`;
  };

  const table1Rows = [
    renderTable1Row(fastalp, true, false),
    renderTable1Row(cppAlp, false, true),
    pco ? renderTable1Row(pco) : null,
    zstd ? renderTable1Row(zstd) : null,
    lz4 ? renderTable1Row(lz4) : null,
    snappy ? renderTable1Row(snappy) : null,
    chimp ? renderTable1Row(chimp) : null,
    gorilla ? renderTable1Row(gorilla) : null,
  ].filter(Boolean).join("\n");

  // Scenario Table Rows
  const scenarioMetaZh = {
    scene_sensor: { name: "十进制环境与气象水文传感", scale: "11 组 (11,264 点)", otherName: "LZ4", otherAlgo: lz4 },
    scene_finance: { name: "高频量化金融交易与资产行情", scale: "7 组 (7,168 点)", otherName: "Snappy", otherAlgo: snappy },
    scene_geo: { name: "地理空间高精测绘与轨迹跟踪", scale: "5 组 (5,120 点)", otherName: "Snappy", otherAlgo: snappy },
    scene_health: { name: "医疗社保理赔与公共卫生处方", scale: "5 组 (5,120 点)", otherName: "Zstd", otherAlgo: zstd },
    scene_macro: { name: "公共政务民生与宏观统计普查", scale: "6 组 (6,144 点)", otherName: "Zstd", otherAlgo: zstd },
    scene_waveform: { name: "物理单调波形、设备指标与稳态流", scale: "3 组 (3,072 点)", otherName: "Zstd", otherAlgo: zstd },
  };

  const scenarioMetaEn = {
    scene_sensor: { name: "Decimal Environmental & Hydrology IoT", scale: "11 sets (11,264 pts)", otherName: "LZ4", otherAlgo: lz4 },
    scene_finance: { name: "Quantitative Trading & Asset Quotes", scale: "7 sets (7,168 pts)", otherName: "Snappy", otherAlgo: snappy },
    scene_geo: { name: "Geospatial & GPS Trajectory Tracking", scale: "5 sets (5,120 pts)", otherName: "Snappy", otherAlgo: snappy },
    scene_health: { name: "Healthcare Claims & Pharma Pricing", scale: "5 sets (5,120 pts)", otherName: "Zstd", otherAlgo: zstd },
    scene_macro: { name: "Public Demographics & Civic Economics", scale: "6 sets (6,144 pts)", otherName: "Zstd", otherAlgo: zstd },
    scene_waveform: { name: "Monotonic Ramp, Storage & Steady Waves", scale: "3 sets (3,072 pts)", otherName: "Zstd", otherAlgo: zstd },
  };

  const scenarioMeta = isZh ? scenarioMetaZh : scenarioMetaEn;

  const renderScenarioRow = (key) => {
    const meta = scenarioMeta[key];
    const faM = computeScenarioMetrics(fastalp, key);
    const cppM = computeScenarioMetrics(cppAlp, key);
    const pcoM = computeScenarioMetrics(pco, key);
    const othM = computeScenarioMetrics(meta.otherAlgo, key);

    const faCell = `**${faM.dec_gb_s.toFixed(1)} GB/s**<br>**${faM.enc_gb_s.toFixed(1)} GB/s**<br>**${faM.ratio.toFixed(2)}x**`;
    const cppCell = `${cppM.dec_gb_s.toFixed(1)} GB/s<br>${cppM.enc_gb_s.toFixed(1)} GB/s<br>${cppM.ratio.toFixed(2)}x`;
    const pcoCell = `${pcoM.dec_gb_s.toFixed(2)} GB/s<br>${pcoM.enc_gb_s.toFixed(1)} GB/s<br>${pcoM.ratio.toFixed(2)}x`;
    const othCell = `${meta.otherName}:<br>${othM.dec_gb_s.toFixed(1)} GB/s<br>${othM.enc_gb_s.toFixed(1)} GB/s<br>${othM.ratio.toFixed(2)}x`;

    return `| **${meta.name}** | ${meta.scale} | ${faCell} | ${cppCell} | ${pcoCell} | ${othCell} |`;
  };

  const scenarioRows = Object.keys(scenarioMeta).map(renderScenarioRow).join("\n");

  if (isZh) {
    return `## 性能评测与多算法对比

### 测试环境与编译配置

所有基准测试均在同一物理机上执行并进行同机对比测试：

- **处理器**: Apple M2 Max (12 核心：8 性能核 @ 3.68 GHz + 4 能效核 @ 2.42 GHz, ARMv8.6-A NEON 指令集)<br>
- **操作系统**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust 编译工具链**: \`rustc 1.98.0 / nightly\` (配置：\`opt-level = 3\`, \`lto = "fat"\`, \`codegen-units = 1\`)<br>
- **C++ 编译工具链**: Homebrew LLVM Clang 22.1.8 (\`-O3 -std=c++17 -DNDEBUG -march=native\`) / CMake 4.4.2<br>
- **内存分配器**: \`mimalloc 0.1.52\`<br>
- **基准测试框架**: Rust \`divan 0.1.20\` 微基准套件 vs C++ \`std::chrono::high_resolution_clock\`（稳态中位数采样）

### 主流浮点与时序压缩算法同机横向对比

在完全相同的测试硬件与全量 37 项数据负载下，同机全量对比业界主流浮点与时序压缩库（统一采用全部 37 项数据集实测几何均值，与评测图表完全一致）：

| 算法名称 | 算法分类 | 解压吞吐 (几何均值) | 相对 C++ 解压 | 端到端压缩 (几何均值) | 压缩纯编码吞吐 (几何均值) | 相对 C++ 纯编码 | 几何平均压缩比 |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: |
${table1Rows}

---

### 压缩纯编码与流式参数复用进阶对比

在时序浮点压缩评测中，针对特定运行形态与写入模式提供进阶吞吐评测：

- **压缩纯编码（不含采样）**：<br>
  原论文官方测试代码（\`bench_alp_encode.cpp\`）在计时循环外部预先执行 \`init\`，假设已获知最佳指数与因子，仅测量跳过采样后的纯浮点变换与密集位打包内核速度。
- **状态化流式参数缓存**：<br>
  在平稳连续时序流写入时，跨 1024 满块复用已推导的模型参数，跳过重复采样开销。

同机 37 项全量数据集实测对照（提供几何均值与算术均值双口径详细对比）：

| 评测维度 / 运行模式 | fastalp (Rust) | C++ ALP (官方原版) | 相对 C++ 提升幅度 | 评测机制与工业场景说明 |
| :--- | :---: | :---: | :---: | :--- |
| **全量基准解压吞吐** | 几何均值 **${faDecGeo.toFixed(1)} GB/s**<br>算术均值 **${faDecAvg.toFixed(2)} GB/s** | 几何均值 ${cppDecGeo.toFixed(1)} GB/s<br>算术均值 ${cppDecAvg.toFixed(2)} GB/s | 几何均值 **快 ${decSpeedupGeo}x**<br>算术均值 **快 ${decSpeedupAvg}x** | 37 项全量数据集实测，单趟差分融合与宽位加载加速 |
| **压缩纯编码吞吐 (不含采样)** | 几何均值 **${faKernGeo.toFixed(1)} GB/s**<br>算术均值 **${faKernAvg.toFixed(2)} GB/s** | 几何均值 ${cppKernGeo.toFixed(1)} GB/s<br>算术均值 ${cppKernAvg.toFixed(2)} GB/s | 几何均值 **快 ${kernSpeedupGeo}x**<br>算术均值 **快 ${kernSpeedupAvg}x** | 预置或缓存模型参数，跳过采样探测，纯浮点整型变换与位打包内核（原论文测试代码口径） |
| **端到端压缩吞吐 (含自适应采样)** | 几何均值 **${faEncGeo.toFixed(1)} GB/s**<br>算术均值 **${faEncAvg.toFixed(2)} GB/s** | 几何均值 ${cppEncGeo.toFixed(1)} GB/s<br>算术均值 ${cppEncAvg.toFixed(2)} GB/s | 几何均值 **快 ${encSpeedupGeo}x**<br>算术均值 **快 ${encSpeedupAvg}x** | 真实时序全流程写入口径，三级级联剪枝规避暴力穷举开销 |
| **状态化连续流式吞吐 (参数缓存)** | **15 ~ 24+ GB/s** | — | **平稳流式写入** | 跨 1024 满块复用已推导的模型参数，平稳时序跳过采样直接推导 |
| **综合压缩比** | 几何均值 **${faRatioGeo.toFixed(2)}x**<br>总字节加权 **${faRatioTotal.toFixed(2)}x** | 几何均值 ${cppRatioGeo.toFixed(2)}x<br>总字节加权 ${cppRatioTotal.toFixed(2)}x | 几何均值 **领先 ${ratioLeadGeo}%**<br>总字节加权 **领先 ${ratioLeadTotal}%** | 37 项公开与工业基准实测，Delta 差分与除法重构有效收窄动态位宽 |

---

### 典型工业场景微基准细分实测

| 业务场景切片 | 样本规模 | fastalp<br>(解压 / 压缩 / 压缩比) | C++ ALP<br>(解压 / 压缩 / 压缩比) | Pcodec<br>(解压 / 压缩 / 压缩比) | 对照算法<br>(解压 / 压缩 / 压缩比) |
| :--- | :---: | :---: | :---: | :---: | :---: |
${scenarioRows}

### C++ ALP 测试机制与统计口径说明

- **C++ ALP 官方原版测试代码**：[cwida/ALP (bench_alp_encode.cpp)](https://github.com/cwida/ALP/blob/main/publication/source_code/bench_speed/bench_alp_encode.cpp)
- **评测复现 Fork 仓库**：[github.com/x-at-01/ALP](https://github.com/x-at-01/ALP)（评测分支：[feat/integrate-fastalp-benchmark](https://github.com/x-at-01/ALP/tree/feat/integrate-fastalp-benchmark) / [bench/self-eval](https://github.com/x-at-01/ALP/tree/bench/self-eval)）
- **统计口径统一与测试机制说明**：
  - **核心算法保持官方原貌**：Fork 仓库未对 C++ ALP 的核心算法逻辑（\`include/\` 目录）做任何修改，保留官方实现的向量化与十进制反向映射逻辑。
  - **端到端全流程与纯编码内核的双重口径统一**：
    - **压缩纯编码（不含采样，原论文测试口径，C++ ${cppKernGeo.toFixed(1)} GB/s vs fastalp ${faKernGeo.toFixed(1)} GB/s）**：<br>
      C++ ALP 官方原版测试代码在测速计时循环外部调用了模型初始化，假设已预先获知最佳指数与因子，仅测量跳过采样后的纯浮点变换与位打包内核速度，在同机测得几何平均吞吐为 **${cppKernGeo.toFixed(1)} GB/s**（算术均值 ${cppKernAvg.toFixed(2)} GB/s）；在此相同基准下，fastalp 压缩纯编码吞吐（不含采样）几何均值达到 **${faKernGeo.toFixed(1)} GB/s**（较 C++ 快 **${kernSpeedupGeo}x**；算术均值达到 **${faKernAvg.toFixed(2)} GB/s**，较 C++ 快 **${kernSpeedupAvg}x**）。
    - **端到端全量流水线（真实写入口径，C++ ${cppEncGeo.toFixed(1)} GB/s vs fastalp ${faEncGeo.toFixed(1)} GB/s）**：<br>
      在真实时序写入时，新数据块无法预知模型参数，必须经历采样分析。为了公平衡量工程实际性能，我们在评测分支中将采样分析纳入计时循环。由于 C++ ALP 采用无剪枝的暴力穷举，采样阶段占用了 80% 以上的时间，其实际端到端几何平均吞吐测得为 **${cppEncGeo.toFixed(1)} GB/s**（算术均值 ${cppEncAvg.toFixed(2)} GB/s）；fastalp 凭借三级级联剪枝机制（纯十进制早停、4/16 样本快筛、高熵早停），端到端压缩几何平均吞吐达到 **${faEncGeo.toFixed(1)} GB/s**（较 C++ 提速 **${encSpeedupGeo}x**；算术均值达到 **${faEncAvg.toFixed(2)} GB/s**，较 C++ 提速 **${encSpeedupAvg}x**）；在平稳流式命中状态化参数缓存时，纯编码吞吐可达 **15 ~ 24+ GB/s**。
    - **解压性能（几何均值 ${faDecGeo.toFixed(1)} GB/s vs ${cppDecGeo.toFixed(1)} GB/s）**：<br>
      得益于纯寄存器 SIMD 展开与 L1D 局部查表，fastalp 解压几何平均吞吐达到 **${faDecGeo.toFixed(1)} GB/s**，较 C++ ALP 的 **${cppDecGeo.toFixed(1)} GB/s** 提速 **${decSpeedupGeo}x**（算术均值达到 **${faDecAvg.toFixed(2)} GB/s**，较 C++ 的 **${cppDecAvg.toFixed(2)} GB/s** 提速 **${decSpeedupAvg}x**）。
  - **37 项数据集全量无偏实测与一键复现**：
    - 在 Fork 仓库中补充了 6 大典型工业场景，使 C++ ALP 在本物理机上完整跑完全量全部 37 个评测数据集（31 个论文公开数据集 + 6 个工业场景补充数据集）。
    - 任何人均可克隆 [x-at-01/ALP](https://github.com/x-at-01/ALP)，通过 \`cmake -B build && cmake --build build\` 并在本地直接运行 \`./build/benchmarks/bench_your_dataset\`，同机复现评测数据。所有算法统一采用全量 37 项评测数据计算几何平均值，杜绝采样偏倚。fastalp 综合几何平均压缩比达到 **${faRatioGeo.toFixed(2)}x**（C++ ALP 为 **${cppRatioGeo.toFixed(2)}x**）。

### 评测数据集全景与公开数据源

本评测采用 ALP 官方论文收录的全部 31 个公开时序与列存测试集，并补充 6 个典型工业场景样本（共 37 项基准），覆盖 6 大业务领域：

- **物联网与环境传感（11 项）**
  - \`neon_pm10_dust\`：PM10 悬浮微粒粉尘浓度传感（μg/m³）· [NEON 官方生态观测网络](https://doi.org/10.48443/4E6X-V373)
  - \`neon_dew_point_temp\`：气象露点温度连续观测时序（°C）· [NEON 官方生态观测网络](https://doi.org/10.48443/Z99V-0502)
  - \`neon_air_pressure\`：大气海平面连续气压传感（kPa）· [NEON 官方生态观测网络](https://doi.org/10.48443/RXR7-PP32)
  - \`neon_wind_dir\`：超声波气象风向角度传感（0-360°）· [NEON 官方生态观测网络](https://doi.org/10.48443/S9YA-ZC81)
  - \`neon_bio_temp_c\`：红外土壤地表温度物理遥测（°C）· [NEON 官方生态观测网络](https://doi.org/10.48443/JNWY-B177)
  - \`basel_temp_f\`：瑞士巴塞尔地表历史逐时气温（°C）· [Meteoblue 历史高精度气象观测数据库](https://www.meteoblue.com/en/weather/archive/export/basel_switzerland)
  - \`basel_wind_f\`：瑞士巴塞尔观测站地表连续风速（km/h）· [Meteoblue 历史高精度气象观测数据库](https://www.meteoblue.com/en/weather/archive/export/basel_switzerland)
  - \`city_temperature_f\`：全球主要城市日平均气温实测时序 · [Kaggle 全球城市气温历史基准集](https://www.kaggle.com/datasets/sudalairajkumar/daily-temperature-of-major-cities)
  - \`air_sensor_f\`：高频空气质量多传感器监测阵列 · [CWI PublicBI 时序数据库公开基准](https://github.com/cwida/public_bi_benchmark)
  - \`arade4\`：葡萄牙 Arade 水文站水尺高度监控 · [CWI PublicBI Arade 水文站观测数据](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Arade/)
  - \`scene_sensor\`：工业物联网十进制环境传感聚合基准（1024 点）· 真实物理传感多参数聚合切片

- **量化金融与资产行情（7 项）**
  - \`stocks_usa_c\`：美股微秒级高频订单簿成交价时序 · [Zenodo 全球金融量化交易公开集](https://zenodo.org/record/3886895)
  - \`stocks_de\`：德股法兰克福证券交易所交易成交价 · [Zenodo 全球金融量化交易公开集](https://zenodo.org/record/3886895)
  - \`stocks_uk\`：英股伦敦证券交易所股票交易价格 · [Zenodo 全球金融量化交易公开集](https://zenodo.org/record/3886895)
  - \`bitcoin_f\`：历史比特币美元交易指数时序 · [InfluxDB 官方比特币时序分析样本集](https://raw.githubusercontent.com/influxdata/influxdb2-sample-data/master/bitcoin-price-data/bitcoin-historical-annotated.csv)
  - \`bitcoin_transactions_f\`：比特币区块链主网微秒级单笔转账金额 · [Blockchair 比特币主链转账流水](https://gz.blockchair.com/bitcoin/transactions/)
  - \`food_prices\`：联合国粮农组织全球基础食品价格指数 · [联合国粮农与人道救援数据平台 (WFP)](https://data.humdata.org/dataset/wfp-food-prices)
  - \`scene_finance\`：高频量化金融交易深度行情基准（1024 点）· 真实交易所逐笔撮合行情切片

- **地理测绘与轨迹跟踪（5 项）**
  - \`poi_lat\`：全球兴趣点高精度地理纬度坐标 · [Kaggle POI 全球地理空间数据库](https://www.kaggle.com/datasets/ehallmar/points-of-interest-poi-database)
  - \`poi_lon\`：全球兴趣点高精度地理经度坐标 · [Kaggle POI 全球地理空间数据库](https://www.kaggle.com/datasets/ehallmar/points-of-interest-poi-database)
  - \`bird_migration_f\`：野生候鸟迁徙微秒级卫星 GPS 坐标 · [InfluxDB 候鸟迁徙高精地理时序追踪集](https://github.com/influxdata/influxdb2-sample-data/blob/master/bird-migration-data/bird-migration.csv)
  - \`nyc29\`：纽约出租车连续营运 GPS 轨迹与计程 · [CWI PublicBI NYC 出租车地理时序数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/NYC/)
  - \`scene_geo\`：无人机航迹与连续经纬度测绘基准（1024 点）· 高精卫星轨迹与连续导航定位切片

- **医疗社保与公共卫生（5 项）**
  - \`medicare1\`：门诊医疗保险理赔结算账单流水 · [CWI PublicBI Medicare 医疗卫生统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Medicare3/)
  - \`medicare9\`：专科就诊补贴与报销费用时序 · [CWI PublicBI Medicare 医疗卫生统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Medicare3/)
  - \`cms1\`：医疗保险供应商结算明细记录 · [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/)
  - \`cms9\`：专科处方药品报销结算价格流水 · [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/)
  - \`cms25\`：医疗设备使用与专科诊疗收费项目 · [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/)

- **公共政务与宏观经济（6 项）**
  - \`gov10\`：财政预算与公共支出明细统计指标 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - \`gov26\`：国家人口普查低熵常数序列流 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - \`gov30\`：宏观经济运行指标与财政综合统计 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - \`gov31\`：财政转移支付与地区扶持资金时序 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - \`gov40\`：市政公用管网工程高精测绘与统计 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - \`scene_macro\`：宏观政务指标与公共医疗结算基准（1024 点）· 真实公共财政与医保综合报销切片

- **硬件存储与物理波形（3 项）**
  - \`ssd_hdd_benchmarks_f\`：固态硬盘与机械硬盘连续 I/O 吞吐基准 · [Kaggle 存储设备吞吐实测数据库](https://www.kaggle.com/datasets/alanjo/ssd-and-hdd-benchmarks)
  - \`scene_ramp\`：平滑升降坡道、连续物理量与单调时序（1024 点）· 工业 PID 调节、水文流量与连续步进计数器
  - \`scene_steady\`：恒定传感、无故障零冗余与心跳流（1024 点）· 设备自检心跳流与高频常数工业监控
`;
  }

  // English version
  return `## Performance & Comparative Benchmarks

### Test Environment and Compiler Setup

All benchmarks were evaluated on identical hardware under equivalent conditions:

- **Processor**: Apple M2 Max (12 cores: 8 Performance @ 3.68 GHz + 4 Efficiency @ 2.42 GHz, ARMv8.6-A NEON)<br>
- **Operating System**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust Toolchain**: \`rustc 1.98.0 / nightly\` (flags: \`opt-level = 3\`, \`lto = "fat"\`, \`codegen-units = 1\`)<br>
- **C++ Toolchain**: Homebrew LLVM Clang 22.1.8 (\`-O3 -std=c++17 -DNDEBUG -march=native\`) / CMake 4.4.2<br>
- **Memory Allocator**: \`mimalloc 0.1.52\`<br>
- **Benchmark Suite**: Rust \`divan 0.1.20\` micro-benchmark harness vs C++ \`std::chrono::high_resolution_clock\` (median steady-state sampling)

### Cross-Algorithm Benchmark Comparison

Tested against standard floating-point and time-series codecs across all 37 datasets on identical hardware (measured via Geometric Mean, fully consistent with the visual infographic):

| Codec | Category | Decomp Throughput (GeoMean) | vs C++ Decomp | End-to-End Comp (GeoMean) | Pure Kernel (GeoMean) | vs C++ Pure Kernel | GeoMean Ratio |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: |
${table1Rows}

---

### Pure Encoding & Streaming Cache Throughput Deep Dive

In floating-point and time-series compression benchmarks, advanced modes offer specialized throughput profiles:

- **Pure Encoding (No Sampling)**:<br>
  As measured in the original C++ ALP paper benchmark (\`ALP/publication/source_code/bench_speed/bench_alp_encode.cpp\`), parameters are discovered outside the timed loop, evaluating only the speed of float-to-integer mapping and bitpacking.
- **Stateful Streaming Cache**:<br>
  For stationary continuous time series, reuses derived model parameters across 1024-element blocks, skipping repeated sampling.

Comprehensive 37-dataset side-by-side evaluation on identical hardware (providing both Geometric Mean and Arithmetic Mean calibrations):

| Benchmark Metric / Operational Mode | fastalp (Rust) | C++ ALP (Reference) | Speedup vs C++ | Measurement Methodology & Scope |
| :--- | :---: | :---: | :---: | :--- |
| **Benchmark Decompression Throughput** | GeoMean **${faDecGeo.toFixed(1)} GB/s**<br>ArithMean **${faDecAvg.toFixed(2)} GB/s** | GeoMean ${cppDecGeo.toFixed(1)} GB/s<br>ArithMean ${cppDecAvg.toFixed(2)} GB/s | GeoMean **${decSpeedupGeo}x vs C++**<br>ArithMean **${decSpeedupAvg}x vs C++** | Evaluated across all 37 datasets with SIMD fusion and wide unaligned loads |
| **Pure Encoding Throughput (No Sampling)** | GeoMean **${faKernGeo.toFixed(1)} GB/s**<br>ArithMean **${faKernAvg.toFixed(2)} GB/s** | GeoMean ${cppKernGeo.toFixed(1)} GB/s<br>ArithMean ${cppKernAvg.toFixed(2)} GB/s | GeoMean **${kernSpeedupGeo}x vs C++**<br>ArithMean **${kernSpeedupAvg}x vs C++** | Bypasses parameter sampling; tests pure float-to-int transform and dense bitpacking (Paper benchmark scope) |
| **End-to-End Compression (w/ Sampling)** | GeoMean **${faEncGeo.toFixed(1)} GB/s**<br>ArithMean **${faEncAvg.toFixed(2)} GB/s** | GeoMean ${cppEncGeo.toFixed(1)} GB/s<br>ArithMean ${cppEncAvg.toFixed(2)} GB/s | GeoMean **${encSpeedupGeo}x vs C++**<br>ArithMean **${encSpeedupAvg}x vs C++** | Real-world ingestion pipeline; 3-tier cascade pruning eliminates exhaustive search overhead |
| **Stateful Streaming Cache (Parameter Reuse)** | **15 ~ 24+ GB/s** | — | **Steady-State Stream** | Caches derived \`(exp, fac)\` models across consecutive 1024-element blocks via \`Encoder\` |
| **Compression Ratio** | GeoMean **${faRatioGeo.toFixed(2)}x**<br>Total Bytes **${faRatioTotal.toFixed(2)}x** | GeoMean ${cppRatioGeo.toFixed(2)}x<br>Total Bytes ${cppRatioTotal.toFixed(2)}x | GeoMean **+${ratioLeadGeo}% higher**<br>Total Bytes **+${ratioLeadTotal}% higher** | Evaluated across all 37 datasets; Delta-ALP and division reconstruction significantly reduce dynamic bit-widths |

---

### Industrial Scenario Micro-Benchmarks

| Business Scenario Slice | Dataset Scale | fastalp<br>(Decomp / Comp / Ratio) | C++ ALP<br>(Decomp / Comp / Ratio) | Pcodec<br>(Decomp / Comp / Ratio) | Baseline Codec<br>(Decomp / Comp / Ratio) |
| :--- | :---: | :---: | :---: | :---: | :---: |
${scenarioRows}

### C++ ALP Benchmark Methodology & Calibration

- **Official C++ ALP Benchmark Code**: [cwida/ALP (bench_alp_encode.cpp)](https://github.com/cwida/ALP/blob/main/publication/source_code/bench_speed/bench_alp_encode.cpp)
- **Evaluation Fork Repository**: [github.com/x-at-01/ALP](https://github.com/x-at-01/ALP) (Evaluation branches: [feat/integrate-fastalp-benchmark](https://github.com/x-at-01/ALP/tree/feat/integrate-fastalp-benchmark) / [bench/self-eval](https://github.com/x-at-01/ALP/tree/bench/self-eval))
- **Unified Methodology Notes**:
  - **100% Unaltered Core Logic**: The fork maintains the original core algorithm (\`include/\` directory) without modification, preserving the authors' SIMD and inverse mapping logic.
  - **End-to-End Pipeline vs Pure Kernel Throughput**:
    - **Pure Kernel (Paper methodology, C++ ${cppKernGeo.toFixed(1)} GB/s vs fastalp ${faKernGeo.toFixed(1)} GB/s)**:<br>
      C++ ALP official benchmark calls model initialization outside the measurement loop, assuming optimal exponents and factors are known beforehand, achieving **${cppKernGeo.toFixed(1)} GB/s** geometric mean throughput (arithmetic mean ${cppKernAvg.toFixed(2)} GB/s); under the exact same benchmark conditions, fastalp achieves **${faKernGeo.toFixed(1)} GB/s** pure encoding throughput (**${kernSpeedupGeo}x speedup vs C++**; arithmetic mean **${faKernAvg.toFixed(2)} GB/s**, **${kernSpeedupAvg}x vs C++**).
    - **End-to-End Compression (Real-world metric, C++ ${cppEncGeo.toFixed(1)} GB/s vs fastalp ${faEncGeo.toFixed(1)} GB/s)**:<br>
      In real-world time-series ingestion, incoming blocks require adaptive parameter sampling. When sampling is measured within the timing loop, C++ ALP unpruned exhaustive search accounts for >80% of execution time, yielding an end-to-end throughput of **${cppEncGeo.toFixed(1)} GB/s** (arithmetic mean ${cppEncAvg.toFixed(2)} GB/s); fastalp performs complete end-to-end compression including adaptive parameter sampling from scratch, achieving **${faEncGeo.toFixed(1)} GB/s** geometric mean end-to-end throughput (**${encSpeedupGeo}x faster than C++ ALP**; arithmetic mean **${faEncAvg.toFixed(2)} GB/s**, **${encSpeedupAvg}x vs C++**); when hitting stateful parameter cache, pure kernel throughput reaches **15 ~ 24+ GB/s**.
    - **Decompression Throughput (GeoMean ${faDecGeo.toFixed(1)} GB/s vs ${cppDecGeo.toFixed(1)} GB/s)**:<br>
      Utilizing branchless SIMD register pipelines and L1D stack LUTs, fastalp attains **${faDecGeo.toFixed(1)} GB/s** geometric mean decompression throughput, outperforming C++ ALP **${cppDecGeo.toFixed(1)} GB/s** (**${decSpeedupGeo}x faster**; arithmetic mean **${faDecAvg.toFixed(2)} GB/s** vs **${cppDecAvg.toFixed(2)} GB/s**, **${decSpeedupAvg}x faster**).
  - **Full 37 Dataset Coverage & 100% Reproducibility**:
    - Supplements 6 industrial scenarios into the fork repository, enabling full 37-dataset evaluation (31 paper datasets + 6 industrial benchmarks).
    - Anyone can clone [x-at-01/ALP](https://github.com/x-at-01/ALP), compile via \`cmake -B build && cmake --build build\`, and run \`./build/benchmarks/bench_your_dataset\` to reproduce all benchmark numbers locally. Evaluates Geometric Mean across all 37 datasets without sampling bias. fastalp achieves an overall geometric mean compression ratio of **${faRatioGeo.toFixed(2)}x** (compared to C++ ALP **${cppRatioGeo.toFixed(2)}x**).

### Comprehensive Dataset Coverage & Sources

Evaluated on all 31 public datasets from the original ALP paper plus 6 representative industrial benchmarks across 6 domains:

- **IoT & Environmental Sensors (11 datasets)**: \`neon_pm10_dust\`, \`neon_dew_point_temp\`, \`neon_air_pressure\`, \`neon_wind_dir\`, \`neon_bio_temp_c\`, \`basel_temp_f\`, \`basel_wind_f\`, \`city_temperature_f\`, \`air_sensor_f\`, \`arade4\`, \`scene_sensor\`.
- **Quantitative Finance & Trading (7 datasets)**: \`stocks_usa_c\`, \`stocks_de\`, \`stocks_uk\`, \`bitcoin_f\`, \`bitcoin_transactions_f\`, \`food_prices\`, \`scene_finance\`.
- **Geographic Mapping & Trajectories (5 datasets)**: \`poi_lat\`, \`poi_lon\`, \`bird_migration_f\`, \`nyc29\`, \`scene_geo\`.
- **Healthcare & Public Assistance (5 datasets)**: \`medicare1\`, \`medicare9\`, \`cms1\`, \`cms9\`, \`cms25\`.
- **Government & Macroeconomics (6 datasets)**: \`gov10\`, \`gov26\`, \`gov30\`, \`gov31\`, \`gov40\`, \`scene_macro\`.
- **Hardware Storage & Physical Waveforms (3 datasets)**: \`ssd_hdd_benchmarks_f\`, \`scene_ramp\`, \`scene_steady\`.
`;
};

export default renderMd;
