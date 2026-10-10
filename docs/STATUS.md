# Origence · 当前状态

更新：2026-10-11。Origence 仍是本地原型。核心写入、权限、来源追溯、版本和检索能力已实现；S0 身份过滤正确性验收完成。S1 工程检索实验已完成，新增题目的独立人工复核仍待进行。

## 已交付与边界

| 范围 | 当前状态 | 主要限制 |
| --- | --- | --- |
| 本地宿主 | SQLite 关系库、LanceDB 向量、SQLite 图和本地 Blob；单宿主、单 Worker | 不自动迁移旧 Kuzu 图文件；切换说明见 [OPERATIONS](OPERATIONS.md#sqlite-图替换边界) |
| 写入与治理 | 记忆/知识发布、版本、scope、幂等、来源追溯、撤回和恢复 | 无企业身份、文档 ACL、配额或生产 SLO |
| API 与接入 | HTTP、CLI、MCP；文件及 PDF 入库 | PDF 线程解析没有硬取消或进程隔离保证，见 [OPERATIONS](OPERATIONS.md#pdf-解析) |
| 身份与上下文 | I1、受限 I2、I4 已实现；完整身份候选前置过滤已验收 | 语义冲突、属性目录、单值/多值和类型配额未实现 |
| 检索质量 | 36 篇/300 题开发与留出切分；真实本地模型六配置及摘要/图消融已完成 | 新增 200 题尚未独立人工复核；12 道无答案题的 vector/hybrid 均误召回 |
| 会话经验 | 目标规格已定义 | Session、反馈、显式 improve 和跨会话 Learning 未实现 |

SQLite 图替换已合入 main `9c292a9`；run [38032343493](https://github.com/TheLudlows/Origence/actions/runs/38032343493) 的五个作业全部成功，三平台默认 lib 28/local 89，13 项图回归通过。S0 身份过滤的独立跨平台验收见 run [37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081)。精确 checkout、job 和计数见 [VALIDATION](VALIDATION.md)。

## AML 当前实现切片

已实现管理员显式启用 namespace、完整 user_id 到独立 workspace 的原子映射，以及 AML Add/Search HTTP 最小闭环。Add 保存不可变消息来源和持久批次幂等记录，整批发布并完成索引后返回成功；超时重试观察原任务。Search 使用原文 vector，关闭摘要/图，返回含角色、时间和会话标注的证据。已有库不自动升级；使用专用评测新库。测试结果及当前限制见 [VALIDATION](VALIDATION.md)。已补齐固定模型的摘要/图等检索分支隔离回归，并完成真实本地 BGE-M3 小规模协议与发布后重启演练；运行工具及清理步骤见 [AML_DRILL](AML_DRILL.md)。正式参赛模型、公网容量、生产数据清理与官方 Smoke 尚未完成。

## 当前阶段与下一步

S1 v2 在 36 篇合成文档、300 道问题（开发 200、留出 100）上完成六配置对照、1,800 次 search、1,728 次 resolve 和摘要/图消融，零请求错误。keyword/vector/hybrid-all 的文档 Recall@5 为 0.35%/97.57%/94.79%；vector 与 hybrid 的 12 道无答案题均全部误召回。摘要没有观察到增益，图配置 Recall 下降 2.78 个百分点。完整开发/留出分数、延迟、模型 hash 与限制见 [S1 实测报告](../evals/s1/results/local-77d43a8-v2/README.md)。

AML Add/Search 最小闭环与本地协议演练已实现，下一步固定正式评测模型与配置、完成 PR/CI，再在目标部署环境推进持续容量、发布中恢复与副本清理验收。现有本地 BGE-M3 演练不代表参赛配置；提供的远端网关已验证聊天调用，embedding 模型名与维度仍待确定。检索质量方面仍需独立人工复核新增 200 题，并在不损害可答题召回的条件下评估拒答；之后再决定中文关键词改进和图配置。该实验不代表生产语料、完整平台用例或 Agent 成功率。

随后推进 I2 单值/多值及歧义契约，再实现 Session → Feedback → Improve → Learning 最小闭环。依赖安全审计、物理擦除、断电和备份恢复仍需独立验收；P2 服务化能力根据实际需求推进。

当前文档分工：[平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md)保存完整架构，[身份规格](superpowers/specs/2026-10-08-memory-identity-and-context.md)保存契约；本文是唯一当前状态摘要，[路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)列未完成工作，[VALIDATION](VALIDATION.md)列当前验收。API 和运行边界见 [API](API.md) 与 [OPERATIONS](OPERATIONS.md)。
