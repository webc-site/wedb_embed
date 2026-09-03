---
name: pr
description: 提交 Pull Request (PR) 与技术发帖自动化推广流程规范指南
---

# Pull Request (PR) 与技术发帖自动化流程规范指南

本规范定义从目标仓库检索、去重判别、/tmp 隔离 Fork/Clone 开发、基准对比数据实测注入，到通过 GitHub CLI (`gh`) 自动化发起 Pull Request 及技术互动、实时维护 `outreach.yml` 去重文件的完整操作流程。

---

## 核心原则与表达规范

1. PR 优先原则（PR First）：
   - 凡是可以直接贡献代码、集成算法适配、添加 benchmark 对比数据、或收录进 awesome 列表的仓库，**一律优先创建 Pull Request**，杜绝仅发口头 issue 或空泛评论。PR 直接携带可运行的代码与客观对比数据，采纳率与技术影响力最高。

2. 磁盘隔离与克隆规范（严格使用 /tmp）：
   - **严禁在当前主项目工作区内 clone 任何外部代码库**，防止污染根工作区的 Cargo.toml 与 .gitignore。
   - **必须统一在系统临时目录 `/tmp` 下使用 `gh` 进行克隆**：
     ```bash
     gh repo fork <owner>/<repo> --clone -- /tmp/<repo_name>
     ```
   - **提交 PR 之后必须立即彻底删除 `/tmp/<repo_name>` 目录**，避免浪费本地磁盘。

3. 表达规范（硬性约束）：
   - 禁止表情：严禁在 PR 标题、描述、代码注释或 Issue/Discussion 回复中使用任何表情符号（Emoji）。
   - 禁止加粗：严禁使用 Markdown 加粗语法（严禁 `**...**`）。
   - 第一人称：统一使用第一人称 `I`，严禁使用 `we`。
   - 链接格式：所有项目链接必须使用标准 Markdown 超链接格式：
     - `[fastalp](https://github.com/webc-site/wedb_embed/tree/main/fastalp)`
     - `[crates.io](https://crates.io/crates/fastalp)`
   - 突出性能与压缩比：
     - 解压吞吐：单核纯寄存器 SIMD 解码达到 55 至 77 GB/s（graupel 实测单核时序解码达到 252 Mpt/s）。
     - 压缩吞吐：批量端到端编码达到 6.5 GB/s（graupel 实测时序编码达到 122 Mpt/s）。
     - 压缩比：ALP 论文 31 个公开标准数据集平均压缩比达到 2.29x。
     - 保底机制：内置 Raw Fallback，高熵与噪声浮点序列零膨胀。
   - 拒绝说教与指点他人：交流与发帖时，**只介绍自己的库（fastalp）、自身库的性能实测数据与工程优化探索**。**严禁指点别人怎么做，严禁教导或批评对方代码有缺陷/bug，严禁指导对方仓库如何做架构设计或重构**。尊重上游项目的既有设计权衡。
   - 聚焦相关性，拒绝清单式推销：针对具体技术议题，只分享直接相关的实测发现与优化经验，杜绝一次性倾倒无关的自身功能特性列表。

4. 去重与记录机制（必须严格执行）：
   - 发帖/提 PR 前必须检查 `fastalp/outreach.yml`，确保目标 URL 完全不存在，避免重复发帖。
   - 发帖/提 PR 后必须立即更新 `fastalp/outreach.yml`，记录类型、仓库、PR/Issue 编号、URL、标题与日期。
   - 立即将 yml 变更 commit 并推送至远端分支，保持 `dev` 与 `main` 分支同步。

---

## 一、Pull Request (PR) 自动化核心流程

### 步骤 1：检查判重与 /tmp 隔离 Fork / Clone
- 确认目标仓库未在 `fastalp/outreach.yml` 中记录。
- 使用 `gh repo fork` 创建远程 Fork 并隔离克隆到 `/tmp` 临时目录：
  ```bash
  gh repo fork <owner>/<repo> --clone -- /tmp/<repo_name>
  ```
- 在 `/tmp/<repo_name>` 目录下创建特性分支：
  ```bash
  git -C /tmp/<repo_name> checkout -b feat/add-fastalp
  ```

### 步骤 2：集成依赖与实现算法适配
- 在 `/tmp/<repo_name>` 目录下添加 fastalp 依赖（如 `cargo add fastalp --manifest-path /tmp/<repo_name>/Cargo.toml`）。
- 按照目标仓库的模块分层（如 `src/codec/alp.rs`）实现对应的编解码器接口。
- 接入零堆分配 `compress_into` 和 `decompress_into` 接口，保证空数据、异常值、单值边界完备。

### 步骤 3：准备测试集、运行真实 Benchmark 跑出对比数据并更新文档
- 准备真实测试数据：调用仓库的数据拉取脚本（如 `bash /tmp/<repo_name>/scripts/fetch-data.sh`）或准备公开真实样本。
- 运行全量回归与基准对比：
  - 接入新算法至回归测试套件（如 `tests/roundtrip.rs`），验证 100% 位精确无损还原。
  - 运行基准评测程序（如 `cargo run --release --manifest-path /tmp/<repo_name>/Cargo.toml --bin ...`）。
  - 精确采集新算法在各项数据集下的指标：字节数、bytes/point、压缩比、编码吞吐量（Mpt/s）、解码吞吐量（Mpt/s）。
- 同步更新文档：
  - 将实测跑出的最新对比表格、吞吐数据以及新算法带来的改善（如最佳组合 auto 收益）完整更新写入仓库的 `README.md` 与技术规格文档（如 `docs/format.md`）。
  - 确保代码变更中自带实测数据支撑，严禁无数据空谈。

### 步骤 4：质量自检（全绿验收）
在目标项目工作区必须通过完整静态检查：
```bash
cargo test --manifest-path /tmp/<repo_name>/Cargo.toml
cargo clippy --manifest-path /tmp/<repo_name>/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path /tmp/<repo_name>/Cargo.toml --check
```

### 步骤 5：提交 Commit 与推送到 Fork 远端
```bash
git -C /tmp/<repo_name> add .
git -C /tmp/<repo_name> commit -m "feat(codec): add ALP (Adaptive Lossless Floating-Point) compression codec"
git -C /tmp/<repo_name> push -u origin feat/add-fastalp
```

### 步骤 6：通过 gh pr create 提交 Pull Request
调用 GitHub CLI 发起 PR：
```bash
gh pr create \
  --repo <owner>/<repo> \
  --head <your_username>:feat/add-fastalp \
  --base main \
  --title "feat(codec): add ALP (Adaptive Lossless Floating-Point) compression codec" \
  --body "<PR 说明内容，客观呈现测试结果与对比数据，无表情，无加粗，第一人称 I>"
```

### 步骤 7：立即清理 /tmp 临时目录（杜绝磁盘浪费）
- PR 成功提交后，临时编译产物与克隆仓库不再需要保留。
- 立即彻底删除 `/tmp/<repo_name>` 目录及下载的数据缓存：
  ```bash
  rm -rf /tmp/<repo_name>
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

---

## 二、技术发帖与互动流程 (无法提 PR 时的降级方案)

仅当目标仓库不适合直接提交代码（例如不可复现的 Issue 讨论、架构选型设计讨论、技术咨询等）时，作为辅助手段：

### 步骤 1：翻页检索与候选挖掘
- 使用 `gh api` 跨页（`page=1..10`）深度检索相关关键词（`"ALP" compression`, `"lossless float"`, `"Gorilla"`, `"Chimp"`, `"Elf"` 等）。
- 过滤掉无关内容，挑选技术匹配度高的高价值议题。

### 步骤 2：检查 outreach.yml 判重
- 检索 `fastalp/outreach.yml`，确认未互动过。若已存在，立即跳过。

### 步骤 3：审查目标上下文并撰写回复
- 深入理解 Issue/Discussion 核心技术痛点与上下背景。
- 撰写技术回复：
  - 严格遵守第一人称 `I`，杜绝 `we`。
  - 标准 Markdown 链接：`[fastalp](https://github.com/webc-site/wedb_embed/tree/main/fastalp)` 和 `[crates.io](https://crates.io/crates/fastalp)`。
  - **只介绍自己的库（fastalp）、自身测得的性能数据与具体优化手段，绝不指点对方如何写代码或设计架构**。
  - 严格聚焦当前讨论的技术点，绝不一次性倾倒无关的 feature 清单。
  - 硬性约束：无任何表情符号（Emoji）、无加粗（禁止 `**...**`）、无客套说教。

### 步骤 4：发送回复与更新 yml
- 使用 `gh issue comment <url> --body "<内容>"` 发送。
- 立即写入 `fastalp/outreach.yml`，commit 并推送同步。
