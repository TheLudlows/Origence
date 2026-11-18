# Origence · 文档入口

Origence 为企业内部研发与业务 Agent 提供统一上下文数据底座。上下文包含记忆、事实、
知识、Skill 与任务状态；首批客户端为 Claude Code 和 Codex。
全文与向量统一使用 AnySeek，SQLite 保存权威数据，Blob 保存原始文件。

## 从这里开始

| 要了解的内容 | 文档 |
| --- | --- |
| 构建与启动 | [项目 README](../README.md) |
| Node.js 管理界面 | [启动、配置与测试](../ui/README.md) |
| 产品目标、六项能力与长期路线 | [产品路线图](ROADMAP.md) |
| 已确认的产品边界、目标架构与首版验收 | [架构规划](design/agent-data-foundation.md) |
| 当前已实现什么、阻塞项与下一步 | [STATUS](STATUS.md) |
| 当前代码的组件、存储分工与一致性 | [实现架构](design/current-implementation.md) |
| HTTP、MCP 与公开库接口 | [API](API.md) |
| 配置、部署、故障处理与备份恢复 | [OPERATIONS](OPERATIONS.md) |
| Claude Code / Codex 接入与 Skill 导出 | [CLIENTS](CLIENTS.md) |
| 精确版本的测试结果、失败与验证边界 | [VALIDATION](VALIDATION.md) |

目标架构描述要交付的产品，不作为能力已经实现的证明；当前进度只在 STATUS 维护，
测试结果只在 VALIDATION 维护，领域契约不再复制阶段排期和历史测试数量。

## 实现时按需查阅

| 领域契约 | 负责的规则 |
| --- | --- |
| [记忆身份与语义](contracts/memory.md) | 身份、条件、版本、单值/多值与来源撤回 |
| [受控提取](contracts/semantic-capture.md) | 模型候选、明确授权、原文校验与持久计划 |
| [Improve / Learning](contracts/learning.md) | 显式反馈学习、适用条件与完整支持链 |
| [EvidenceBundle](contracts/evidence-bundle.md) | 分路召回、预算、冲突与引用 |
| [Skill](contracts/skills.md) | 私有候选、选定审阅包、批准、发布与分发 |
| [Run](contracts/runs.md) | 私有轨迹、上下文准备记录、客户端报告与采集缺口 |
| [存储不变量](contracts/storage.md) | 身份占位、发布账本、原始证据与同版本恢复 |
| [AML 适配](contracts/aml-adapter.md) | 默认关闭的可选外部协议与通用批次接口 |

[效果评估标准](Origence_效果评估与对比标准.md)定义评估方法；可运行入口见
[evals](../evals/README.md)。它们不代表企业试点或竞品对照已经完成。

## 历史资料

旧接入流水、已处理的架构审阅、字段盘点、依赖审计和逐阶段验收不再作为活动文档维护。
需要追溯时查看[清理前的固定版本](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs)。
`evidence/` 内带日期的原始工件保持不变，仅证明各自标明的版本；不代表当前状态。
