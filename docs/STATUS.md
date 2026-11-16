# Origence · 当前进度

更新：2026-11-16。实现核对基线为已合入 PR #71 的 main `17fa39582aefb2c53165fae62bf8c41de4aab34c`。
[精简架构规划](superpowers/specs/2026-11-16-agent-data-foundation-architecture.md) v0.5 明确：
上下文包含记忆、知识、Skill 与任务状态；Skill 从记忆和 Agent 经历沉淀，供用户审视；
**目标全文与向量检索统一 AnySeek**，运行追溯贯穿整个闭环。规划变更不代表代码已完成替换。

## 当前实现与核心差距

| 范围 | 当前代码 | 首版要补齐 |
| --- | --- | --- |
| 上下文与来源 | 文本/Markdown/PDF、记忆语义、Turso 权威版本、原始 Blob、引用和撤回、EvidenceBundle | 任务交付清单与必要状态，保持来源和权限校验 |
| 检索 | 默认 LanceDB 全文/向量；可选 AnySeek 全文已合入 | AnySeek 全文/向量统一、移除 LanceDB 检索实现与无用依赖、真实后端验证 |
| Skill | 有 SessionTurn/Feedback、Improve → Learning，尚无 Skill 沉淀与分发服务 | 记忆/经历到候选、用户审视/修改/发布、两客户端版本分发 |
| 最小可观测性 | 显式会话与反馈归档，尚无通用 Run 证据链 | 关联上下文、Skill、可观察工具、结果与采集缺口 |
| 客户端 | HTTP/CLI、四个只读 stdio MCP 工具、Claude Code 配置示例 | Claude Code/Codex 的真实数据读取、Skill 加载与事件能力验收 |

AnySeek 全文接入及目录后端绑定已有公开库、真实后端和 HTTP/MCP 验证；
向量尚在 LanceDB。精确合同见 [API](API.md#anyseek-全文适配m1-首个切片)，版本证据见 [VALIDATION](VALIDATION.md)。
协议 fixture 不代表真实客户端、任务收益或新规划已验收；Learning 也不等于 Skill。

## 下一步

1. 将全文与向量统一到 AnySeek，整理依赖，完成两客户端的真实搜索/读取。
2. 贯通运行依据、Skill 候选、用户审视与版本发布，验证研发/业务各一个 Skill 的复用。
3. 补齐任务上下文清单和 Run 追溯，让用户能核验依据、工具、结果与采集缺口。

顺序与验收统一见[规划第 7 节](superpowers/specs/2026-11-16-agent-data-foundation-architecture.md#7-实施顺序与验收)；
前序切片仅作[工程记录](superpowers/plans/2026-11-16-anyseek-claude-code-mvp.md)。

## 范围与证据

首批可使用授权覆盖全部成员的同权限 workspace；企业身份与差异文档 ACL 当前未实现，不能混入权限不兼容资料。
实现只面向新建数据库，不提供旧库升级/迁移；保留同版本恢复与来源持久化，不能覆盖或删除既有数据。
实际客户端、Skill 提炼质量、负载/收益与恢复分别验收；
[#64](https://github.com/TheLudlows/Origence/issues/64)/[#65](https://github.com/TheLudlows/Origence/issues/65) 的完整质量/负载与维护恢复结论仍未完成。
当前实现架构见[平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md)，运行见 [OPERATIONS](OPERATIONS.md)。
