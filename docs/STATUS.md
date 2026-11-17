# Origence · 当前进度

实现以 main `43708c8`（已合入 [PR #72](https://github.com/TheLudlows/Origence/pull/72)）和
[首版规划](design/agent-data-foundation.md) v0.8 为基线。本分支只面向新建数据库，
不自动转换已有 schema 或旧检索索引。

| 范围 | 此分支实现 | 仍需验证或补齐 |
| --- | --- | --- |
| 检索 | AnySeek 全文和向量、同组发布、目录和模型 profile 绑定，移除 LanceDB 实现及依赖 | 真实 AnySeek 版本的向量/HTTP/MCP 合同及质量、容量 |
| 私有会话 | 稳定逻辑 key 绑定服务端归档，显式归档/读取、授权轮换、撤销 | Claude Code 与 Codex 的真实同 key 跨客户端任务 |
| Skill | 显式候选、选定审阅包、精确批准、不可变发布、在线授权下载和双客户端格式导出 | 自动提炼策略、原生受管加载与撤回、研发/业务真实试点 |
| Run | 私有 Run、服务端上下文准备清单、客户端上报、Skill/结果/工件引用和采集缺口 | 两真实客户端的事件采集与端到端追溯；未上报项保持未知 |

workspace 资产可供同空间有权限成员使用。原始会话、Run、未发布候选和私有来源不随
发布共享；发布批准不授予工具执行权限。源码行为、HTTP 和本地夹具验证不等于真实客户端、
模型效果或生产验收。准确结果见 [VALIDATION](VALIDATION.md)。

## 范围与证据

首批可使用授权覆盖所有成员的同权限 workspace；企业身份与差异文档 ACL 未实现，
不能混入权限不兼容资料。实现只面向新建库，保留同版本恢复和来源持久化，不覆盖或删除既有数据。
[#64](https://github.com/TheLudlows/Origence/issues/64) /
[#65](https://github.com/TheLudlows/Origence/issues/65) 的质量、负载及维护恢复结论未完成。
当前实现见[平台设计](design/current-implementation.md)，操作见 [OPERATIONS](OPERATIONS.md)
及[客户端接入](CLIENTS.md)。
