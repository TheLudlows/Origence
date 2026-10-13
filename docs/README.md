# Origence · 文档入口

先读 [当前状态](STATUS.md)，了解已实现能力与限制；架构契约见 [平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md)，调用与运维分别见 API/OPERATIONS。以下入口区分当前契约、后续目标与历史证据。

| 你要做什么 | 入口 | 内容边界 |
| --- | --- | --- |
| 安装、启动 | [项目 README](../README.md) | 快速开始 |
| 了解产品进度 | [STATUS](STATUS.md) | 当前能力、缺口、下一步 |
| 推进产品开发 | [产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md) | S2 → S3 → S4 的任务与验收 |
| 准备 AML 打榜 | [AML 计划](AGENT_MEMORY_LEADERBOARD.md) | 参榜剩余工作与进入条件 |
| 调用接口 | [API](API.md) | 请求、权限、版本与错误契约 |
| 运行、恢复、维护 | [OPERATIONS](OPERATIONS.md)、[AML 本地演练](AML_DRILL.md) | 命令、步骤和运行边界 |
| 核验结果 | [VALIDATION](VALIDATION.md)、[评估入口](../evals/README.md) | 精确版本、测试、CI 与原始工件 |

## 设计与验收：按需深入

| 文档 | 用途 | 阅读重点 |
| --- | --- | --- |
| [平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md) | 当前架构与领域契约，显式标注未实现目标 | A2/A5：Turso 权威库、独立 Turso 图投影、LanceDB 全文/向量与 Blob；A3/A4 保留目标；接口统一到 API |
| [身份与上下文规格](superpowers/specs/2026-10-08-memory-identity-and-context.md) | S2/S4 语义设计 | 身份、冲突、条件与证据预算；已实现接口链接到 API |
| [效果评估标准](Origence_效果评估与对比标准.md) | 定义怎么测 | 先治理门槛，再受控检索/生命周期/真实 Agent 三类对照 |
| [依赖审计](DEPENDENCY_AUDIT.md) | 安全保留项 | 按审计版本处理，不把旧报告视为持续通过 |
| [验收记录](VALIDATION.md) | 查询具体测试与 CI | 按章节日期、代码 SHA 和运行 ID 查证；记录正文及工件保留，不作为日常进度页 |

## 历史研究快读

历史实施计划、研究和验收记录保留当时的引擎名称、命令、状态与结果；文件日期不能单独决定是否为当前契约。下表是阅读导航，不把历史建议升级为已实现能力；引用、公式、图源和分析正文保留在原文。

| 研究 | 提炼结论 | 现在如何使用 |
| --- | --- | --- |
| [演进取舍 v2](Origence_演进取舍与现有规划对比_v2.0.md) | 保持 S1–S4，补可靠会话输入与诊断；重排先实验，自动 Wiki/Skill 等暂缓 | 保留为建议稿，正式实施范围看产品路线图 |
| [AML 检索决策](AML_RETRIEVAL_DECISION.md) | 区分召回、排序、证据充分性和答案支持，采用同预算消融 | 当时计划已有后续实验，最新结论看 STATUS |
| [Serverless 旧方案](agent_memory_knowledgebase_serverless.md) | 权威数据与派生索引、逻辑租户与物理资源分别管理 | 分片/弹性扩展留 P2，不能作为当前部署步骤 |

当前行为以代码和 API 为准，架构与领域边界以维护中的设计规格为准，进度以 STATUS 为准，验收以对应版本的记录为准。版本升级、旧库兼容与迁移不属于当前新库阶段范围。状态页只保留结论；接口细节、运行步骤和实验数字分别维护在各自文档中。历史研究、旧实施计划与实验不代表当前已交付能力。
