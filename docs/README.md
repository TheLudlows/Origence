# Origence · 文档入口

**简体中文** · [English](README.en.md)

Origence 是面向智能体的统一数据底座，涵盖记忆、知识、Skill 和 Trace。全文、向量与一跳图检索由 AnySeek 提供，SQLite 保存权威元数据和持久记录，Blob 保存原始文件。

首版在企业内网面向研发与业务团队试点，首批客户端为 Claude Code 和 Codex。

## 从这里开始

| 要了解的内容 | 文档 |
| --- | --- |
| 构建与启动 | [项目 README](../README.zh-CN.md) |
| Node.js 管理界面 | [启动、配置与测试](../ui/README.md) |
| 产品定位、核心数据和阶段目标 | [产品路线图](ROADMAP.md) |
| 已确认的产品边界、目标架构与首版验收 | [架构规划](design/agent-data-foundation.md) |
| 当前已实现什么、阻塞项与下一步 | [STATUS](STATUS.md) |
| 固定主干版本的能力与验收边界 | [能力盘点](CAPABILITIES.md) |
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
| [Improve / Learning](contracts/learning.zh-CN.md) | 显式反馈学习、适用条件与完整支持链 |
| [EvidenceBundle](contracts/evidence-bundle.zh-CN.md) | 分路召回、预算、冲突与引用 |
| [Skill](contracts/skills.zh-CN.md) | 私有候选、选定审阅包、批准、发布与分发 |
| [Trace / Run](contracts/runs.zh-CN.md) | 私有轨迹、上下文准备记录、客户端报告与采集缺口 |
| [Run 外部核验](contracts/run-verification.zh-CN.md) | 对精确 Run 前缀的外部评估记录与撤回 |
| [存储不变量](contracts/storage.md) | 身份占位、发布账本、原始证据与同版本恢复 |
| [所属 workspace 与消息批次](API.md#通用所属-workspace-与消息批次库接口) | 通用库接口、派生授权和批次发布状态 |

[效果评估标准](Origence_%E6%95%88%E6%9E%9C%E8%AF%84%E4%BC%B0%E4%B8%8E%E5%AF%B9%E6%AF%94%E6%A0%87%E5%87%86.md)定义评估方法；可运行入口见
[evals](../evals/README.md)。它们不代表企业试点或竞品对照已经完成。

## 历史资料

旧接入流水、已处理的架构审阅、字段盘点、依赖审计和逐阶段验收不再作为活动文档维护。
需要追溯时查看[清理前的固定版本](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs)。
带日期的原始验证工件可在[固定提交的归档](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/evidence)中查阅。当前进度见 [STATUS](STATUS.md)。

每份文档均有简体中文和英文版本，可使用页首链接切换。提交文档变更时同时更新两版，保持章节、表格、命令、参数、数值和验证结论逐项对应。代码示例和原始证据使用同一份内容。
