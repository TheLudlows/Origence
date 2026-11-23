# Run 外部核验记录（P0，#89）

**简体中文** · [English](run-verification.md)

基线为 `293e2bf6af3b180d43c8050731047b552ac07519`。

首版只记录 **Run 所有者转录的外部核验**。平台既不实施核验，也不认证外部核验人的身份。服务保存已认证的凭据主体、逻辑所有者、回执时间和精确不可变 Run 前缀（序号和 SHA256）。`verifier_label` 与 `assessed_at_ms` 属于所有者声明。客户端 `reporter=user/test` 不是已认证角色。核验人标签不授予访问 Run、会话、来源或工作空间的权限。

`POST /v1/runs/{run}/verifications` 接受严格的独立 DTO，包含 `verification_id`、`through_sequence`、`prefix_digest`、`verifier_label`、`assessed_at_ms`、`conclusion`（`passed|failed|inconclusive`）、`basis_summary`、`basis_sequences`（前缀内互异的正整数事件序号）、`reported_skill_version`（null 表示未知，或精确 `skill_id`、正整数 `version` 和规范 `digest`）。版本是声明，不能由准备记录、最新出版物或成功上报推断。依据序号指向不可变事件，摘要是长度受限的脱敏文本，不能当作要抓取的 URL 或可信 HTML。至少提供一条依据序号和非空摘要。

只有具备读写权限的 Run 所有者可以创建或撤回。读取要求所有者与读权限。创建、重放、读取或撤回前，事务内检查前缀可见性及全部原始来源。摘要不一致返回冲突，来源不可访问时拒绝。任何接口均不接受调用方指定的所有者、主体、平台状态、经认证的核验角色或回执时间。服务返回 `evidence_origin:owner_transcribed_external`、`verifier_authentication:unverified_claim`、`platform_verification:not_performed`。外部判定失败可与客户端自报成功并存，互不改写，也不能据此推断模型使用。已准备好的 v2 bundle 只能证明准备。

UUID 是 Run 范围内的重试身份。相同请求返回同一持久记录和回执时间，包括后续撤回。改变负载返回冲突。记录不可变，只有单向撤回墓碑。`POST /v1/runs/{run}/verifications/{id}/withdraw` 接收 `{reason}`。相同重试保留首个墓碑，不同理由冲突，撤回不会重新激活证据。更正使用新 UUID。每个 Run 最多 1,000 条。`GET /v1/runs/{run}/verifications?after=UUID&limit=25` 按 UUID 排序，范围 1–100，返回 `items,next_after`，包括已撤回记录，每条重新检查前缀。不提供总通过标记、后台执行、外部抓取或新身份角色。

仅适用于新库。启用 `oc_run_verifications` 时要求精确结构，拒绝旧的不兼容库且不修改。无 JOIN 的查询与事务内 Rust 核验维持私有来源契约。原 MCP 与客户端采集接口不变。接入和试点负责人使用该 HTTP/服务 DTO，展示归属和撤回，区分自报、准备和评估。尚无生产任务或模型验收结论。

## PRD 与采集边界

本次仅对应“Run 所有者记录外部签认依据”，即转录外部核验声明和引用。未实现独立签认，也不允许用户核证自己的业务结果或计算已核证的业务通过率。即使 `conclusion:passed`，也仍是未经验证的外部声明，`platform_verification:not_performed`。#89 仍开放。

#87 使用既有 Run 接口实现显式客户端采集与准备
并明确维持核验语义。此次不改变原 Run 事件、报告者或准备数据结构，以及 MCP 采集接口、权限角色或共享 DTO。接入须分列准备、客户端上报、外部核验和平台核验，不能把 `user/test` 上报提升为经认证的独立签认。
