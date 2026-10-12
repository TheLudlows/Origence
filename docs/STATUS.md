# Origence · 当前状态

更新：2026-10-12。**P0、S2 显式语义、S3 受控对话提取与 SessionTurn/幂等 Feedback 已实现；Improve/Learning 经验学习闭环尚未实现。** 当前适合本地开发和验证，未完成生产级验收。

## 当前阶段范围

设计、实现、测试与验收统一以新建数据库为目标；版本升级、旧库 schema 兼容和数据迁移不在当前范围内，不作为当前缺口、验收门槛或待办。只有后续明确调整需求时才重新考虑。开发约束见 [AGENTS](../AGENTS.md#current-stage-database-scope)，执行范围见[产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md#当前阶段范围)。

这一范围不放宽正常持久化、重启/重试恢复、同版本备份恢复、来源保留或不兼容 schema 拒绝打开的要求，也不授权删除或覆盖既有数据。已有工具和历史升级/迁移说明仅描述既有行为，不代表当前建设目标。

## 产品能力

| 范围 | 已实现 | 主要缺口或边界 |
| --- | --- | --- |
| 入库与存储 | 文本/Markdown/PDF、结构化记忆、受限原文抽取；SQLite 关系/图、LanceDB、本地 Blob | 单宿主、单 Worker；PDF 无硬取消/进程隔离 |
| 记忆身份与语义 | 显式身份、精确 lookup/过滤、版本条件；workspace 属性目录、单值更新/多值集合、冲突与未归一化证据 | 仅面向新库；显式写入或受控模型提取；不自动推断稳定身份/别名，真实模型分类质量未验收 |
| 检索与上下文 | keyword/vector/hybrid、摘要/图开关、带来源与身份的 search/resolve | 跨证据冲突识别、类型预算和统一 EvidenceBundle 未完成；未证明真实 Agent 任务收益 |
| 治理与恢复 | workspace/tenant 隔离、角色/密钥、幂等任务、版本/恢复、来源撤回、重启恢复 | 逻辑删除不等于物理擦除；无企业身份、文档 ACL、配额或生产 SLO |
| 接入与维护 | HTTP、CLI、只读 MCP、显式离线向量压实 | 不自动调度维护；持续容量、维护故障和断电边界仍需验收 |
| 会话学习 | 跨会话历史检索；原文候选提取 → S2 写入 → 同 workspace 跨调用方复用；实际证据归档与幂等反馈 | Improve/Learning 尚未实现；捕获和归档均由调用方显式触发 |

接口与运行细节分别见 [API](API.md)、[OPERATIONS](OPERATIONS.md)；历史测试和 CI 的精确版本见 [VALIDATION](VALIDATION.md)。

## AML 已交付及产品复用

Add/Search 已实现：管理员启用 namespace，完整 user_id 映射独立 workspace；保存不可变批次、消息原文与定位，整批发布并可检索后确认成功。同一用户的多个 session 持续保存并共同检索，session_id 标记来源，不限制为单会话搜索。使用专用评测新库，旧库不自动升级。

这些路径已复用产品 Service、单 Worker、权限、来源和存储。离线压实是共享底座能力；恢复及长历史实验验证了共享路径。用户映射和消息角色/时间/session 适配属于 AML 专项。**跨 session 历史检索已实现，不等于反馈驱动的经验学习闭环已经实现。**

正式 AML Search 保持原文 vector，关闭摘要/图，不生成 Answer。聊天重排、本地 BGE reranker、时间辅助与同模型核验留在实验层。正式组别/模型、持续容量与数据清理验收、官方 Smoke/Full 尚未完成；服务器和域名按用户要求后置。剩余任务见 [AML 计划](AGENT_MEMORY_LEADERBOARD.md)。

## S2 显式语义契约

属性目录、确定性写入路由和端到端回归按三步串行交付。重复值不增版本；显式单值更新追加版本；多值补充保留成员原始来源；冲突、否定、提议和歧义以 unnormalized 证据发布，不能覆盖当前身份。整快照任一支持来源撤回即退出读取，后续显式补充可重建有效集合。旧库不自动安装 S2，旧 fact_key 不重新解释。S2 与 S3 首个切片已由 [PR #37](https://github.com/TheLudlows/Origence/pull/37) 合入 main `daf25b1f`；其 head `a0ec23d1` 的 [PR CI](https://github.com/TheLudlows/Origence/actions/runs/38150659736) 成功，fast-check、Linux 通过，Windows/macOS/container 按 main-only 策略跳过。具体 API/运行边界及本地验证见 API、OPERATIONS 和 VALIDATION。

## S3 受控对话提取首个切片

新增 semantic capture：模型在调用方指定的完整主体/属性/条件范围内生成候选及 operation/value，经过精确原文引用校验后复用 S2。单值覆盖必须由调用方 allow_update 明确授权；歧义、提议、否定与冲突保留未归一化证据。候选计划在 embedding 前持久化，重试/重开复用原分类与 provenance，所有候选原子发布并沿用来源撤回治理。

本轮只面向新库，不开发升级。已交付入口不是完整的 SessionTurn → Feedback → Improve → Learning；实际使用证据归档与幂等反馈已在下述第三阶段实现；条件化经验蒸馏仍待实现。工程回归使用确定性模型桩，不能宣称真实模型分类质量或 Agent 任务收益。见 [实施契约](superpowers/plans/2026-10-11-s3-semantic-capture.md)、API、OPERATIONS、VALIDATION。

## 第二阶段：内部任务类型与证据可见性回归

在既有 SQLite JSON v1 任务信封上新增内部 JobKind/JobPayload，八种持久化操作对应九种有类型输入，显式区分旧 fact_key 与 identified 提取。Service 生产者由类型派生操作名；Worker 在已有失败结算路径内解码，未知操作或损坏字段正常失败而不阻断后续任务。保存的 publication 按 generation 先过滤再解码，semantic_capture_plan、权限、CAS、来源撤回与重试流程保持原样。

共享测试矩阵核对 get/lookup、keyword/vector/hybrid 与 resolve 的来源和身份可见性，覆盖整快照成员撤回、根来源撤回、独立未归一化证据、重复来源与预算边界。不新增存储后端、schema、升级迁移、Worker 或发布路径；该阶段不包含 SessionTurn/Feedback，后者见下节；Improve/Learning 仍是后续阶段。具体执行证据见 [VALIDATION](VALIDATION.md)。

## 第三阶段：SessionTurn 与幂等 Feedback

新增 writer 及以上可用的同步关系型会话归档、确切读取与反馈 HTTP/Service API。调用方显式提交实际使用的 chunk/entity/relation 子集，保存精确历史资产版本、图支持所有权、原 locator 和 S2 原始成员来源；显式空证据与漏传不同。问答、来源事件、引用、审计与回执同事务落盘，命令及领域 ID 均幂等，改变已归档内容或评分返回冲突。

反馈使用 helpful/unhelpful/neutral 保守枚举，只针对实际轮次或其稳定 evidence_ref_id。归档来源或任一引用支持失效时，整轮问答、反馈和原始 archive event 在清理前即隐藏，重放不能恢复可见。原始档案只允许 writer 及以上，仍是 workspace 共享权限。没有新增 Worker、模型调用、索引、排序调权、升级迁移或自动监听；尚未实现 Improve/Learning、会话检索及 S4 EvidenceBundle，也未证明真实 Agent 收益。API 限制与语义见 [API](API.md#会话归档与幂等反馈s3-第二个切片)，验证状态见 [VALIDATION](VALIDATION.md)。

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

- **产品主线**：S2 与 [S3 受控对话提取](superpowers/plans/2026-10-11-s3-semantic-capture.md)已实现，SessionTurn/Feedback 后，下一项为显式 Improve → Learning 最小闭环 → S4 统一上下文与真实 Agent 对照；验收条件见 [产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)。
- **可靠性与质量待办**：维护故障、持续容量、冷启动与标签复核已移入[后续验收及启动条件](superpowers/plans/2026-10-09-next-stage-roadmap.md#后续验收及启动条件)，不继续延长本轮实验；依赖保留项见 [审计](DEPENDENCY_AUDIT.md)。
- **AML 专项**：当前实验收尾，保留[正式参榜门槛](AGENT_MEMORY_LEADERBOARD.md)；准备提交时再启动相应验收，服务器和域名继续后置。

完整 SaaS、分布式及新增后端按实际需求另行立项。设计、实现、工程测试与产品效果分别判断。

