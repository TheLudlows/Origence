# Origence · 文档入口

产品方向为企业内部 Agent 数据底座，首批客户端已确认为 Claude Code 与 Codex。
记忆、知识、Skill 与运行时状态统一纳入上下文，可观测性贯穿其供给与使用过程。
Skill 从记忆与 Agent 执行经验提炼，候选内容、依据与变更供用户审视，再按获准版本分发复用。
精简[架构规划](superpowers/specs/2026-11-16-agent-data-foundation-architecture.md)明确全文与向量统一 AnySeek；
当前只接入可选 AnySeek 全文，向量仍在 LanceDB，替换待实施。Turso 保留权威数据和来源。

| 目的 | 唯一入口 |
| --- | --- |
| 快速构建/启动 | [项目 README](../README.md) |
| 首版核心范围、架构与验收 | [企业 Agent 数据底座精简规划](superpowers/specs/2026-11-16-agent-data-foundation-architecture.md) |
| 当前能力、缺口、下一步 | [STATUS](STATUS.md) |
| 前序 AnySeek 全文接入的工程事实 | [AnySeek / Claude Code 实施记录](superpowers/plans/2026-11-16-anyseek-claude-code-mvp.md) |
| 当前实现架构、数据归属与不变量 | [平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md) |
| HTTP/MCP/库接口 | [API](API.md) |
| 配置、部署、恢复与 Claude Code 接入 | [OPERATIONS](OPERATIONS.md) |
| 精确测试/CI/历史失败证据 | [VALIDATION](VALIDATION.md)、[评估入口](../evals/README.md) |

## 按需深入的合同

- [身份与上下文规则](superpowers/specs/2026-10-08-memory-identity-and-context.md)：主体、条件、更新权与冲突边界。
- [S2 语义](superpowers/plans/2026-10-11-s2-memory-semantics.md)、[受控提取](superpowers/plans/2026-10-11-s3-semantic-capture.md)、[Improve/Learning](superpowers/plans/2026-10-12-s3-improve-learning.md)、[EvidenceBundle](superpowers/plans/2026-10-12-s4-evidence-bundle.md)：仍有效的领域细节，不作为新排期。
- [AML 适配](superpowers/plans/2026-10-12-aml-api-adapter-refactor.md)：生产协议的确定性 ID、批次与授权合同；专项评测设施已移除。
- [身份布局](contracts/identity-asset-layout.md)、[字段盘点](contracts/storage-field-audit-2026-10-15.md)、[索引/账本理由](contracts/index-ledger-audit-2026-10-15.md)：物理存储与保留决定。
- [效果评估标准](Origence_效果评估与对比标准.md)、[依赖审计](DEPENDENCY_AUDIT.md)：评估口径和固定版本审计边界，不代表当前质量或审计持续通过。

已被现行设计/计划取代的研究草案、旧 Serverless 方案、S2–S4 重复路线图和已完成的
Turso/Lance 接入流水从活动树删除；可从[清理前固定版本](https://github.com/TheLudlows/Origence/tree/db434ae48fd20e7a8f54e44999a7592457390323/docs)查阅。
VALIDATION、原始工件和仍有效的领域合同保留。进度不复制历史验收，设计不复制接口和运行说明。
