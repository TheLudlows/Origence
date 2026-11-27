# Origence · 能力与验收盘点

**简体中文** · [English](CAPABILITIES.en.md)

2026-11-27 快照。以主干 [`8d6605f`](https://github.com/TheLudlows/Origence/commit/8d6605ff6dcb4ceaa60c72cd5a9106dcd2525461) 为准，
该提交的 `fast-check`、`node-ui`、`linux-native` 和 `windows` 均通过。[主干 CI](https://github.com/TheLudlows/Origence/actions/runs/45935327181)

持续进度见 [STATUS](STATUS.md)，测试环境与实际结果见 [VALIDATION](VALIDATION.md)，产品边界见[目标架构](design/agent-data-foundation.md)。

## 已有能力与验证边界

| 能力 | 已有操作与条件 | 已验证与待验证 |
| --- | --- | --- |
| 显式记忆与受控提取 | 「记忆管理」、`POST /v1/memories/semantic` 和 `POST /v1/captures/semantic`。提取需要 writer、抽取模型和调用方限定的身份。已有单值出现不同抽取结果时分别保留证据，显式确认可发布新的当前版本。 | 并发、重放、冲突保留和来源撤回已有工程回归。提取准确率待实测。[记忆契约](contracts/memory.md) · [受控提取](contracts/semantic-capture.md) |
| 长期记忆与经验 | `context_memory_retain` 接受事实原文，或引用当前逻辑 Key 私有会话的反馈发起经验核验。reader 可以用带 `query` 的 `context_memory_recall` 按词项共同召回事实和已发布经验。按完整身份或 ID 可读事实的时间线、版本和依据。 | 已用合成模型验证生效时间、冲突、经验适用条件、授权和撤回。词项命中标为 `unverified`，实际模型质量和跨会话收益待验证。[长期记忆契约](contracts/memory-consolidation.md) |
| 知识与任务上下文 | Text、Markdown、PDF 入库，支持版本读取、撤回和原文定位。`POST /v1/context` 与 `context_bundle` 可按业务时点 `as_of_ms` 召回归纳事实，返回时间线和原文依据。Run 准备入口明确拒绝显式 `as_of_ms`。 | 合成报销案例经真实 HTTP、MCP 和 Chromium 验证了日期、冲突、预算、精确引用与撤回。真实任务效果和大数据量延迟待验收。[上下文契约](contracts/evidence-bundle.zh-CN.md) |
| 私有问答与反馈 | writer 可以显式归档。Codex 和 Claude Code 可按项目启用增量同步，默认关闭。当前逻辑 Key 可通过 `context_bundle` 的 `session` 分支召回。同步问答的实际证据使用标为 `unknown`，Improve 会跳过这些轮次。 | 显式归档有双客户端工程证据。新增同步在合成 Hook 与真实 HTTP 上验证了脱敏、隔离和重放。原生双客户端及 Windows/macOS 的增量同步待验证。[客户端接入](CLIENTS.md#按项目开启私有问答增量归档) |
| Skill | Run 所有者可在调用链路选定截止事件，从固定前缀手工起草、编辑和提交候选。审阅、批准和显式发布沿用 CLI/API。已发布包支持双客户端导出。 | 合成数据验证了固定前缀、幂等重试、修改后旧批准失效、权限和撤回。原生双端加载有独立的公开样例证据，未见题复用待 #91 验收。[候选操作](SKILL-CANDIDATE-CLI.zh-CN.md#在控制台从-run-起草) |
| Trace 与计量 | 两端插件默认安装结构化 Run Hook。控制台展示私有 Run、精确引用、采集声明、外部核验和实际收到的执行方计量。 | 原生双端安装和默认 Hook 有合成工程证据。完整事件覆盖、真实整轮用量和独立业务结果待验收。[Run 契约](contracts/runs.zh-CN.md) |
| 管理与接入 | 支持账号登录、工作区授权和创建、API Key 管理。一个逻辑 Key 可获多个工作区授权，通过 HTTP 显式指定工作区，或在 MCP 连接时指定。`origence-setup` 接入 Agent，`origence-deploy` 部署本地服务。 | 已验证控制台工作区切换及原生插件安装。原生 Agent 会话内切换、企业身份和差异文档权限仍是缺口。[安装说明](INSTALLER.md) · [工作区现状](STATUS.md#多-workspace-api-凭据) |

## 检索基线与正式验收

- [#163](https://github.com/TheLudlows/Origence/pull/163) 恢复默认 AND，查询词项必须出现在同一个索引分块内，混合检索的关键词分支也使用该条件。真实 AnySeek 契约 8 项通过。[#154 的 OR 实验](https://github.com/TheLudlows/Origence/pull/154) 保留为历史对照。[检索契约](API.md#检索)
- [#152 中文文档评测](https://github.com/TheLudlows/Origence/issues/152#issuecomment-7192373228) 在 `13e2a4c` 使用 36 篇合成文档和 300 道题。Origence 来源 Recall@5 为 97.57%（281/288），12/12 道无答案题仍返回候选。[跨会话检查点](https://github.com/TheLudlows/Origence/issues/152#issuecomment-7193961142) 跑通 21 道接线题，完整 500 题的答题准确率和拒答率待测。指标各自适用于记录的修订与配置。
- [#91 正式试点](https://github.com/TheLudlows/Origence/issues/91) 仍为 `NOT_RUN`。研发排障与报销复核已有公开合成演练。正式冻结仍缺获准的未见题和材料、模型版本与预算，以及独立验收的 DBA、制度负责人等角色。[冻结准备表](../evals/pilot/FREEZE-READINESS.md)
