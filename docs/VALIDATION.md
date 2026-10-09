# 验证记录

## 2026-10-09：PR #11 修复分支验收

验收代码：`8d8d2247c355330be9903fb7e9e3f1d2107e058b`，[PR #11](https://github.com/TheLudlows/openContext/pull/11)，基于 main `73fc51a`。主证据：[run 37871058933](https://github.com/TheLudlows/openContext/actions/runs/37871058933)，event=pull_request；CI checkout 为 PR merge commit `104a1b34df5975681dd4fdbd5fe3410380607cf1`。运行于 2026-10-09 12:03（UTC+8）completed/success，7/7 作业通过，全部日志及两平台评估工件已核验。本节记录修复后的结果，下节保留修复前 main 的观测快照。

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

## 2026-10-09：PR #10 合入基线（09:39 快照）

核验基线：main `73fc51a2671bc3d7895794b1981a5fdc48f7ce9b`，[PR #10](https://github.com/TheLudlows/openContext/pull/10) 已合入。状态观测于 2026-10-09 09:39（UTC+8），以下为当时结果；没有 conclusion 的作业不标通过。本轮编辑环境没有 Rust/Docker，记录来自 GitHub Actions API 和已完成作业日志。本次分支另外修复下述测试变量遮蔽，修复后的 CI 需单独验收，不能复用基线通过数。

主证据：[main run 37868289334](https://github.com/TheLudlows/openContext/actions/runs/37868289334)，event=push；PR head `bcf8f57765f8e29db137ebd77f0f2006f7ef12ec` 的 [run 37868153300](https://github.com/TheLudlows/openContext/actions/runs/37868153300) 作为辅助证据，不替代 main 的结果。

| main 作业 | job ID | 当前实际结果 | 尚未覆盖 |
| --- | --- | --- | --- |
| memory-identity | 113620169079 | success；已读日志，fmt、27 项 lib、58 项无原生集成均通过，0 failed/ignored | local-storage 下的 retrieval/Worker/HTTP/CLI 原生用例 |
| evaluation-adapter | 113620169069 | success；已读日志，Python 5 项通过 | Rust 宿主真实运行及真实模型质量 |
| native (macos-14) | 113620169057 | failure；fmt/clippy 通过，29 项原生 lib 通过；集成 76 通过、1 失败 | 测试变量修复后的完整集成、release smoke 与关键词宿主评估；原运行后两项被跳过 |
| native (ubuntu-24.04) | 113620168980 | fmt 已通过，clippy 运行中 | 完整 clippy/测试、release smoke、关键词宿主评估 |
| windows | 113620168588 | 构建/验证脚本运行中 | 当前代码完整生命周期测试与无原生接口检查 |
| msrv | 113620168975 | Rust 1.88 身份领域测试已通过，默认后端 all-targets check 运行中 | 默认后端 check 结果；该作业本身不含 Rust 1.88 完整运行测试 |
| container | 113620169043 | Compose config 通过，Docker 构建运行中 | 当前镜像构建结果与 HTTP smoke |

### 已确认历史失败与修复

[run 37776541957](https://github.com/TheLudlows/openContext/actions/runs/37776541957)，PR head `a5d0cedcc527477d2c9e6e554adc2d5698eef19d`，conclusion=failure。读取 job `113308733015` 和 `113308733188` 日志后确认：

- `tests/local_app.rs` 的 `first_body` 换行不符合 rustfmt，导致多个作业在格式门失败，后续 clippy/测试未执行。提交 `197f6aec8220ecfdfd170ad3b645e2a76b3c1c69` 按格式输出修正；当前 main 的轻量和 native 格式步骤已通过。
- 该次 Docker 镜像实际构建成功，但 readiness 请求遇到 `ConnectionResetError` 后 smoke 直接失败。提交 `bcf8f57765f8e29db137ebd77f0f2006f7ef12ec` 仅在既有 readiness 等待中增加 `ConnectionError` 重试，保留 120 秒总超时；后续鉴权、发布和检索错误仍会失败。Python 语法检查通过，修复后的真实容器 smoke 仍需等待当前作业结果。
- 早先 `google/protobuf/empty.proto` 缺失已补 `libprotobuf-dev`；旧运行的镜像构建成功证明修复覆盖了该构建问题，不证明当前镜像 smoke 通过。

### 完成边界与后续核验

当前 main 的 macOS 日志确认 `local_host_api_cli_mcp_models_and_recovery` 在 `tests/local_app.rs:892` 失败，原因是原文字符串读取为 None。身份 capture 切片在同一测试函数中用 `let published = job(...)` 遮蔽前面的记忆受理响应；后续 MCP get 使用 `published["asset_id"]`，而任务资产在 `result.memories` 中，发出的参数实际为 null。该失败发生在新增身份/版本/过滤及撤回断言之后，不是 MCP 正文读取契约改变。

本次将新变量改为 `identity_job`，保留原记忆受理响应给 MCP get 使用，并在读取正文前断言无 JSON-RPC error，后续失败可直接显示响应。未删除或放宽原有读取正文、key 撤销和恢复断言。修复后的 fmt/clippy/原生生命周期测试仍待新 CI；本地仅完成 diff --check 和源码绑定核对，不宣称 Rust 测试通过。

P0 基础实现和 I1/受限 I2 的轻量行为验证成立；当前 main 全量 CI 未通过，PR 合入与成功步骤不能替代全部原生/平台验收。剩余作业应逐项读取结果和失败日志，失败先定位再修复；取消的旧运行不记为通过，也不为取得绿灯盲目重跑已通过项。

取得 release 评估工件后须核验 manifest 的 commit、数据 hash、模型关闭状态、imports、逐题响应和 summary，再报告 12 文档/24 查询的合成种子结果。当前没有真实模型质量、费用、完整 300 用例、组件消融或竞品分数。Rust 1.88 完整运行测试、cargo audit、断电恢复、长期规模及备份恢复仍为独立待办。

以下为历史记录，保留其当时的失败、修复和未验证边界；当前 CI 状态以本节及 [STATUS](STATUS.md) 顶部快照为准。

## 2026-10-08：容器构建失败与身份模块验收入口

[PR 运行 37751884631](https://github.com/TheLudlows/openContext/actions/runs/37751884631) 的 container job `113227369753` 在 Docker release 编译阶段实际失败：`lance-encoding v1.0.1` 调用 protoc 时找不到 `google/protobuf/empty.proto`，因此尚未进入镜像 smoke。已读取失败日志。

修复：Docker build 阶段与 Linux native/MSRV 作业显式安装 `libprotobuf-dev`，并在 Rust 编译前让 protoc 导入该 well-known type 生成 descriptor。Debian 包文件清单确认该包提供 `/usr/include/google/protobuf/empty.proto`：[官方文件清单](https://packages.debian.org/bookworm/amd64/libprotobuf-dev/filelist)。此修复不修改运行镜像依赖；修复后镜像构建/运行尚待 CI。

新增记忆身份独立模块及 8 项 Rust 单元测试；专用 CI 以默认关闭原生后端的 lib 测试检查身份模块，MSRV 作业也运行这组测试后再检查默认后端。默认应用的完整 Windows/native 验收继续保留。编辑环境没有 Rust/Docker，本轮不宣称新 Rust 测试或镜像已通过。

## 2026-10-08：远端 Windows CI 核验与后续验收入口

本轮通过 GitHub Actions API 核验已有运行，不重新执行或改写 2026-09-30 的本地测试记录：

- [运行 36732894394](https://github.com/TheLudlows/openContext/actions/runs/36732894394)，commit `9d0ddc1550cdfbfbb06ae93cfa8ea8ec81832de1`，2026-09-30，Windows job `109946901559`，conclusion=success。
- 作业步骤显示工具准备/格式/clippy/本地生命周期测试及 no-default-features 基础接口检查成功；已读取作业日志。此证据只对应该 commit 和 Windows 作业，不证明 Linux/macOS/release、主应用 Rust 1.88 或容器通过。
- 新增 native（Linux/macOS）、msrv（1.88 默认后端 all-targets check）和 container 作业；release/container 通过 `tools/smoke.py` 使用隔离数据、关闭模型，检查 ready、未认证拒绝、记忆发布和关键词检索。MSRV check 不替代该版本的运行测试。
- 本轮编辑环境没有 cargo、rustc、Docker 或原生构建工具，未在此环境执行 Rust 编译、全套测试或镜像运行。新增 CI 结果需单独记录，仍为待验收。

本轮后续追加：`python -m unittest discover -s evals -p 'test_*.py' -v` 实测 5 项通过，覆盖文档去重、多来源 Recall/nDCG、无答案口径、失败请求计零与凭据不写报告。HTTP fixture 不是 Rust 运行或语义质量验证。已加入 native release 后的真实关键词评估与公开合成结果工件。

后续计划见 [验收与评估批次](superpowers/plans/2026-10-08-validation-and-evaluation.md)。下文历史记录中的“远端 CI 未验证”描述当时记录状态，当前 Windows 证据以上述链接为准。


## 2026-09-30：并行初始化串行化与覆盖补强

环境：Windows x64/MSVC、Rust/Cargo 1.96.0，锁定仓库依赖。本轮修复测试套件默认并行执行下 `initialization::sqlite_concurrent_initialization_is_serialized` 稳定失败（SQLite `code: 5 database is locked`，当日 4/4 复现、隔离执行通过）的问题：同进程并发 `SqliteStore::open` 现按路径经异步互斥串行完成连接与初始化（跨进程仍由 OS 文件锁拒绝）；同时补齐 2026-09-29 声明的未验证断言与遗留代码 Minor。实施映射见 [并行初始化串行化计划](superpowers/plans/2026-09-30-parallel-open-serialization-and-coverage.md)。

| 检查 | 实测结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --locked --all-targets -j 1 -- -D warnings` | 通过 |
| `cargo test --locked -j 1 --no-fail-fast` | **68 通过，0 失败，0 ignored**；8 项单元 + 60 项集成（新增竞态 superseded 用例 1 项），集成执行约 20.86 秒，非性能基准 |
| 并行回归 | 修复后 `cargo test --locked -j 1 --test local`（默认并行 test 线程）连续 5 次全绿；修复前同命令当日 4/4 失败于并发初始化用例 |
| `cargo check --locked --no-default-features --lib -j 4` | 通过 |

本轮新增/调整的验收覆盖：

- 同进程并发打开同一数据库：两个并发 `SqliteStore::open` 串行完成连接与初始化，默认并行套件下不再出现 `database is locked`；跨进程打开仍被 OS 锁拒绝。
- accept 响应字段：首次受理 `conflict:false` 与 `source_event_id` 存在性显式断言。
- extract 任务结果：`readiness:"ready"` 与 `index_capabilities`（keyword/vector）显式断言。
- 已删除路由：`GET /v1/candidates`、`GET /v1/candidates/{id}`、`POST /v1/candidates/{id}/review` 断言 404。
- 竞态 superseded：同一 expected_version 的两个发布任务，先提交者发布、后提交者在提交复核处 superseded，不产生第三个版本。

未验证：沿用既有清单（Linux/macOS/release、容器镜像构建和运行、远端 CI、主应用最低 Rust 1.88、在线备份、断电恢复、长期压力/规模与真实模型语义效果）。

## 2026-09-29：自动发布验收

环境：Windows x64/MSVC、Rust/Cargo 1.96.0，锁定仓库依赖。本轮为 A1 治理语义变更（自动发布），存储与运行栈沿用 M5 本地宿主：默认启用 local-storage，测试使用真实临时 SQLite/LanceDB/Kuzu 与本地文件，模型为本机 HTTP stub，无外部付费调用。行为契约见 [API](API.md)，实施映射见 [自动发布实施计划](superpowers/plans/2026-09-28-auto-publish-implementation.md)。

| 检查 | 实测结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --locked --all-targets -j 1 -- -D warnings` | 通过 |
| `cargo test --locked -j 1 --no-fail-fast` | **67 通过，0 失败，0 ignored**；8 项单元 + 59 项集成（sqlite_domain 删 2 项候选审核用例、initialization 增 1 项新库结构检查，较 M5 基线 68 少 1），集成执行约 21.03 秒，非性能基准 |
| `cargo check --locked --no-default-features --lib -j 4` | 通过；基础接口构建无需启用原生后端 |

本轮新增/调整的验收覆盖：

- writer 直接发布：writer 提交结构化记忆后 publish 任务直达 `outcome=published`，无候选/审核中转；reader 不能读取任务，提交按 Write 权限复核。
- accept 期 expected_version 冲突检测：受理时 fact_key 已有当前版本则响应 `conflict:true`，expected_version 记录受理时版本供 Worker 提交前复核。
- extract 多事实发布与 fact_key 去重：一次 capture 抽取的多条事实按 fact_key 定位资产槽逐条独立发布，同批重复 fact_key 去重；空抽取以 `result.memories:[]` 完成。
- 保存计划重放：外部写完成后保存的多记忆发布计划可重放，不产生重复版本。
- 候选面删除后旧库兼容：新库无 `oc_candidates`/`oc_reviews` 与 `oc_versions.review_id`；遗留多余表的旧库仍通过结构检查。

未验证：其余沿用既有清单（Linux/macOS/release、容器镜像构建和运行、远端 CI、主应用最低 Rust 1.88、在线备份、断电恢复、长期压力/规模与真实模型语义效果）。竞态 superseded 与已删除路由 404 的断言已于 2026-09-30 补齐，见上方 2026-09-30 节。

## 2026-09-28：M5 本地宿主验收

环境：Windows x64/MSVC、Rust/Cargo 1.96.0，锁定仓库依赖。默认启用 local-storage，测试使用真实临时 SQLite/LanceDB/Kuzu 与本地文件；模型为本机 HTTP stub，无外部付费调用。当前部署和 API 契约见 [文档索引](README.md)，实现映射见 [M5](superpowers/plans/2026-09-28-local-host-delivery.md)。

| 检查 | 实测结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --offline --locked --all-targets -j 16 -- -D warnings` | 通过 |
| `cargo build --offline --locked --lib -j 16` | 通过；并行构建原生库，后续测试串行链接 |
| `cargo test --offline --locked -j 1 --no-fail-fast` | **68 通过，0 失败，0 ignored**；8 项单元 + 60 项集成，集成执行约 20.54 秒，非性能基准 |
| `cargo check --offline --locked --no-default-features --lib -j 4` | 通过；基础接口构建无需启用原生后端 |
| `cargo test --offline --locked --test local local_app -j 1` | 最终调整共享 SQLite 关闭顺序后，4 项应用定向回归全部通过（约 23.48 秒） |
| CLI `--help` | 本地 serve、workspace/key 管理、search/get/resolve、MCP 和显式 offline；无 DATABASE_URL/runtime-setup/独立 worker 命令 |
| 运行依赖核对 | Apalis 已从依赖图移除；正常运行依赖未启用 sqlx-postgres；不需要 PG 服务 |
| `docker compose config --quiet` | 通过；Docker CLI 提示本机用户配置不可读，但配置校验退出码为 0 |
| 构建脚本 PowerShell 语法检查 | 通过；实际构建采用上列手动命令，不把脚本语法检查等同于跨机器安装验收 |
| 文档本地链接、`git diff --check` | 通过；20 份 Markdown 文档的本地链接无断链 |

本轮新增/补强的验收：

- 自动创建空库、重复/并发 SQLite 初始化；缺领域列或部分 Kuzu 表结构拒绝启动且不静默补建。Kuzu 新库 DDL 在单事务创建。
- IMMEDIATE 写事务与撤销串行化；旧授权不能开启新事务；候选撤回/作业取消严格限定事务 scope。
- HTTP/CLI/MCP 共享单宿主；离线命令与第二宿主受 OS 锁限制；宿主不可达不回退；MCP 每次调用重新授权并拒绝已撤销 key。
- 候选审核、授权首次发布、知识版本、追加恢复、源撤回、幂等、中文关键词和 reader/writer 角色限制；原始 PDF 上传、子进程解析、字节下载校验。
- keyword/vector/hybrid、摘要原文证据、图一跳与共享 owner、HTTP/MCP 图引用及统一预算。向量先过滤关系库可见候选再 top-k，旧/隐藏产物不能挤占结果。
- 模型处理中取消→新 generation 重试、撤销创建者和删除资产，迟到结果不能发布；强杀真实宿主后重启立即恢复 processing，只发布一个版本。
- 在真实存储构造“图已写、SQLite 仅 pending、版本未发布”的边界，重开清理并重放已保存计划；检查一次发布。暂停 Worker 后撤回来源，残留原生图/向量不能映射为有效证据，随后清理收敛。
- graceful host shutdown 等待 Worker 停止；原生后端关闭钩子先于共享 SQLite 队列/关系连接池关闭。

构建与修复记录：首次环境未在 PATH 中找到 Ninja，补齐 VS CMake/Ninja 与 vendored protoc 后通过。移除 PG 特性导致原生依赖缓存重建；单线程原生编译中止后改为 `cargo build --lib -j 16`，测试维持 `-j 1` 避免并发链接内存压力。初次静态检查发现一处嵌套条件风格问题，新增发布计划字段时发现函数参数误替换，均已修正后重新验证；这些中间失败不计入通过结果。

未验证：Linux/macOS/release、当前 Linux 容器镜像构建和运行、远端 CI、主应用最低 Rust 1.88、在线备份、断电恢复、长期压力/规模与真实模型语义效果。`docker info` 因本机 Docker daemon 未运行而失败，不能复用旧 PG 镜像成功记录证明本地镜像可运行。保存计划边界注入与真实进程强退是两类不同测试，不宣称覆盖所有崩溃指令点。

以下各节保留历史记录，描述当时环境与限制，不代表当前运行架构或本轮验收范围。

## 2026-09-27–28：M4 本地适配器验收修复

环境：Windows x64/MSVC、Rust/Cargo 1.96.0，锁定仓库依赖，使用真实临时 SQLite/LanceDB/Kuzu 数据文件，无外部模型调用。此轮验证的是存储适配器，CLI/API/Worker/MCP 仍运行 PG 基线。

| 检查 | 实测结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --locked --offline --all-targets -j 4 -- -D warnings` | 通过 |
| `cargo clippy --locked --offline --all-targets --features local-storage -j 16 -- -D warnings` | 通过；修正 Kuzu 两处嵌套条件风格告警，逻辑不变 |
| `cargo test --locked --offline -j 1` | **48 通过，3 ignored**；默认构建不启用 LanceDB/Kuzu |
| `cargo test --locked --offline --features local-storage -j 1` | **60 通过，3 ignored**；含 LanceDB 6、Kuzu 4、跨库账本组合 1，以及 Kuzu 取消回归单元测试 1 |
| `git diff --check`、更新文档的本地链接检查 | 通过；历史未跟踪的 target 报告不在链接检查范围 |

本轮新增回归覆盖：

- LanceDB 拒绝零/溢出维度、声明维度与向量长度不符、NaN/Infinity；批次中非法输入不会先写入其他 profile 的向量。
- profile 已有表的维度冲突及查询声明维度不符返回 `StorageError::Conflict`；`limit=0` 返回空结果。查询显式绕过 ANN 索引，与精确检索能力声明一致。
- 幂等键包含 scope、来源 ID/版本、产物 ID 和 generation；同产物的不同 generation、来源版本、profile、tenant/workspace 可并存。重开数据库后仍可读取，重复删除只清除指定 scope/来源版本。
- Kuzu 异步调用取消后，尚未结束的原生阻塞操作仍持有串行许可，后续操作不会提前进入；结束后正常释放许可。

构建排障：初次默认并行度运行本地全套测试，MSVC 链接报 `LNK1102`（内存不足），并出现页面文件不足引起的 metadata 映射失败；改用 `-j 1`。首轮回归还发现 LanceDB 的 schema 错误需映射为统一的 `Conflict`，已补齐。以上失败不计入通过结果。

未覆盖：PG 的三个 ignored 集成测试、应用级本地宿主/检索融合/命中可见性复核、共享 owner 清理编排、真实崩溃注入与重启恢复、最低 Rust 版本、Linux/macOS/release、容器及远端 CI。既有账本组合测试验证 pending→幂等外部写→committed→对账，不等同于完整故障恢复验收。

## 2026-09-23：M0 本地 Rust 库探针（Windows 通过）

隔离工程位于 [tools/storage-probe](../tools/storage-probe/README.md)，直接依赖 SQLx 0.8.6、LanceDB 0.23.1（Lance 1.0.1、Arrow 56.2.1）、Kuzu 0.11.3。Windows x64/MSVC、Rust 1.88.0 完整可执行文件构建和 **14 项真实文件/子进程检查通过**。主应用尚未接入这些库；探针通过不代表生产后端已经交付。

| 后端 | 检查数 | 实际通过范围 |
| --- | --- | --- |
| SQLite | 4 | 重复初始化、跨 tenant/workspace 同 ID（含引号）、删除后重开；双进程写入与长连接刷新；已确认写入后强退；WAL 下未提交写不可见、写锁竞争、强退回滚及后续写入 |
| LanceDB | 3 | 相同 scope/重开检查及带 scope 预过滤的精确向量 top-1；双进程追加与长连接刷新；已确认写入后强退 |
| Kuzu | 7 | 相同 scope/重开检查；已确认写入后强退；带 scope 的边及 DETACH DELETE；读写宿主文件锁与单宿主命令访问；双只读进程；未提交事务强退回滚；共享 Database 的跨线程连接在写事务前后读取正确快照 |

关键输出：SQLite 竞争写入报 `database is locked`；Kuzu 写宿主存在时，第二个读写或只读进程均报 `Could not set lock on file`。单宿主多连接通过，因此本地目标调整为 API/Worker 同进程共享引擎。LanceDB 使用 `read_consistency_interval(Duration::ZERO)`，持久连接能看到其他进程的提交。

实际命令：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/storage-probe/build.ps1 -Toolchain 1.88.0 -TargetDirectory target/storage-probe-msrv -Jobs 16
python tools/storage-probe/verify.py --binary target/storage-probe-msrv/debug/opencontext-storage-probe.exe
```

完整报告：`target/storage-probe/runs/run-69exqwqr/report.json`（历史本地输出，不作为仓库链接）。报告和临时数据库留在本地、不纳入 Git，脚本可重建结果。之前也在 Rust 1.96.0 以 `--no-default-features --features sqlite,vector` 构建并通过 SQLite 4 项、LanceDB 3 项检查；不将其描述为三个库在 1.96.0 上的完整构建通过。

本轮固定的构建条件：

- MSVC C++ 工具链、CMake、Ninja；Lance 使用构建辅助 crate 提供的 protoc。这些是构建工具，测试没有启动数据库服务。
- Kuzu 固定 `cxx = 1.0.138`，但默认解析到的 `cxx-build 1.0.202` 生成不兼容符号，最终链接出现 117 个未解析符号。探针显式固定生成器为 1.0.138。
- Kuzu 保留默认扩展特性。关闭默认特性时，原生构建仍含扩展加载器，Rust 未链接扩展，出现 algo/fts/json/vector 的 4 个未解析符号；恢复默认特性后最终链接和运行通过。LanceDB 关闭默认云端特性。

未覆盖：Linux/macOS、release、预编译 Kuzu 外部库分发、并发初始化和完整结构兼容检查、生产权限/来源校验、完整 API/Worker 及 CLI/MCP 转发、Worker OS 锁与队列、跨库账本、ANN、长期并发和断电恢复。LanceDB 探针写入为唯一测试 ID 的追加，尚非业务幂等 upsert；强杀进程不等于断电测试。后续按 M1–M5 接入和验收。

## 2026-09-23：自动初始化与存储设计调整

- `cargo fmt --all -- --check`、`cargo clippy --locked --offline --all-targets -- -D warnings`、`cargo test --locked --offline` 通过；7 个单元测试通过。
- CLI `--help` 已确认移除旧数据库升级命令。
- 新增 `tests/initialization.rs`：在独立临时数据库验证连接时自动初始化、并发启动、重复初始化保留数据、队列入队函数、不兼容结构拒绝且数据保留、不创建升级历史表。CI 已配置运行此测试。
- initialization、lifecycle、processes 三个数据库集成套件本次均未执行：本机未配置测试数据库环境变量，Docker daemon 未运行。因此尚未验证本次 SQL 初始化定义在真实 PostgreSQL 上的执行结果，也未运行容器构建。
- 此次自动初始化改动未接入 SQLite/LanceDB/Kuzu；独立 M0 探针的后续运行记录见上节。

## 历史基线

以下为 2026-09-21 历史基线，不能作为本次自动初始化实现的验收记录。

日期：2026-09-21。Windows 本机 Rust 1.96.0，独立 Docker PostgreSQL 17 + pgvector，数据库管理员与 `oc_runtime` 分离。容器构建使用 Rust 1.96 / Debian bookworm。

| 检查 | 实际结果 |
| --- | --- |
| `cargo check --locked --offline` | 通过 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | 通过 |
| `cargo test --locked --offline` | 3 个单元测试通过；2 个需要数据库的测试明确 ignored |
| `cargo test --locked --offline --test lifecycle -- --ignored` | 实际数据库运行通过，1 个综合套件，约 2.5 秒 |
| `cargo test --locked --offline --test processes -- --ignored` | 实际子进程运行通过，1 个综合套件，约 16.7 秒 |
| `docker compose config --quiet` | 通过 |
| `docker build -t opencontext:local .` | Linux release 镜像构建成功 |
| 独立 `opencontext-smoke` Compose 项目 | 数据库初始化/运行角色、workspace 创建、HTTP ready、Worker 发布、get 正文、Files 卷上传均通过；完成后停止服务 |

以上时间只是单次测试耗时，不是服务性能指标。CI 工作流已加入仓库；本记录不代表远端 CI 已运行或通过。

## 生命周期套件覆盖

RLS 未设置 scope 时默认拒绝、跨 workspace 写入失败、跨 workspace get/search/candidate/job/file 无泄露；writer 不能直接确认记忆；reader 无候选/原文件权限；同键相同请求与并发复用、不同请求冲突；重复执行不增加版本；过期审核拒绝；冲突旧值保留；恢复追加 v3；来源撤回阻断相关历史版本；墓碑阻断排队发布及重试；取消与 generation；文件撤回；无模型 capture 明确失败；混合检索显式降级；业务与 Apalis 共同回滚；长 UTF-8 文本及空白区间保真；历史标题和恢复标题；角色降级不保留旧权限；HTTP 认证和 key 撤销。

## 进程套件覆盖

真实 HTTP API + 独立 Apalis Worker；本地确定性模型 stub 的 embedding、hybrid、extract；外部模型自报发布标记不能越权；PDF CLI 子进程解析及页码引用；无效 PDF 失败；第二个 Worker 被拒绝；处理中杀死 Worker 后真实队列恢复、run_token 增加且只发布一个版本；处理中删除不复活；MCP initialize/tools/list/tools/call；同一 MCP 进程下一次调用拒绝已撤销 key。

## 未测试或不包含

真实付费模型供应商的语义质量/费用；主应用的 Rust 1.88 最低版本（独立存储探针已通过）；CI 云端执行；生产 SSO、物理擦除和备份恢复；长期并发/多租户公平性；多 Worker 扩容；向量 generation 在线切换；OS 沙箱隔离；压力/容量/竞品评测。完整后续清单见 [STATUS](STATUS.md)。

## 结构化身份持久化/API 切片（2026-10-08）

上一轮身份编码 8 项测试与 MSRV 检查已通过。新增 `sqlite_identity` 的复用/发布读取/跨 scope 主体条件隔离、墓碑、旧槽碰撞、无隐式迁移测试；轻量 CI 无需原生后端即可执行。release/container 烟测新增真实身份发布、同身份版本 2 和条件隔离。[CI run 37759685825](https://github.com/TheLudlows/openContext/actions/runs/37759685825) 在 `3b9afa5ecbf08b816a3c7dc3f355874c79227a2e` 通过 fmt、8 项身份编码、4 项 SQLite 身份测试与 5 项 Python adapter 测试。默认原生后端编译、完整 HTTP 发布烟测、容器和平台验收仍在运行，不将轻量测试等同于完整运行验收。旧库缺表时新入口 503；旧接口继续可用，capture 身份匹配仍待实现；显式升级切片见下节。

## 旧库显式身份表升级（2026-10-08）

新增 SQLite 升级用例覆盖：预检不安装、安装与重复幂等、旧资产/来源/版本保留且不创建映射、新身份入口可写；不存在文件不新建；同进程和跨进程锁拒绝；基础列不兼容不修复；身份表缺约束拒绝；同名视图保留。CLI 用例验证 `--offline` 要求、锁冲突、三种状态及不加载原生存储。release/container 烟测增加新库升级预检。[CI run 37765440784](https://github.com/TheLudlows/openContext/actions/runs/37765440784) 在代码提交 `0dd6f4ee1b23616f742461fcc27813ade239bff8` 通过 fmt、8 项身份编码、52 项无原生集成（含 6 项新增升级）和 5 项 Python 测试。原生 CLI、release/container 及跨平台验收仍待执行，不能由轻量测试代替。新库初始化执行完整 SQL 脚本，避免分号注释被误拆成 SQL。

## 单身份 capture（2026-10-08）

新增证据解析测试：UTF-8 原文区间、零结果、重复 quote、伪造/越界区间、单身份不同断言、模型身份/scope 字段注入与批量上限。原生 HTTP 测试增加真实 Worker 的身份 capture、相同身份版本追加与资产身份读取（model stub，仅验收行为）。轻量 CI 扩为全 lib 测试。[CI run 37768221568](https://github.com/TheLudlows/openContext/actions/runs/37768221568) 在代码 `c99baed4044567550af512fae3d2412cd433cc4c` 通过 fmt、22 项 lib（含 8 项新证据解析）、52 项无原生集成和 5 项 Python 测试。原生 HTTP/跨平台/release/container 用例仍待验收；没有真实模型语义质量证据。

## 身份状态与来源读取（2026-10-08）

SQLite 新增源事件 scope/撤回可见性、文件删除阻断、三类 normalization_status 测试。原生 HTTP 用例新增失败 capture 原文读取、reader 403、跨 scope 404 和撤回后 404。PR #6 已合入 `578957d`；[CI run 37770696604](https://github.com/TheLudlows/openContext/actions/runs/37770696604) 的 macOS fmt 在 `bccdad2` 通过，新增 SQLite 与原生 HTTP 验收仍未完成。状态字段不构成语义真值保证，原文 GET 不开放给 reader。

## 精确身份只读查找（2026-10-08）

新增 3 项 SQLite 用例：reader 当前版本读取与主体/条件/scope 隔离；未命中提交后资产/身份/来源/任务表均无新增；未发布、墓碑和撤回不可见。旧库用例增加缺表 503 契约。原生 HTTP 用例增加 reader 无 Idempotency-Key 查找当前版本与跨 scope 404。Python adapter 5 项本地通过，Rust 格式、编译和新增测试待 CI；未验收自动匹配或真实模型质量。

## PR #7 轻量验收与上下文标注（2026-10-08）

[CI run 37771835095](https://github.com/TheLudlows/openContext/actions/runs/37771835095) 在代码 `7cc8723` 通过 fmt、22 项 lib 与 58 项无原生集成，包含 PR #6 的 3 项来源/身份状态与 PR #7 的 3 项精确查找 SQLite 用例。原生 HTTP、平台/release/container 仍在运行，不能由轻量测试替代。

本轮新增旧 SearchHit 解码与三类状态测试；原生 render 用例验证完整身份标注、UTF-8 及恰好/不足一个字节的预算边界；原生 HTTP 用例验证 reader search/resolve 的身份与引用。Python 5 项本地通过，新增 Rust 测试和原生断言待 CI。

## 精确身份过滤（2026-10-08）

新增 SearchInput/ResolveInput 默认兼容、MCP Schema 属性与未知 scope 字段拒绝测试；身份匹配覆盖无过滤、旧未识别、主体差异、条件差异、知识类型排除。原生 HTTP 验证相同关键词 production/staging 在 limit=1 下分别命中，并验证 hybrid resolve 不携带其他身份或图对象。Python 5 项本地通过，新增 Rust/HTTP 用例待 CI；PR #8 的轻量 CI run 37773799817 在 `94d45ea` 已通过 fmt、23 项 lib、58 项无原生集成与 Python 5 项；原生 retrieval 编译、上下文预算/HTTP 与平台验收仍待完成。

## 调用方版本前置条件（2026-10-08）

新增 3 项无原生单元用例：省略/null 不改变规范幂等 payload；0/正数对未发布与当前版本的匹配/冲突；负数在受理前拒绝。原生 HTTP 覆盖两个入口的过期/0/负数拒绝、capture 0 创建与 1 更新、当前已到 v2 后原幂等请求仍重放原结果，后续读取仍为 v2。Python adapter 5 项本地通过，Rust fmt、编译与新增/原生用例待 CI；PR #9 完整验收仍需完成。

PR #9 轻量验收补记：[CI run 37775517348](https://github.com/TheLudlows/openContext/actions/runs/37775517348) 在 `9a42540` 通过 fmt、24 项 lib 和 58 项无原生集成；原生 retrieval/HTTP 与平台验收仍未完成。该证据不覆盖本轮新增版本前置条件测试。

