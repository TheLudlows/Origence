# Origence · 文档入口

产品方向为企业内部 Agent 数据底座，首批客户端已确认为 Claude Code 与 Codex。
记忆、知识、Skill 与运行时状态统一纳入上下文，可观测性贯穿其供给与使用过程。
目标会话按用户 API key 私有隔离；正式沉淀的长期记忆、事实、Skill 和知识在 workspace 内复用。
平台会话提供归档/查询；工作现场恢复不在范围。Skill 通过选定审阅包批准共享，执行权限继续由客户端和业务工具控制。
Skill 从记忆与 Agent 执行经验提炼，候选内容、依据与变更供用户审视，再按获准版本分发复用。
精简[架构规划](design/agent-data-foundation.md)明确全文与向量统一 AnySeek；
当前只接入可选 AnySeek 全文，向量仍在 LanceDB，替换待实施。Turso 保留权威数据和来源。

| 目的 | 唯一入口 |
| --- | --- |
| 快速构建/启动 | [项目 README](../README.md) |
| 首版核心范围、架构与验收 | [企业 Agent 数据底座精简规划](design/agent-data-foundation.md) |
| 架构边界检视与官方产品参考 | [架构检视记录](reviews/2026-11-16-agent-data-foundation-review.md) |
| 当前能力、缺口、下一步 | [STATUS](STATUS.md) |
| 前序 AnySeek 全文接入的工程事实 | [AnySeek / Claude Code 实施记录](implementation/2026-11-16-anyseek-integration.md) |
| 当前实现架构、数据归属与不变量 | [平台设计](design/current-implementation.md) |
| HTTP/MCP/库接口 | [API](API.md) |
| 配置、部署、恢复与 Claude Code 接入 | [OPERATIONS](OPERATIONS.md) |
| 精确测试/CI/历史失败证据 | [VALIDATION](VALIDATION.md)、[评估入口](../evals/README.md) |

## 目录结构

```text
docs/
├── design/          目标架构与当前实现设计
├── contracts/       身份、记忆、证据、接口适配与存储契约
├── implementation/  已完成切片的实施记录
├── reviews/         架构检视与处理决定
└── evidence/        固定版本的原始验证工件
```

README、STATUS、API、OPERATIONS 和 VALIDATION 保留在文档根目录，分别维护入口、进度、接口、运行与验证。
领域规则按契约归档，实施记录仅保留工程事实，当前排期统一从目标架构和 STATUS 查看。

## 按需深入的合同

- [身份与上下文规则](contracts/memory-identity-and-context.md)：主体、条件、更新权与冲突边界。
- [S2 语义](contracts/memory-semantics.md)、[受控提取](contracts/semantic-capture.md)、[Improve/Learning](contracts/learning.md)、[EvidenceBundle](contracts/evidence-bundle.md)：仍有效的领域细节，不作为新排期。
- [AML 适配](contracts/aml-adapter.md)：生产协议的确定性 ID、批次与授权合同；专项评测设施已移除。
- [身份布局](contracts/identity-asset-layout.md)、[字段盘点](contracts/storage-field-audit-2026-10-15.md)、[索引/账本理由](contracts/index-ledger-audit-2026-10-15.md)：物理存储与保留决定。
- [效果评估标准](Origence_效果评估与对比标准.md)、[依赖审计](DEPENDENCY_AUDIT.md)：评估口径和固定版本审计边界，不代表当前质量或审计持续通过。

已被现行设计/计划取代的研究草案、旧 Serverless 方案、S2–S4 重复路线图和已完成的
Turso/Lance 接入流水从活动树删除；可从[清理前固定版本](https://github.com/TheLudlows/Origence/tree/db434ae48fd20e7a8f54e44999a7592457390323/docs)查阅。
VALIDATION、原始工件和仍有效的领域合同保留。进度不复制历史验收，设计不复制接口和运行说明。
