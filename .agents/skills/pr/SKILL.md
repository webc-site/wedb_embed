---
name: pr
description: 提交 Pull Request (PR) 与技术发帖自动化推广流程规范指南
---

# Pull Request (PR) 与技术发帖自动化流程规范指南

本规范定义从目标仓库检索、去重判别、本地 Fork/Clone 开发、基准对比数据注入，到通过 GitHub CLI (`gh`) 自动化发起 Pull Request 及技术互动、实时维护 `outreach.yml` 去重文件的完整操作流程。

---

## 核心守则与表达规范

1. 表达规范（硬性约束）：
   - 禁止表情：严禁在 PR 标题、描述、代码注释或 Issue/Discussion 回复中使用任何表情符号（Emoji）。
   - 禁止加粗：严禁使用 Markdown 加粗语法（严禁 `**...**`）。
   - 第一人称：统一使用第一人称 `I`，严禁使用 `we`。
   - 链接格式：所有项目链接必须使用标准 Markdown 超链接格式：
     - `[fastalp](https://github.com/webc-site/wedb_embed/tree/main/fastalp)`
     - `[crates.io](https://crates.io/crates/fastalp)`
   - 突出性能与压缩比：
     - 解压吞吐：单核纯寄存器 SIMD 解码达到 55 至 77 GB/s。
     - 压缩吞吐：批量端到端编码达到 6.5 GB/s。
     - 压缩比：ALP 论文 31 个公开标准数据集平均压缩比达到 2.29x。
     - 保底机制：内置 Raw Fallback，高熵与噪声浮点序列零膨胀。
   - 拒绝说教与多余客套：客观陈述技术原理与测试数据，严禁指导别人如何使用，严禁添加客套废话。

2. 去重与记录机制（必须严格执行）：
   - 发帖/提PR前必须检查 `fastalp/outreach.yml`，确保目标 URL 完全不存在。
   - 发帖/提PR后必须立即更新 `fastalp/outreach.yml`，记录类型、仓库、PR/Issue 编号、URL、标题与日期。
   - 立即将 yml 变更 commit 并推送至远端分支，保持 `dev` 与 `main` 分支同步。

---

## 一、技术发帖与互动流程 (Issue / Discussion)

### 步骤 1：翻页检索与候选挖掘
- 使用 `gh api` 跨页（`page=1..10`）深度检索相关关键词：
  - 核心关键词：`"ALP" compression`, `"floating point" compression`, `"lossless float"`
  - 竞品关键词：`"Gorilla"`, `"Chimp"`, `"Chimp128"`, `"Elf"`, `"Patas"`, `"BtrBlocks"`, `"ByteStreamSplit"`, `"pcodec"`, `"pco"`
- 对检索结果进行技术匹配度打分（相关技术场景：时序数据库、列存、向量/浮点波形压缩、数据湖格式）。过滤掉无关项目与垃圾贴。

### 步骤 2：检查 outreach.yml 判重
- 读取 `fastalp/outreach.yml` 中的已记录 URL 集合：
  ```bash
  grep "url:" fastalp/outreach.yml
  ```
- 确认当前候选 issue/discussion/PR 尚未被互动过。若已存在，立即跳过。

### 步骤 3：审查目标上下文并撰写回复
- 使用 `gh issue view <url> --json title,body,comments` 详细审查对方的技术疑问或痛点。
- 撰写精准切中其痛点的回复（遵守第一人称、Markdown 链接、突出 55~77 GB/s 与 2.29x、无表情、无加粗、不说教）。

### 步骤 4：发送回复与更新 yml
- 发送评论：
  ```bash
  gh issue comment <url> --body "<内容>"
  ```
- 立即将返回的评论链接记录写入 `fastalp/outreach.yml`：
  ```yaml
    - type: issue_comment
      repo: <owner>/<repo>
      issue: <number>
      url: <comment_url>
      title: "<title>"
      status: posted
      date: YYYY-MM-DD
  ```
- 提交并推送：
  ```bash
  git commit -am "chore(fastalp): track new outreach in outreach.yml" && git push origin dev
  ```

---

## 二、Pull Request (PR) 自动化流程

适用于可直接 Clone、本地接入对比或添加收录的算法库、基准测试库与开源项目（如 `graupel` 等）。

### 步骤 1：检查判重与创建 Fork / Clone
- 确认目标仓库未在 `fastalp/outreach.yml` 中记录。
- 使用 `gh repo fork` 创建远程 Fork 并克隆到本地目录：
  ```bash
  gh repo fork <owner>/<repo> --clone -- <local_dir>
  cd <local_dir>
  git checkout -b feat/add-fastalp-benchmark
  ```

### 步骤 2：集成依赖与实现算法适配
- 使用包管理器接入算法库（如 Rust 项目执行 `cargo add fastalp`）。
- 按照目标仓库的模块分层（如 `src/codec/alp.rs`）实现对应的编解码器接口。
- 接入零堆分配 `compress_into` 和 `decompress_into` 接口，保证空数据、异常值、单值边界完备。

### 步骤 3：准备测试集、运行真实 Benchmark 跑出对比数据并更新文档
- 准备真实测试数据：调用仓库的数据拉取脚本（如 `scripts/fetch-data.sh`）或准备公开真实样本。
- 运行全量回归与基准对比：
  - 接入新算法至回归测试套件（如 `tests/roundtrip.rs`），验证 100% 位精确无损还原。
  - 运行基准评测程序（如 `cargo run --release --bin ...` 或 `cargo run --release --example compare`）。
  - 精确采集新算法在各项数据集下的指标：字节数、bytes/point、压缩比、编码吞吐量（Mpt/s）、解码吞吐量（Mpt/s）。
- 同步更新文档：
  - 将实测跑出的最新对比表格、吞吐数据以及新算法带来的改善（如最佳组合 auto 收益）完整更新写入仓库的 `README.md` 与技术规格文档（如 `docs/format.md`）。
  - 确保代码变更中自带实测数据支撑，严禁无数据空谈。

### 步骤 4：质量自检（全绿验收）
在目标项目工作区必须通过完整静态检查：
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

### 步骤 5：提交 Commit 与推送到 Fork
```bash
git add .
git commit -m "feat(codec): add ALP floating-point compression codec"
git push origin feat/add-fastalp-benchmark
```

### 步骤 6：通过 gh pr create 提交 Pull Request
调用 GitHub CLI 发起 PR：
```bash
gh pr create \
  --repo <owner>/<repo> \
  --head <your_username>:feat/add-fastalp-benchmark \
  --base main \
  --title "feat(codec): add ALP (Adaptive Lossless Floating-Point) compression" \
  --body "<PR 说明内容，客观呈现测试结果与对比数据，无表情，无加粗>"
```

### 步骤 7：清理本地 Clone 目录（避免浪费磁盘空间）
- PR 成功提交后，本地编译产物与克隆仓库不再需要保留。
- 立即彻底删除本地克隆目录及下载的数据缓存，杜绝磁盘冗余浪费：
  ```bash
  rm -rf <local_dir>
  ```

### 步骤 8：更新 outreach.yml 并同步主干
- 提取 PR URL，追加至 `fastalp/outreach.yml`：
  ```yaml
    - type: pull_request
      repo: <owner>/<repo>
      pr_number: <number>
      url: <pr_url>
      title: "<pr_title>"
      status: submitted
      date: YYYY-MM-DD
  ```
- 提交跟踪文件并推送到远端分支，合并同步到 `main`。
