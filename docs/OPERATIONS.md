# 本地运维说明

适用单宿主 SQLite 权威数据、Blob 和独立 AnySeek 检索。按操作阅读：[启动与凭据](#初始化与凭据) · [AnySeek](#anyseek-全文与向量) · [模型](#本地-embedding-服务) · [恢复](#失败与恢复) · [备份](#备份与恢复)。进度与实测边界见 [STATUS](STATUS.md) 和 [VALIDATION](VALIDATION.md)。

## 当前阶段范围

只面向新建数据库，启动要求完整当前 schema；不自动补表、升级或迁移已有数据。
同版本持久化、重启/重试和备份恢复仍是要求，不兼容结构拒绝打开。
保留原始来源及备份，不能因使用新目录而删除或覆盖旧数据。
事务查询、身份占位与发布计划约束见[存储不变量](contracts/storage.md)。

## 目录与进程

`OC_DATA_DIR` 默认 `.data`，统一包含：

| 路径 | 内容 |
| --- | --- |
| `context.db` 及 SQLite WAL | 权限、原始来源事件、版本指针/内容哈希、locator、摘要召回词项、幂等、任务/恢复计划、owner、账本、审计 |
| `context.db.lock` | 独占宿主的 OS 锁文件；是否有文件不等于锁仍被持有 |
| AnySeek workspace collection（远端） | 已发布分块正文、摘要/模型版本与定位信息，以及同条 Record 的全文/向量/图索引；必须单独备份 |
| `full-text-backend.json` | AnySeek endpoint、tenant/workspace 自动路由规则和 embedding profile/维度绑定；不含 key，随整目录备份 |
| `blobs/<tenant>/<workspace>/uploads/<hash>` | 原始文件内容；按作用域和内容 hash 定位 |

实际锁文件命名和数据库路径以配置及启动实现为准。不要直接修改任一库的业务记录；向量和图不是来源状态的权威。数据目录只授予宿主 OS 身份访问权限，API key 不能保护已经取得本地文件访问权的用户。

`origence serve`（兼容别名 `api`）运行 HTTP 和单 Worker，Ctrl+C 请求优雅停止：HTTP 停止接收，Worker 完成当前任务后关闭存储。进程强退后 OS 自动释放锁；下次启动提升未完成作业的 run_token 并重放。不要删除锁文件来绕过正在运行的宿主。

`/health/live` 表示进程存活；`/health/ready` 要求 Worker 正在运行且存储检查通过。模型服务不计入 ready，单个模型失败体现在任务状态。启动对账或存储检查失败时不开放业务服务。

## PDF 解析

文本 PDF 使用 pdf_oxide 0.3.78，单次打开、逐页提取；同步 IO/解析在 spawn_blocking 中由单 Worker 串行等待，解析模块可直接通过库 API 调用。

| 检查 | 边界 |
| --- | --- |
| 输入与输出 | 实际读取 ≤10 MiB、1–200 页、提取全文 ≤1,000,000 UTF-8 字节；不是解析器内存预算 |
| 完整性 | 每页至少 3 个字母/数字；空白/扫描无文本页、加密、内容操作数超限或对象提前 EOF 导致整份失败，不发布部分文本 |
| 功能与错误 | 无 OCR/密码解密；IO 失败为内部错误，不支持/不合格 PDF 为输入错误 |
| 来源定位 | 页码从 1 开始，字节/行号为页内提取文本位置，不是 PDF 二进制偏移；新 parser 为 pdf-oxide-0.3.78-v1，旧版本和标记不改写，仅重新导入/更新使用新解析器 |
| 日志 | 即使 RUST_LOG 提高级别也关闭库原始日志；Worker 只记录任务 ID 与安全错误码 |

Cargo.lock 保持兼容的 office_oxide 0.1.9；0.1.13 的 DocumentIR 字段变更会使 pdf_oxide 0.3.78 编译失败。升级须运行 locked 构建与解析回归，不以删除锁文件解决。

旧实现通过子进程实现 30 秒超时后强制终止解析。本实现移除了这个硬超时：
`spawn_blocking` 的同步闭包不能通过取消 future/JoinHandle 停止，输入和输出
上限也不是解析器内部的硬内存或 CPU 预算。优雅退出仍等待当前任务完成；
作业取消阻止迟到发布，不代表立即停止解析。若后续要求不可信文档的硬时间/
内存限制或崩溃隔离，应使用专用受限解析进程，并独立设计其生命周期，
不把子进程协议放回 `main`。当前版本不承诺任意 PDF 的有界解析时长。

## 初始化与凭据

先执行 `origence --offline workspace-create NAME`，保存返回的 token，再启动宿主。在线 `key-create`/`key-revoke` 通过 `/admin/keys` 限定在 token 所属 workspace 内；需 admin。离线管理依赖操作系统授权，不要求 API token，必须独占数据目录。

程序读取进程环境变量，不自动读取 `.env`；Compose 会读取 `.env`。模型凭据、API token 和 `.env` 不应进入 Git。命令参数以 origence --help 为准；本地模型配置见下节。

## AnySeek 全文与向量

全文、向量和一跳图候选只走 AnySeek，SQLite 仍决定版本、授权、来源和可见性。已发布正文与摘要文本由同一 AnySeek collection 持久保存。Claude Code/Codex
只需要 Origence key，见[客户端接入](CLIENTS.md)。

1. 准备支持整组替换、analysis、查询 diagnostics v2、服务凭据和 UUID 租户头的 AnySeek（Catalog 7；版本见 CI pin）。
2. 配置 URL 与服务间凭据，使用独立新 Origence 数据目录启动：

```sh
export OC_ANYSEEK_URL=http://127.0.0.1:8081
# 通过宿主私密环境设置 OC_ANYSEEK_API_KEY（AnySeek 服务间凭据）。
# 不启用 embedding 时可设 OC_ENABLE_MODELS=false 验证全文。
```

不再配置 `OC_ANYSEEK_TENANT` 或 `OC_ANYSEEK_COLLECTION`。每个数据请求从服务端已鉴权
Scope 取得 `tenant_id`，作为 `X-AnySeek-Tenant`；`workspace_id` 转为
`ws_<去掉连字符的 UUID>` 集合名。一个 workspace 对应一个集合，同租户不同 workspace
使用不同集合，不同租户使用不同命名空间。客户端和模型不能指定 AnySeek 路由或服务凭据。
Origence 自身的用户 key、私有会话、候选范围过滤和最终授权/来源/版本复核仍然生效。

宿主启动验证 `/api/v1/live` 服务凭据和版本；不会创建占位集合。workspace 首次检索或
发布时先读取集合，缺失才按固定 schema 创建。并发和创建响应丢失后，通过严格回读核验
schema、分词器实现、停用词摘要、容量、维度与 embedding_space_id，不覆盖不兼容集合。
`origence anyseek-schema [--with-space]` 仍可用于查看所需 schema，无须手工建集合。

`OC_ANYSEEK_URL` 要求 HTTPS origin（loopback 可用 HTTP），不能带路径、凭据或查询参数。
容器的 loopback 指容器自身。后端绑定 `origence-content-backend-v2` 保存 endpoint、路由规则、固定模型 profile/维度，
不保存服务凭据；首次使用集合时，在远端发布前持久化 analysis 摘要。只有尚无已发布全文的
空目录允许摘要暂未初始化。重开先检查已有权威发布对应的集合；已提交集合丢失或配置漂移
明确失败，不自动重建空集合。同目标轮换服务凭据不改绑定，旧绑定格式不自动转换。
备份须保存整个 Origence 目录，并按 AnySeek 规则备份各 tenant/workspace 集合。

每次发布最多 64 块、编码后的请求最多 2 MiB，文本与向量一起写入一组 Record。
SQLite 在远端提交后才发布权威版本。超时未知时检查当前组，合法作业按原持久计划恢复；
未决且超过原操作窗口时保留现场，不靠新操作 ID 猜测结果。
检索先按授权身份过滤、核对索引候选，再由 SQLite 复核可见性。撤回先阻断读取，索引清理异步执行。
实际模型效果、生产容量、跨系统在线快照不在本地 UT 证明范围。

## 本地 embedding 服务

以下以 Ollama 的 BGE-M3 提供兼容 OpenAI API 的 embedding 为例：

```powershell
$env:OC_ENABLE_MODELS = 'true'
$env:OC_MODEL_BASE_URL = 'http://127.0.0.1:11434/v1'
$env:OC_MODEL_API_KEY = 'ollama'
$env:OC_EMBEDDING_MODEL = 'bge-m3'
$env:OC_EMBEDDING_DIMENSION = '1024'
```

ollama 是本机服务的占位 key。上述命令配置当前 PowerShell 会话；新终端需重新设置或显式配置持久环境，已运行宿主不会自动刷新。先下载 bge-m3 并保持 Ollama 运行，接口为 POST /v1/embeddings；该配置不提供抽取或生成模型。

更换模型或维度不会重建旧向量；评估使用独立临时数据目录，业务数据需按既有 profile 约束重新导入/发布。运行入口见 [评估说明](../evals/README.md)；旧引擎上的历史实验不能当作当前 AnySeek 质量结果。

## 离线记忆身份检查

原生 SQLite 新库包含完整身份 schema，身份存于 oc_assets，删除资产仍保留占位；详见[存储不变量](contracts/storage.md)。

`identity-check` 对已有数据库执行
只读同版本检查，包括格式标记和完整 authority schema，不创建或修复任何表。

1. 正常停止宿主及所有离线客户端，备份整个数据目录。
2. 执行 `origence --data-dir DATA_DIR --offline identity-check`。
3. 合法同版本库输出 `{"feature":"memory-identity-v1","valid":true}`；检查失败以非零状态退出。

缺文件、缺表、未标记旧库、弱约束或同名视图占用均拒绝，不安装、不转换。
命令不加载模型、AnySeek、图或 Blob；依赖 OS 目录权限并要求独占路径锁。
不存在的 context.db 不会被新建；只读检查没有升级状态或写入模式。
库调用方改用 `SqliteStore::check_memory_identity(path)`，成功返回 `()`。

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
| 已发布正文缺失或哈希/locator 不符 | 停止依赖该数据，保留现场并从匹配的完整备份恢复；不自动重解析来源或用空正文回退 |
| schema 不兼容 | 停止并保留现场，不自动 ALTER；使用匹配版本或新目录，不覆盖旧数据 |

已受理任务的 embedding profile 与 Worker 配置必须一致；改模型不会自动重建已有向量。强退发生在发布计划保存前，模型调用可能重复。发布保证幂等，不保证外部供应商计费恰好一次。

## 图存储与来源边界

对外 hybrid 检索只扩展一跳。实体和边由 AnySeek 的 `origence-content-sha256-v1` collection
提供候选，按来源版本附在一个现有 chunk Record 上，跟随同一组原子发布。上限仍为 64 个
chunk，并受完整请求字节预算限制；超限拒绝，不分批暴露。

`context.db` 的 owner 和发布账本仍是证据权威。来源撤回立即阻断其证据，AnySeek 中的残留投影
不会让关系复活；同一关系有其他有效来源时仍可召回。图投影随所属来源的 chunk 组清理。
宿主不再创建、打开或维护独立的 `graph.db`；已有的图文件不读、不迁移、不删除。
不提供旧库升级，不要手工清空账本或索引来试图迁移。

## 备份与恢复

SQLite 随程序编译，无需安装 SQLite 服务或客户端库。可写连接固定 WAL、
`synchronous=FULL` 与外键约束。宿主正常关闭会等待连接线程完成 SQL、回滚并关闭数据库；
请求取消不保证立即停止正在执行的 SQL；取消的租约不能继续执行后续业务。
已经发出的 COMMIT 可能完成但未确认，重试必须沿用原命令幂等键核对结果。
元数据精简、格式与只读 WAL 检查边界见[存储契约](contracts/storage.md#sqlite-与最小元数据)。

1. 正常停止宿主，确认进程结束且无离线客户端持有数据目录。
2. 将**整个** `OC_DATA_DIR` 复制到备份位置，包含 SQLite 及其 WAL、AnySeek 绑定、blobs 和关联文件；记录二进制版本和非敏感模型配置。按 AnySeek 操作指南另存对应 collection 的同版本备份；不要认为本地目录包含 AnySeek 的正文、摘要、全文、向量或图索引。
3. 恢复到独立目录，先用同版本离线工具检查，再启动并检查 ready、样本 get/search 和 job 状态。

不要在线分别复制数据库或只备份 context.db；当前没有在线跨库快照协议。历史专用测试库曾完成停机整库备份恢复，生产备份恢复及断电仍待独立验收。逻辑删除保留原文，备份也包含原文。

## 通用消息批次与所属 workspace

所属 workspace 的访问能力由 root 管理员显式授予。具备能力的 root 凭据用于受信服务，派生访问受 reader 或 writer 权限限制。调用方通过公开 Rust API 选择目标 workspace。接口、幂等与授权细节见 [API](API.md#通用所属-workspace-与消息批次库接口)。

提交消息批次需要配置与接受规格一致的 embedding profile。批次以原文向量入库，接受规格与任务一同持久化。解析和外部调用继续在存储事务外执行，入库使用既有单 Worker。轮换前可先等旧凭据提交的任务完成，撤销会阻断该凭据的未完成发布。

库调用方通过 `wait_message_batch` 指定观察时限。等待结束只停止观察，原持久任务由 Worker 继续处理。HTTP 宿主已移除旧评测入口的长等待处理。优雅停止仍等待当前 Worker 和进行中的 HTTP 请求。

## 检索分段诊断

设置 RUST_LOG=error,origence::search_timing=debug 可记录成功 Search 的 authorization_ms、embedding_ms、eligibility_ms、vector_ms、branches_ms、verification_ms、total_ms，以及 vector_batches/native_count/returned_count。日志不含 query、证据、用户/资产 ID、路径或凭据。默认 info 不输出这些 debug 事件。

branches_ms 包含候选装载、融合及已启用的辅助分支；verification_ms 是最终新权限快照及身份复核。total_ms 从输入校验后计到结果序列化前后的一部分，不包含外层 HTTP 编码或网络；不能当 HTTP 端到端延迟。失败和身份不存在的提前返回不生成该完成事件，故不能用这些事件统计错误率。完整 HTTP 耗时须另行测量。本次只增加计时，Worker、超时、取消、关闭、隔离和读取权限保证不变。

## 记忆与受控提取运行边界

属性由管理员在首次分配相关身份前注册，定义不可变，不允许改表绕过约束。
语义写入复用单 Worker、保存计划、版本竞争及来源复核；任一支持来源撤回会使整版退出读取，
通用 restore 不能绕过。详细规则见[记忆契约](contracts/memory.md)。

`semantic_extract` 使用既有 extraction 配置，单次模型请求上限 45 秒。
模型调用、JSON 校验与分词在事务外运行；计划在 embedding 前持久化。
已保存计划按原 provenance 重用，计划提交前失败可能重新调用模型，不保证外部计费一次。
尚无计划时 extraction 配置 fingerprint 必须匹配受理配置，embedding profile 始终须匹配。

调用方显式提交 targets/expected_version/allow_update，没有自动监听或新 Worker。
恢复与最终提交复核 creator、generation/run_token、版本及完整来源；取消阻止迟到发布，
不保证停止已发出的模型调用或 blocking 工作，优雅退出等待当前作业。
空候选/duplicate 可合法完成，superseded 需重读版本再提交。输入/输出限制见
[受控提取](contracts/semantic-capture.md)和 [API](API.md#对话语义捕获)。

## 查询诊断与 Run

先比较 binding 中的 analysis_fingerprint 与 AnySeek collection 的 analysis.fingerprint。
通过新 API 的查询响应可按 x-anyseek-request-id 关联服务器的 response_prepared 调试日志；
HTTP 错误提供安全的 x-anyseek-error-code。该日志只说明响应已准备，body 交付仍未确认。
查询诊断 v2 必须返回与配置一致的 embedding_space_sha256；同维度换空间也会拒绝。
成功 Run 准备可在后端故障时复用私有快照，重放仍核验当前权限和原始来源；
该成功回执不代表后端已经恢复，新准备请求仍需正常访问后端。
Run 中的 retrieval 诊断可区分未发出、网络失败、HTTP 错误和不合规诊断；原始错误正文不落库。
后端索引进度在查询后采样，出现落后/失败/恢复标志时先处理 AnySeek 健康状态，
不能手动推进消费序号或以空召回掩盖故障。完整字段与采集边界见 [API](API.md#检索诊断)。

## Windows 原生开发联启

已初始化的本机 AnySeek + Origence 可用 [`tools/start-dev.ps1`](WINDOWS-DEV.md) 在同一
PowerShell 7.4+ 控制台启动，可选 Node UI。首次初始化、readiness、凭据和停止限制见该文档；
开发入口禁用模型，不替代生产服务管理或迁移。
