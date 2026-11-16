# Origence · AnySeek / Claude Code 前序接入记录

更新：2026-11-16。本文只保留前序全文接入的工程事实，现状见 [STATUS](../../STATUS.md)。
首版功能与后续顺序统一见[精简架构规划](../specs/2026-11-16-agent-data-foundation-architecture.md)：
首批客户端为 Claude Code/Codex，全文和向量统一 AnySeek，Skill 从记忆/执行经验沉淀并供用户审视。

## 已完成的接入切片

此前先以 Claude Code 配置和标准只读 MCP 验证 AnySeek 全文接入；
向量仍在 LanceDB，权威状态、图与原始 Blob 保留原职责。这是当时的实现状态，不是目标存储分工。

- [x] 核对既有 MCP、Turso/LanceDB/Blob 与 AnySeek HTTP 合同。
- [x] 接入可选 AnySeek 全文，沿用发布账本、单 Worker 和持久重试。
- [x] 固定作用域、来源/版本/generation、词项匹配与安全错误，候选过滤后仍做权威复核。
- [x] 用稳定 group/record 和保存的 epoch 完成组写入、清理、响应丢失确认与原计划恢复。
- [x] 持久绑定 endpoint/collection，拒绝隐式更换后端或损坏绑定。
- [x] 通过公开 Service、真实 AnySeek、HTTP/stdio MCP 的搜索、引用、更新/历史、撤销和重开验证。
- [x] 提供 Claude Code MCP 配置示例。
- [ ] 真实 Claude Code 客户端验收尚未完成，已纳入双客户端统一验收。

四个现有工具为 `context_search/context_get/context_resolve/context_bundle`。
协议 fixture 不能代替真实客户端或任务收益。精确版本、测试和限制只在 [VALIDATION](../../VALIDATION.md)维护。

## 已固定的两仓边界

Origence 管理权限、来源、版本、解析/embedding、发布与引用；AnySeek 保存标准 Record 并提供检索候选。
AnySeek Tenant key 只由宿主持有，不替代员工或文档权限。当前切片只调用全文，目标的向量接入仍待实现。

固定真实 fixture 为 [AnySeek 6614bbf](https://github.com/TheLudlows/anyseek/tree/6614bbf29bad0041c65738831bc1a16da4c69660)。
接口与编码见 [API](../../API.md#anyseek-全文适配m1-首个切片)，
部署与恢复见 [OPERATIONS](../../OPERATIONS.md#anyseek-全文与-claude-code)，当前不变量见[平台设计](../specs/2026-09-22-memory-knowledge-platform-design.md)。

## 后续工作入口

本记录不另行维护产品路线图，原 M2/M3 的功能枚举由精简规划取代。
全文/向量统一、真实双客户端、Skill 沉淀与最小运行追溯按新规划拆分；
现有验证证据与领域合同保留，不把未执行项算作通过。
