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

## PDF 解析

文本 PDF 使用 `pdf_oxide 0.3.78`，单次打开、逐页提取。同步文件 IO 与
解析在 `spawn_blocking` 中完成，由既有单 Worker 串行等待；解析模块可直接
通过库 API 调用，不再重启宿主二进制或使用隐藏的 `parse-pdf` 命令。

解析入口检查实际读取内容不超过 10 MiB、页数为 1–200、全文 UTF-8 文本
不超过 1,000,000 字节。沿用逐页质量检查：任一页不足 3 个字母/数字字符，
包括空白页或没有文本层的扫描页，整份文档失败，不发布前面已提取的页。
不启用 OCR 或密码解密；加密 PDF 拒绝。文件 IO 失败是内部错误，
不支持或不合格的 PDF 是输入错误。

宿主关闭 `pdf_oxide` 的原始日志（即使 `RUST_LOG` 打开更详细级别），避免
第三方诊断包含文档内部内容；Worker 记录 Origence 的任务 ID 和安全错误码。

上游会在触发内容操作数上限或遇到对象提前 EOF 时返回部分文本；此类
结构化诊断也使整份导入失败，不能把非空的部分提取结果当作完整证据。
`Cargo.lock` 将其传递依赖 `office_oxide` 保持为声明兼容的 0.1.9：0.1.13
增加 `DocumentIR` 字段，导致 pdf_oxide 0.3.78 编译失败。升级依赖必须重新
执行 locked 构建与解析回归，不应只删除锁文件重新解析版本。

locator 页码从 1 开始，字节和行号区间相对于该页提取文本，不能作为原始
PDF 二进制偏移。新文档记录 `pdf-oxide-0.3.78-v1`；旧版本内容及 parser
标记不改写。重新导入或更新文件才使用新解析器，不自动重建历史版本。

旧实现通过子进程实现 30 秒超时后强制终止解析。本实现移除了这个硬超时：
`spawn_blocking` 的同步闭包不能通过取消 future/JoinHandle 停止，输入和输出
上限也不是解析器内部的硬内存或 CPU 预算。优雅退出仍等待当前任务完成；
作业取消阻止迟到发布，不代表立即停止解析。若后续要求不可信文档的硬时间/
内存限制或崩溃隔离，应使用专用受限解析进程，并独立设计其生命周期，
不把子进程协议放回 `main`。当前版本不承诺任意 PDF 的有界解析时长。

## 初始化与凭据

先执行 `origence --offline workspace-create NAME`，保存返回的 token，再启动宿主。在线 `key-create`/`key-revoke` 通过 `/admin/keys` 限定在 token 所属 workspace 内；需 admin。离线管理依赖操作系统授权，不要求 API token，必须独占数据目录。

程序读取进程环境变量，不自动读取 `.env`；Compose 会读取 `.env`。模型凭据、API token 和 `.env` 不应进入 Git。配置列表见 [项目 README](../README.md)。

## 本地 embedding 服务

本机已使用 Ollama 的 BGE-M3 提供兼容 OpenAI API 的 embedding，配置为：

```powershell
$env:OC_ENABLE_MODELS = 'true'
$env:OC_MODEL_BASE_URL = 'http://127.0.0.1:11434/v1'
$env:OC_MODEL_API_KEY = 'ollama'
$env:OC_EMBEDDING_MODEL = 'bge-m3'
$env:OC_EMBEDDING_DIMENSION = '1024'
```

`ollama` 是本机服务的占位 key。上述值已在本机持久化为 Windows 用户环境变量；已打开的终端和宿主不会自动刷新，运行当前会话时可执行上述配置后再启动 Origence。Ollama 需保持运行，且已下载 `bge-m3`；接口路径为 `POST /v1/embeddings`。本配置只提供 embedding，不表示已配置抽取或生成模型。

更换模型或维度不会重建旧向量；评估使用独立临时数据目录，业务数据需按既有 profile 约束重新导入/发布。S1 结果和限制见 [VALIDATION](VALIDATION.md)，运行入口见 [评估说明](../evals/README.md)。

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

## AML 评测 namespace 与批次任务

AML 使用独立评测数据目录。新 SQLite 关系库创建 `oc_aml_namespaces`、`oc_aml_users`、`oc_aml_adds`。管理员通过库 API 或 `POST /admin/aml/namespace` 显式启用 namespace 后才允许在线创建用户 workspace。首次创建在同一个 BEGIN IMMEDIATE 事务中写入 workspace、映射和审计；并发首次请求复用同一映射，失败整体回滚。审计不保存外部 user_id 或消息正文。

已有数据库缺少两张映射表时仍可使用原功能，启动不会补建；AML 接口返回不可用。本切片未提供旧库升级命令，应创建专用评测新库，不手工向业务库复制 DDL。仅存在部分表、同名 view 或不兼容 AML 表定义时拒绝打开，不自动修复。已有身份 schema 的显式升级机制不变。

评测凭据的 namespace 是其原生 tenant/workspace；轮换为同 workspace 的新 key 可重新取得原用户映射。撤销旧 key 后，旧派生 AuthContext 和该 key 受理的待发布任务失去权限。同 namespace 的新 key 复用 Add receipt；密钥轮换不自动转移已受理任务的 created_by，因此应先排空旧 key 的任务再撤销它。

仅含前两张映射表的上一切片库仍可打开并使用 scope 接口，但 Add 返回 503；不会自动安装 oc_aml_adds。不兼容的 Add 表定义同样拒绝启动。评测继续使用专用新目录。

配置 embedding 后再启用 Add（环境变量沿用上文，库宿主可用 Models::configured 显式配置）。首版固定原文 vector，不调用摘要/抽取/图模型。每批正文与 receipt、资产、单个 aml_ingest job 在事务中一起落库；消息解析及预分词在 spawn_blocking、关系事务外执行，既有串行 Worker 等待完成后继续索引。没有新增解析进程或独立 Worker。超时/断线只终止观察，不取消同步闭包或持久任务，不提供硬时间/内存隔离保证。

Add 的 HTTP 等待上限为 25 分钟；上游模型每次调用沿用 45 秒上限。网关需允许此长连接，或使用相同 request_id/相同请求重试；不得换 ID 绕过超时。排队、模型调用都计入等待，单 Worker 和多次分块调用可能使大批次超过上限；这些输入上限不是吞吐/SLO 声明。优雅停止沿用等待当前 Worker 任务的语义，已有 Add 观察请求还可能等待到 25 分钟结束；强退后按原恢复机制继续持久工作。

模型失败沿用既有 failed 状态，不自动增加模型重试；相同 Add 重发仍指向失败任务并返回 503。授权库宿主用 submit_aml_add 的 receipt.job_id，在 lookup_aml_user 返回的 scope 内调用 Service::job_action(...,"retry") 显式重试原任务。当前没有额外 AML HTTP job 管理入口，原生 namespace 的 /v1/jobs 不能跨 scope 读取它。输入失败需新 request_id/修正内容；临时存储故障仍由原 Worker 的有限 retry_wait 处理。重放不保证上游模型计费一次。

日志和审计不输出 AML 消息、query、options、外部用户 ID 或模型诊断；原文仍在来源/版本/任务发布计划中持久保存，墓碑不等于物理擦除。固定模型的全分支隔离回归与真实本地 BGE-M3 小规模协议演练已通过；正式模型配置、持续容量、数据物理清除、公网部署和正式 Smoke 仍待独立验收。可重复预检、运行演练和退役步骤见 [AML_DRILL](AML_DRILL.md)。
