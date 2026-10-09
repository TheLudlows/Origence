# Origence 验证记录

> 历史 CI 与 PR 证据创建于仓库仍名为 `TheLudlows/openContext` 时；下方保留原始链接以便追溯，当前仓库为 `TheLudlows/Origence`。这些旧链接不表示产品仍使用旧名称。

> 2026-10-09 CI 与工具链策略变更（[PR #12](https://github.com/TheLudlows/openContext/pull/12)）：MSRV 1.98，Linux 原生 CI 使用 Rust 1.98.0，开发/Windows/macOS/Docker 固定 1.98.1。仅 `main` push 和目标 `main` 的 PR 触发；PR 为 2 个 Job（fast-check：无原生特性测试/格式/Python；linux-native：MSRV 身份测试、原生 Clippy/测试、Debug smoke 与评估），main 额外执行 Windows、macOS、Linux/macOS Release 和 Docker 构建/Smoke。以下 1.88/1.96 测试结果保留为历史实测，新版本须独立重新验证。
> 2026-10-09 后续 Linux Native 优化（新 PR，待独立 CI 验证）：PR 与 main 的 `linux-native` 统一运行 Dev/Test/Debug Smoke 和 Debug 关键词评估，不再进行 Linux 原生 Release 冷编译；Cargo Clippy/Test/Debug build 的构建并行度从 `-j 1` 调为 `-j 2`，CMake 并行度保持 2，避免 Kuzu/LanceDB 链接峰值内存过高。main 的 Docker Release 镜像构建及 Smoke 仍保留，macOS/Windows 工作流不变。这是构建配置变更，不是实测耗时下降或 CI 通过证明。


## 2026-10-09：P0/S0 候选修复与 CI 门禁收口（进行中）

- 主干起点 `99f8926`（PR #13/#14 已合入）。S0 在 SQLite 先按授权 scope 的完整 identity 找已有资产，再对 keyword/vector 候选限域；`tests/local_app.rs` 包含 130 个强相关干扰、当前版本、跨 workspace、撤回及 search/resolve 回归；`tests/sqlite_identity.rs` 包含向量候选前置过滤及旧库不升级反例。
- [PR #13 的 CI run 37900697049](https://github.com/TheLudlows/openContext/actions/runs/37900697049) 原生 Clippy 因 `src/storage/sqlite/app.rs:168` 的 `clippy::collapsible_if` 失败；合入后的 [main run 37905676866](https://github.com/TheLudlows/openContext/actions/runs/37905676866) Windows Job 复现同一失败，其余未完成作业不能记为成功。这些证据**不支持**宣布 S0 已验收完成。
- 本轮 P0 只合并嵌套 if 的 Clippy 结构，不改错误类型、授权查询或可见性条件；在 PR `fast-check` 补 `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings`，使这类 Rust 代码风格错误能在原生编译前暴露。Rust 1.98.0 的 full-native Clippy 和 Windows/macOS 检查仍必须执行。
- [PR #14 的 CI run 37902084305](https://github.com/TheLudlows/openContext/actions/runs/37902084305) 仅证明当时分支上的 Linux `-j 2`、Debug Smoke 和测试通过（约 29 分钟），不证明 PR #13 后合并主干全绿。
- 本次提交后新 CI 的具体 run、Clippy/test/smoke 与跨平台结果**待核验**；主干是否启用 Required Checks 是独立仓库治理配置，当前 GitHub API 显示 `main` 未受保护。建议设置 `fast-check`、`linux-native` 为合并必需检查；仓库管理员须通过 GitHub Settings → Rules → Rulesets / Branch protection 设置并验证生效。

## 2026-10-09：PR #11 修复分支验收

验收代码：`8d8d2247c355330be9903fb7e9e3f1d2107e058b`，[PR #11](https://github.com/TheLudlows/openContext/pull/11)，基于 main `73fc51a`。主证据：[run 37871058933](https://github.com/TheLudlows/openContext/actions/runs/37871058933)，event=pull_request；CI checkout 为 PR merge commit `104a1b34df5975681dd4fdbd5fe3410380607cf1`。运行于 2026-10-09 12:03（UTC+8）completed/success，7/7 作业通过，全部日志及两平台评估工件已核验。本节记录修复后的结果；修复前 main 的观测快照见下方历史里程碑表。

| 修复分支作业 | job ID | 已核验结果 |
| --- | --- | --- |
| memory-identity | 113629105658 | success；已读日志，fmt、27 项 lib、58 项无原生集成通过，0 failed/ignored |
| evaluation-adapter | 113629105614 | success；已读日志，Python 5 项通过 |
| windows | 113629105355 | success；已读日志，fmt/clippy、29 项 lib、77 项原生集成及无原生接口检查通过，0 failed/ignored；MCP 生命周期用例通过 |
| container | 113629105631 | success；已读日志，Compose config、镜像 release 构建和 HTTP smoke 通过；检查 readiness、鉴权、记忆发布、身份版本/隔离与关键词检索 |
| msrv | 113629105685 | success；已读日志，Rust 1.88 身份 8 项测试及默认原生后端 all-targets check 通过；不含该版本完整运行测试 |
| native (macos-14) | 113629105633 | success；已读日志，fmt/clippy、29 项 lib、77 项原生集成通过，0 failed/ignored；release 构建、HTTP smoke 与关键词宿主评估通过，工件已核验 |
| native (ubuntu-24.04) | 113629105543 | success；已读日志，fmt/clippy、29 项 lib、77 项原生集成通过，0 failed/ignored；release 构建、HTTP smoke 与关键词宿主评估通过，工件已核验 |

现有 CI 验收已收口。修复将身份 capture 的任务结果改名为 `identity_job`，避免遮蔽原记忆受理响应；MCP get 保留原资产 ID、正文读取与 key 撤销断言，并增加 JSON-RPC error 诊断。三平台各 29 项 lib、77 项集成通过，MCP 生命周期用例均为 ok。修复前 main 的 Linux/macOS/Windows 都是同一处失败（各 29 项 lib 通过、集成 76 通过/1 失败）；该 main 运行已结束，container/MSRV/轻量作业通过，没有第二种已确认失败。

Linux release 冷构建用时 87m44s，macOS 为 72m53s；两者 HTTP smoke 随后均通过，构建耗时不计入检索延迟。容器镜像 release 构建与同一 HTTP smoke 也通过，核验 readiness、鉴权、记忆发布、身份版本/隔离及关键词检索。

### 无模型关键词合成基线

已下载并核验两个平台工件，均包含 manifest、imports、results.jsonl、summary 四个文件。manifest 的 commit 为 CI checkout `104a1b34df5975681dd4fdbd5fe3410380607cf1`，mode=keyword、k=5、model_calls_enabled=false，模型/维度为空。

| 工件 | ZIP SHA256 |
| --- | --- |
| [macOS 11593902620](https://github.com/TheLudlows/openContext/actions/runs/37871058933/artifacts/11593902620) | `2999b88574fccee9a534dccd3ad59d29550c832612338a64b8dcccddc49274b9` |
| [Linux 11595430170](https://github.com/TheLudlows/openContext/actions/runs/37871058933/artifacts/11595430170) | `49b99a0548588bdfe43dea333796bc001d337792080f6589da2b7b0985caaadd` |

- corpus SHA256：`aa8845d4c55f7206f698acfdd0915594cf7e1e35cddcb9497983abe1ede6937a`。
- cases SHA256：`38fb6533f788870a9e216ff4dd10489c7d993fe8e1c5a12bfbaf646ea72c9c4a`。
- 两个平台的两个数据 hash 均与该 checkout 的 `evals/corpus.jsonl` / `evals/retrieval.jsonl` 原始字节相符；各 12 次导入均 completed/published、version=1。逐题 ID、类别、query 与 gold labels 匹配仓库数据；各 24 次请求均成功、effective_mode=keyword。原始命中与导入资产/文档映射、正文 UTF-8 字节区间已核对，独立重算指标与各自 summary 相符。

| 各平台单次运行指标 | macOS | Linux |
| --- | --- | --- |
| 可答 / 不可答 | 22 / 2 | 22 / 2 |
| Document Recall@5 / MRR@5 / nDCG@5 | 均为 0.6363636364（63.6%） | 均为 0.6363636364（63.6%） |
| 请求错误 / 不可答误召回 | 0 / 0 | 0 / 0 |
| 请求耗时 p50 / p95 / p99 | 2.00 / 4.36 / 7.10 ms | 1.44 / 2.10 / 2.26 ms |
| 导入总耗时 | 2641.99 ms | 1456.62 ms |

两平台均为 14 个可答用例完整命中、同一组 8 个返回空结果：retrieval-15–22，含 5 个改写、1 个多来源和 2 个否定问句。当前 keyword 要求查询词集合是单个 chunk 词集合的子集；问句词项与多来源句式的召回应单独改进。本次只有 12 个文档，不能将这些失败归因于 top-100 身份过滤截断；身份候选下推仍需独立反例。

这只是各平台单次顺序运行的合成关键词基线，不是性能压测、真实语义模型或竞品质量结果；不含 Agent 生成、capture 质量和组件消融，不依据这些单次延迟比较平台性能。CI success 表示程序和验收步骤完成，不能替代召回质量达标。后续需固定失败用例，评估 keyword 查询词策略及 vector/hybrid，再扩充人工标注数据。

本轮编辑环境没有 Rust/Docker，运行证据来自 Actions API 和已完成日志。最终验收记录只补 Markdown，不改变已验证的 Rust、测试、工作流、Docker、锁定依赖或评估程序；代码运行证据归属于 `8d8d224`，后续文档提交触发的新 CI 需按其实际状态判断。Rust 1.88 完整运行测试、依赖审计、真实模型/300 用例/消融、断电与备份恢复仍为独立待办。

## 历史里程碑

以下为各轮验收摘要；每条的 commit、CI run 与覆盖范围均保留可追溯，详细日志见 GitHub Actions 及对应计划文档。当前状态以上方 PR #11 节及 [STATUS](STATUS.md) 顶部快照为准。

| 时间 | 范围 | commit / PR | CI run | 结论 |
| --- | --- | --- | --- | --- |
| 2026-10-09 | PR #10 合入基线（修复前快照） | main `73fc51a`；PR head `bcf8f57` | [37868289334](https://github.com/TheLudlows/openContext/actions/runs/37868289334) | 三平台同一处失败（`published` 变量遮蔽致 MCP get 的 asset_id 为 null）；轻量/MSRV/container 通过。PR #11 已修复 |
| 2026-10-08 | 容器构建修复 + 身份模块入口 | — | [37751884631](https://github.com/TheLudlows/openContext/actions/runs/37751884631) | Lance 缺 `google/protobuf/empty.proto` 致容器失败；已补 `libprotobuf-dev` 与 protoc 预检。身份编码 8 项测试入口 |
| 2026-10-08 | 远端 Windows CI 核验 | `9d0ddc1` | [36732894394](https://github.com/TheLudlows/openContext/actions/runs/36732894394) | Windows job `109946901559` success；新增 native/msrv/container 作业；不含 Linux/macOS/release |
| 2026-09-30 | 并行初始化串行化 + 覆盖补强 | — | — | 68 通过；修复并发 `SqliteStore::open` 的 `database is locked`，按路径异步互斥串行；补 superseded/404 断言。计划 [并行串行化](superpowers/plans/2026-09-30-parallel-open-serialization-and-coverage.md) |
| 2026-09-29 | 自动发布（A1）验收 | — | — | 67 通过；writer 直接发布、expected_version 冲突、extract 多事实去重、保存计划重放、候选面删除后旧库兼容。计划 [自动发布实施](superpowers/plans/2026-09-28-auto-publish-implementation.md) |
| 2026-09-28 | M5 本地宿主验收 | — | — | 68 通过；SQLite/LanceDB/Kuzu/本地文件闭环、单宿主、跨库账本、崩溃恢复、删除与共享来源。计划 [M5 交付](superpowers/plans/2026-09-28-local-host-delivery.md) |
| 2026-09-27–28 | M4 适配器验收修复 | — | — | 默认 48、local-storage 60 通过（3 ignored）；LanceDB 维度/数值校验、Kuzu 取消后串行许可。计划 [M4 适配器](superpowers/plans/2026-09-27-local-vector-graph.md) |
| 2026-09-23 | M0 本地库探针 | — | — | 14 项真实文件/子进程检查；确定单宿主进程边界（Kuzu 写宿主排斥第二进程）。[storage-probe](../tools/storage-probe/README.md) |
| 2026-09-23 | 自动初始化与存储设计调整 | — | — | 7 单元测试；移除旧库升级命令；新增 `tests/initialization.rs` |
| 2026-09-21 | 历史基线（PG） | — | — | PG + pgvector 基线；不代表当前运行架构 |
| 2026-10-08 | 身份持久化/API 切片 | `3b9afa5` | [37759685825](https://github.com/TheLudlows/openContext/actions/runs/37759685825) | 8 编码 + 4 SQLite 身份 + Python 5 项通过；release/container 烟测待原生构建 |
| 2026-10-08 | 显式身份表升级 | `0dd6f4e` | [37765440784](https://github.com/TheLudlows/openContext/actions/runs/37765440784) | 8 编码 + 52 无原生集成 + Python 5 项通过；`--offline memory-identity-upgrade` |
| 2026-10-08 | 单身份 capture | `c99baed` | [37768221568](https://github.com/TheLudlows/openContext/actions/runs/37768221568) | 22 lib + 52 无原生集成通过；调用方明确 identity 的 capture、原文区间 |
| 2026-10-08 | 身份状态与来源读取 | `578957d`（PR #6） | [37770696604](https://github.com/TheLudlows/openContext/actions/runs/37770696604) | SQLite 来源/撤回可见性；writer 来源原文 GET；原生 HTTP 待验收 |
| 2026-10-08 | 精确身份只读查找 | `7cc8723`（PR #7） | [37771835095](https://github.com/TheLudlows/openContext/actions/runs/37771835095) | 22 lib + 58 无原生集成通过；`POST /v1/memories/lookup` |
| 2026-10-08 | 上下文标注（I4） | `7cc8723`（PR #7） | [37771835095](https://github.com/TheLudlows/openContext/actions/runs/37771835095) | SearchHit 解码、身份标注、UTF-8 预算边界测试；冲突/类型预算未实现 |
| 2026-10-08 | 精确身份过滤 | `94d45ea`（PR #8） | [37773799817](https://github.com/TheLudlows/openContext/actions/runs/37773799817) | 23 lib + 58 无原生集成 + Python 5 项通过；limit 前身份过滤 |
| 2026-10-08 | 调用方版本前置条件 | `9a42540`（PR #9） | [37775517348](https://github.com/TheLudlows/openContext/actions/runs/37775517348) | 24 lib + 58 无原生集成通过；可选 expected_version（0=未发布，正数=当前版本） |

## 套件覆盖维度

- **生命周期套件**：scope 隔离（未设 scope 默认拒绝、跨 workspace 无泄露）、writer/reader 权限、幂等与冲突、版本追加、来源撤回阻断、墓碑阻断排队发布、取消与 generation、文件撤回、无模型 capture 失败、混合检索显式降级、业务与入队共同回滚、UTF-8 区间保真、历史/恢复标题、角色降级不保留旧权限、HTTP 认证与 key 撤销。
- **进程套件**：真实 HTTP API + 单 Worker、确定性模型 stub、外部模型自报标记不能越权、PDF 子进程解析与页码引用、无效 PDF 失败、第二 Worker 拒绝、强杀后队列恢复且 run_token 增加只发布一个版本、处理中删除不复活、MCP initialize/tools/list/tools/call、同 MCP 进程内 key 撤销后拒绝。

## 持续未验证项

真实付费模型语义质量/费用；主应用 Rust 1.88 完整运行测试（独立探针已通过）；`cargo audit` 与 RustSec 告警（含 fs2/fs4 锁生命周期评估）；断电恢复与在线备份；长期并发/多租户公平性；多 Worker 扩容；向量 generation 在线切换；OS 沙箱隔离；压力/容量/竞品评测；P1 会话主闭环与 P2 增强。完整后续清单见 [STATUS](STATUS.md)。
