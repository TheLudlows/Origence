# Origence · 当前进度

更新：2026-11-17。代码基线为 main
[`0a1a56c`](https://github.com/TheLudlows/Origence/commit/0a1a56c1da74e78be0118782b04a3f5a37251060)
（[PR #73](https://github.com/TheLudlows/Origence/pull/73)）；产品边界见[架构规划](design/agent-data-foundation.md)。
**首版核心路径已实现，CI 尚有失败，真实双客户端和企业任务尚未完成验收。**

| 范围 | 已实现 | 尚未完成 |
| --- | --- | --- |
| 检索 | AnySeek 全文/向量、同组发布、模型与目录绑定；真实引擎合同回归通过 | 真实模型检索质量、容量与延迟验收 |
| 私有会话 | 逻辑 key 绑定唯一平台归档，独立 key 隔离，授权轮换与撤销 | Claude Code / Codex 同 key 跨客户端真实任务验证 |
| Skill | 调用方提交候选、选定审阅包、精确批准、不可变版本、在线下载与两种原生目录格式导出 | 从经验生成候选的产品入口、原生受管加载与撤回、研发/业务试点 |
| Run | 私有 Run、服务端上下文准备清单、工具/Skill/结果/工件报告与采集缺口 | 两真实客户端的事件采集及可读的端到端追溯体验 |

## 当前阻塞与下一步

1. 修复无默认特性构建：测试辅助模块无条件导入受特性控制的 `Service`。
2. 修复 MCP 冒烟脚本：空参数 schema 缺少 `properties` 时触发 KeyError。
3. 通过 CI 后，冻结客户端/模型版本，以研发、业务各一个真实任务验证归档、Skill 复用和 Run 追溯。

精确测试结果、CI 运行及诊断限制见 [VALIDATION](VALIDATION.md)。
Skill 的 API 审批和目录导出不等于真实用户审阅、客户端加载或任务收益验收。

## 范围与证据

只面向新建数据库；同版本持久化、重启/重试与备份恢复仍是要求。
原始会话、Run 和未发布候选按逻辑 key 私有隔离；workspace 成员身份不授予他人原始归档访问权。
正式沉淀的长期资产供同 workspace 获授权成员复用，支持来源仍需有效；Skill 的
私有会话依据可能限制其他 key 的审阅或分发，详见 [Run 契约](contracts/runs.md)。
首批采用成员授权范围一致的 workspace；企业身份与差异文档 ACL 尚未实现。
平台只提供归档/查询，不恢复客户端工作现场。Skill 发布不授予工具执行权限。
