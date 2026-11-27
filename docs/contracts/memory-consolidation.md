# 长期记忆

长期记忆统一接收事实和经验的来源。事实保存按生效时间归纳的结论，经验保存经反馈和核验发布的做法。

| 操作 | HTTP | MCP | Rust |
| --- | --- | --- | --- |
| 提交来源 | `POST /v1/memories/retain` | `context_memory_retain` | `Service::retain_long_term_memory` |
| 统一召回 | `POST /v1/memories/long-term/recall` | `context_memory_recall` 带 `query` | `Service::recall_long_term_memories` |
| 按身份查看事实 | `POST /v1/memories/long-term/lookup` | `context_memory_recall` 仅带 `identity` 和可选时间、版本 | `Service::recall_long_term_memory` |
| 按 ID 查看事实 | `GET /v1/memories/long-term/{id}` | — | `Service::recall_long_term_memory_by_id` |

## 写入

工作区 writer 通过 `Idempotency-Key` 请求头或 MCP 的 `idempotency_key` 提供 1–200 个 ASCII 字符的重试键。提交 `{content}` 或 `{experience:{session_id,improve:{improve_id,conditions,max_feedback?}}}` 二选一。同键同请求复用原作业，同键改为另一种来源会冲突。

- **事实**提交可进入工作区的原文，最大 64 KiB。原文、归纳作业和 `{state:"accepted",source_event_id,job_id}` 回执在同一事务保存。
- **经验**引用当前逻辑 key 所有的私有会话和已提交的反馈。显式发起这个请求即授权系统按 `conditions` 核验并发布经验结论。原始问答和反馈按私有会话权限读取。回执含 `improve_id` 和 `job_id`，完整发布进度可从 `GET /v1/improvements/{id}` 查询。[经验核验与发布](learning.zh-CN.md)

归纳需要已配置的提取模型，以及管理员通过 `/admin/memory-properties` 登记的单值属性和必填条件。主体标识、适用条件值和事实值须出现在原文引文中。引文使用 UTF-8 字节半开区间。不合法的引文导致作业失败。

生效起点须明确，支持 `YYYY-MM-DD` 或 `YYYY年M月D日`，按 UTC 当日零点解释。缺少适用条件、检测到生效日期却漏报、或存在明确截止日期或临时有效期时，原文留存，相关事实暂不归纳。

`GET /v1/jobs/{job_id}` 可查看事实作业的结果。作业完成时返回 `outcome=consolidated`，`result.memories` 列出生成的事实 ID 和版本。没有符合条件的事实时列表为空。召回使用已发布的结论。

## 统一召回

工作区 reader 提交 `{query,identity?,as_of_ms?,limit?,applicability_context?}`。例如以「酒店 报销」为检索词，结果可以同时包含当前报销上限（`fact`）和政策冲突时的核对方法（`experience`）。每类默认最多返回 10 条，`limit` 可设为 1–20。

- 事实使用最多 32 个查询词项，在按主题 ID 和证据版本排列的最多 512 条有效证据中匹配。如果扫描范围之外还有证据，`fact_scan_exhausted` 为 `true`。指定完整 `identity` 可跳过扫描，直接读取该事实。词项命中只代表相关，标为 `unverified`。精确身份命中标为 `caller_identity_exact_match`。每条结果携带事实版本和所用来源的字节区间。
- 经验使用已发布内容的全文索引召回，每条只返回工作区可读的结论、适用条件及发布分块引用。只有调用方显式传入的 `applicability_context` 覆盖全部原始条件时才标为 `caller_context_exact_match`，否则标为 `unverified`。两类结论在同一个最终授权快照内检查来源有效性。

`as_of_ms` 只指定事实的生效时点（Unix 毫秒），默认当前时间。经验返回当前可见的发布版本。

## 事实详情和撤回

按完整 `identity`（主体、属性和适用条件）或 `memory_id` 查看事实详情。两个入口均可指定 `version` 和 `as_of_ms`，默认使用最新版本和当前时间。

例如，属性 `hotel.limit` 要求 `country` 和 `unit`。先写入「Atlas 在中国的酒店报销上限为每晚 100 元」，再写入「Atlas 在中国的酒店报销上限自 2026-12-01 起为每晚 80 元」，即可在同一身份下查询两个时点的结论。结果包含 `memory_id`、`version`、`identity`、`state`、`timeline` 和 `evidence`。

- 按最近已生效的日期取值，未到生效日时使用无日期事实作为基线。同一日期或无日期基线内的值不一致时返回 `conflicting`，没有适用证据时返回 `unknown`。`current` 提供 `value`。
- 证据集合变化时新增版本，主题保留稳定 ID。`timeline` 保存结论变化及生效时间，`evidence` 提供来源事件、引文和生效时间。
- 撤回事实来源后基于剩余证据重新计算。没有可用证据或历史版本引用了已撤回来源时返回 404。主题收到新证据时继续沿用原 ID。撤回经验的支持来源会使该经验退出召回。
- 每个主题最多 256 条有效事实，每个工作区最多读取 128 个单值属性。超限时保留原文和失败回执。

事实的来源区间可供接入方引用。会话的结构化 `used_evidence` 当前接收分块、实体和关系引用，经验的发布分块可直接归档。[验证记录](../VALIDATION.md)
