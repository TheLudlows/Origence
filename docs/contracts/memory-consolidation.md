# 长期记忆归纳

写入先保存新事实的原始输入，单个持久 Worker 再读取同一主题的证据，归纳带来源的长期记忆。接入方提交新内容和幂等键即可。工作区和权限由服务端根据凭据确定。

| 操作 | HTTP | MCP | Rust |
| --- | --- | --- | --- |
| 追加新内容 | `POST /v1/memories/retain` | `context_memory_retain` | `Service::retain_memory` |
| 查询长期记忆 | `POST /v1/memories/long-term/lookup` | `context_memory_recall` | `Service::recall_long_term_memory` |
| 按记忆 ID 读取 | `GET /v1/memories/long-term/{id}` | — | `Service::recall_long_term_memory_by_id` |

## 追加新内容

写入需要工作区 writer 权限和已配置的提取模型。管理员通过 `POST /admin/memory-properties` 登记可归纳的单值属性及必填条件。例如登记 `hotel.limit`，要求 `country` 和 `unit`。

HTTP 请求体只包含新的 `content`，通过 `Idempotency-Key` 请求头传入稳定键。MCP 参数包含 `content` 和 `idempotency_key`。原文上限为 64 KiB，幂等键为 1–200 个 ASCII 字符，同一请求重试时沿用键和原文。受理在同一事务保存 `memory_retain` 来源事件、作业和 `{state:"accepted",source_event_id,job_id}` 回执。

```json
{"content":"Atlas 在中国的酒店报销上限为每晚 100 元。"}
```

```json
{"content":"Atlas 在中国的酒店报销上限自 2026-12-01 起为每晚 80 元。"}
```

模型只负责从这次原文提取事实。归纳使用登记的单值属性、主体的稳定标识和精确适用条件。主体标识、条件值和事实值须逐字出现在引用中，引用须匹配原文 UTF-8 半开字节区间。引用中的日期须明确标为生效起点，例如「自 2026-12-01 起」或 `effective from 2026-12-01`。日期支持 `YYYY-MM-DD` 或带完整年份的 `YYYY年M月D日`，都按 UTC 当日零点解释。检测到原文含生效条件而模型漏报日期时，该事实暂不进入归纳，原文仍可核查。缺少年份、事实归属或其他适用条件时，同样保留原文，等待补充证据。伪造或不合法的引用会使作业失败。原文保存和模型提取是两个阶段。

首版支持从某日开始持续生效的规则。检测到截止日期或临时有效期的文档只保留原文，避免将临时规则归纳成长期有效的结论。

用 `GET /v1/jobs/{job_id}` 检查状态。完成的作业在 `result.memories` 返回本次产生的长期记忆 ID 和版本。无可归纳事实时，列表为空。归纳期间，新原文已保存，读取仍以当时提交的版本为准。

## 查询结论和证据

工作区 reader 可按完整 `identity` 查询。省略 `version` 读取当前版本，省略 `as_of_ms` 按当前时间计算结论。`as_of_ms` 是 Unix 毫秒时间戳，可查询同一证据版本在不同生效日期的结论。

拿到作业返回的 `memory_id` 后，也可以通过 `GET /v1/memories/long-term/{id}` 读取。可在 URL 中提供 `version` 和 `as_of_ms`，授权和结果与按身份查询一致。

```json
{
  "identity": {
    "subject": {"kind": "project", "stable_id": "Atlas"},
    "predicate": "hotel.limit",
    "context": {"country": "中国", "unit": "每晚"}
  },
  "as_of_ms": 1796083200000
}
```

返回 `memory_id`、`version`、`identity`、`state`、`timeline` 和 `evidence`。`state.status` 为 `current`、`conflicting` 或 `unknown`，只有 `current` 含 `value`。`timeline` 记录结论变化及起始生效时间，`evidence` 列出来源事件、原文引文及生效时间。读取整份源输入须具备原文读取权限。

归纳在同一工作区及完整身份范围内计算。查询时间之前或当天已经生效的有日期证据中，取最后生效日期上的事实。相同日期存在不一致的值时，结论为 `conflicting`。如果当前还没有已经生效的有日期证据，则使用无日期的事实作为基线。无日期证据之间不一致同样会产生冲突。没有适用证据时，结论为 `unknown`。因此，例子中 11 月 30 日仍是 100 元，12 月 1 日起为 80 元，先后收到两份通知的顺序无关。

同一主题保留稳定 ID。独立的新证据可以改变结论，也可以为相同结论新增佐证，证据集合有变化时产生新版本。原幂等键的重试复用原作业。撤回来源时，从受影响主题的剩余证据重算。引用了已撤回来源的历史版本退出读取。

未归纳出该身份、指定版本的证据已撤回或当前主题的全部证据已撤回时，查询返回 404。全部撤回后再次积累有效证据，会沿用原主题 ID。单个主题最多保存 256 条有效事实，单个工作区的归纳属性目录最多读取 128 个已登记的单值属性。超限的归纳保留原文和失败回执，结论仍基于完整的已发布证据集合。

长期记忆走独立的精确查询接口。通用 `search`、`resolve` 和已有的显式记忆入口继续按各自契约工作。基于已归档会话反馈的 Learning 使用[学习契约](learning.zh-CN.md)。

## 验证范围

工程验证以合成原文和模型桩检查写入、归纳、召回、冲突、来源撤回与隔离。真实模型的证据提取和适用条件识别质量需单独评估。实际执行记录见[验证](../VALIDATION.md)。
