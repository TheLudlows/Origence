# Origence · 当前状态

更新：2026-10-13。当前适合本地开发和验证，未完成生产级验收。

## 当前架构与接入状态

[PR #58](https://github.com/TheLudlows/Origence/pull/58) 已合入 main
`1d5e0554b38e12df094a894f11a23fb26a117568`。单宿主内 API 与单 Worker 共享存储：

- Turso Database 0.8.2 Rust 引擎的 context.db 是权限、来源、版本、任务、幂等、owner 和发布账本的权威。
- 独立 Turso graph.db 保存图投影；图命中仍由 context.db 复核，两个库不共享业务事务。
- LanceDB 0.40.0 保存 chunk 全文与向量派生索引；原文关键词走原生 BM25，向量按冻结条件选择 ANN 或 exact。
- BlobStore 保存上传原文件；Summary 仍在 Turso/app 按同 chunk 跨行聚合后做 AND，不使用 Turso FTS。

写入先持久化计划和 pending 账本，再写原生索引，最后在权威事务中发布。
检索先筛选有效身份，再原生排名，返回前重新核对权威状态；撤回先隐藏再清理。
实现边界见 [平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md#storage-design)，
运行限制见 [OPERATIONS](OPERATIONS.md#turso--lancedb-运行边界)。

<a id="当前证据与结论"></a>

接入实现提交 `b50e8748` 的[工程 CI](https://github.com/TheLudlows/Origence/actions/runs/38405514854)
通过：无默认特性 78 + 99、默认特性 93 + 221 项 Rust 测试，HTTP/MCP、受控模型进程恢复和 keyword 种子评估。
合入提交的结果独立查看 [main CI](https://github.com/TheLudlows/Origence/actions/runs/38417503442)，不以 PR 的绿灯代替。
历史逐次 PASS/FAIL/NOT_RUN 与工件见 [VALIDATION](VALIDATION.md)；真实模型质量、生产 SLO、任意断电保证未验收。

## 当前 CI 范围

PR 运行 fast-check、linux-native；main 另运行 Windows。macOS 和 Docker 不在日常自动 CI 内，
保留本地构建、Dockerfile/Compose 和容器冒烟工具。Linux 继续执行原生测试、HTTP/MCP、进程恢复及关键词评估。
历史“五项 CI”只适用于对应提交。当前配置以 [.github/workflows/ci.yml](../.github/workflows/ci.yml) 为准。

## 当前阶段范围

设计、实现、测试与验收统一以新建数据库为目标；版本升级、旧库 schema 兼容和数据迁移不在当前范围内，不作为当前缺口、验收门槛或待办。只有后续明确调整需求时才重新考虑。开发约束见 [AGENTS](../AGENTS.md#current-stage-database-scope)，执行范围见[产品路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md#当前阶段范围)。

这一范围不放宽正常持久化、重启/重试恢复、同版本备份恢复、来源保留或不兼容 schema 拒绝打开的要求，也不授权删除或覆盖既有数据。已有工具和历史升级/迁移说明仅描述既有行为，不代表当前建设目标。

## 产品能力

| 范围 | 已实现 | 主要缺口或边界 |
| --- | --- | --- |
| 入库与存储 | 文本/Markdown/PDF、结构化记忆、受限原文抽取；Turso 权威关系库与独立图投影、LanceDB 原生全文/向量、本地 Blob | 单宿主、单 Worker；PDF 无硬取消/进程隔离 |
| 记忆身份与语义 | 显式身份、精确 lookup/过滤、版本条件；workspace 属性目录、单值更新/多值集合、冲突与未归一化证据 | 仅面向新库；显式写入或受控模型提取；不自动推断稳定身份/别名，真实模型分类质量未验收 |
| 检索与上下文 | keyword/vector/hybrid、摘要/图开关、带来源与身份的 search/resolve | 统一 EvidenceBundle、独立类型召回/字节预算、明确单值身份冲突已实现；不推断普通文本矛盾，未证明真实 Agent 任务收益 |
| 治理与恢复 | workspace/tenant 隔离、角色/密钥、幂等任务、版本/恢复、来源撤回、重启恢复 | 逻辑删除不等于物理擦除；无企业身份、文档 ACL、配额或生产 SLO |
| 接入与维护 | HTTP、CLI、只读 MCP、显式离线向量压实 | 不自动调度维护；持续容量、维护故障和断电边界仍需验收 |
| 会话学习 | 跨会话历史检索；原文候选提取 → S2 写入 → 同 workspace 跨调用方复用；实际证据归档与幂等反馈 | 捕获、归档和 Improve 均显式触发；无自动改权，真实模型质量未验收 |

接口与运行细节分别见 [API](API.md)、[OPERATIONS](OPERATIONS.md)；历史测试和 CI 的精确版本见 [VALIDATION](VALIDATION.md)。

## AML 接入与产品复用

Add/Search 的协议只保留在 API：认证 tenant/root 与完整 user_id 通过固定 UUIDv5 派生一层所属 workspace；不保存 AML namespace/user 映射。root key 仅供受信后端使用，可访问该 root 的全部所属 workspace；接入服务负责终端用户认证，不能将 root key 分发给用户。每个新 key 或轮换 key 均须显式授予默认关闭的 allow_owned_workspaces。

Add 转为通用 MessageBatch，保存不可变批次、消息原文、接受规格和精确批次/消息范围，复用单 Worker 整批发布，全部索引可检索后才确认成功。同一用户多个 session 共同检索，session_id 只标记来源。跨 key 重放复用原回执但不转移任务 creator；撤销旧 key 不允许新 key 接管未完成任务。Search 对未知用户只读返回空，不创建 workspace。

正式 AML Search 保持原文 vector，关闭摘要/图，不生成 Answer。聊天重排、本地 BGE reranker、时间辅助与同模型核验留在实验层。新来源 parser 为 message-batch-ranges-v1，带来源事件/批次/版本信息；历史 parser/报告不改写。跨 session 历史检索不等于反馈学习，真实模型质量、正式组别/模型、持续容量/清理验收、官方 Smoke/Full 尚未完成。服务器和域名按用户要求后置，剩余任务见 [AML 计划](AGENT_MEMORY_LEADERBOARD.md)。

## 下一步与历史证据

- 依赖风险按新锁文件独立审计；[DEPENDENCY_AUDIT](DEPENDENCY_AUDIT.md) 顶部区分当前未知项与旧版本结果。
- 真实 Agent 对照、维护故障、持续容量、冷启动及标签复核按[路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md#后续验收及启动条件)另行验收。
- AML 官方 Smoke/Full 与真实评测导入仍未完成；服务器和域名后置，进入条件见 [AML 计划](AGENT_MEMORY_LEADERBOARD.md)。
- S4 合成诊断工件的历史上传待办见[阶段记录](STATUS_HISTORY.md#第五阶段s4-evidencebundle-与独立-agent-框架)，不代表真实模型验收。

S2、S3、SessionTurn/Feedback、Improve/Learning、S4、AML 收敛、可见性整理与旧依赖处置的
阶段叙述集中到 [历史状态快照](STATUS_HISTORY.md)。原始验证记录、实验数据及结论不改写；
历史“下一步”不自动成为当前计划。完整 SaaS、分布式和新增后端按实际需求另行立项。
