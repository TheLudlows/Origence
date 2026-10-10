# 本地运维说明

适用：M5 固定 SQLite/LanceDB/SQLite 图 运行栈。当前范围与实测平台见 [STATUS](STATUS.md) 和 [VALIDATION](VALIDATION.md)。

## 目录与进程

`OC_DATA_DIR` 默认 `.data`，统一包含：

| 路径 | 内容 |
| --- | --- |
| `context.db`、SQLite WAL/SHM | 权限、事件、版本、幂等、任务、owner、索引元数据、账本、审计 |
| `context.db.lock` | 独占宿主的 OS 锁文件；是否有文件不等于锁仍被持有 |
| `vectors/` | 按模型 profile 分表的 LanceDB 向量 |
| `graph.db`、SQLite WAL/SHM | SQLite 图实体、关系与遍历投影 |
| `blobs/<tenant>/<workspace>/uploads/<hash>` | 原始文件内容；按作用域和内容 hash 定位 |

实际锁文件命名和数据库路径以配置及启动实现为准。不要直接修改任一库的业务记录；向量和图不是来源状态的权威。数据目录只授予宿主 OS 身份访问权限，API key 不能保护已经取得本地文件访问权的用户。

`origence serve`（兼容别名 `api`）运行 HTTP 和单 Worker，Ctrl+C 请求优雅停止：HTTP 停止接收，Worker 完成当前任务后关闭存储。进程强退后 OS 自动释放锁；下次启动提升未完成作业的 run_token 并重放。不要删除锁文件来绕过正在运行的宿主。

`/health/live` 表示进程存活；`/health/ready` 要求 Worker 正在运行且存储检查通过。模型服务不计入 ready，单个模型失败体现在任务状态。启动对账或存储检查失败时不开放业务服务。

## 初始化与凭据

先执行 `origence --offline workspace-create NAME`，保存返回的 token，再启动宿主。在线 `key-create`/`key-revoke` 通过 `/admin/keys` 限定在 token 所属 workspace 内；需 admin。离线管理依赖操作系统授权，不要求 API token，必须独占数据目录。

程序读取进程环境变量，不自动读取 `.env`；Compose 会读取 `.env`。模型凭据、API token 和 `.env` 不应进入 Git。配置列表见 [项目 README](../README.md)。

## 显式启用记忆身份

本命令只为兼容当前本地基础 schema、但缺少 `oc_memory_identities` 的既有 SQLite 库安装身份表，不是通用版本迁移或 PG 导入。启动仍不会自动升级。

1. 正常停止宿主及所有离线客户端，按下节备份整个数据目录。
2. 预检：`origence --data-dir DATA_DIR --offline memory-identity-upgrade --dry-run`。
3. 安装：`origence --data-dir DATA_DIR --offline memory-identity-upgrade`。
4. 重启宿主；旧 get/search 应保持可用，新入口 `/v1/memories/identified` 可以写入。

输出 `{feature:"memory-identity-v1",status,dry_run}`：预检缺表返回 `required`，安装成功 `enabled`，已安装时 `already_enabled`。重复执行不会增加映射或版本。预检不改 schema/业务数据，但仍要求独占锁；它不是在线检查。

升级只在 `BEGIN IMMEDIATE` 事务内创建身份表，基础 schema 检查失败、已有身份表与已知 DDL 不一致（包括缺失唯一约束/外键）、同名视图占用、锁冲突时拒绝，不自动修复。已有身份表只允许当前已知定义，SQL 大小写和空白差异不影响检查。失败不会提交 schema 变更。

命令不要求 API key，也不加载模型、LanceDB/图存储或 Blob；授权依赖数据目录的操作系统访问权限。不存在的 `context.db` 不会被新建。旧资产、fact_key、事件和版本不被转换，已有记忆读取的 identity 仍为 null；新身份写入与旧槽碰撞仍返回 409。此工具仅启用新表，不为旧记忆推断主体或条件。

## 失败与恢复

| 现象 | 处理 |
| --- | --- |
| 锁被占用 | 使用 HTTP CLI/MCP；确需离线管理时先正常停止宿主 |
| 模型/解析失败 | 修正配置或输入；有效来源的 failed/cancelled 作业可显式 retry |
| 短暂存储错误 | 作业进入 retry_wait，最多 5 次；错误持久化，耗尽后 failed |
| Worker 或宿主退出 | 修复存储错误后重启；processing 在独占启动时恢复，无需等待租约超时 |
| 取消/删除发生在模型处理中 | 迟到提交复核 generation/权限/来源，不能覆盖或复活；外部孤儿由串行清理移除 |
| 原生写完成、SQLite 发布未提交 | 保存的发布计划和账本用于重放；启动先清除无 owner 产物，再幂等写入并发布 |
| cleanup 失败 | 读取仍被墓碑阻断；修复存储后重试失败的 cleanup，或重启触发全范围对账 |
| schema 不兼容 | 停止并保留现场，不自动 ALTER；使用匹配版本或新目录，不覆盖旧数据 |

已受理任务的 embedding profile 与 Worker 配置必须一致；改模型不会自动重建已有向量。强退发生在发布计划保存前，模型调用可能重复。发布保证幂等，不保证外部供应商计费恰好一次。

## SQLite 图替换边界

图存储使用 `graph.db`，沿用 `GraphStore`、确定性对象 ID 和 SQLite owner/跨存储账本。所有连接启用外键，实体删除级联移除入边、出边和自环；有向遍历最多三跳、去重并排除起点。新文件自动创建完整结构，已有不兼容结构拒绝打开，不自动修复。

本版本不加载旧 `graph/kuzu.db`，也不提供旧图文件格式迁移。按替换设计，图是可重新发布的派生投影。已有数据目录的 owner/账本不会因为新图文件为空而自动重建已发布图谱；不能把“服务可启动”当作“旧图已恢复”。切换前停服并备份整个目录，使用新目录重新导入/发布来源，再验证 graph/hybrid 检索后切换。不要只删除旧图文件或单独清空账本来尝试迁移。已有原始来源和旧备份应保留至重建完成。

## 备份与旧基线

1. 正常停止宿主，确认进程结束且无离线客户端持有数据目录。
2. 将**整个** `OC_DATA_DIR` 复制到备份位置，包含 SQLite 及其 WAL、向量、图、blobs 和关联文件；记录二进制版本和非敏感模型配置。
3. 恢复到独立目录，先用同版本离线工具检查，再启动并检查 ready、样本 get/search 和 job 状态。

不要在运行中分别复制各个数据库或只备份 `context.db`；目前没有在线跨库快照协议。本轮测试验证进程强退恢复，未完成完整备份演练或断电测试。删除保留原始内容，备份也仍包含这些内容。

PG 基线代码和测试可在 Git 提交 `72fb5aa` 查阅。M5 移除了 `src/db.rs`、PG 初始化/授权 SQL、Apalis 依赖及 PG 专用测试；新本地进程套件替代其核心应用验收。没有 PG 数据自动迁移功能，不应将 PG 数据目录当作 `OC_DATA_DIR`。
