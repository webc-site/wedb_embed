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

3. 表达与沟通规范（硬性约束）：
   - 只介绍自己的库：发帖、评论与 PR 描述中，**只介绍自身库（fastalp）、实测性能数据、基准对比与工程实现**。
   - 严禁说教与指点他人：**严禁指点别人怎么做，严禁教导或批评对方代码有缺陷/Bug，严禁指导对方仓库如何做架构设计或重构**。尊重上游与第三方项目的既有设计权衡（如原算法偏重向量化乘法吞吐等取舍）。
   - 聚焦相关性，杜绝清单式推销：严格针对具体议题的核心痛点进行交流，只讨论直接相关的技术模块（如遇采样仅讨论采样，遇计数器仅讨论差分）。严禁无差别倾倒无关特性列表。
   - 事实严谨性验证：凡涉及浮点数原理（如 IEEE 754 舍入、ULP 差异）、算法边界或性能断言，提交前必须先用本地代码（Rust/Python）严格验证（例如 `0.35` 存在乘法 1-ULP 误差，而 `12.3` 则无误差），严禁凭借直觉编写未经实测的反例。
   - 禁止表情：严禁在 PR 标题、描述、代码注释或 Issue/Discussion 回复中使用任何表情符号（Emoji）。
   - 禁止加粗：严禁使用 Markdown 加粗语法（严禁 `**...**`）。
   - 第一人称：统一使用第一人称 `I`，严禁使用 `we` 或 `our`。
   - 链接格式：所有项目链接必须使用标准 Markdown 超链接格式：
     - `[fastalp](https://github.com/webc-site/wedb_embed/tree/main/fastalp)`
     - `[crates.io](https://crates.io/crates/fastalp)`
   - 核心性能指标基准：
     - 解压吞吐：单核纯寄存器 SIMD 解码达到 55 至 77 GB/s（graupel 实测单核时序解码达到 252 Mpt/s；chimp 基准实测解码延迟统一为 0.423 µs/1000 点即 423 ns，折合 23.6 亿点/秒，较 Gorilla 5.920 µs、Chimp 9.270 µs 提速 14x 至 22x）。
     - 压缩吞吐：批量端到端编码达到 6.1 至 23.1 GB/s（graupel 实测时序编码达到 122 Mpt/s；chimp 基准实测编码延迟统一为 2.255 µs/1000 点，较 Gorilla 6.042 µs、Chimp 8.631 µs 提速 2.7x 至 3.8x）。
     - 压缩比：ALP 论文 31 个公开标准数据集平均压缩比达到 2.29x；真实气象水文数据达到 1.225 bytes/point（13.1x vs uncompressed）；chimp 基准（气象、德股、磁盘真实混合时序）实测达到 16.34 bits/val（优于 Chimp128 17.29 bits/val、Patas 21.51 bits/val、Gorilla 52.70 bits/val）。
     - 保底机制：内置 Raw Fallback，高熵与噪声浮点序列零膨胀。

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
- 按照目标仓库的模块分层（如 `src/codec/fastalp.rs`）实现对应的编解码器接口。
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
# 若本地存在全局 SSH rewrite 规则导致 SSH 鉴权失败，使用 GIT_CONFIG_GLOBAL=/dev/null 走 HTTPS 令牌推送：
GIT_CONFIG_GLOBAL=/dev/null git -C /tmp/<repo_name> push "https://x-at-01:$GH_TOKEN@github.com/<your_username>/<repo_name>.git" feat/add-fastalp
```

### 步骤 6：通过 gh pr create 提交 Pull Request
PR 正文编写硬性标准：
- 结构清晰：包含背景契合度（Motivation & Fit）、改动概述（Summary of changes）、fastalp 架构特性（Key architectural highlights）、实测基准数据对比（Measured results）以及测试验收（Testing）。
- 严禁批评他方：严禁包含任何指责其他实现缺陷的语言，全部围绕 fastalp 自身能力展开陈述。
- 无表情、无加粗、第一人称 I。

调用 GitHub CLI 发起 PR：
```bash
gh pr create \
  --repo <owner>/<repo> \
  --head <your_username>:feat/add-fastalp \
  --base main \
  --title "feat(codec): add fastalp floating-point compression codec" \
  --body "<符合规范的 PR 说明内容>"
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

仅当目标仓库不适合直接提交代码（例如架构选型讨论、算法理论交流、无法直接落地的咨询议题等）时采用：

### 步骤 1：检索与候选挖掘
- 使用 `gh api` 深度检索相关关键词（`"ALP" compression`, `"lossless float"`, `"Gorilla"`, `"Chimp"`, `"Elf"` 等）。
- 挑选技术契合度高、正在讨论浮点压缩痛点的议题。

### 步骤 2：检查 outreach.yml 判重
- 严格检索 `fastalp/outreach.yml`，确认未互动过。若已存在，立即跳过。

### 步骤 3：审查目标上下文并针对性撰写回复
- 深入阅读主贴与跟帖讨论，定位对方最关心的单点技术难题。
- 严禁套用大而全的模板，按以下专业范式针对性应答：
  - **若讨论采样开销**：只分享 fastalp 的下界成本剪枝（lower-bound pruning）与小位宽零异常快速退出机制（采样延迟 ~200 ns），附实测吞吐。
  - **若讨论单调计数器/时序**：只分享 fastalp 在标度整型上的自适应差分（Delta-ALP）与离群值平滑隔离，附位宽压缩数据。
  - **若讨论内存占用与零拷贝**：只分享 `compress_into` 与 `decompress_into` 的单 pass 寄存器流式写入设计与栈复用。
  - **若讨论列存随机访问**：只讨论 Frame-of-Reference (FOR) 定宽整型映射的 O(1) 随机寻址能力，对比 XOR 连续流式依赖。
- 硬性约束复核：第一人称 `I`、标准链接、无表情、无加粗、不说教他人。

### 步骤 4：发送回复与更新 yml
- 使用 `gh issue comment <url> --body "<内容>"` 发送。
- 立即写入 `fastalp/outreach.yml`，commit 并推送同步。

---

## 三、历史发帖动态审查与维护机制

当表达原则或技术规范发生演进时，必须对历史记录进行回溯审查与就地优化：
1. 提取 `outreach.yml` 中所有未关闭的历史 Issue、Comment 与 Discussion。
2. 审查是否存在说教措辞、批评他人实现、未验证反例、或者倾倒无关特性的问题。
3. 利用 GitHub CLI 批量原地修正更新：
   - 更新评论：`gh api -X PATCH repos/<owner>/<repo>/issues/comments/<id> -f body="..."`
   - 更新 Issue：`gh issue edit <url> --title "..." --body "..."`
   - 更新 Discussion 评论：调用 GraphQL `mutation { updateDiscussionComment(...) }`
   - 更新 PR 说明：`gh pr edit <url> --body "..."`
4. 同步修正 `outreach.yml` 中记录的标题与元数据，保持全流程严谨一致。
