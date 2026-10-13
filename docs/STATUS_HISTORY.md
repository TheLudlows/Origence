# 历史状态快照 · 2026-10-13 接入归档

以下原样保留 main `1d5e0554b38e12df094a894f11a23fb26a117568` 的状态正文。
其中“当前”“待审”“下一步”、旧存储名称与 CI 数量均属于记录时点，不代表现状。
最新能力及边界见 [STATUS](STATUS.md)，原始验收记录见 [VALIDATION](VALIDATION.md)。

---

# Origence · 当前状态

更新：2026-10-13。**P0、S2 显式语义、S3 受控对话提取与 SessionTurn/幂等 Feedback 已实现；显式 Improve/Learning 最小闭环已合入；S4 EvidenceBundle 与独立 Agent 框架已实现，工程 CI/审查以 [PR #44](https://github.com/TheLudlows/Origence/pull/44) 的最终 head 记录为准，真实模型对照 NOT_RUN。** 当前适合本地开发和验证，未完成生产级验收。

## Turso / LanceDB 正式接入（工程 CI 通过，草稿待审）

[PR #58](https://github.com/TheLudlows/Origence/pull/58) 基于恢复后的主干
`86fac48`，接入 Turso Database 0.8.2 Rust 引擎权威存储与 LanceDB 0.40.0
原生 chunk 全文/向量检索。实现提交 `b50e8748` 的[工程 CI](https://github.com/TheLudlows/Origence/actions/runs/38405514854)
全部适用作业通过：无默认特性 78 + 99、默认特性 93 + 221 项 Rust 测试，
HTTP/MCP、受控模型进程恢复及 keyword 种子评估均完成。PR 保持草稿未合入；
归档后精确 head 的 CI 以 PR 为准。真实模型质量、生产 SLO 与任意断电保证
未验收。范围、发布协议与检查见
[实施合同](superpowers/plans/2026-10-13-turso-lance-integration.md)。

仅面向新库；同版本重开、取消/崩溃恢复、幂等与来源撤回仍在验收范围。
Summary 保持权威文本按同 chunk 跨行聚合后的 AND 语义，不使用 Turso FTS。

## AML API 收敛（Issue #48）

[执行契约](superpowers/plans/2026-10-12-aml-api-adapter-refactor.md)已冻结并包含真实 UUIDv5 输入字节/ID 向量。[P1/P2 PR #50](https://github.com/TheLudlows/Origence/pull/50) 最终 head `95917b89811a259ccfa944cb21422344c8472992` 的独立审查无阻断项，[适用 PR CI](https://github.com/TheLudlows/Origence/actions/runs/38233936996) 全通过。通用一层所属 workspace、显式默认关闭的 key 能力、workspace 批次幂等、不可变接受规格、MessageBatch 及实时发布状态可被非 AML 库调用方复用。

P3 已将 AML DTO、ID 派生、等待与响应转换集中到 API，切换同一 Worker/发布链并删除专表、专用任务与旧授权路径。AML 路由默认关闭；受信后端显式启用并授予 root key 能力。旧 schema 被拒绝，不删除或转换。[P3 PR #51](https://github.com/TheLudlows/Origence/pull/51) 实现 head `9c49231704430ea685e6e30661088442e1e62833` 的独立审查无阻断项，[适用 PR CI](https://github.com/TheLudlows/Origence/actions/runs/38236518092) 全通过，包括原生写失败、进程恢复及停服整库备份恢复。后续文档/部署配置提交同样以 PR 的精确 head 检查为合入门禁；完整记录见 VALIDATION。

真实模型、官方 Smoke/Full 与真实评测导入保持 NOT_RUN；不扩大旧库迁移范围，不修改历史实验结论。

## 当前 CI 范围

2026-10-12 按用户要求取消 macOS 和 Docker 自动 CI：移除 `macos`、`container` Job，以及 `fast-check` 中的 Docker 构建上下文检查。PR 保留 `fast-check`、`linux-native`；main 在此基础上保留 `windows`，Windows 仍仅在主干运行。

Linux 原生测试、HTTP 冒烟、AML 进程恢复、关键词检索评估及证据归档保持不变。不删除测试用例、Dockerfile、Compose、容器冒烟脚本或 macOS 本地构建能力；macOS 兼容性与容器构建/运行不再由日常 CI 自动验证。下文和 VALIDATION 中五项 CI 的记录保留为对应历史提交的证据，不代表当前覆盖范围。该配置调整的实际 CI 结果以相应提交的 Actions 记录为准。

## 当前阶段范围

设计、实现、测试与验收统一以新建数据库为目标；版本升级、旧库 schema 兼容和数据迁移不在当前范围内，不作为当前缺口、验收门槛或待办。只有后续明确调整需求时才重新考虑。开发约束见 [AGENTS](../AGENTS.md#current-stage-database-scope)，执行范围见[产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md#当前阶段范围)。

这一范围不放宽正常持久化、重启/重试恢复、同版本备份恢复、来源保留或不兼容 schema 拒绝打开的要求，也不授权删除或覆盖既有数据。已有工具和历史升级/迁移说明仅描述既有行为，不代表当前建设目标。

## 产品能力

| 范围 | 已实现 | 主要缺口或边界 |
| --- | --- | --- |
| 入库与存储 | 文本/Markdown/PDF、结构化记忆、受限原文抽取；Turso 权威关系/图、LanceDB 原生全文/向量、本地 Blob | 单宿主、单 Worker；PDF 无硬取消/进程隔离 |
| 记忆身份与语义 | 显式身份、精确 lookup/过滤、版本条件；workspace 属性目录、单值更新/多值集合、冲突与未归一化证据 | 仅面向新库；显式写入或受控模型提取；不自动推断稳定身份/别名，真实模型分类质量未验收 |
| 检索与上下文 | keyword/vector/hybrid、摘要/图开关、带来源与身份的 search/resolve | 统一 EvidenceBundle、独立类型召回/字节预算、明确单值身份冲突已实现；不推断普通文本矛盾，未证明真实 Agent 任务收益 |
| 治理与恢复 | workspace/tenant 隔离、角色/密钥、幂等任务、版本/恢复、来源撤回、重启恢复 | 逻辑删除不等于物理擦除；无企业身份、文档 ACL、配额或生产 SLO |
| 接入与维护 | HTTP、CLI、只读 MCP、显式离线向量压实 | 不自动调度维护；持续容量、维护故障和断电边界仍需验收 |
| 会话学习 | 跨会话历史检索；原文候选提取 → S2 写入 → 同 workspace 跨调用方复用；实际证据归档与幂等反馈 | 捕获、归档和 Improve 均显式触发；无自动改权，真实模型质量未验收 |

接口与运行细节分别见 [API](API.md)、[OPERATIONS](OPERATIONS.md)；历史测试和 CI 的精确版本见 [VALIDATION](VALIDATION.md)。

## AML 接入与产品复用

Add/Search 的协议只保留在 API：认证 tenant/root 与完整 user_id 通过固定 UUIDv5 派生一层所属 workspace；不保存 AML namespace/user 映射。root key 仅供受信后端使用，可访问该 root 的全部所属 workspace；接入服务负责终端用户认证，不能将 root key 分发给用户。每个新 key 或轮换 key 均须显式授予默认关闭的 allow_owned_workspaces。

Add 转为通用 MessageBatch，保存不可变批次、消息原文、接受规格和精确批次/消息范围，复用单 Worker 整批发布，全部索引可检索后才确认成功。同一用户多个 session 共同检索，session_id 只标记来源。跨 key 重放复用原回执但不转移任务 creator；撤销旧 key 不允许新 key 接管未完成任务。Search 对未知用户只读返回空，不创建 workspace。

正式 AML Search 保持原文 vector，关闭摘要/图，不生成 Answer。聊天重排、本地 BGE reranker、时间辅助与同模型核验留在实验层。新来源 parser 为 message-batch-ranges-v1，带来源事件/批次/版本信息；历史 parser/报告不改写。跨 session 历史检索不等于反馈学习，真实模型质量、正式组别/模型、持续容量/清理验收、官方 Smoke/Full 尚未完成。服务器和域名按用户要求后置，剩余任务见 [AML 计划](AGENT_MEMORY_LEADERBOARD.md)。

## S2 显式语义契约

属性目录、确定性写入路由和端到端回归按三步串行交付。重复值不增版本；显式单值更新追加版本；多值补充保留成员原始来源；冲突、否定、提议和歧义以 unnormalized 证据发布，不能覆盖当前身份。整快照任一支持来源撤回即退出读取，后续显式补充可重建有效集合。旧库不自动安装 S2，旧 fact_key 不重新解释。S2 与 S3 首个切片已由 [PR #37](https://github.com/TheLudlows/Origence/pull/37) 合入 main `daf25b1f`；其 head `a0ec23d1` 的 [PR CI](https://github.com/TheLudlows/Origence/actions/runs/38150659736) 成功，fast-check、Linux 通过，Windows/macOS/container 按 main-only 策略跳过。具体 API/运行边界及本地验证见 API、OPERATIONS 和 VALIDATION。

## S3 受控对话提取首个切片

新增 semantic capture：模型在调用方指定的完整主体/属性/条件范围内生成候选及 operation/value，经过精确原文引用校验后复用 S2。单值覆盖必须由调用方 allow_update 明确授权；歧义、提议、否定与冲突保留未归一化证据。候选计划在 embedding 前持久化，重试/重开复用原分类与 provenance，所有候选原子发布并沿用来源撤回治理。

该首个切片只面向新库，不开发升级，当时尚未包含完整的 SessionTurn → Feedback → Improve → Learning；实际使用证据归档、幂等反馈及条件化经验提议/核验随后已在下述第三、第四阶段实现。工程回归使用确定性模型桩，不能宣称真实模型分类质量或 Agent 任务收益。见 [实施契约](superpowers/plans/2026-10-11-s3-semantic-capture.md)、API、OPERATIONS、VALIDATION。

## 第二阶段：内部任务类型与证据可见性回归

在既有 SQLite JSON v1 任务信封上新增内部 JobKind/JobPayload，八种持久化操作对应九种有类型输入，显式区分旧 fact_key 与 identified 提取。Service 生产者由类型派生操作名；Worker 在已有失败结算路径内解码，未知操作或损坏字段正常失败而不阻断后续任务。保存的 publication 按 generation 先过滤再解码，semantic_capture_plan、权限、CAS、来源撤回与重试流程保持原样。

共享测试矩阵核对 get/lookup、keyword/vector/hybrid 与 resolve 的来源和身份可见性，覆盖整快照成员撤回、根来源撤回、独立未归一化证据、重复来源与预算边界。不新增存储后端、schema、升级迁移、Worker 或发布路径；该阶段当时不包含 SessionTurn/Feedback 与 Improve/Learning；两者后续交付分别见第三、第四阶段。具体执行证据见 [VALIDATION](VALIDATION.md)。

## 第三阶段：SessionTurn 与幂等 Feedback

新增 writer 及以上可用的同步关系型会话归档、确切读取与反馈 HTTP/Service API。调用方显式提交实际使用的 chunk/entity/relation 子集，保存精确历史资产版本、图支持所有权、原 locator 和 S2 原始成员来源；显式空证据与漏传不同。问答、来源事件、引用、审计与回执同事务落盘，命令及领域 ID 均幂等，改变已归档内容或评分返回冲突。

反馈使用 helpful/unhelpful/neutral 保守枚举，只针对实际轮次或其稳定 evidence_ref_id。归档来源或任一引用支持失效时，整轮问答、反馈和原始 archive event 在清理前即隐藏，重放不能恢复可见。原始档案只允许 writer 及以上，仍是 workspace 共享权限。没有新增 Worker、模型调用、索引、排序调权、升级迁移或自动监听；该切片当时不包含 Improve/Learning、会话检索及 S4 EvidenceBundle；后续交付见下文第四、第五阶段，仍未证明真实 Agent 收益。API 限制与语义见 [API](API.md#会话归档与幂等反馈s3-第二个切片)，验证状态见 [VALIDATION](VALIDATION.md)。

## 第四阶段：显式 Improve → Learning

新增有条件、受限、可重放的提议 → 核验 → 普通知识发布闭环。单调反馈序列与 principal-qualified 标识防止同时间戳遗漏和重复计数；冻结批次、两阶段计划及子任务标识持久化，最后子任务成功前不推进完成水位。每条 Learning 独立原子发布；显式取消未完成 run 不消费反馈，并保留此前已发布且支持仍有效的条目。

条件不可由模型扩大，每个检索片段保留条件和支持；本切片仅对 Learning 禁用 summary/graph，防止脱离适用范围的转述。整轮次证据与有界传递支持在读取、索引及发布时统一核验，来源撤回先隐藏再清理；普通更新/恢复不能绕过。没有新 Worker、provider、迁移、自动触发、排序改权或 Guidance 生命周期。该阶段当时尚未交付 S4 EvidenceBundle；其工程实现见第五阶段，独立真实质量仍未验收。详见 [实施契约](superpowers/plans/2026-10-12-s3-improve-learning.md)、API 与本提交 VALIDATION；该阶段已由 [PR #43](https://github.com/TheLudlows/Origence/pull/43) 合入 main `55b5d879`，其最终 head 的 PR CI 与独立审查通过；不替代 S4 CI。

## 本轮实验已结束

2026-10-11 按用户确认收尾。冻结结论：正式 AML Search 保持原文 vector；重排、时间辅助及同模型核验留实验层；显式离线压实已交付，不增加自动维护。相关代码与工件经 PR #33–#35 合入 main 2b7b6a6，该提交[五项 CI](https://github.com/TheLudlows/Origence/actions/runs/38138182208)全部成功；[收尾核对](evidence/2026-10-11-experiment-closeout.json)记录 78 项哈希检查全部通过。历史报告中的“下一步”保留为当时记录，后续任务以路线图为准。

本轮完成不等于生产可靠性、独立质量或官方 AML 验收完成。实验进程已结束，执行记录确认临时库已清理；不重跑已观察题来宣称新质量收益。

收尾文档经 [PR #36](https://github.com/TheLudlows/Origence/pull/36) 合入 main `db939960`；其 head `582d6be2` 的 [PR CI](https://github.com/TheLudlows/Origence/actions/runs/38142952091) 成功，fast-check、Linux 通过，Windows/macOS/container 按 main-only 策略跳过。

## 当前证据与结论

2026-10-12 核验：第一阶段收口已由 [PR #39](https://github.com/TheLudlows/Origence/pull/39) 合入 main `7a20540cfadf94199d2787415c619de2aaba9721`；[CI run 38194107923](https://github.com/TheLudlows/Origence/actions/runs/38194107923) 的 fast-check、Linux、Windows、macOS、container 五项均成功。该结果不替代后续提交的 CI。

| 验证 | 已得到的结论 | 不能据此宣称 |
| --- | --- | --- |
| [S0 验收](VALIDATION.md)、[S1 产品检索](../evals/s1/results/local-77d43a8-v2/README.md) | 身份候选过滤已验收；36 篇/300 题六配置实验完成，摘要未见增益，图降低召回 | 新增 200 题尚未独立人工复核；无答案处理与真实 Agent 效果未验收 |
| [AML 本地演练](AML_DRILL.md) | 真实 BGE-M3 协议、64 用户并发、跨 session、重启和停机备份恢复完成；固定模型处理中强退恢复通过 | 官方 Smoke、长期容量或断电保证 |
| [同预算质量实验](../evals/aml/budget-v1/README.md)、[时间诊断](../evals/aml/temporal-v1/README.md) | 重排尚无稳定质量优势；发现标签歧义、时间问题和裁判错判，继续留实验层 | 当前 64 题已被观察，不能作为未见验收或官方分数 |
| [完整库维护](../evals/aml/maintenance-scale-v1/README.md) | 32 用户、15,685 条消息；168 次 Search 完整响应/排序/原文字节复核通过；压实后延迟改善，追加后再次维护有效 | 冷启动尾部仍在；无长期 SLO、维护中断/磁盘不足或实际多模型性能验收 |

早期 [查询扩展](../evals/aml/experiments/results/README.md)、[重排诊断](../evals/aml/diagnosis/README.md)及[向量后端定位](../evals/aml/vector-perf-v1/README.md)保留为历史依据；原始分数和工件不改写。容器缺 examples 的问题已修复，具体提交与验收见 VALIDATION。

## 下一步

- **产品主线**：S2、S3、SessionTurn/Feedback、显式 Improve → Learning 已实现；S4 统一上下文与 Agent 框架已实现，工程记录见 PR #44，当前验证 Turso 权威存储与 LanceDB 原生检索接入及跨库恢复，依赖风险单独处理；随后进行有授权且可复现的真实 Agent 对照，不能用 fixture 代替；验收条件见 [产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)。
- **可靠性与质量待办**：维护故障、持续容量、冷启动与标签复核已移入[后续验收及启动条件](superpowers/plans/2026-10-09-next-stage-roadmap.md#后续验收及启动条件)，不继续延长本轮实验；依赖保留项见 [审计](DEPENDENCY_AUDIT.md)。
- **AML 专项**：当前实验收尾，保留[正式参榜门槛](AGENT_MEMORY_LEADERBOARD.md)；准备提交时再启动相应验收，服务器和域名继续后置。

完整 SaaS、分布式及新增后端按实际需求另行立项。设计、实现、工程测试与产品效果分别判断。


## 第五阶段：S4 EvidenceBundle 与独立 Agent 框架

新增只读 `/v1/context` 与 MCP `context_bundle`，保留 search/resolve；memory、
knowledge、Learning 独立候选，session 默认关闭且仅 writer。条件/引用/冲突警告
按整 UTF-8 块预算，完整来源及传递支持在最终快照复核。仅检测明确单值身份冲突，
不推断一般文本矛盾；会话引用不能回填现有 used_evidence，已明确披露。

独立新冻结合成用例与有界模型/工具 Agent 框架区分无记忆、简单 hybrid、Bundle、
非部署 gold 诊断。真实模型评测 NOT_RUN，fixture 不证明任务收益。此前约 84 KB 合成诊断工件上传仍待完成，本轮结构整理不上传该工件；不将其视为真实模型验收。无新后端、
迁移、Worker、编排器或 AML 重启。完整边界见 [S4 契约](superpowers/plans/2026-10-12-s4-evidence-bundle.md)
与 [验证状态](VALIDATION.md#2026-10-12--s4-evidencebundle-工程实现与独立-agent-框架)。


## 结构治理：SQLite 通用可见性

在 PR #45 既有职责拆分之后，私有 visibility 模块收口当前 chunk 的 scope、
current-version、未删除、活动版本根与 S2 本地支持谓词，以及读取/归档共享的
精确版本根规则。当前检索与历史归档分开；Learning 完整依赖闭包、原始 session
权限、图所有权与原始 S2 成员来源继续保持专用检查。没有统一成万能可见性布尔值，
没有新增 schema、迁移、后端或产品功能。执行状态以本阶段精确 head 的 PR CI
和 VALIDATION 为准，不能沿用前一 PR 的通过结论。


## MCP 依赖处置（2026-10-12）

[PR #47](https://github.com/TheLudlows/Origence/pull/47) 将 stdio SDK 独立更新到 rmcp 1.8.0，
仅显式开启 server/macros/transport-io，保持四个只读工具和既有认证/错误/host 转发行为。
真实二进制 stdio 回归及实现提交 `8e1c6cb5` 的 Linux/fast-check 已通过；最终 head 与主干
CI 以 PR/Actions 当前状态为准。协议协商和 schema 方言差异、执行记录见 [VALIDATION](VALIDATION.md)。

rmcp 告警已消除，但完整审计仍退出 1，保留 rsa、lru、维护状态及 yoke-derive 撤回版本问题；
不得表述为依赖风险全部解决。Lance/PDF 未扩大更新，详见 [依赖审计与证据](DEPENDENCY_AUDIT.md)。
