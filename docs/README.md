# Origence · 文档入口

先读 [当前状态](STATUS.md)，了解已实现能力与限制；再按下表进入唯一负责该类内容的文档。产品路线图记录目标与启动条件，不替代当前状态或精确版本验收。

| 你要做什么 | 入口 | 内容边界 |
| --- | --- | --- |
| 安装、启动 | [项目 README](../README.md) | 快速开始 |
| 了解产品进度 | [STATUS](STATUS.md) | 当前能力、缺口、下一步 |
| 推进产品开发 | [产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md) | S2 → S3 → S4 的任务与验收 |
| 调用接口 | [API](API.md) | HTTP/库接口、权限、版本、错误与破坏性变更 |
| 运行、恢复、维护 | [OPERATIONS](OPERATIONS.md) | 当前命令、步骤和查询/恢复边界 |
| 核验结果 | [VALIDATION](VALIDATION.md)、[评估入口](../evals/README.md) | 精确版本、测试、CI 与原始工件，不外推到新提交 |

## 设计与验收：按需深入

| 文档 | 用途 | 阅读重点 |
| --- | --- | --- |
| [平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md) | 架构与领域契约 | 正文看全局，A2 看存储，A3 看会话目标；接口清单统一到 API |
| [身份与上下文规格](superpowers/specs/2026-10-08-memory-identity-and-context.md) | S2/S4 语义设计 | 身份、冲突、条件与证据预算；已实现接口链接到 API |
| [效果评估标准](Origence_效果评估与对比标准.md) | 定义怎么测 | 先治理门槛，再受控检索/生命周期/真实 Agent 三类对照 |
| [依赖审计](DEPENDENCY_AUDIT.md) | 安全保留项 | 按审计版本处理，不把旧报告视为持续通过 |
| [验收记录](VALIDATION.md) | 查询具体测试与 CI | 按章节日期、代码 SHA 和运行 ID 查证；保留非 AML 专项历史证据，不作为日常进度页 |

当前存储布局与精简决策见[身份存储合同](contracts/identity-asset-layout.md)、[全字段盘点](contracts/storage-field-audit-2026-10-15.md)及[索引/账本保留理由](contracts/index-ledger-audit-2026-10-15.md)。

## 历史研究快读

下表是阅读导航，不把历史建议升级为已实现能力；引用、公式、图源和分析正文保留在原文。

| 研究 | 提炼结论 | 现在如何使用 |
| --- | --- | --- |
| [演进取舍 v2](Origence_演进取舍与现有规划对比_v2.0.md) | 保持 S1–S4，补可靠会话输入与诊断；重排先实验，自动 Wiki/Skill 等暂缓 | 保留为建议稿，正式实施范围看产品路线图 |
| [Serverless 旧方案](agent_memory_knowledgebase_serverless.md) | 权威数据与派生索引、逻辑租户与物理资源分别管理 | 分片/弹性扩展留 P2，不能作为当前部署步骤 |

AML 的生产 API 与授权合同继续保留；专用测试、实验、fixture 和报告已移除，不再作为当前 CI 或操作入口。

当前行为以代码和 API 为准，进度以 STATUS 为准，目标以设计规格为准，验收以对应版本的记录为准。状态页只保留结论；接口细节、运行步骤和实验数字分别维护在各自文档中。历史研究与实验不代表当前已交付能力。
