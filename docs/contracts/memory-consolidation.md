# 长期记忆归纳

| 操作 | HTTP | MCP | Rust |
| --- | --- | --- | --- |
| 追加原文 | `POST /v1/memories/retain` | `context_memory_retain` | `Service::retain_memory` |
| 按身份查询 | `POST /v1/memories/long-term/lookup` | `context_memory_recall` | `Service::recall_long_term_memory` |
| 按 ID 查询 | `GET /v1/memories/long-term/{id}` | — | `Service::recall_long_term_memory_by_id` |

## 写入

工作区 writer 提交 `{content}`，通过 `Idempotency-Key` 请求头或 MCP 的 `idempotency_key` 提供重试键。原文上限 64 KiB，键为 1–200 个 ASCII 字符。原文、作业和 `{state:"accepted",source_event_id,job_id}` 回执在同一事务内保存。同键同内容重放复用回执。

归纳需要已配置的提取模型，以及管理员通过 `/admin/memory-properties` 登记的单值属性和必填条件。主体标识、适用条件值和事实值须出现在原文引文中。引文使用 UTF-8 字节半开区间。不合法的引文导致作业失败。

生效起点须明确，支持 `YYYY-MM-DD` 或 `YYYY年M月D日`，按 UTC 当日零点解释。缺少适用条件、检测到生效日期却漏报、或存在明确截止日期或临时有效期时，原文留存，相关事实暂不归纳。

`GET /v1/jobs/{job_id}` 可查看结果。作业完成时返回 `outcome=consolidated`，`result.memories` 列出对应的长期记忆 ID 和版本。没有符合条件的事实时列表为空。作业处理中，查询使用已提交的结论。

## 查询和撤回

工作区 reader 可使用完整的 `identity`（主体、属性和适用条件）或作业返回的 `memory_id`。两个入口均可指定 `version` 和 `as_of_ms`（Unix 毫秒），默认使用最新版本和当前时间。

例如，属性 `hotel.limit` 要求 `country` 和 `unit`。先写入「Atlas 在中国的酒店报销上限为每晚 100 元」，再写入「Atlas 在中国的酒店报销上限自 2026-12-01 起为每晚 80 元」，即可在同一身份下查询两个时点的结论。结果包含 `memory_id`、`version`、`identity`、`state`、`timeline` 和 `evidence`。

- 按最近已生效的日期取值，未到生效日时使用无日期事实作为基线。同一日期或无日期基线内的值不一致时返回 `conflicting`，没有适用证据时返回 `unknown`。`current` 提供 `value`。
- 证据集合变化时新增版本，主题保留稳定 ID。`timeline` 保存结论变化及生效时间，`evidence` 提供来源事件、引文和生效时间。
- 撤回来源后基于剩余证据重新计算。没有可用证据或历史版本引用了已撤回来源时返回 404。主题收到新证据时继续沿用原 ID。
- 每个主题最多 256 条有效事实，每个工作区最多读取 128 个单值属性。超限时保留原文和失败回执。

长期记忆使用独立的精确查询。工程验证覆盖范围见[验证记录](../VALIDATION.md)。
