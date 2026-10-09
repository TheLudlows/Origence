# 实现状态与后续工作

更新：2026-10-09。当前代码基线为 main `73fc51a2671bc3d7895794b1981a5fdc48f7ce9b`（PR #10 已合入）。M5 本地宿主与 A1 自动发布已实现；P0 完整交付验收尚未收口，P1 主闭环及 P2 尚未交付。实际检查及平台范围见 [VALIDATION](VALIDATION.md)，使用入口见 [文档索引](README.md)。

## 当前能力与验收快照

以下区分代码已实现、运行验收通过与效果已验证；历史切片的“待 CI”描述只表示记录当时状态。

| 范围 | 当前实现 | 已核验证据与剩余边界 |
| --- | --- | --- |
| M5 / A1 | SQLite/LanceDB/Kuzu/本地 Blob，单宿主与单 Worker，直接发布 | Windows 历史完整验收通过；当前代码原生/平台/release/container 验收仍在运行 |
| I1 记忆身份 | v1 编码、唯一绑定、显式写入、离线安装、精确 lookup、调用方版本前置条件 | 当前 main fmt、27 项 lib、58 项无原生集成通过；原生 HTTP/Worker 用例待完成 |
| I2 抽取匹配 | 调用方完整身份的单身份 capture、原文区间、来源读取与绑定状态 | 无原生解析/SQLite 用例通过；自动推断、属性目录、未归一化状态和语义冲突未实现 |
| I4 上下文 | 类型/身份标注、完整身份过滤、引用与 UTF-8 字节预算 | 默认请求/身份匹配测试通过；原生 render/HTTP 待完成，冲突/类型预算未实现 |
| 检索质量 | keyword/vector/hybrid、摘要与一跳图扩展；12 文档/24 查询合成种子 | Python adapter 5 项通过；当前宿主评估作业待完成，无真实模型质量/成本及组件消融结果 |
| P1 会话到经验 | 目标设计已明确 | session/feedback/guidance/learning/improve 主闭环未实现 |
| P2 增强与服务化 | 目标设计已明确 | 时间有效期、GraphCompletion、企业身份、配额和生产后端未实现 |

当前 CI：[main run 37868289334](https://github.com/TheLudlows/openContext/actions/runs/37868289334)。macOS fmt/clippy 与 29 项原生 lib 通过，集成 76 通过、1 失败；已定位为综合测试中 `published` 变量遮蔽导致 MCP get 的 asset_id 为 null，本次分支已修复但新 CI 未验收。其余原生作业仍在运行。完整 job/commit 证据与核验时间统一记录在 VALIDATION；当前 main 全量 CI 未通过，不能将轻量测试通过等同于全部平台支持。

## 已交付

- 单宿主共享引擎：先初始化、检查、恢复任务与对账清理，再运行 HTTP 和单 Worker。OS 文件锁拒绝另一进程打开同库；Kuzu 同步调用通过有界阻塞执行器串行执行。
- `OC_DATA_DIR` 统一数据目录；CLI 的 `serve`、workspace/key 管理、search/get/resolve 和 MCP。客户端默认转发 HTTP；离线显式 `--offline`，宿主不可达不回退。
- SQLite 显式 scope 过滤，复合外键、WAL、IMMEDIATE 写事务、原子幂等/审计/入队；撤销与短写事务串行化，外部 IO 后重新认证和核验提交条件。
- writer 结构化记忆与 capture 抽取直接发布（A1 自动发布）：无候选/审核表，expected_version 乐观并发，账本化多事实发布；知识入库、不可变版本、历史标题、追加恢复、文件上传和文本 PDF 解析。
- 持久化发布计划与跨库账本；确定性产物 ID、generation/run_token 隔离、取消、有限自动重试、显式重试和启动恢复。外部产物只有在 SQLite 最终提交后可作为证据。
- 中文预分词关键词检索、精确向量检索、摘要/图谱证据与 RRF、引用和保守预算。最终结果核验当前版本、来源、资产和授权。
- 删除立即阻断读取，后台按 SQLite owner 清理派生数据；共享实体和关系有其他有效来源时保留，最后一个 owner 消失才删除。
- 普通测试用临时数据目录运行，涵盖 API/CLI/MCP/PDF 子进程、本地模型模拟和恢复；CI、容器配置及文档统一到单宿主。

旧 PostgreSQL 运行代码、Apalis SQL 和专用测试从活动树裁剪，历史参考为提交 `72fb5aa`。未提供 PG 到本地库的数据迁移或可选 PG 运行模式；未来可实现存储接口，不能据此宣称已支持。

## 当前推进批次（2026-10-09）

执行与验收清单见 [P0 验收补齐与评估基线](superpowers/plans/2026-10-08-validation-and-evaluation.md)。本轮收口 PR #10 合入后的 CI 证据、历史失败及修复记录，并统一状态、目标设计与计划。原生、MSRV 默认后端与容器尚未完成时保留未勾项，不把配置加入或 PR 合入当作验收通过。

P1 仍为会话问答、指导、反馈、经验蒸馏、水位与阶段化 improve；时间有效期/as_of 仍属 P2。真实质量评估沿用 300 用例目标。本轮新增隔离 HTTP 检索评估器、12 文档/24 查询的合成种子和指标/adapter 测试；真实宿主结果仍待 CI，不能把样本或 fixture 通过当作完整质量结果。

[记忆身份与上下文前置设计](superpowers/specs/2026-10-08-memory-identity-and-context.md) 的 I1、受限 I2 与 I4 标注/过滤切片已落入代码，验收范围以上表为准。原文区间与 explicit_identity 只表示可追溯性和绑定方式，不证明语义正确。完整身份过滤在最终响应 limit 前执行，但检索分支已截断候选，仍可能漏召回；图实体仍按名称生成 ID，记忆身份不能替代图谱同名消歧。Service 仍依赖 LocalEngine/SqliteTx，存储接口基础已交付，应用层完整后端解耦尚未完成。

## 接下来

1. 完成当前 main 原生/平台/release/container CI 与 Rust 1.88 默认后端检查，读取失败日志后修复；同步 VALIDATION/本计划状态。Rust 1.88 完整运行测试和依赖审计仍单列。
2. 将身份限定下推到检索候选生成，补超过 100 个干扰候选的反例；保留当前 scope/来源/版本边界，不以查找成功代替语义质量验证。
3. 冻结人工证据标注、数据 hash 与配置，先取得可复现的 keyword/vector/hybrid 基线，再用独立控制验证摘要/图收益；逐步扩至 300 用例，记录误合并、延迟及成本。
4. 补 I2 必要语义契约：属性单值/多值、更新/补充与歧义状态；推进 P1 最小闭环“会话与实际证据归档 → 幂等反馈 → 显式 improve → 有来源的经验重新入库”。不要求第一版一次实现全部 improve 阶段。
5. 按质量/规模结果选择 ANN、重排、profile 重建、tokenizer 和分页；运维治理继续保留 retention、物理擦除、孤儿文件和备份恢复演练，不能宣称已完成。
6. P2 企业身份、文档 ACL、配额、公平调度、审计查询、指标及生产后端按后续设计推进；不扩展本地多 Worker。

当前仍是本地原型交付，不承诺生产 SLO、完整 V3.1 场景或竞品效果排名。依赖安全审计（`cargo audit` 核验 RustSec 告警；fs2 疑似归档/不再维护，单 Worker OS 文件锁依赖它，需评估 fs4 替换）、PDF OS 资源沙箱、断电恢复和长期压测尚未完成。

## 历史切片记录

以下保留各轮当时的实现与验收记录；当前状态以上方快照及 VALIDATION 最新核验为准。

### 2026-10-08 后续实现与构建修复

- 首个 I1 切片：主体/属性/条件及 scope 的确定性身份编码，附 8 项 Rust 测试；编码的 8 项测试与 Rust 1.88 检查已通过上一轮 CI。新增持久化/API 切片含 4 项 SQLite 测试及 release/container 身份发布烟测，身份编码 8 项和 SQLite 4 项测试、fmt 已在远端 CI 通过；release/container 发布烟测仍待原生构建。
- 容器实际构建发现 Lance 缺失 `google/protobuf/empty.proto`；Docker 与 Linux CI 已补 `libprotobuf-dev` 和 protoc 导入预检。修复后运行仍待验收，详见 VALIDATION。

## 2026-10-08 显式身份表升级切片

- PR #3 已合入 main（`b52a302`）；身份/SQLite/Python 轻量测试通过，Windows 全套测试与 Rust 1.88 检查也通过；Linux/macOS release 和容器验收仍在运行。
- 新增 `--offline memory-identity-upgrade [--dry-run]`：只打开已有库、独占锁、验证基础 schema 和身份约束、原子安装同一份 DDL；不推断或转换旧身份，不启动原生引擎。
- 新增 6 项 SQLite 升级测试、1 项原生构建 CLI 测试，并补跨进程升级锁测试；轻量 CI 扩至完整无原生后端集成套件。[CI run 37765440784](https://github.com/TheLudlows/openContext/actions/runs/37765440784) 已通过 fmt、8 项身份编码、52 项无原生集成及 5 项 Python 测试；CLI 和容器烟测单独验收。
- 后续重点：capture 的可追溯抽取与精确身份匹配，再进入 P1 会话/反馈闭环；不引入第二后端或多 Worker。

## 2026-10-08 单身份 capture 切片

PR #4 已合入 main（`51bd534`）。本轮新增调用方明确 identity 的 capture 入口，模型只返回原文证据片段，保留来源字节区间并沿用受理版本/发布任务治理；不同断言不自动择一覆盖。新增 8 项无原生证据解析测试及原生 HTTP 生命周期断言，22 项 lib 和 52 项无原生集成测试已通过；原生 HTTP 用例仍待验收。I2 的自动身份推断、未归一化状态、属性目录与语义匹配仍未交付，不能把这一入口标为 I2 全部完成。

## 2026-10-08 身份状态与来源读取

PR #5 已合入 main（`248747b`）。本轮资产读取增加 explicit_identity/legacy_unidentified/not_applicable，明确身份绑定方式而非事实真伪；新增 writer 来源原文 GET，按 scope、来源撤回与文件删除过滤。新增 3 项 SQLite 契约测试已随 PR #7 轻量 CI 通过，原生 HTTP 权限/撤回断言仍待验收。此切片没有新增自动身份推断或未归一化资产状态机。

## 精确身份查找推进（2026-10-08）

PR #6 已合入 main（`578957d`），macOS fmt 已通过；新增 SQLite 用例已随 PR #7 轻量 CI 通过，原生 HTTP 验收仍未完成。本轮新增 reader 可用的 `POST /v1/memories/lookup`：完整身份精确定位当前发布视图，不命中不创建资产，未发布/墓碑/撤回返回 404，旧库缺表返回 503。新增 3 项 SQLite 只读/隔离/治理用例、旧库查找断言和原生 HTTP reader/跨 scope 断言，Python 5 项本地通过，Rust fmt、22 项 lib 和 58 项无原生集成已通过 CI run 37771835095；原生 HTTP/平台/release/container 仍在运行。设计文档开头的进度已更新；自动推断、属性目录、未归一化资产状态机与 P1 会话闭环仍待推进。

## 检索与上下文身份标注（2026-10-08）

PR #7 已合入 main（`ec06ffa`）。本轮 search hits 与 resolve 引用/文本保留身份绑定方式和完整业务身份；身份在最终授权读事务中按资产读取，同一资产复用元数据。上下文标注计入完整字节预算，不截断引用，策略标识为 identity-provenance-v1。新增向后兼容解码/类型状态测试，扩充预算边界与原生 HTTP 标注断言；本轮 Rust 结果待 CI。此为 I4 的标注切片，不实现冲突检测、类型配额、自动身份匹配或会话闭环。

## 精确身份检索过滤（2026-10-08）

PR #8 已合入 main（`29a89cd`）；CI run 37773799817 已通过 fmt、23 项 lib、58 项无原生集成与 Python 5 项，原生 retrieval/HTTP 和平台验收仍未完成。本轮 search/resolve 新增可选完整 memory_identity：过滤在 limit 之前，精确匹配主体/属性/条件，保留 scope 和最终来源治理；启用时只取显式身份记忆，不携带知识/图扩展。新增默认请求/Schema 与精确匹配反例测试，原生 HTTP 覆盖 production/staging 同词 limit=1 隔离与 hybrid resolve 过滤；Python 5 项本地通过，Rust 结果待 CI。自动身份推断、未归一化状态、冲突识别与 P1 会话仍未实现。

## 调用方版本前置条件（2026-10-08）

PR #9 已合入 main（`7a12ede`）；CI run 37775517348 通过 fmt、24 项 lib 和 58 项无原生集成，原生 retrieval/HTTP 与平台验收仍未完成。本轮显式 memory/capture 写入支持可选 expected_version，在来源/任务写入前的授权事务中比对；0 为尚无发布版本，正数为精确当前版本，Worker 保留受理后复核。省略/null 保持旧行为和幂等 payload，已成功重放优先返回缓存。新增 3 项版本/兼容单元测试，原生 HTTP 增加创建/更新/过期/非法/重放断言；本轮 Rust 待 CI，Python 5 项本地通过。此为并发契约补齐，不是自动语义匹配或事实有效时间。

