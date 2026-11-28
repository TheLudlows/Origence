# 长期记忆

长期记忆统一接收事实和经验的来源。事实保存按生效时间归纳的结论，经验保存按适用条件发布的做法。

| 操作 | HTTP | MCP | Rust |
| --- | --- | --- | --- |
| 提交来源 | `POST /v1/memories/retain` | `context_memory_retain` | `Service::retain_long_term_memory` |
| 统一召回 | `POST /v1/memories/long-term/recall` | `context_memory_recall` 带 `query` | `Service::recall_long_term_memories` |
| 按身份查看事实 | `POST /v1/memories/long-term/lookup` | `context_memory_recall` 仅带 `identity` 和可选时间、版本 | `Service::recall_long_term_memory` |
| 按 ID 查看事实 | `GET /v1/memories/long-term/{id}` | — | `Service::recall_long_term_memory_by_id` |

## 写入

调用方使用工作区 Key，并提供 1–200 个 ASCII 字符作为 HTTP `Idempotency-Key` 或 MCP `idempotency_key`。以下请求体任选一项。同键同请求复用原作业，修改请求返回冲突。

- `{content}` 提交事实原文，最大 64 KiB。原文、归纳作业和 `{state:"accepted",source_event_id,job_id}` 回执在同一事务保存。
- `{experience:{statement,why,conditions,basis}}` 直接提交已选定的做法、理由和适用条件。依据可为 `{kind:"asset_version",asset_id,version,content_sha256}`，固定本工作区已发布的知识版本和摘要，也可为 `{kind:"human_confirmation"}`，由提交者声明人工确认。系统记录提交凭据身份及时间。直接提交经过既有知识入库作业，无需私有会话或归档问答。回执含 `learning_id`、`asset_id`、`source_event_id`、`job_id` 和 `status:"accepted"`。作业发布后才进入召回。
- `{experience:{session_id,improve:{improve_id,conditions,max_feedback?}}}` 引用当前逻辑 key 所有的私有会话及其反馈，并授权按 `conditions` 核验和发布经验。原始问答和反馈受私有会话权限约束。回执含 `improve_id` 和 `job_id`，可通过 `GET /v1/improvements/{id}` 查看发布进度。[经验核验与发布](learning.zh-CN.md)

归纳需要已配置的提取模型，以及管理员通过 `/admin/memory-properties` 登记的单值属性和必填条件。主体标识、适用条件值和事实值须出现在原文引文中。引文使用 UTF-8 字节半开区间。不合法的引文导致作业失败。

生效起点须明确，支持 `YYYY-MM-DD` 或 `YYYY年M月D日`，按 UTC 当日零点解释。缺少适用条件、检测到生效日期却漏报、或存在明确截止日期或临时有效期时，原文留存，相关事实暂不归纳。

`GET /v1/jobs/{job_id}` 可查看事实作业的结果。完成时返回 `outcome=consolidated`，`result.memories` 列出已生成的事实 ID 和版本，可能为空。

## 统一召回

调用方使用工作区 Key 提交 `{query,identity?,as_of_ms?,limit?,applicability_context?}`。`as_of_ms` 指定事实的生效时点（Unix 毫秒，默认当前时间）。经验使用当前可见的发布版本。每类默认最多返回 10 条，`limit` 可设为 1–20。

- `fact` 使用最多 32 个查询词项匹配按主题 ID 和证据版本排列的最多 512 条有效证据。还有证据未扫描时，`fact_scan_exhausted` 为 `true`。指定完整 `identity` 可跳过扫描。词项命中标为 `unverified`，精确身份命中标为 `caller_identity_exact_match`。结果包含事实版本和所用来源的字节区间。
- `experience` 使用已发布内容的全文索引，每条经验返回结论、解释、适用条件和一个发布分块引用。直接提交的经验还返回 `publication`，包含选定依据和提交记录。它记录发布者声明和知识版本的有效性。调用方传入的 `applicability_context` 逐字匹配全部原始条件时标为 `caller_context_exact_match`，否则标为 `unverified`。

两类结果在同一个最终授权快照内检查来源有效性。经验的发布分块引用可直接用于会话归档的 `used_evidence`。

## 事实详情和撤回

按身份查看时提交 `{identity,version?,as_of_ms?}`，其中 `identity` 包含主体、属性和适用条件。按 `memory_id` 查看时通过查询参数传 `version` 和 `as_of_ms`。详情返回 `{memory_id,version,identity,state,timeline,evidence}`，默认使用最新版本和当前时间。

例如，先后提交同一酒店报销规则「每晚 100 元」和「2026-12-01 起每晚 80 元」，即可按同一身份查询不同生效时点的结论。

- 按最近已生效的日期取值，未到生效日时使用无日期事实作为基线。同一日期或无日期基线内的值不一致时返回 `conflicting`，没有适用证据时返回 `unknown`。`current` 提供 `value`。
- 证据集合变化时新增版本，主题保留稳定 ID。`timeline` 保存结论变化及生效时间，`evidence` 提供来源事件、引文和生效时间。
- 撤回事实来源后基于剩余证据重新计算。没有可用证据或历史版本引用了已撤回来源时返回 404。主题收到新证据时继续沿用原 ID。撤回经验的支持来源会使该经验退出召回。
- 每个主题最多 256 条有效事实，每个工作区最多读取 128 个单值属性。超限时保留原文和失败回执。

[验证记录](../VALIDATION.md)
