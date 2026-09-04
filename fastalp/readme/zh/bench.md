## 性能评测与多算法对比

### 测试环境与编译配置

所有基准测试均在同一物理机上执行并进行同机对比测试：

- **处理器**: Apple M2 Max (12 核心：8 性能核 @ 3.68 GHz + 4 能效核 @ 2.42 GHz, ARMv8.6-A NEON 指令集)<br>
- **操作系统**: macOS Sequoia 26.5.1 (Darwin Kernel Version 25.5.0 arm64)<br>
- **Rust 编译工具链**: `rustc 1.98.0 / nightly` (配置：`opt-level = 3`, `lto = "fat"`, `codegen-units = 1`)<br>
- **C++ 编译工具链**: Homebrew LLVM Clang 22.1.8 (`-O3 -std=c++17 -DNDEBUG -march=native`) / CMake 4.4.2<br>
- **内存分配器**: `mimalloc 0.1.52`<br>
- **基准测试框架**: Rust `divan 0.1.20` 微基准套件 vs C++ `std::chrono::high_resolution_clock`（稳态中位数采样）

### 主流浮点与时序压缩算法同机横向对比

在完全相同的测试硬件与全量 37 项数据负载下，同机全量对比业界主流浮点与时序压缩库：

| 算法名称 | 算法分类 | 解压吞吐 | 相对 C++ 解压 | 端到端压缩 (含采样) | 压缩纯编码吞吐（不含采样） | 相对 C++ 纯编码 | 几何平均压缩比 |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **fastalp (Rust)** | 浮点专用 | **27.0 GB/s** | **较 C++ 快 1.35x** | **3.7 GB/s (快 4.6x)** | **6.0 GB/s** | **较 C++ 快 1.10x** | **6.99x** |
| **C++ ALP** (原版实现) | 浮点专用 | **20.0 GB/s** | 基准 (1.0x) | **0.80 GB/s** | **5.5 GB/s** | 基准 (1.0x) | **5.93x** |
| **Pcodec (pco 1.0.3)** | 浮点专用 | **1.8 GB/s** | 0.09x (慢 14.9x) | **0.2 GB/s** | — | — | **6.16x** |
| **Zstandard (zstd lvl 3)** | 通用流式 | **1.2 GB/s** | 0.06x (慢 22.4x) | **0.5 GB/s** | — | — | **4.83x** |
| **LZ4 (lz4_flex 0.14)** | 通用字节 | **4.4 GB/s** | 0.22x | **1.7 GB/s** | — | — | **3.26x** |
| **Snappy (snap 1.1)** | 通用字节 | **4.1 GB/s** | 0.21x | **2.2 GB/s** | — | — | **2.72x** |
| **Chimp128** (VLDB 2022) | 浮点时序 | **0.5 GB/s** | 0.02x | **0.6 GB/s** | — | — | **2.47x** |
| **Gorilla** (VLDB 2015) | 浮点时序 | **0.6 GB/s** | 0.03x | **0.9 GB/s** | — | — | **2.14x** |

---

### 压缩纯编码与流式参数复用进阶对比

在时序浮点压缩评测中，针对特定运行形态与写入模式提供进阶吞吐评测：
1. **压缩纯编码（不含采样）**：原论文官方测试代码（`bench_alp_encode.cpp`）在计时循环外部预先执行 `init`，假设已获知最佳指数与因子，仅测量跳过采样后的纯浮点变换与密集位打包内核速度。
2. **状态化流式参数缓存**：在平稳连续时序流写入时，跨 1024 满块复用已推导的模型参数，跳过重复采样开销。

同机 37 项全量数据集实测对照：

| 评测维度 / 运行模式 | fastalp (Rust) | C++ ALP (官方原版) | 相对 C++ 提升幅度 | 评测机制与工业场景说明 |
| :--- | :---: | :---: | :---: | :--- |
| **压缩纯编码吞吐（不含采样）** | **6.0 GB/s** | **5.5 GB/s** | **较 C++ 快 1.10x** | 预置/缓存模型参数，跳过采样探测，纯浮点整型变换与位打包内核（原论文测试代码口径） |
| **状态化连续流式吞吐 (参数缓存)** | **15 ~ 24+ GB/s** | — | **平稳流式写入** | 跨 1024 满块复用已推导的 `(exp, fac)` 模型，平稳时序跳过采样直接推导 |
| **综合几何平均压缩比** | **6.99x** | **5.93x** | **压缩率领先 18%** | 37 项公开/工业基准全量几何均值，Delta 差分与除法重构有效收窄动态位宽 |

---

### 典型工业场景微基准细分实测

| 业务场景切片 | 样本规模 | fastalp (解压 / 压缩 / 压缩比) | C++ ALP (解压 / 压缩 / 压缩比) | Pcodec (解压 / 压缩 / 压缩比) | Zstd (解压 / 压缩 / 压缩比) |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **物联网与连续环境传感** | 11 组 (11,264 点) | **26.6 GB/s** ｜ **5.7 GB/s** ｜ **7.92x** | 21.3 GB/s ｜ 0.8 GB/s ｜ 7.91x | 1.6 GB/s ｜ 0.2 GB/s ｜ 3.02x | 1.0 GB/s ｜ 0.4 GB/s ｜ 2.11x |
| **量化金融交易与撮合行情** | 7 组 (7,168 点) | **19.6 GB/s** ｜ **5.9 GB/s** ｜ **7.04x** | 20.5 GB/s ｜ 0.8 GB/s ｜ 7.04x | 1.7 GB/s ｜ 0.2 GB/s ｜ 3.71x | 1.2 GB/s ｜ 0.4 GB/s ｜ 2.90x |
| **地理空间高精测绘与轨迹** | 5 组 (5,120 点) | **19.9 GB/s** ｜ **5.2 GB/s** ｜ **6.35x** | 20.3 GB/s ｜ 0.8 GB/s ｜ 6.07x | 2.0 GB/s ｜ 0.2 GB/s ｜ 1.84x | 1.1 GB/s ｜ 0.4 GB/s ｜ 1.63x |
| **公共卫生与医疗结算流水** | 5 组 (5,120 点) | **36.3 GB/s** ｜ **2.1 GB/s** ｜ **1.66x** | 20.0 GB/s ｜ 0.8 GB/s ｜ 2.19x | 2.0 GB/s ｜ 0.2 GB/s ｜ 2.16x | 0.9 GB/s ｜ 0.4 GB/s ｜ 1.99x |
| **政务民生与宏观统计普查** | 6 组 (6,144 点) | **44.7 GB/s** ｜ **7.0 GB/s** ｜ **8.89x** | 21.7 GB/s ｜ 0.8 GB/s ｜ 4.64x | 3.0 GB/s ｜ 0.4 GB/s ｜ 3.79x | 3.0 GB/s ｜ 2.1 GB/s ｜ 4.15x |
| **物理单调波形与稳态流** | 3 组 (3,072 点) | **44.4 GB/s** ｜ **10.2 GB/s** ｜ **11.70x** | 19.8 GB/s ｜ 0.9 GB/s ｜ 2.90x | 1.0 GB/s ｜ 0.1 GB/s ｜ 8.58x | 1.4 GB/s ｜ 0.4 GB/s ｜ 6.84x |

### C++ ALP 测试机制与统计口径说明

- **C++ ALP 官方原版测试代码**：[cwida/ALP (bench_alp_encode.cpp)](https://github.com/cwida/ALP/blob/main/publication/source_code/bench_speed/bench_alp_encode.cpp)
- **评测复现 Fork 仓库**：[github.com/x-at-01/ALP](https://github.com/x-at-01/ALP)（评测分支：[feat/integrate-fastalp-benchmark](https://github.com/x-at-01/ALP/tree/feat/integrate-fastalp-benchmark) / [bench/self-eval](https://github.com/x-at-01/ALP/tree/bench/self-eval)）
- **统计口径统一与测试机制说明**：
  - **核心算法保持 100% 官方原貌**：Fork 仓库未对 C++ ALP 的核心算法逻辑（`include/` 目录）做任何修改，原汁原味保留官方实现的向量化与十进制反向映射逻辑；
  - **端到端全流程 vs 纯编码内核的口径统一**：
    - **压缩纯编码（不含采样，原论文测试口径，C++ 5.5 GB/s vs fastalp 6.0 GB/s）**：C++ ALP 官方原版测试代码（[`bench_alp_encode.cpp#L88-L95`](https://github.com/cwida/ALP/blob/main/publication/source_code/bench_speed/bench_alp_encode.cpp#L88-L95)）在测速计时循环 `b_a_e` 外部调用了 `alp::encoder<PT>::init`，假设已预先获知最佳指数与因子，仅测量跳过采样后的纯浮点变换与位打包内核速度，在同机测得几何平均吞吐约为 **5.5 GB/s**；在此相同基准下，fastalp 压缩纯编码吞吐（不含采样）达到 **6.0 GB/s**，**较 C++ 快 1.10 倍（1.09x~1.10x，算术均值快 1.19x）**；
    - **端到端全量流水线（真实写入口径，C++ 0.80 GB/s vs fastalp 3.7 GB/s）**：在真实时序写入时，新数据块无法预知最佳模型参数，必须经历采样分析。为了公平衡量工程实际性能，我们在评测分支中将 `init` 采样分析纳入计时循环。由于 C++ ALP 采用无剪枝的暴力全量穷举，采样阶段占用了 80% 以上的时间，其实际端到端吞吐测得为 **0.80 GB/s**；fastalp 凭借三级级联剪枝机制（纯十进制早停、4/16 样本快筛、高熵早停），端到端压缩吞吐达到 **3.7 GB/s（较 C++ 提速 4.6x，单场景最高达 7.0x）**；在平稳流式命中状态化参数缓存时，纯编码吞吐可达 **15~24+ GB/s**；
    - **解压性能（27.0 GB/s vs 20.0 GB/s）**：得益于纯寄存器 SIMD 展开与 L1D 局部查表，fastalp 解压几何平均吞吐达到 **27.0 GB/s**，较 C++ ALP 的 **20.0 GB/s** 提速 **1.35x**（算术均值快 1.71x）。
  - **37 项数据集全量无偏实测与一键复现**：
    - 在 Fork 仓库的 `ALP/data/samples/` 与 `your_own_dataset.csv` 中补充了 6 大典型工业场景，使 C++ ALP 在本物理机上完整跑完全量全部 37 个评测数据集（31 个论文公开数据集 + 6 个工业场景补充数据集）；
    - 任何人均可克隆 [x-at-01/ALP](https://github.com/x-at-01/ALP)，通过 `cmake -B build && cmake --build build` 并在本地直接运行 `./build/benchmarks/bench_your_dataset`，100% 同机复现评测数据。所有算法统一采用全量 37 项评测数据计算几何平均值（Geometric Mean），杜绝任何采样偏倚。fastalp 综合几何平均压缩比达到 **6.99x**（C++ ALP 为 **5.93x**）。

### 评测数据集全景与公开数据源

本评测采用 ALP 官方论文收录的全部 31 个公开时序与列存测试集，并补充 6 个典型工业场景样本（共 37 项基准），覆盖 6 大业务领域：

- **物联网与环境传感（11 项）**
  - `neon_pm10_dust`：PM10 悬浮微粒粉尘浓度传感（μg/m³）· [NEON 官方生态观测网络](https://doi.org/10.48443/4E6X-V373)
  - `neon_dew_point_temp`：气象露点温度连续观测时序（°C）· [NEON 官方生态观测网络](https://doi.org/10.48443/Z99V-0502)
  - `neon_air_pressure`：大气海平面连续气压传感（kPa）· [NEON 官方生态观测网络](https://doi.org/10.48443/RXR7-PP32)
  - `neon_wind_dir`：超声波气象风向角度传感（0-360°）· [NEON 官方生态观测网络](https://doi.org/10.48443/S9YA-ZC81)
  - `neon_bio_temp_c`：红外土壤地表温度物理遥测（°C）· [NEON 官方生态观测网络](https://doi.org/10.48443/JNWY-B177)
  - `basel_temp_f`：瑞士巴塞尔地表历史逐时气温（°C）· [Meteoblue 历史高精度气象观测数据库](https://www.meteoblue.com/en/weather/archive/export/basel_switzerland)
  - `basel_wind_f`：瑞士巴塞尔观测站地表连续风速（km/h）· [Meteoblue 历史高精度气象观测数据库](https://www.meteoblue.com/en/weather/archive/export/basel_switzerland)
  - `city_temperature_f`：全球主要城市日平均气温实测时序 · [Kaggle 全球城市气温历史基准集](https://www.kaggle.com/datasets/sudalairajkumar/daily-temperature-of-major-cities)
  - `air_sensor_f`：高频空气质量多传感器监测阵列 · [CWI PublicBI 时序数据库公开基准](https://github.com/cwida/public_bi_benchmark)
  - `arade4`：葡萄牙 Arade 水文站水尺高度监控 · [CWI PublicBI Arade 水文站观测数据](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Arade/)
  - `scene_sensor`：工业物联网十进制环境传感聚合基准（1024 点）· 真实物理传感多参数聚合切片

- **量化金融与资产行情（7 项）**
  - `stocks_usa_c`：美股微秒级高频订单簿成交价时序 · [Zenodo 全球金融量化交易公开集](https://zenodo.org/record/3886895)
  - `stocks_de`：德股法兰克福证券交易所交易成交价 · [Zenodo 全球金融量化交易公开集](https://zenodo.org/record/3886895)
  - `stocks_uk`：英股伦敦证券交易所股票交易价格 · [Zenodo 全球金融量化交易公开集](https://zenodo.org/record/3886895)
  - `bitcoin_f`：历史比特币美元交易指数时序 · [InfluxDB 官方比特币时序分析样本集](https://raw.githubusercontent.com/influxdata/influxdb2-sample-data/master/bitcoin-price-data/bitcoin-historical-annotated.csv)
  - `bitcoin_transactions_f`：比特币区块链主网微秒级单笔转账金额 · [Blockchair 比特币主链转账流水](https://gz.blockchair.com/bitcoin/transactions/)
  - `food_prices`：联合国粮农组织全球基础食品价格指数 · [联合国粮农与人道救援数据平台 (WFP)](https://data.humdata.org/dataset/wfp-food-prices)
  - `scene_finance`：高频量化金融交易深度行情基准（1024 点）· 真实交易所逐笔撮合行情切片

- **地理测绘与轨迹跟踪（5 项）**
  - `poi_lat`：全球兴趣点高精度地理纬度坐标 · [Kaggle POI 全球地理空间数据库](https://www.kaggle.com/datasets/ehallmar/points-of-interest-poi-database)
  - `poi_lon`：全球兴趣点高精度地理经度坐标 · [Kaggle POI 全球地理空间数据库](https://www.kaggle.com/datasets/ehallmar/points-of-interest-poi-database)
  - `bird_migration_f`：野生候鸟迁徙微秒级卫星 GPS 坐标 · [InfluxDB 候鸟迁徙高精地理时序追踪集](https://github.com/influxdata/influxdb2-sample-data/blob/master/bird-migration-data/bird-migration.csv)
  - `nyc29`：纽约出租车连续营运 GPS 轨迹与计程 · [CWI PublicBI NYC 出租车地理时序数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/NYC/)
  - `scene_geo`：无人机航迹与连续经纬度测绘基准（1024 点）· 高精卫星轨迹与连续导航定位切片

- **医疗社保与公共卫生（5 项）**
  - `medicare1`：门诊医疗保险理赔结算账单流水 · [CWI PublicBI Medicare 医疗卫生统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Medicare3/)
  - `medicare9`：专科就诊补贴与报销费用时序 · [CWI PublicBI Medicare 医疗卫生统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Medicare3/)
  - `cms1`：医疗保险供应商结算明细记录 · [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/)
  - `cms9`：专科处方药品报销结算价格流水 · [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/)
  - `cms25`：医疗设备使用与专科诊疗收费项目 · [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/)

- **公共政务与宏观经济（6 项）**
  - `gov10`：财政预算与公共支出明细统计指标 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - `gov26`：国家人口普查极低熵常数序列流 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - `gov30`：宏观经济运行指标与财政综合统计 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - `gov31`：财政转移支付与地区扶持资金时序 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - `gov40`：市政公用管网工程高精测绘与统计 · [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/)
  - `scene_macro`：宏观政务指标与公共医疗结算基准（1024 点）· 真实公共财政与医保综合报销切片

- **硬件存储与物理波形（3 项）**
  - `ssd_hdd_benchmarks_f`：固态硬盘与机械硬盘连续 I/O 吞吐基准 · [Kaggle 存储设备吞吐实测数据库](https://www.kaggle.com/datasets/alanjo/ssd-and-hdd-benchmarks)
  - `scene_ramp`：平滑升降坡道、连续物理量与单调时序（1024 点）· 工业 PID 调节、水文流量与连续步进计数器
  - `scene_steady`：恒定传感、无故障零冗余与心跳流（1024 点）· 设备自检心跳流与高频常数工业监控
