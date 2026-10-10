# Origence · 当前验收摘要

本页记录当前验收结果。CI 通过、测试通过与产品效果分别判断。

## S1 v2 检索实验

代码 `77d43a859d9c83c93d3a88d4fb0c7928a10d4f16`，36 篇合成来源、300 题（开发 200/留出 100），288 可答/12 无答案。BGE-small-zh-v1.5，六配置共 1,800 次 search、1,728 次 resolve，零请求错误。

| 配置 | Recall@5 | Coverage@2000B | 无答案误召回 |
| --- | ---: | ---: | ---: |
| keyword | 0.35% | 0.35% | 0/12 |
| vector | 97.57% | 98.61% | 12/12 |
| hybrid-all | 94.79% | 95.66% | 12/12 |

摘要相对纯原文 Recall 变化为 0；加图下降 2.78 个百分点。详细开发/留出分数、排名、延迟、usage、费用、逐题审计和模型 hash 见[完整 S1 报告](../evals/s1/results/local-77d43a8-v2/README.md)。新增 200 题尚未独立人工复核；单轮 CPU 延迟不是负载测试，留出集结果不能代表生产质量。CPU 成本未定价。

## 当前 CI 与实现验收

- **SQLite 图替换**：PR #23 已合入 main `9c292a99aec658ba924d5b82c1a86f229ad4d115`。main run [38032343493](https://github.com/TheLudlows/Origence/actions/runs/38032343493) 五个作业全部成功；Linux、Windows、macOS 均为 lib 28/local 89，包含 13 项图回归；Docker Smoke 成功。逐 job 证据见 [主干证据](evidence/2026-10-10-sqlite-graph-main.json)。
- **S1 实验代码 PR #24**：run [38034197676](https://github.com/TheLudlows/Origence/actions/runs/38034197676) 的 Linux/MSRV 与 fast-check 两个实际作业成功，默认 lib 29/local 89、轻量 lib 28/local 72、Python 9、Clippy/fmt/build/HTTP smoke 通过；Windows/macOS/container 按 main-only 策略跳过。该 PR 运行不能单独证明这些平台已验收。精确计数见 [CI 静态证据](evidence/2026-10-10-s1-pr24-ci.json)。
- **S0 身份过滤**：跨平台 main run [37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081) 五作业成功，属于 PR #23 合入前代码；只作为 S0 正确性证据。
- **PDF 解析**：`pdf_oxide 0.3.78` 保留页码、UTF-8 来源范围和整份失败策略；取消/隔离边界见 [OPERATIONS](OPERATIONS.md#pdf-解析)。

整理后的本地文档链接检查与 `git diff --check` 通过。Rust 1.98.0 `cargo fmt --all -- --check` 和 `cargo clippy --locked --all-targets -j 2 -- -D warnings` 已在前一轮通过；本轮只改文档，未重跑代码测试或远端 CI。
