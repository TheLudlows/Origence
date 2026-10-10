# Origence 验证记录

## 2026-10-10 · PDF 解析替换与编码约束

基线 main `4d389f317897d1cc8b47197b225fcf0946811cdf`；实现提交
`4f8af95599b470cd8be91c77393245583821d21d`。其文件树 `4e44d18ab64e48159f5182ed1cfdc164e6be7c3c`
与本地最终提交 `ba6e10a8e8e807af1261ba420a14588b5e5ab367` 完全一致。用 `pdf_oxide =0.3.78`
替换 `lopdf`/`pdf-extract`，关闭默认功能，删除隐藏 `parse-pdf` CLI、
`current_exe()` 重启和 JSON 进程通信。入口通过 `spawn_blocking` 调用库，
单次打开、逐页分块；文件大小在实际读取时限制。保留来源页码、UTF-8
页内区间和整份失败策略，拒绝操作数截断/提前 EOF 的部分提取结果。

本地 Linux 验证环境使用 `CARGO_PROFILE_DEV_DEBUG=0`、
`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_INCREMENTAL=0`。无默认后端使用仓库
声明的 Rust 1.98.0；默认后端复用 Rust 1.98.1 构建缓存，两种工具链的结果
分别记录，不把后者冒充 1.98.0 原生/MSRV 验收。默认后端构建使用现有
vendored Protobuf 的 `PROTOC`/`PROTOC_INCLUDE`，没有修改项目构建要求。

| 检查 | 本地实测结果 |
| --- | --- |
| Rust 1.98.0 · `cargo fmt --all -- --check`、`git diff --check` | 通过 |
| Rust 1.98.0 · `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings` | 通过 |
| Rust 1.98.0 · `cargo test --locked --no-default-features --lib -j 2` | 27 passed，0 failed/ignored |
| Rust 1.98.0 · `cargo test --locked --no-default-features --test local -j 2` | 81 passed，0 failed/ignored；包含全部 9 项新增解析回归 |
| Rust 1.98.1 · `cargo clippy --locked --all-targets -j 2 -- -D warnings` | 通过 |
| Rust 1.98.1 · `cargo test --locked -j 1 --no-fail-fast` | lib 28、local 98 passed，0 failed/ignored；bin/doc 各 0；涵盖真实 vector/hybrid 与完整宿主恢复套件 |
| Rust 1.98.1 · 最终日志配置后的宿主定向复测 | 1 passed、97 filtered，0 failed/ignored；PDF 原始库诊断不再输出 |
| Rust 1.98.1 · `cargo build --locked -j 1` 与 HTTP Smoke | 构建通过；Smoke PASS |
| Python · `python3 -m unittest discover -s evals -p 'test_*.py' -v` | 5 passed |

完整套件在初始实现提交 `a34389b914ba56a252ca8ea06f979c1882ff5829`
（tree `c0174460bfdae28e67285225337885696990e228`）执行。最终提交仅追加
宿主关闭原始 pdf_oxide 日志的配置及对应运维说明；解析器、依赖和测试
未变。对此重新执行最终默认 Clippy、格式检查和 HTTP/CLI/MCP/PDF/恢复
宿主定向回归，不能把先前完整套件当成最终日志配置的全套重跑。

新增的 9 项库/API 回归覆盖：多页与分块区间、中文 ToUnicode 映射及 UTF-8
边界、损坏/截断输入、后页没有可用文本时整份失败、200/201 页边界与空
文档、超过 10 MiB、文档级全文大小、操作数上限后返回的部分文本，以及
文件 IO/格式/UTF-8 错误分类。无默认后端的测试可由普通测试可执行文件
直接调用解析库，不依赖应用 CLI。HTTP 宿主套件另外增加两页上传、原始
Blob 不变、检索页号/parser 标记检查，以及无效 PDF 作业失败且不发布资产。

依赖核对：Cargo 生成锁文件后，旧 `lopdf`/`pdf-extract` 与其专属依赖消失；
原有仍保留包的版本/checksum 未改变；新增 58、删除 16 个包版本（包含新
解析器带来的图像/字体/Office IR 等传递依赖，不声称依赖总数减少）。
源码、测试、构建配置中不再含旧 PDF 实现或 PDF 子进程协议；存储锁测试
保留其独立的跨进程测试用途。CI/Docker/Windows 配置没有 PDF 专用安装步骤，
无需添加外部解析器、OCR 模型或 Python 运行时。

首次解析器构建选择 `office_oxide 0.1.13`，其 `DocumentIR` 新增字段与
pdf_oxide 0.3.78 不兼容；使用 Cargo 将锁文件保持在上游声明的 0.1.9 后
编译通过。默认后端首次 `-j 2` 构建在第三方 DataFusion 归档遇到零长度
对象文件，随后改为 `-j 1` 重试并通过；不修改第三方源码或跳过回归。

[AGENTS.md](../AGENTS.md) 新增入口/库职责、进程引入条件、阻塞任务、资源
限制、取消语义、证据和替换验证要求。旧子进程的 30 秒硬终止能力明确
移除，线程解析不具备硬时间/内存或崩溃隔离保证，见
[OPERATIONS](OPERATIONS.md#pdf-解析)。本节是本地证据；新 PR/main CI、
Windows、macOS 和 Docker 尚未核验，不复用旧图替换或 S0 运行结果。

## 2026-10-10 · S1 真实模型工程实验与独立消融
应用代码 `77d43a859d9c83c93d3a88d4fb0c7928a10d4f16`，tree `6420a9d8e001d469c890266092731d1531130e11`。真实 CPU BGE-small-zh-v1.5 + Qwen2.5-0.5B-Instruct 在新临时 workspace 发布 36 篇来源，一次投影供六配置共用；300 题含开发 200 / 来源独立留出 100，k=5、B=2000 UTF-8 字节。keyword/vector/hybrid-all Recall@5 分别 0.35%/97.57%/94.79%，1800 次 search、1728 次 resolve 错误均为 0。向量/混合的 12 个无答案问题全部误召回；不把请求成功解释为正确拒答。逐题证据与实际渲染引用、预算、版本、组件关闭及两组分数已独立复算一致。
全部结果、局限、配对消融、失败问句、模型与二进制 hash、离线生成成本、命令和原始响应压缩包见 [S1 报告](../evals/s1/results/local-77d43a8-v2/README.md)。保留原 100 题既有人工标签，新增 200 题为助手编写/原文核对，未独立人工复核；不是完整平台 300 用例或生产代表样本。费用为本地 provider 0 USD、CPU 未定价。生成约束保证格式/端点一致，不证明小模型图事实正确。
[源码 PR CI run 38034197676](https://github.com/TheLudlows/Origence/actions/runs/38034197676) attempt 1：Linux/MSRV `114161140456` 与 fast-check `114161140493` 成功，main-only Windows/macOS/container skipped。实际 checkout `442d962de012e1a301726ae627dc932714a281e8` 的 tree 与上述源代码一致。默认 lib29/local89、no-default lib28/local72、Python9、format、Clippy、build/HTTP smoke 均通过；两个真实 LanceDB 身份过滤回归通过。精确计数/名称见 [静态 CI 证据](evidence/2026-10-10-s1-pr24-ci.json)。本地 Linux/Rust1.98.1 同样完成默认/轻量全套、fmt、双 Clippy、HTTP smoke 与 Python9。S0 固定向量测试只验证行为，S1 分数来自真实 BGE；后续证据归档提交没有重跑模型，其 CI 按实际状态另行核验。
## 2026-10-10 · SQLite 图替换主干验收完成
PR #23 已合入 main `9c292a99aec658ba924d5b82c1a86f229ad4d115`，tree `4335d460bc96aa8360a9556c47f9ec65016a2c8b`。[main run 38032343493](https://github.com/TheLudlows/Origence/actions/runs/38032343493) attempt 1 的五个作业全部 completed/success。三平台原始日志 checkout 均为该完整 SHA；默认 lib 各 28、local 各 89，0 failed/ignored/filtered，实际 local 测试名称集合一致；均包含全部 13 个 SQLite 图契约及两个真实 LanceDB 身份过滤 HTTP 回归。Linux 以 1.98.0 验证 MSRV，Windows/macOS 为 1.98.1。Clippy、Linux Debug/macOS Release build 和 HTTP smoke、Docker Release 镜像与 smoke 全通过；Windows 执行既有 Validate 与 no-default check。fast-check 为 no-default lib 27/local 72、Python 5。精确 job、checkout、计数和原始日志摘录见 [主干静态证据](evidence/2026-10-10-sqlite-graph-main.json)。
Linux/macOS 的 24 题 keyword 种子 Recall/MRR/nDCG@5 均为 63.6%，错误与无答案误召回均为 0。该主干验收只证明图替换和既有行为，不替代 S1 真实模型质量实验。以下 PR 阶段记录保留其当时的 skipped 状态。
## 2026-10-10 · SQLite 图替换（PR #23）

基线 main `2b66baaf30c9f4c2dbf3313c99ed5e2a4df41834`。实现提交 `d3222e5ae1e50b955436aace50b53f2a0d981c24`，tree `3e42545bcae14900bebb290bb76d08f87a5692b9`。按 [替换计划](superpowers/plans/2026-10-10-replace-kuzu-with-sqlite-graph.md) 删除 Kuzu/CXX/CMake 图依赖，接入 SQLite 图；本轮不复用旧 S0 主干 run 作为替换证据。

[PR #23](https://github.com/TheLudlows/Origence/pull/23) 的 [run 38030946507](https://github.com/TheLudlows/Origence/actions/runs/38030946507) attempt 1 成功。两个实际执行作业均 completed/success；三个 main-only 作业 skipped。实际合成 checkout 为 `70c3e133ab5025c3e6667a3c0a23a574a14e4e02`，Git 数据 API 已核对它与实现提交的 tree 相同。精确测试名称、计数和评估摘要保存于 [静态证据](evidence/2026-10-10-sqlite-graph-pr23.json)。

| 作业 | 实测结果 |
| --- | --- |
| [fast-check / 114151579844](https://github.com/TheLudlows/Origence/actions/runs/38030946507/job/114151579844) | Rust 1.98.1；format、no-default Clippy `-D warnings`；lib 27、local 72、Python 5 全通过，0 failed/ignored；包含全部 13 个图回归 |
| [Linux Native / 114151579974](https://github.com/TheLudlows/Origence/actions/runs/38030946507/job/114151579974) | Rust 1.98.0/MSRV；默认 Clippy `-D warnings`、完整 lib 28/local 89、Debug build、HTTP smoke、keyword 种子评估全部通过；bin/doc 各 0；身份子集 8 passed/19 filtered |
| Windows/macOS/Docker | PR 按既有 main-only 策略 skipped，不能当作本轮跨平台通过 |

原始 Linux 日志已核验 `s0_vector_identity_prefilter_real_lancedb`、`s0_hybrid_identity_prefilter_real_lancedb`、`saved_publication_recovers_after_external_graph_write`、`pending_write_confirms_and_reconcile_is_idempotent` 为 `ok`。图契约涵盖幂等、scope、三跳/环/起点排除、缺失端点、来源标签、级联删除、并发重放、稳定 snapshot/重开、残缺 schema/外键/view 拒绝。未跳过测试，未放宽 lint。

keyword 固定 12 文档/24 题：22 可答、2 无答案；Recall/MRR/nDCG@5 均 `0.6363636363636364`，请求错误 0、无答案误召回 0，与替换前同种子基线一致。固定向量服务的 vector/hybrid 正确性测试及 keyword 种子不是实际模型质量评测。

本地 Linux/Rust 1.98.1 也已实测 format、默认/no-default Clippy、lib 28/local 89、no-default lib 27/local 72、HTTP smoke、Python 5 与 probe-protoc。首次本地编译在第三方 ttf-parser/arrow-cast 的零长度对象文件处失败，重试通过；CI 独立验证了相同应用代码。最终审查修正了离线身份升级测试中的旧目录断言，改为确认 `graph.db` 不被创建；本地完整 89 项集成套件覆盖该最终断言。后续提交只包含这一测试断言及文档/证据，应用源码不变；其新 CI 不与本次实现 run 混为同一运行。

两个锁文件均由 Cargo 更新：Kuzu/CMake/CXX 依赖消失，保留包版本/checksum 未变。活动 Rust、测试、探针与构建配置不存在旧图引擎符号；历史方案与原始证据仍保留。旧图文件不迁移、已发布账本不自动重建空图，切换限制见 OPERATIONS。


## 2026-10-10：S1 已启动（冻结首版开发数据集）

已新增 `evals/s1/` v1：12 篇合成 source/version 1 文档、100 道人工编写问题（96 道可答、4 道无答案），每道可答题带来源 ID、版本、文档 locator 与原文引句。数据完整性核对确认所有 ID 唯一、来源均存在、金标与 evidence 引句一致且引句逐条出现在来源正文中；SHA-256 记录在 `evals/s1/manifest.json`。本集合仅为开发集，不代表生产样本，也不是调参后的独立留出集。

首轮 keyword@5 已用 Windows 本机构建的 `target/debug/origence.exe` 完成，配置 commit=`47d1375`，结果包保存在 `evals/s1/results/keyword-47d1375/`。100 题中 96 道可答、4 道无答案、0 请求错误；document Recall@5、MRR@5、nDCG@5 均为 `0.0104167`（1/96），仅精确标识 `INC2026XYZ` 命中；无答案误召回为 0。成功请求延迟 p50/p95/p99 为 15.81/32.62/33.66 ms，导入 12 篇文档共 3.10 s。这是小型合成开发集上的本机单轮文档级结果，不能外推生产延迟或整体质量。

为区分运行器故障，同一宿主、同一 commit 另跑原有 24 题种子，得到 22 道可答、2 道无答案、0 错误，Recall/MRR/nDCG 均为 `0.636364`，与历史核验值一致。两套问题分布不同，不能将分数差解释为版本回退。当前 100 题包含大量自然问句及同义问法，极低 keyword 命中表明应在后续 vector/hybrid 对照中检查语义检索收益；不能通过看到结果后删除难题来抬分。vector/hybrid 需要明确 embedding 服务，不能用 S0 固定向量模型充当语义质量结果。

## 2026-10-09：S0 验收完成（Issue #21，主干原生证据归档）

**S0 正确性验收完成。** 本节重新读取 GitHub 的 commit、PR、run/attempt/job 元数据及五个作业的原始日志，不沿用聊天短 SHA 或历史计数。验收代码是已合入 main 的 `42db5c21a8ef4788d07209be7110d85e01d7a85f`；[main run 37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081)，event=`push`，branch=`main`，**attempt 1，completed/success，5/5 作业 success**。最后一个作业于 2026-10-09 11:46:23 UTC（19:46:23 UTC+8）完成。

本次仅归档证据、同步文档，不修改生产代码、测试、依赖或 CI；不新增验收脚本，也不将一次性日志核对当作原生回归执行。以下 run 验证上述代码提交，不冒充随后证据归档提交的 CI。后续归档 PR/merge 的实际 SHA 与检查结果以 [Issue #21](https://github.com/TheLudlows/Origence/issues/21) 的完成记录为准。

### 完整源码对应关系

| 对象 | 完整 SHA / 关系 |
| --- | --- |
| PR #20 base | `e8ee1ae628bd3f4868f3cf981d385ecd30a0a5cd` |
| [PR #20](https://github.com/TheLudlows/Origence/pull/20) 最终 head | `1b9b68f65446c69e0fc030455698d326bc9a7fed` |
| PR #20 CI 合成 checkout | `8b98c33b56a4ab205746eddc40b88e64bc17e4f8`；parents 为上述 base/head |
| PR #20 实际合入 main | `42db5c21a8ef4788d07209be7110d85e01d7a85f`；parents 也是上述 base/head，2026-10-09 10:31:12 UTC 合入 |
| 最终 head / 合成 checkout / main 的 tree | 均为 `d2145fdb6f1b1b15904a74c683a5272f9a2321e2`；提交身份不同，但完整文件树相同 |
| `tests/identity_vector_regression.rs` blob | `b7858ff04e779c1bcde6e782e67acc3c23a9f95b` |
| `tests/local.rs` blob | `e4ebcbd4269748ddc1b9de769551c2fc8a44c6b8` |
| `.github/workflows/ci.yml` blob | `eb65fd5394b37ad4f1f781a2ff4bad65ececc857` |

最终 head 的 [PR run 37917562934](https://github.com/TheLudlows/Origence/actions/runs/37917562934) attempt 1 已成功：fast-check job `113777355953`、linux-native job `113777356326`。两者原始 checkout 日志均为 `8b98c33b56a4ab205746eddc40b88e64bc17e4f8`，并非 run 元数据中的 head SHA；Windows/macOS/container 在该 PR run 为 skipped，**不计入主干跨平台验收**。

此前测试文件 `71c56435e9209f8bec6f3dcd2c012df98c5153b4` 对应格式修订阶段，随后 `4fba2a1a6bc0a54d047ef203c8728a47b11d70af` 为 fixture 注册真实 owner，防止正常启动孤儿清理删除无 owner 的 seeded chunk。测试名称未重命名。`1b9b68f65446c69e0fc030455698d326bc9a7fed` 将关键词评估输出移出 Cargo cache，保留拒绝覆盖策略。当前最终 blob 与历史 blob 不混用。

### main attempt 1 的原始运行证据

以下所有 job 的元数据 `head_sha` 和 checkout 步骤 `git log -1 --format=%H` 输出均为 `42db5c21a8ef4788d07209be7110d85e01d7a85f`。未跨 run 或 attempt 拼接结果。

| 作业 / job ID | 实际工具链与执行 | 套件结果（passed / failed / ignored / measured / filtered） |
| --- | --- | --- |
| [fast-check / 113779676713](https://github.com/TheLudlows/Origence/actions/runs/37918105081/job/113779676713) | Rust 1.98.1，fmt、无原生 Clippy `-D warnings` | lib `27/0/0/0/0`；local `59/0/0/0/0`；Python `5` 项，OK |
| [Linux Native/MSRV / 113779676656](https://github.com/TheLudlows/Origence/actions/runs/37918105081/job/113779676656) | rustc 1.98.0 (`88d9e12ae`)，默认原生后端、Clippy `-D warnings`、Debug build/HTTP Smoke | 单独 MSRV identity 筛选套件 `8/0/0/0/19`；完整 lib `29/0/0/0/0`、local `81/0/0/0/0`；bin/doc 各 `0/0/0/0/0` |
| [Windows / 113779676444](https://github.com/TheLudlows/Origence/actions/runs/37918105081/job/113779676444) | rustc 1.98.1 (`48a229cea`)，MSVC；`build.ps1 -Action Validate -Jobs 1`，fmt/Clippy `-D warnings`；无原生接口 check | lib `29/0/0/0/0`、local `81/0/0/0/0`；bin/doc 各 `0/0/0/0/0` |
| [macOS / 113779676800](https://github.com/TheLudlows/Origence/actions/runs/37918105081/job/113779676800) | rustc 1.98.1 (`48a229cea`)，aarch64；Cargo `-j 2`、CMake 2，Clippy `-D warnings`、Release build/HTTP Smoke | lib `29/0/0/0/0`、local `81/0/0/0/0`；bin/doc 各 `0/0/0/0/0` |
| [Docker / 113779676764](https://github.com/TheLudlows/Origence/actions/runs/37918105081/job/113779676764) | `rust:1.98.1-bookworm` build stage；Cargo `BUILD_JOBS=3`、CMake 2；Compose config、Release 镜像与 HTTP Smoke | 日志包含 Release 构建完成及 Smoke `PASS`；不把镜像构建算成原生测试套件 |

MSRV 的 19 filtered 是明确筛选 `memory_identity` 的独立轻量套件；三平台完整原生 local 套件全部为 0 filtered、0 ignored。原生测试本身均在 Debug/Test profile 运行；macOS 的 Release 是另行构建及 Smoke，不能称作 Release 原生测试。

| 三平台 `tests/local.rs` 内的精确命名用例 | Linux UTC | Windows UTC | macOS UTC |
| --- | --- | --- | --- |
| `identity_vector_regression::s0_vector_identity_prefilter_real_lancedb` | 10:34:42.5312342，ok | 11:12:21.5838745，ok | 10:55:04.0439320，ok |
| `identity_vector_regression::s0_hybrid_identity_prefilter_real_lancedb` | 10:34:42.7258231，ok | 11:12:21.5839712，ok | 10:55:04.1097800，ok |

逐个平台比对了 local 套件的 **81 个实际 `ok` 测试名称，集合完全相同**。既有 `local_app::identity_filter_survives_top_100_distractors_and_stale_versions`、版本/撤回/幂等、并发 expected_version、旧库不隐式升级、SQLite 向量候选身份下推、撤销权限串行化和提交时权限复核均仍为 `ok`。原始日志中的 checkout、工具链、命名用例、套件计数、Smoke 与作业元数据摘录见 [固定证据快照](evidence/2026-10-09-s0-main-37918105081.json)；该文件是静态归档，不是新测试结果。

### 正确性与质量边界

已重新审阅最终 blob：真实 SQLite/LanceDB、130 个更近的其他身份、未限定原生 top-100 排除目标、物理历史 v1 与当前 v2、实际 `origence serve` 子进程及 HTTP search/resolve 全部保留。目标正文无 `needle`，keyword-only 明确为空；两种模式均不能靠关键词假通过。跨 tenant/workspace、缺失身份/条件、503 与 allow_partial、当前来源撤回不回退 v1、墓碑及其他 scope 仍可读的断言均在同一命名用例内执行。

复现命令：

```sh
cargo test --locked -j 2 --test local identity_vector_regression -- --nocapture
```

本次优先复用已成功且完整绑定最终代码的三平台原生套件，没有再执行一次本地定向构建；上述命令是复现入口，不能冒充新的执行记录。loopback 固定向量只证明 S0 正确性，不证明 S1 真实语义召回、整体 P0 完成或生产就绪。

Linux/macOS 同一 run 的关键词种子评估均为 24 cases、22 answerable、2 unanswerable、0 errors，document Recall@5 `0.6363636363636364`，无答案误召回 0，仍仅为合成关键词基线。各自 retrieval artifact 为 `11610027623` / `11613712549`；本节核对日志输出和 artifact 元数据，不声称重新下载核验工件内容。未启动 S1/S2/S3。

领取时 main 的 `protected=false`，继承 rulesets 为空；本任务不修改分支治理设置。正常 PR 检查及合并独立进行，不由 S0 通过宣称 Required Checks 已配置。

## 2026-10-09：S0 vector/hybrid 定向回归（PR #20，历史验收进行中记录）

> 下述为 PR #20 初期的原始观测；当前验收状态以上方完整 main 证据为准。保留当时的失败、待核验状态和 blob，避免用最终成功改写历史。

**状态：测试代码已提交，不能提前声明 S0 已闭环。** [PR #20](https://github.com/TheLudlows/Origence/pull/20) 基于 main `e8ee1ae628bd3f4868f3cf981d385ecd30a0a5cd`。测试初始提交 `90b50fea8cd93f154ab278d8cb7068f04f9cb303`；按 CI 的 Rust 1.98 rustfmt 输出修正为 `f2daf5b48e95d5f115cf7700e47369944cd8c9a3`，测试文件 blob 为 `71c56435e9209f8bec6f3dcd2c012df98c5153b4`。文档提交不改变该测试源码，后续修复须重新记录版本。

### 用例与可复现范围

`tests/identity_vector_regression.rs` 通过现有 `tests/local.rs` 编入同一个原生集成可执行程序，无新增独立原生 Job。

- `identity_vector_regression::s0_vector_identity_prefilter_real_lancedb`
- `identity_vector_regression::s0_hybrid_identity_prefilter_real_lancedb`

```sh
cargo test --locked -j 2 --test local identity_vector_regression -- --nocapture
```

每个用例使用真实 SQLite、真实 LanceDB、实际 `origence serve` 子进程和 HTTP `/v1/search`、`/v1/resolve`；只把 embedding 模型替换为独立 loopback 固定向量服务，不修改全局环境，不调用付费/外部模型。

| 断言 | 防止的假通过或回归 |
| --- | --- |
| 130 个不同身份的更近向量，原生无限定 top-100 确实不含目标 | 先证明存在候选挤出，不能用一个没有干扰的目标测试代替 |
| 带完整身份时 vector/hybrid 的 search 和 resolve 只返回目标当前 v2 | 检验候选前置限定、原生排名、HTTP 绑定与上下文链路 |
| 目标正文与关键词不匹配，keyword-only 明确为空 | hybrid 不能靠关键词分支掩盖向量路径问题 |
| v1 向量物理存在，候选可见性只选当前 v2 | 旧版本不能凭更高向量得分成为当前事实 |
| 同一身份在不同 workspace、不同 tenant 中各有自己的资产 | 验证业务身份与鉴权 scope 共同限定结果 |
| 不存在主体、不同环境条件均为空 | 不允许退回其他主体或部分条件匹配 |
| 模型失败：严格模式 503；allow_partial 显式降级 keyword 并保留身份过滤 | 不静默降级，不泄露其他主体的关键词结果 |
| 撤回当前来源后 search/resolve 为空，而有效 v1 仍能显式 GET | 不自动回退历史版本 |
| 资产墓碑后对应 search/resolve 为空，其他 tenant 仍可读 | 删除与范围隔离不回归 |

固定向量仅验证正确性，不提供真实语义模型 Recall、延迟或费用结论，不代替 S1 质量评估。

### 已观测的运行证据

| 修订 / run | 结果与范围 |
| --- | --- |
| `90b50fe` / [37914896553](https://github.com/TheLudlows/Origence/actions/runs/37914896553) | 首次 fast-check 在 rustfmt 处失败；已按差异修正，不能记为测试成功 |
| `f2daf5b` / [37915145655](https://github.com/TheLudlows/Origence/actions/runs/37915145655) | fast-check 已通过；新用例受 local-storage 特性控制，轻量通过并不证明这两个原生用例通过；完整原生结果待核验 |
| 原 main `e8ee1ae` / [37913212181](https://github.com/TheLudlows/Origence/actions/runs/37913212181) | Linux Native 已成功；不含 PR #20 两个新用例，不得用作本轮最终验收证据 |

**最终关闭条件**：取得包含这两个测试的已合入 main commit；其 fast-check、Linux Native/MSRV、Windows、macOS、Docker Smoke 全部成功；读取三个原生平台的日志，记录两个命名测试为 `ok`、完整套件实际计数、run/job ID、源码与 merge SHA，再同步 STATUS 和路线图。pending/skipped/cancelled 不等于成功，不能推算或复用历史测试计数。仓库分支保护为独立治理配置，不因 S0 测试成功自动完成。

## 2026-10-09：CI 并行度与命名记录

> macOS 的 Clippy、完整测试和 Release Build 从 Cargo `-j 1` 调为 `-j 2`（CMake 仍为 2）；Linux Docker 镜像 CI 的 Cargo `BUILD_JOBS` 从 2 调为 3，构建阶段显式设置 `CMAKE_BUILD_PARALLEL_LEVEL=2`。macOS/容器 Job 只在 main 执行；PR 成功不能替代这两项的实际通过、内存峰值和耗时验证。

> 历史 CI 与 PR 证据创建于仓库仍名为 `TheLudlows/openContext` 时；下方保留原始链接以便追溯，当前仓库为 `TheLudlows/Origence`。这些旧链接不表示产品仍使用旧名称。

> [PR #12](https://github.com/TheLudlows/openContext/pull/12) 将 MSRV 调整为 1.98：Linux 原生 CI 使用 Rust 1.98.0，开发/Windows/macOS/Docker 固定 1.98.1；PR 为 fast-check 和 linux-native，main 追加 Windows、macOS 和容器验证。PR #14 移除了 Linux Native Release，统一 Dev/Test/Debug Smoke 和 Debug 关键词评估，Cargo 并行度调为 2；macOS 和 Docker 保留 Release。这些配置与历史 Rust 1.88/1.96 的验收证据应分别记录。

## 2026-10-09：P0/S0 候选修复与 CI 门禁收口（历史推进记录）

- 主干起点 `99f8926`（PR #13/#14 已合入）。S0 在 SQLite 先按授权 scope 的完整 identity 找已有资产，再对 keyword/vector 候选限域；`tests/local_app.rs` 包含 130 个强相关干扰、当前版本、跨 workspace、撤回及 search/resolve 回归；`tests/sqlite_identity.rs` 包含向量候选前置过滤及旧库不升级反例。
- [PR #13 的 CI run 37900697049](https://github.com/TheLudlows/openContext/actions/runs/37900697049) 原生 Clippy 因 `src/storage/sqlite/app.rs:168` 的 `clippy::collapsible_if` 失败；合入后的 [main run 37905676866](https://github.com/TheLudlows/openContext/actions/runs/37905676866) Windows Job 复现同一失败，其余未完成作业不能记为成功。这些证据不支持当时宣布 S0 已验收完成。
- **PR #15 已合入修复**：合并嵌套 if，不改错误类型、授权查询或可见性条件；fast-check 补 `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings`。不能继续把该告警记为尚未修复；新代码运行是否通过应读取对应 CI。
- [PR #14 的 CI run 37902084305](https://github.com/TheLudlows/openContext/actions/runs/37902084305) 仅证明当时分支上的 Linux `-j 2`、Debug Smoke 和测试通过（约 29 分钟），不证明 PR #13 后合并主干全绿。
- 当时 API 显示 main 未受保护；建议设置 fast-check、linux-native 为合并必需检查。当前仓库设置必须另行读取，不能由历史配置或一次 CI 结果推断。

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

以下保留各轮验收摘要；每条 commit/run 及覆盖范围均留存，详细日志见 GitHub Actions 与对应计划。当前状态以本文最上方最新 S0 记录和 [STATUS](STATUS.md) 为准，旧证据不能自动套用到新源码。

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
- **S0 定向回归（PR #20 新增，尚待原生验收）**：真实 LanceDB 的 vector/hybrid top-100 身份过滤、三种 scope 对照、当前/旧版本、撤回/墓碑和显式降级；不与模型质量指标混淆。

## 持续未验证项

真实付费模型语义质量/费用；`cargo audit` 与 RustSec 告警（含 fs2/fs4 锁生命周期评估）；断电恢复与在线备份；长期并发/多租户公平性；多 Worker 扩容；向量 generation 在线切换；OS 沙箱隔离；压力/容量/竞品评测；P1 会话主闭环与 P2 增强。当前 Rust 最低版本为 1.98.0；旧 Rust 1.88 完整运行测试的未验收历史记录不等于当前仍支持 1.88。完整后续清单见 [STATUS](STATUS.md)。
