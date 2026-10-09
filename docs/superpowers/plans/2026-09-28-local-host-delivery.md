# M5 本地宿主交付与验收

日期：2026-09-28。设计来源：[存储主计划](2026-09-22-pluggable-storage-engine.md)、[整体设计 A2](../specs/2026-09-22-memory-knowledge-platform-design.md#storage-design)。当前事实见 [STATUS](../../STATUS.md)，实测结果见 [VALIDATION](../../VALIDATION.md)。

2026-10-09 核验补记：PR #10 已合入 main `73fc51a`，最新轻量 CI 已通过 fmt、27 项 lib、58 项无原生集成和 Python 5 项；原生/平台/release/container 作业尚未全部完成。下方测试映射说明代码覆盖位置，不代表所有新增用例已在当前 commit 的原生构建中通过。Service 仍依赖 LocalEngine/SqliteTx；M5 固定装配成立，应用层完全后端解耦尚未交付。

## 实现

| 范围 | 实现位置与行为 |
| --- | --- |
| 固定装配 | `service.rs`：OC_DATA_DIR 下打开 SQLite、LanceDB、Kuzu 和 Blob；初始化、检查、恢复、清理后才交给宿主 |
| 宿主生命周期 | `host.rs`：一个 API 和单 Worker 共享 Service/Engine，监督退出并优雅关闭；OS 文件锁拒绝第二进程 |
| 客户端访问 | `client.rs`、`main.rs`、`mcp.rs`：默认 HTTP，显式离线，转发鉴权，不隐式回退 |
| 权限与事务 | `storage/sqlite.rs`：IMMEDIATE 写、短事务、scope 与撤销复核、原子业务/审计/幂等/入队 |
| 跨库发布 | `worker.rs`：保存版本化 publication payload 与 pending 账本，外部幂等写，重新核验并原子确认发布 |
| 崩溃恢复 | processing 提升 run_token 后重排；启动清理无 owner 原生产物，保存计划可重放且版本不重复 |
| 删除与共享来源 | SQLite 先墓碑/撤回；Worker 按 owner 删除精确向量 ID、关系/实体孤儿，先清摘要再删 chunk |
| 检索 | `retrieval.rs`：可见候选先过滤再向量 top-k；关键词/摘要/图证据映射，RRF，返回前再核验 |
| 发布入口 | 默认 `local-storage`；CLI/Compose/CI 不需要 PG/Apalis。旧代码参考 Git `72fb5aa` |

短事实 publish 和 restore 不自动生成摘要/图；知识 ingest 在配置 extraction model 时生成。启用加工失败则任务失败，不静默宣称图可用。共享实体只返回规范化身份和 SQLite 核实的来源证据，避免泄露已失效来源的最后写入描述。

## 测试映射

- `tests/local_app.rs`：真实 HTTP、CLI、MCP、PDF 子进程；权限、版本恢复、中文检索、幂等、共享图来源、取消重试、撤销/删除迟到工作、宿主强退重启、显式离线与无回退、优雅关闭。
- `saved_publication_recovers_after_external_graph_write`：构造真实持久化边界（图已写、SQLite 仅 pending、版本尚未发布），关闭重开后清理并重放；验证只新增一个版本且最后来源撤回后图消失。这是可控边界注入，不冒称逐指令进程崩溃覆盖。
- `tests/lancedb_store.rs`：scope/profile/dimension/generation/source-version 隔离、输入校验、精确候选过滤在 top-k 前执行、幂等和重开删除。
- SQLite/账本/owner/锁与 Kuzu 适配器原有套件继续经 `tests/local.rs` 执行；存储测试使用真实临时文件，模型使用本地 stub。

完整质量门的实际结果、失败修复和未验证项统一记入 VALIDATION，不用历史 PG 或 M0 探针结果代替 M5 验收。

## 文档与交付边界

README、API、STATUS、运维、验收、环境模板、Docker/Compose、CI 和评估入口同步；文档索引明确区分运行契约、未来产品设计、历史代码草稿和固定版本调研。保留原研究事实，不将旧 serverless/PG 方案当作现行部署步骤。

Windows 本地基线是此次交付目标；Linux/macOS/release、镜像运行、远端 CI、最低 Rust 版本、断电恢复、在线备份与长期规模测试需后续独立验收。自动发布不在当日 M5 范围内，随后已于 2026-09-29 完成；候选审核已移除。P1 会话/蒸馏、P2 SaaS 和未来 PG 扩展仍未交付。Windows 远端 CI 后续已通过，见 VALIDATION 的 2026-10-08 核验记录。

