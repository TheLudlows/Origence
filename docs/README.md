# Origence · 文档索引

Origence（原 openContext）的详细技术资料、实现状态与历史验收记录。

更新：2026-10-09。当前交付为 M5 本地宿主 + A1 自动发布及 I1/受限 I2/I4 切片；真实模型质量、P1 会话主闭环及 P2 尚未交付。运行事实以代码、API 为准，能力与验收快照见 STATUS 顶部，精确 commit/run/job 证据见 VALIDATION 最新核验。历史记录中的“待 CI”保留当时状态；历史调研中的 PostgreSQL、serverless 描述不代表当前运行能力。

| 目的 | 文档 |
| --- | --- |
| 安装与第一个闭环 | [项目 README](../README.md) |
| 接口、权限、任务与可见性 | [API](API.md) |
| 数据布局、备份和故障恢复 | [OPERATIONS](OPERATIONS.md) |
| 已交付与待办 | [STATUS](STATUS.md) |
| 当前及剩余任务计划 | [下一阶段实施路线图 S0–S4](superpowers/plans/2026-10-09-next-stage-roadmap.md)、[P0 验收补齐与评估基线](superpowers/plans/2026-10-08-validation-and-evaluation.md) |
| 实测命令、结果和未验证项 | [VALIDATION](VALIDATION.md) |
| M5 交付与验收映射 | [本地宿主交付](superpowers/plans/2026-09-28-local-host-delivery.md) |
| 已实施的自动发布（writer 记忆与抽取直接发布） | [自动发布](superpowers/plans/2026-09-22-auto-publish.md) |
| M0–M5 存储路线 | [可插拔存储计划](superpowers/plans/2026-09-22-pluggable-storage-engine.md) |
| P1 记忆身份与上下文前置设计 | [身份/匹配/冲突契约](superpowers/specs/2026-10-08-memory-identity-and-context.md) |
| 目标架构与 P0/P1/P2 | [平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md) |
| 图谱设计与验收来源 | [图谱计划](superpowers/plans/2026-09-22-knowledge-graph-core.md) |
| 已完成的底层设计记录 | [M3 账本](superpowers/plans/2026-09-24-cross-store-ledger.md)、[M4 适配器](superpowers/plans/2026-09-27-local-vector-graph.md) |
| 底层可行性探针 | [storage-probe](../tools/storage-probe/README.md) |
| 评估方法及样例 | [效果评估标准](ContextDB_效果评估与对比标准.md)、[evals](../evals/README.md) |
| Cognee 固定版本调研 | [架构](Cognee_技术架构分析.md)、[细节比较](Cognee_技术细节与方案对比.md)、[图源和核验材料](assets/cognee/README.md) |
| 历史方案备查 | [serverless 方案](agent_memory_knowledgebase_serverless.md) |

计划中的代码草稿用于解释当时设计，不应直接覆盖当前实现。自动发布已实施：候选/审核门已移除，writer 记忆与抽取直接发布。会话记忆、经验蒸馏、多租户 SaaS 不属于已完成范围。

