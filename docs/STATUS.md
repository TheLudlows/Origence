# Origence · 当前进度

更新：2026-11-16。[PR #71](https://github.com/TheLudlows/Origence/pull/71) 已合入 main，
核对基线为 `17fa39582aefb2c53165fae62bf8c41de4aab34c`。
产品方向已确认为服务研发与业务团队的企业内部 Agent 数据底座，首批客户端为 Claude Code 与 Codex。
完整[目标架构规划](superpowers/specs/2026-11-16-agent-data-foundation-architecture.md)待确认；
上下文包含记忆、知识、Skill 与运行时状态，运行追溯贯穿其供给与使用；新增能力不计为已实现。

## 已实现与边界

| 范围 | 当前代码 | 仍需完成 |
| --- | --- | --- |
| 数据与来源 | 文本/Markdown/PDF、原始 Blob、不可变版本/locator、Turso authority | 新数据源连接和持续同步按试点需求接入 |
| 检索 | 默认 LanceDB 全文/向量；可选 AnySeek 全文；候选前置过滤与最终权威复核 | AnySeek 向量接入、真实语料相关性/容量/延迟验收 |
| 接入 | HTTP/CLI、四个只读 stdio MCP 工具、宿主转发、Claude Code 配置示例 | Claude Code 与 Codex 的真实客户端验证、Codex 配置与能力核对 |
| 身份与语义 | 显式 MemoryIdentity、单值/多值规则、受控提取、冲突证据 | 自动身份/别名匹配、真实模型语义质量 |
| 会话与经验 | 显式 SessionTurn/Feedback、Improve → Learning、带来源的 EvidenceBundle | 真实 Agent 收益；session 引用回填既有 used_evidence 尚不支持 |
| 目标新增能力 | 现有 Bundle/会话/反馈可复用 | 任务上下文快照、Skill 注册/发布/分发、Run/事件/工具/结果证据链均待设计实施 |
| 治理与恢复 | tenant/workspace、角色/key、撤回/墓碑、发布账本、原计划重试、同版本重开 | 企业身份、文档 ACL、项目范围映射、生产恢复/容量与新依赖图审计 |

## M1：AnySeek 全文首个切片

已接入原有发布与搜索路径，Turso 保留权限、来源和版本权威；向量、图与 Blob 保持原职责。
每个发布最多 64 块、编码后请求 2 MiB，具体合同见 [API](API.md#anyseek-全文适配m1-首个切片)。
检索字段采用固定词项编码，BM25 排序不承诺与默认后端一致。

本轮代码检视复现并修复了启动选择问题：已有本地发布不能直接改用 AnySeek；
AnySeek 目录将 endpoint/collection 绑定到 `full-text-backend.json`，在远端 IO 前拒绝
后端/目标变化和损坏绑定，凭据不写入文件。原数据仍可用原后端重开。

真实 AnySeek 的公开库/Service/HTTP/MCP 验证覆盖原计划恢复、隔离、更新、历史、
撤销和重开；新增启动绑定用例后为 6 项真实用例。精确源码、实际检查和未执行项只在
[VALIDATION](VALIDATION.md) 维护，远端 CI 按具体提交核验。
MCP fixture 通过不等于真实 Claude Code/Codex 客户端或目标产品整体完成。

## 下一步

1. 评审目标架构的待确认决策；选择研发/业务各一个任务、负责人和初始数据源。
2. 核验 Claude Code/Codex 的 MCP、原生 Skill 加载和事件采集能力，明确版本与覆盖限制。
3. 确认上下文快照、Skill 发布和 Run 证据的最小合同，再贯通记忆/知识/Skill 等上下文的管理、供给与追溯。
4. 试点数据接入前落实员工身份、文档权限、保留策略和源更新/删除同步；冻结任务建立收益基线。

目标阶段与验收见[架构规划](superpowers/specs/2026-11-16-agent-data-foundation-architecture.md#14-交付阶段与验收)，
前序 M1–M3 检索切片见[实施记录](superpowers/plans/2026-11-16-anyseek-claude-code-mvp.md)，不再作为完整产品范围。
[Issue #64](https://github.com/TheLudlows/Origence/issues/64) 的相关性/延迟/资源与
[Issue #65](https://github.com/TheLudlows/Origence/issues/65) 的维护恢复全范围尚未验收；
已有精确版本的受控故障证据不代表生产容量或任意断电保证。

## 范围与证据

设计/实现/测试只面向新建数据库，不提供旧库升级或迁移；保留同版本恢复、来源和持久化要求。
完整 SaaS、分布式与通用 Agent 编排后置。真实模型效果、企业权限、生产 SLO 和断电保证分别验收。

架构见[当前设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md)，运行见
[OPERATIONS](OPERATIONS.md)，依赖历史结论见 [DEPENDENCY_AUDIT](DEPENDENCY_AUDIT.md)。
旧 PR 流水、实验数字和失败修复只在 VALIDATION/固定工件中保留，不再复制到进度页。
