# Origence · 当前状态

更新：2026-10-11。**P0、S2 显式语义与 S3 受控对话提取首个切片已落地；P1 会话反馈与经验学习闭环尚未实现。** 当前适合本地开发和验证，未完成生产级验收。

## 产品能力

| 范围 | 已实现 | 主要缺口或边界 |
| --- | --- | --- |
| 入库与存储 | 文本/Markdown/PDF、结构化记忆、受限原文抽取；SQLite 关系/图、LanceDB、本地 Blob | 单宿主、单 Worker；PDF 无硬取消/进程隔离；旧 Kuzu 图不自动迁移 |
| 记忆身份与语义 | 显式身份、精确 lookup/过滤、版本条件；workspace 属性目录、单值更新/多值集合、冲突与未归一化证据 | 仅面向新库；显式写入或受控模型提取；不自动推断稳定身份/别名，真实模型分类质量未验收 |
| 检索与上下文 | keyword/vector/hybrid、摘要/图开关、带来源与身份的 search/resolve | 跨证据冲突识别、类型预算和统一 EvidenceBundle 未完成；未证明真实 Agent 任务收益 |
| 治理与恢复 | workspace/tenant 隔离、角色/密钥、幂等任务、版本/恢复、来源撤回、重启恢复 | 逻辑删除不等于物理擦除；无企业身份、文档 ACL、配额或生产 SLO |
| 接入与维护 | HTTP、CLI、只读 MCP、显式离线向量压实 | 不自动调度维护；持续容量、维护故障和断电边界仍需验收 |
| 会话学习 | 跨会话历史检索；原文候选提取 → S2 写入 → 同 workspace 跨调用方复用 | SessionTurn、Feedback、Improve、Learning 尚未实现；当前捕获由调用方显式触发 |

接口与运行细节分别见 [API](API.md)、[OPERATIONS](OPERATIONS.md)；历史测试和 CI 的精确版本见 [VALIDATION](VALIDATION.md)。

## AML 已交付及产品复用

Add/Search 已实现：管理员启用 namespace，完整 user_id 映射独立 workspace；保存不可变批次、消息原文与定位，整批发布并可检索后确认成功。同一用户的多个 session 持续保存并共同检索，session_id 标记来源，不限制为单会话搜索。使用专用评测新库，旧库不自动升级。

这些路径已复用产品 Service、单 Worker、权限、来源和存储。离线压实是共享底座能力；恢复及长历史实验验证了共享路径。用户映射和消息角色/时间/session 适配属于 AML 专项。**跨 session 历史检索已实现，不等于反馈驱动的经验学习闭环已经实现。**

正式 AML Search 保持原文 vector，关闭摘要/图，不生成 Answer。聊天重排、本地 BGE reranker、时间辅助与同模型核验留在实验层。正式组别/模型、持续容量与数据清理验收、官方 Smoke/Full 尚未完成；服务器和域名按用户要求后置。剩余任务见 [AML 计划](AGENT_MEMORY_LEADERBOARD.md)。

## S2 显式语义契约

属性目录、确定性写入路由和端到端回归按三步串行交付。重复值不增版本；显式单值更新追加版本；多值补充保留成员原始来源；冲突、否定、提议和歧义以 unnormalized 证据发布，不能覆盖当前身份。整快照任一支持来源撤回即退出读取，后续显式补充可重建有效集合。旧库不自动安装 S2，旧 fact_key 不重新解释。具体 API/运行边界及本地验证见 API、OPERATIONS 和 VALIDATION；本工作区改动尚无新远端 CI 证据。

## S3 受控对话提取首个切片

新增 semantic capture：模型在调用方指定的完整主体/属性/条件范围内生成候选及 operation/value，经过精确原文引用校验后复用 S2。单值覆盖必须由调用方 allow_update 明确授权；歧义、提议、否定与冲突保留未归一化证据。候选计划在 embedding 前持久化，重试/重开复用原分类与 provenance，所有候选原子发布并沿用来源撤回治理。

本轮只面向新库，不开发升级。已交付入口不是完整的 SessionTurn → Feedback → Improve → Learning；后续仍需实际使用证据归档、幂等反馈和条件化经验蒸馏。工程回归使用确定性模型桩，不能宣称真实模型分类质量或 Agent 任务收益。见 [实施契约](superpowers/plans/2026-10-11-s3-semantic-capture.md)、API、OPERATIONS、VALIDATION。

## 本轮实验已结束

2026-10-11 按用户确认收尾。冻结结论：正式 AML Search 保持原文 vector；重排、时间辅助及同模型核验留实验层；显式离线压实已交付，不增加自动维护。相关代码与工件经 PR #33–#35 合入 main 2b7b6a6，该提交[五项 CI](https://github.com/TheLudlows/Origence/actions/runs/38138182208)全部成功；[收尾核对](evidence/2026-10-11-experiment-closeout.json)记录 78 项哈希检查全部通过。历史报告中的“下一步”保留为当时记录，后续任务以路线图为准。

本轮完成不等于生产可靠性、独立质量或官方 AML 验收完成。实验进程已结束，执行记录确认临时库已清理；不重跑已观察题来宣称新质量收益。

## 当前证据与结论

| 验证 | 已得到的结论 | 不能据此宣称 |
| --- | --- | --- |
| [S0 验收](VALIDATION.md)、[S1 产品检索](../evals/s1/results/local-77d43a8-v2/README.md) | 身份候选过滤已验收；36 篇/300 题六配置实验完成，摘要未见增益，图降低召回 | 新增 200 题尚未独立人工复核；无答案处理与真实 Agent 效果未验收 |
| [AML 本地演练](AML_DRILL.md) | 真实 BGE-M3 协议、64 用户并发、跨 session、重启和停机备份恢复完成；固定模型处理中强退恢复通过 | 官方 Smoke、长期容量或断电保证 |
| [同预算质量实验](../evals/aml/budget-v1/README.md)、[时间诊断](../evals/aml/temporal-v1/README.md) | 重排尚无稳定质量优势；发现标签歧义、时间问题和裁判错判，继续留实验层 | 当前 64 题已被观察，不能作为未见验收或官方分数 |
| [完整库维护](../evals/aml/maintenance-scale-v1/README.md) | 32 用户、15,685 条消息；168 次 Search 完整响应/排序/原文字节复核通过；压实后延迟改善，追加后再次维护有效 | 冷启动尾部仍在；无长期 SLO、维护中断/磁盘不足或实际多模型性能验收 |

早期 [查询扩展](../evals/aml/experiments/results/README.md)、[重排诊断](../evals/aml/diagnosis/README.md)及[向量后端定位](../evals/aml/vector-perf-v1/README.md)保留为历史依据；原始分数和工件不改写。容器缺 examples 的问题已修复，具体提交与验收见 VALIDATION。

## 下一步

- **产品主线**：S2 与 [S3 受控对话提取](superpowers/plans/2026-10-11-s3-semantic-capture.md)已实现，下一项为 S3 SessionTurn/Feedback/Improve/Learning 最小闭环 → S4 统一上下文与真实 Agent 对照；验收条件见 [产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)。
- **可靠性与质量待办**：维护故障、持续容量、冷启动与标签复核已移入[后续验收及启动条件](superpowers/plans/2026-10-09-next-stage-roadmap.md#后续验收及启动条件)，不继续延长本轮实验；依赖保留项见 [审计](DEPENDENCY_AUDIT.md)。
- **AML 专项**：当前实验收尾，保留[正式参榜门槛](AGENT_MEMORY_LEADERBOARD.md)；准备提交时再启动相应验收，服务器和域名继续后置。

完整 SaaS、分布式及新增后端按实际需求另行立项。设计、实现、工程测试与产品效果分别判断。
