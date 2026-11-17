# Origence · 当前进度

更新：2026-11-17。本轮基于 main
[`c70a1aa`](https://github.com/TheLudlows/Origence/commit/c70a1aaa0b3cb75872109406dc17b1b461d1f771)
同步 AnySeek 服务凭据与 workspace 自动路由；SQLite 权威存储、检索诊断和 Run 关联保持已有实现。
产品边界见[架构规划](design/agent-data-foundation.md)。
**核心 API 与首版 UI 路径已实现；完整浏览器验收、双客户端和企业任务仍未完成。**

| 范围 | 已实现 | 尚未完成 |
| --- | --- | --- |
| 检索 | AnySeek 全文、向量与一跳图候选同组发布；SQLite 仍核验图证据，独立 `graph.db` 不再使用；真实引擎本地回归通过 | 真实模型检索质量、容量与延迟验收 |
| 私有会话 | 逻辑 key 绑定唯一平台归档，独立 key 隔离，授权轮换与撤销 | Claude Code / Codex 同 key 跨客户端真实任务验证 |
| Skill | 调用方提交候选、选定审阅包、精确批准、不可变版本、在线下载与两种原生目录格式导出 | 从经验生成候选的产品入口、原生受管加载与撤回、研发/业务试点 |
| Run | 私有 Run、服务端上下文准备清单、工具/Skill/结果/工件报告与采集缺口 | 两真实客户端的事件采集及可读的端到端追溯体验 |
| Node.js 管理界面 | 五页 UI、逐用户 HTTP 转发、受限资产/Run 目录、异步入库与固定版本/来源查看；见 [启动说明](../ui/README.md) | 真实浏览器布局与无障碍验收、生产计量与部署 |

## AnySeek 服务凭据与 workspace 集合

只配置 `OC_ANYSEEK_URL` 与 `OC_ANYSEEK_API_KEY`；已鉴权 tenant UUID 直接作为
AnySeek 命名空间，workspace UUID 对应 `ws_<无连字符 UUID>` 集合，无须另配租户或集合。
首次使用按固定 schema 自动初始化，并发及创建响应丢失后严格回读；已有发布的集合在
启动时检查，丢失/配置漂移不会静默建空。绑定 v6 固定路由及模型配置，首次集合使用时
持久化分析摘要。AnySeek 不签发租户 Key；Origence 用户权限和私有会话保持原语义。
验证见 VALIDATION。

## SQLite 权威存储与最小元数据

进一步将 SQLite/文件锁依赖设为可选：无默认特性的核心库可独立使用，纯核心与
Blob/PDF 测试不加载元数据库；SQLite authority 单独通过 `--features sqlite` 验证。
服务端合并事务入口的重复权限查询，关键词路径复用授权/候选快照，空读集合及没有
候选的后续路径不访问额外元数据表。提交与返回前仍核验当前授权和发布身份。

本地关系数据和持久任务只使用 rusqlite + bundled SQLite；旧引擎实现、依赖与公开别名
已移除。关闭驱动默认 features，不引入 ORM/异步数据库库；锁文件净减少 112 个包。
删除版本内容哈希列和账本重复幂等字符串，分别从不可变正文和现有完整主键获得。
授权、来源、版本历史、冻结发布计划与恢复状态仍是持久事实，详见[存储契约](contracts/storage.md)。
本次在 PR 的 Linux/Windows CI 验证真实 SQLite 的锁、WAL、事务取消与恢复；
精确本地结果、远端 CI 及未验收边界统一见 VALIDATION 和 PR。

## 检索模块拆分（已合入 PR #79）
## #76 关联 Run 本地切片

已同步 main `f62168b`（#81 SQLite）：资产详情可读取具体版本的当前 logical key 已记录 Run 引用。
有界索引与事件同事务写入，读取再次核验权威引用/资产/来源/所有权，不返回 Run 正文；
固定 v1 往返、另一管理员隔离、撤回、分页与重开已验证，见 [VALIDATION](VALIDATION.md)。
索引保留首条引用证明，不替代完整采集或 trace 可读性；新 schema fresh-only，旧库拒绝而不迁移。
本切片对应草稿 #83；第一批浏览器分支独立保留。#76 的计量、告警、PDF 页内文本和外部任务验收仍缺。


已同步 main `3d474d9`（含 #77 UI、#78 诊断）。已核验分路的排序/融合位于
`retrieval::rank`，最终发布及返回复核由 SQLite 事务方法负责；Search 与 EvidenceBundle
保持各自召回、原有快照边界和外部 IO 顺序，同时保留主干诊断采集及 Run 重放保护。
合并后的验证与各历史提交的结果分别见 [VALIDATION](VALIDATION.md)。

## 检索配置与诊断

当前切片基于 main `44e06df`：核对分析配置与 embedding 空间并持久绑定摘要；
Search/Context 返回有界查询诊断，Run 准备路径保存成功与失败观察。
成功准备的重放在核验原权限和来源后读取私有快照，不再依赖模型或 AnySeek；
查询诊断 v2 逐次核对 embedding 空间摘要，阻断启动后的空间变化。
最近 128 次尝试和明确省略数不构成完整客户端采集或跨存储快照。字段见 [API](API.md#检索诊断)。
CI 保留 Node UI 构建、真实 UI 门禁和固定夹具缓存，缓存与检出共用诊断 v2 后端的精确 SHA。
最终检查与真实 AnySeek 联调结果统一记录到 VALIDATION，历史 CI 结果按原提交保留。

## 本地 Run UI 定向验证

基于 `01d4f82` 的本地改动区分准备成功、服务端观察到准备失败与客户端上报。
真实 Chromium 已验证 Run → 固定 v1 → 来源撤回、旧引用再次点击及另一个 key 拒绝；
精确范围见 [VALIDATION](VALIDATION.md)。仅本地验证，#76 的其余规格仍未完成。

## 下一步

1. 完成 UI 真实浏览器布局、键盘/触屏、无障碍及 Windows 验证；预览 relay 故障仍需解决。
2. 通过合并后的 CI 后，冻结客户端/模型版本，以研发、业务各一个真实任务验证归档、Skill 复用和 Run 追溯。

精确测试结果、CI 运行及诊断限制见 [VALIDATION](VALIDATION.md)。
Skill 的 API 审批和目录导出不等于真实用户审阅、客户端加载或任务收益验收。

## 范围与证据

只面向新建数据库；同版本持久化、重启/重试与备份恢复仍是要求。
原始会话、Run 和未发布候选按逻辑 key 私有隔离；workspace 成员身份不授予他人原始归档访问权。
正式沉淀的长期资产供同 workspace 获授权成员复用，支持来源仍需有效；Skill 的
私有会话依据可能限制其他 key 的审阅或分发，详见 [Run 契约](contracts/runs.md)。
首批采用成员授权范围一致的 workspace；企业身份与差异文档 ACL 尚未实现。
平台只提供归档/查询，不恢复客户端工作现场。Skill 发布不授予工具执行权限。
