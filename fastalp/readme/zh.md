# fastalp : 基于 ALP 算法的无损浮点数压缩引擎

纯 Rust 实现的自适应无损浮点数压缩 ALP 算法库，通过统一泛型接口支持 `f64` 与 `f32` 数据流。

<p align="center">
  <img src="https://fastly.jsdelivr.net/gh/webc-fs/-@h8/jH5a1Wl8dkKGgASAat-A.svg" alt="fastalp 浮点压缩算法全量性能与压缩比横向对比" width="100%">
  <br>
  <sub><b>评测环境</b>: 芯片: Apple M2 Max (12 核) ｜ 环境: macOS 26.5.1 ｜ 工具链: Rust 1.98.0 / Clang (-O3)</sub>
</p>

---


## 功能特性

在物联网传感器采集、金融量化交易、GPS 经纬度定位以及时序监控等场景中，浮点数据通常以十进制形式产生。<br>
由于 IEEE 754 浮点数的阶码与尾数位分布离散，通用压缩算法与整型位打包算法难以获得理想的压缩效率。

`fastalp` 实现 ALP 压缩算法：

- **严格无损重构**：<br>
  保证解码数据与原始 IEEE 754 二进制位严格一致，支持 `NaN`、`+Inf`、`-Inf` 与 `-0.0` 等特殊值。

- **紧凑自描述头与超大数组支持**：<br>
  采用 2-bit 长度标签紧凑头部架构，标准 1024 满块压缩头仅占 3 字节，RAW 保底模式仅占 1 字节；<br>
  原生支持超过 65,535 元素的超大数组，自动升级为 4 字节数量与 4 字节异常索引，解除单块长度截断限制。

- **时序差分自适应编码**：<br>
  自动评估连续平滑的时序物理波形（气象、水文、传感器），自适应采用一阶相邻差分与前缀和递推，位宽进一步收窄 15% ~ 38%。

- **十进制精确除法重构**：<br>
  消除 IEEE 754 浮点乘法（如 `* 0.1`）引起的无限循环二进制尾数截断误差，以十进制除法精确重构，将观测时序异常点直接归零。

- **栈上 LUT 查表与 SIMD 混合加速**：<br>
  小位宽利用 256 项栈上查找表（L1D 缓存命中）消除循环内硬件除法延迟；对直接模式采用纯寄存器 SIMD 向量化计算，吞吐高达 55+ GB/s。

- **自适应参数推导**：<br>
  通过对输入数据进行采样，计算使编码位宽最小的最优参数组合 `(exp, fac, use_div)`。

- **基准偏移与位打包**：<br>
  将转换后的整型序列进行基准值消除（FOR / Delta），并按 1 至 64 位动态位宽进行密集位打包。

- **独立异常值处理**：<br>
  无法无损整型化的数值与特殊浮点数记录于独立异常流，避免降低主数据流压缩比。

- **原始保底模式**：<br>
  当随机噪声或不可压缩数据导致编码后体积膨胀时，自动回退至原始保底模式，杜绝负压缩。

- **零额外分配复用**：<br>
  提供 `_into` 系列接口，支持调用方直接复用已有内存缓冲区。

- **统一泛型接口**：<br>
  `compress`、`compress_into`、`decompress` 与 `decompress_into` 统一适用于 `f64` 与 `f32`。

---

## 使用示例

### 添加依赖

```bash
cargo add fastalp
```

### 基础压缩与解压

```rust
use fastalp::{compress, decompress, Result};

fn main() -> Result<()> {
  let sensor_data = vec![20.5, 20.6, 20.8, 21.0, 20.9, 21.2];

  // 压缩浮点数切片为字节向量 (自动适配 f64 / f32)
  let compressed = compress(&sensor_data);

  // 解压字节向量恢复原始浮点数切片
  let decompressed: Vec<f64> = decompress(&compressed)?;

  assert_eq!(decompressed, sensor_data);
  Ok(())
}
```

### 内存缓冲区复用

```rust
use fastalp::{compress_into, decompress_into, Result};

fn main() -> Result<()> {
  let batch = vec![100.12, 100.15, 100.18, 100.22];

  let mut compressed_buf = Vec::new();
  compress_into(&batch, &mut compressed_buf);

  let mut restored = Vec::new();
  decompress_into(&compressed_buf, &mut restored)?;

  assert_eq!(restored, batch);
  Ok(())
}
```

### 单精度浮点数据处理

```rust
use fastalp::{compress, decompress, Result};

fn main() -> Result<()> {
  let coordinates = vec![116.4074f32, 39.9042f32, 121.4737f32, 31.2304f32];

  let compressed = compress(&coordinates);
  let decompressed: Vec<f32> = decompress(&compressed)?;

  assert_eq!(decompressed, coordinates);
  Ok(())
}
```

---

## 核心特性

- **位级精确无损**：<br>
  解码浮点数与原始输入在二进制位层面保持一致（`a.to_bits() == b.to_bits()`）。

- **十进制高压缩比**：<br>
  在常见十进制浮点序列上可获得 3x 至 8x+ 压缩比。

- **统一泛型支持**：<br>
  单一接口支持 `f64` 与 `f32` 零成本抽象编解码。

- **完整异常值支持**：<br>
  支持 `NaN`、无穷大与不可无损转换的高精度浮点数。

- **零堆分配接口**：<br>
  通过 `compress_into` 与 `decompress_into` 直接写入现有缓冲区。

---

## 架构设计

`fastalp` 编解码流程划分为以下阶段：

```mermaid
graph TD
  Input["输入浮点数切片 (&[f64] / &[f32])"] --> Sampler["参数采样器<br/>评估代价模型并推导最优 (exp, fac)"]
  Sampler --> Encoder["无损整型编码<br/>快速常量舍入与位精确校验"]
  Encoder --> Split{"是否支持无损编码"}
  Split -- 是 --> IntStream["FOR 基准值消除<br/>计算非负整型偏移量"]
  Split -- 否 --> ExcStream["异常值记录<br/>存储索引位置与 IEEE 754 原始位"]
  IntStream --> Bitpacker["密集位打包<br/>按动态位宽打包进字节流"]
  ExcStream --> Frame["二进制帧封装<br/>包头 + 基准值 + 位流 + 异常值列表"]
  Bitpacker --> Frame
  Frame --> Output["压缩字节负载 (Vec<u8>)"]
```

### 压缩流程

- **全等探测与保底分流 (`encoder.rs`)**：<br>
  先对数据进行常数序列快速校验；若全等且可编码，直接写入自描述紧凑头部与基准值；<br>
  若为不可压缩随机数据且编码体积超过原始大小加上极简头部，则自动回退至原始保底模式（1024 满块仅 1 字节头部），直接以原始字节流存储。

- **采样评估 (`sampler.rs`)**：<br>
  在数据序列中均匀采样至多 32 个数值，遍历 `(exp, fac)` 参数组合，<br>
  选取使得 `位宽 * 样本量 + 异常数 * 惩罚权重` 最小的参数组合。

- **无损转换与验证 (`sampler.rs`, `float.rs`)**：<br>
  将浮点数乘以 $10^{\text{exp}} \times 10^{-\text{fac}}$，利用常量完成快速向近舍入并转换为整型，<br>
  再通过反向整型乘法与逆缩放验证浮点位级一致性。

- **基准消除与位打包 (`bitpack/pack.rs`, `encoder.rs`)**：<br>
  获取有效整型中的最小值作为基准值，计算偏移量并获取所需位宽，<br>
  利用 128 位寄存器滑动窗口将数值紧凑打包入字节流。

- **异常流序列化 (`encoder.rs`)**：<br>
  无法无损转换的浮点数按索引位置与 IEEE 754 原始位记录于尾部异常表中。

### 解压流程

- **自描述头解析 (`header.rs`, `decoder.rs`)**：<br>
  读取首字节描述符，由 2-bit 长度标签解码元素总数并确定参数偏移；<br>
  若类型为原始保底数据，通过内存复制直出恢复；若为 ALP 压缩数据，提取 `(exp, fac, bit_width)` 缩放参数与基准值。

- **位流解包与 SIMD 寄存器流水重构 (`bitpack/unpack.rs`)**：<br>
  针对 8/16/32/64 bit 采用纯寄存器 SIMD 自动向量化计算，消除堆栈查表与内存间接 gather 寻址延迟；针对 1/2/4 bit 采用微型局部表快速还原。

- **异常值覆盖 (`decoder.rs`)**：<br>
  若存在尾部异常表，读取对应索引位置的数值并覆盖为原始 IEEE 754 浮点值。

---

## 技术栈

- **开发语言**：Rust Edition 2024
- **错误处理**：`thiserror`
- **测试与基准**：`anyhow`, `aok`, `fastrand`

---

## 目录结构

```
fastalp/
├── Cargo.toml          # 项目配置与依赖声明
├── README.md           # 生成的多语言文档
├── README.mdt          # 多语言文档模板
├── readme/             # 文档源码目录
│   ├── en.md           # 英文技术文档
│   └── zh.md           # 中文技术文档
├── src/                # 核心源代码
│   ├── bitpack/        # 模块化位打包与位解包
│   │   ├── mod.rs      # 门面导出
│   │   ├── pack.rs     # 128 位累加器位打包算子
│   │   └── unpack.rs   # 局部查表与直接位解包算子
│   ├── constants.rs    # 静态幂次表与格式常量
│   ├── decoder/        # 泛型流式解压与除法重构
│   │   ├── mod.rs      # 解压门面与模式派发
│   │   ├── standard.rs # 标准 FOR 还原解压
│   │   └── delta.rs    # Delta 一阶差分解码
│   ├── delta/          # 一阶差分自适应收益评估与前缀和
│   │   └── mod.rs
│   ├── encoder/        # 泛型压缩流水线与保底回退
│   │   ├── mod.rs      # 编码门面与向量化流
│   │   ├── standard.rs # 标准 FOR 编码流水线
│   │   └── delta.rs    # Delta 一阶差分编码流水线
│   ├── error.rs        # 错误枚举定义与 Result 类型别名
│   ├── float/          # AlpFloat 浮点抽象特征与泛型无损转换
│   │   ├── mod.rs      # AlpFloat trait 定义与查表构建
│   │   ├── f32.rs      # 单精度 f32 乘法/除法编解码实现
│   │   └── f64.rs      # 双精度 f64 乘法/除法编解码实现
│   ├── header.rs       # 紧凑自描述头部编解码与 2-bit 长度标签档位管理
│   ├── lib.rs          # 导出接口与高层封装
│   ├── params.rs       # 紧凑位域参数打包与位宽计算
│   └── sampler.rs      # 参数采样与无损重构验证
├── test.sh             # 测试运行脚本
└── tests/              # 集成与压力测试
    ├── test_alp_dataset.rs # ALP 论文 31 真实数据集往返与压缩比评测
    ├── test_delta.rs       # Delta 差分时序专项与异常测试
    └── test_roundtrip.rs   # 往返无损与边界测试
```

---

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

在完全相同的测试硬件与数据负载下，对比业界主流浮点与时序压缩库：
- **fastalp** (Rust Edition 2024, SIMD NEON)
- **C++ ALP** (官方 C++ 论文原版实现, Clang 22.1.8 -O3)
- **Pcodec / pco 1.0.3** (现代列式数值压缩, ANS 熵编码)
- **Zstandard / zstd 0.13** (通用流式字典压缩, Level 3)
- **LZ4 / lz4_flex 0.14** (极速通用字节压缩)
- **Snappy / snap 1.1** (Google 高速字节压缩)
- **Chimp128** (VLDB 2022 浮点时序压缩)
- **Gorilla** (VLDB 2015 XOR 浮点时序压缩)

### 评测数据集全景与公开数据源 (35 项工业与学术基准)

本评测严格采用 ALP 官方论文收录的全部 31 个公开时序与列存测试集，并补充 4 个工业真实极端负载场景（共 35 项基准），涵盖物联网、工业制造、量化金融、地理测绘、医疗社保及政务统计：

| 领域分类 | 数据集名称 | 数据特征与物理意义 | 官方数据源与权威链接 |
|---|---|---|---|
| **物联网与环境传感** | `neon_pm10_dust` | PM10 悬浮微粒粉尘浓度传感 (μg/m³) | [NEON 官方生态观测网络 (DOI: 10.48443/4E6X-V373)](https://doi.org/10.48443/4E6X-V373) |
| | `neon_dew_point_temp` | 气象露点温度连续观测时序 (°C) | [NEON 官方生态观测网络 (DOI: 10.48443/Z99V-0502)](https://doi.org/10.48443/Z99V-0502) |
| | `neon_air_pressure` | 大气海平面连续气压传感 (kPa) | [NEON 官方生态观测网络 (DOI: 10.48443/RXR7-PP32)](https://doi.org/10.48443/RXR7-PP32) |
| | `neon_wind_dir` | 超声波气象风向角度传感 (0-360°) | [NEON 官方生态观测网络 (DOI: 10.48443/S9YA-ZC81)](https://doi.org/10.48443/S9YA-ZC81) |
| | `neon_bio_temp_c` | 红外土壤地表温度物理遥测 (°C) | [NEON 官方生态观测网络 (DOI: 10.48443/JNWY-B177)](https://doi.org/10.48443/JNWY-B177) |
| | `basel_temp_f` | 瑞士巴塞尔地表历史逐时气温 (°C) | [Meteoblue 历史高精度气象观测数据库](https://www.meteoblue.com/en/weather/archive/export/basel_switzerland) |
| | `basel_wind_f` | 瑞士巴塞尔观测站地表连续风速 (km/h) | [Meteoblue 历史高精度气象观测数据库](https://www.meteoblue.com/en/weather/archive/export/basel_switzerland) |
| | `city_temperature_f` | 全球主要城市日平均气温实测时序 | [Kaggle 全球城市气温历史基准集](https://www.kaggle.com/datasets/sudalairajkumar/daily-temperature-of-major-cities) |
| | `air_sensor_f` | 高频空气质量多传感器监测阵列 | [CWI PublicBI 时序数据库公开基准](https://github.com/cwida/public_bi_benchmark) |
| | `arade4` | 葡萄牙 Arade 水文站水尺高度监控 | [CWI PublicBI Arade 水文站观测数据](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Arade/) |
| | `scene_sensor` | 工业物联网十进制环境传感聚合基准 (1024 点) | 真实物理传感多参数聚合切片 |
| **量化金融与资产行情** | `stocks_usa_c` | 美股微秒级高频订单簿成交价时序 | [Zenodo 真实全球金融量化交易公开数据集](https://zenodo.org/record/3886895) |
| | `stocks_de` | 德股法兰克福证券交易所交易成交价 | [Zenodo 真实全球金融量化交易公开数据集](https://zenodo.org/record/3886895) |
| | `stocks_uk` | 英股伦敦证券交易所股票交易价格 | [Zenodo 真实全球金融量化交易公开数据集](https://zenodo.org/record/3886895) |
| | `bitcoin_f` | 历史比特币美元交易指数时序 | [InfluxDB 官方比特币时序分析样本集](https://raw.githubusercontent.com/influxdata/influxdb2-sample-data/master/bitcoin-price-data/bitcoin-historical-annotated.csv) |
| | `bitcoin_transactions_f` | 比特币区块链主网微秒级单笔转账金额 | [Blockchair 比特币主链历史大宗转账流水](https://gz.blockchair.com/bitcoin/transactions/) |
| | `food_prices` | 联合国粮农组织全球基础食品价格指数 | [联合国粮农与人道救援数据平台 (WFP)](https://data.humdata.org/dataset/wfp-food-prices) |
| | `scene_finance` | 高频量化金融交易深度行情基准 (1024 点) | 真实交易所逐笔撮合行情切片 |
| **政务普查与医疗医保** | `gov10` | 财政预算与公共支出明细统计指标 | [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/) |
| | `gov26` | 国家人口普查极低熵常数序列流 | [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/) |
| | `gov30` | 宏观经济运行指标与财政综合统计 | [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/) |
| | `gov31` | 财政转移支付与地区扶持资金时序 | [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/) |
| | `gov40` | 市政公用管网工程高精测绘与统计 | [CWI PublicBI CommonGovernment 统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CommonGovernment/) |
| | `medicare1` | 门诊医疗保险理赔结算账单流水 | [CWI PublicBI Medicare 医疗卫生统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Medicare3/) |
| | `medicare9` | 专科就诊补贴与报销费用时序 | [CWI PublicBI Medicare 医疗卫生统计集](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/Medicare3/) |
| | `cms1` | 医疗保险供应商结算明细记录 | [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/) |
| | `cms9` | 专科处方药品报销结算价格流水 | [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/) |
| | `cms25` | 医疗设备使用与专科诊疗收费项目 | [CWI PublicBI CMSProvider 医疗保险数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/CMSprovider/) |
| | `scene_macro` | 宏观政务指标与公共医疗结算基准 (1024 点) | 真实公共财政与医保综合报销切片 |
| **地理测绘与轨迹跟踪** | `poi_lat` | 全球兴趣点高精度地理纬度坐标 | [Kaggle POI 全球地理空间数据库](https://www.kaggle.com/datasets/ehallmar/points-of-interest-poi-database) |
| | `poi_lon` | 全球兴趣点高精度地理经度坐标 | [Kaggle POI 全球地理空间数据库](https://www.kaggle.com/datasets/ehallmar/points-of-interest-poi-database) |
| | `bird_migration_f` | 野生候鸟迁徙微秒级卫星 GPS 坐标 | [InfluxDB 候鸟迁徙高精地理时序追踪集](https://github.com/influxdata/influxdb2-sample-data/blob/master/bird-migration-data/bird-migration.csv) |
| | `nyc29` | 纽约出租车连续营运 GPS 轨迹与计程 | [CWI PublicBI NYC 出租车地理时序数据库](https://homepages.cwi.nl/~boncz/PublicBIbenchmark/NYC/) |
| | `scene_geo` | 无人机航迹与连续经纬度测绘基准 (1024 点) | 高精卫星轨迹与连续导航定位切片 |
| **硬件存储与物理波形** | `ssd_hdd_benchmarks_f` | 固态硬盘与机械硬盘连续 I/O 吞吐基准 | [Kaggle 存储设备吞吐实测数据库](https://www.kaggle.com/datasets/alanjo/ssd-and-hdd-benchmarks) |
| | `scene_ramp` | 平滑升降坡道、连续物理量与单调时序 (1024 点) | 工业 PID 调节、水文流量与连续步进计数器 |
| | `scene_steady` | 恒定传感、无故障零冗余与心跳流 (1024 点) | 设备自检心跳流与高频常数工业监控 |

---

## 架构演进与优化全景 (Architecture & Optimization Breakdown)

fastalp 并非简单的语言转译，而是在完整吸收 C++ ALP 论文精髓的基础上，针对现代多核流水线与时序数据库列存痛点重构的高性能压缩引擎。

### 一、参考与借鉴 C++ ALP 的架构设计（用于解决什么问题）

在架构演进中，fastalp 完整保留并吸收了 C++ ALP 经数学严密证明的优秀工业设计：

1. **状态化编码器与跨块参数缓存（Stateful Encoder & Parameter Caching）**：
   - **用途**：解决时序数据库连续写入时频繁重复采样的性能瓶颈。
   - **机制**：在工业时序流中，同一指标列（如温度）相邻数据块的量纲和精度具有高度连续性。fastalp 借鉴 C++ 设计，支持跨 1024 块复用上一数据块探测出的指数 `exp` 与因子 `fac`。连续写入时直接跳过昂贵的全部样本扫描，使连续压缩吞吐由 4~5 GB/s 跃升至 **15~20+ GB/s**。
2. **12.5% 异常阈值保底回退（Exception Threshold RAW Fallback）**：
   - **用途**：彻底消除高熵浮点数（如高精 GPS 坐标、科学计算随机数）压缩时空间膨胀的“负压缩”隐患。
   - **机制**：当异常值数量超过 128 个（占 1024 元素的 12.5%）时，强制判定该数据块不可有效进行十进制变换，立即终止后续分析，直接降级存储为单字节头部的 RAW 紧凑原始流，杜绝 C++ 原版中曾出现的 2 倍体积膨胀。
3. **十进制除法重构模式（Decimal Division Mode）**：
   - **用途**：消除 IEEE 754 乘法舍入误差导致的“虚假异常点”。
   - **机制**：浮点乘法 `x * 0.1` 无法精确表示十进制小数，会导致大量本可无损还原的工业传感器数据（如 `12.3`）因尾数截断误差而被误判为不可缩放的异常。fastalp 借鉴并优化了除法重构模式，以精确除法将虚假异常彻底清零，使真实环境传感数据的每点占用减少 20%~38%。

---

### 二、fastalp 自主研发的极致原创优化（用于解决什么问题）

为了突破 C++ 原版的吞吐上限与时序压缩率天花板，fastalp 自主研发了以下核心架构创新：

1. **熔合一阶差分位打包（Fused Delta Bitpacking）**：
   - **用途**：消除差分压缩时 8KB 内存回写带来的内存带宽与缓存挤占开销。
   - **机制**：传统实现采用“遍历计算差分并写回 8KB 临时内存 + 另起循环读取临时内存做 Bitpacking”的两遍扫描模式。fastalp 独创 8 路寄存器级熔合流水线：在读取相邻元素求差的同时，直接减去基准、并流水线移位推入 128 位寄存器打包输出，全过程**零临时内存分配、零内存回写**，差分压缩吞吐提升 30% 以上。
2. **数学前置短路差分快筛（Mathematical Delta Early Pruning）**：
   - **用途**：消除对无序/震荡数据无意义的全量一阶差分计算。
   - **机制**：基于数学定理“局部子集的一阶极值跨度必小于等于全局极值跨度”，在决定是否启用差分模式时，仅探测前 16 个采样点。若前 16 项的差分位宽已大于等于 FOR 基准位宽，则数学证明全局差分绝不可能更优，即刻早停跳出，避免了 90% 非平滑序列的全量差分扫描。
3. **4 路流水线无闭包展开编码（4-Way Loop Unrolling & Inlined Pipeline）**：
   - **用途**：释放现代 CPU 超标量流水线的乱序执行与多算术逻辑单元（ALU）吞吐潜能。
   - **机制**：将核心采样与整型缩放循环彻底消除动态闭包与间接跳转，特化为专用的 4 路展开指令流。连续 4 项无异常时走全寄存器极值更新路径，使压缩吞吐从 C++ 原版的 0.84 GB/s 暴增至 **4.4~6.8 GB/s**。
4. **单次比较全等快跳（Identical Floats Fast-Skip）**：
   - **用途**：应对工业断线、设备待机与心跳常数流的极致瞬时压缩。
   - **机制**：在编码入口仅用 1 次 `slice[1] == slice[0]` 快速比对。非全等序列仅耗费 1 个 CPU 时钟周期即可退出；全等序列仅需 11 字节即可压缩 1024 元素（压缩比高达 **744x**，解压吞吐达 **88.9 GB/s**）。
5. **智能离群点剪枝与 0-bit 稀疏常数压缩（Outlier Pruning with 0-bit Compression）**：
   - **用途**：针对 99% 为 0.0 仅有极少突变脉冲的数据集（如财政公共支出 `gov30`），实现百倍压缩比。
   - **机制**：自动将少量脉冲离群值分离到异常字典中，主位流以 0-bit 存储，压缩体积从原版的 2100 字节骤降至 43 字节（压缩比突破 **150x**）。配合前 16 采样离群点快筛，高熵数据 2 个采样点即刻早停，零额外性能损耗。
6. **两级采样探测非十进制全面早停（Non-Decimal Sampling Early Break）**：
   - **用途**：防止对不可压缩浮点数据盲目枚举 170 种乘除因子导致编码性能崩塌。
   - **机制**：在第 1 级 32 点快筛中，若在基础十进制指数下异常率已达 100%，判定为科学高熵浮点，直接跳过第 2 级因子枚举，将不可压缩数据的编码耗时缩减 80%。
7. **栈缓冲融合与异常值单次批量提交（Batched Exception Writing & Zero Extra Allocations）**：
   - **用途**：杜绝动态扩容与堆内存碎片。
   - **机制**：解码与编码全程利用固定大小栈缓存；异常值位置索引与原始值在栈上定长组装后单次批量推入，将异常写出的系统开销降低 50%。对外提供 `compress_into` 与 `decompress_into` 零内存分配接口。
