# 本地运维说明

适用单宿主原生 Turso/LanceDB/Turso 图；存储接入 PR #58 与查询改写 PR #59 已合入。PR #59 主干为 `75c238ffa830d50840d4b84c2dce6d7d69a4793c`，不代表生产容量或任意断电验收。按操作阅读：[启动与凭据](#初始化与凭据) · [模型](#本地-embedding-服务) · [恢复](#失败与恢复) · [备份](#备份与旧基线) · [身份检查](#离线记忆身份检查) · [AML](#aml-适配启用与批次任务) · [向量压实](#显式停机压实向量文件)。当前进度及验证见 [STATUS](STATUS.md)、[VALIDATION](VALIDATION.md)。

## 当前阶段范围

当前开发与验收以新建数据库为目标，不建设或验收版本升级、旧库 schema 兼容及数据迁移能力；只有后续明确调整需求才重新考虑，见 [STATUS](STATUS.md#当前阶段范围)。当前身份入口为只读 `identity-check`，旧升级命令已移除；旧基线说明仅供识别安全边界，不提供升级或安装。

同版本运行中的持久化、重启/重试恢复和备份恢复要求不变。不兼容 schema 仍应拒绝打开；使用新目录不意味着可以删除、覆盖或静默转换旧数据，原始来源和备份继续按既有要求保留。

## PR #59 查询与恢复边界

关联查询采用同一事务快照内的参数化单表批量读取与 Rust 关联，不以跨表
EXISTS/IN/CTE 替代 JOIN。权限、历史版本、source/version/generation/publication
校验和最终返回前复核保留；Context/startup 的读取范围和调用方投影按所需数据收窄。
隐藏行或未消费的异常字段不应污染合法结果；选中必要非法字段仍安全报错，不保留
多个坏值的首错顺序或旧 panic。自动 SQL guard 及其测试已撤除，禁 JOIN 编码约束
与人工审核要求仍按 [AGENTS](../AGENTS.md#sql-query-constraints) 执行；历史静态检查不等于全程序证明。

Windows 原离线恢复回归和主线程原 1 MiB 栈保留。PR #59 已合入的精确主干
[CI 38554048610](https://github.com/TheLudlows/Origence/actions/runs/38554048610)
通过 Linux/Windows 门禁；该结果仅证明已测调用路径，不证明任意 Turso SQL
解析路径均无栈风险。分阶段故障、原生读取与保存/重试实测见
[VALIDATION](VALIDATION.md#pr-59无-join-分阶段验证)。

固定槽位（多数 8，向量就绪匹配 4）限制请求数，不是字节/总内存预算。
状态检查不把完整向量计划读入 Rust；准备及重试仍承担计划序列化和原生校验成本，
ID 元数据仍为 O(total ID bytes)，没有新增输入上限。

MessageBatch 发布计划与轻量回执摘要原子保存，同代重试复用并核对摘要。
正分块已准备计划缺失或损坏摘要会明确报无效，重试不会自动修复，不提供已有
计划的回填迁移。出现该错误应保留原库和现场，不手工补摘要或清除账本。
缺失/null/空 chunks 的既有状态语义保留；正分块计划遵循上述摘要要求，
异常 chunks 根类型明确报错。有摘要不等于发布成功，实际索引与发布账本仍须满足可检索门槛。
直接改数据库中的向量计划而不改摘要，不会由状态查询完整重扫检测。
这些变化不提供新容量、任意断电或 parser 内存隔离保证。

## 目录与进程

`OC_DATA_DIR` 默认 `.data`，统一包含：

| 路径 | 内容 |
| --- | --- |
| `context.db` 及原生 Turso WAL | 权限、事件、版本、幂等、任务、owner、索引元数据、账本、审计 |
| `context.db.lock` | 独占宿主的 OS 锁文件；是否有文件不等于锁仍被持有 |
| `vectors/` | 按模型 profile 分表的 LanceDB 向量及 chunk_terms_v1 全文表 |
| `graph.db` 及原生 Turso WAL | Turso 图实体、关系与遍历投影 |
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

## 本地 embedding 服务

本机已使用 Ollama 的 BGE-M3 提供兼容 OpenAI API 的 embedding，配置为：

```powershell
$env:OC_ENABLE_MODELS = 'true'
$env:OC_MODEL_BASE_URL = 'http://127.0.0.1:11434/v1'
$env:OC_MODEL_API_KEY = 'ollama'
$env:OC_EMBEDDING_MODEL = 'bge-m3'
$env:OC_EMBEDDING_DIMENSION = '1024'
```

ollama 是本机服务的占位 key。上述命令配置当前 PowerShell 会话；新终端需重新设置或显式配置持久环境，已运行宿主不会自动刷新。先下载 bge-m3 并保持 Ollama 运行，接口为 POST /v1/embeddings；该配置不提供抽取或生成模型。

更换模型或维度不会重建旧向量；评估使用独立临时数据目录，业务数据需按既有 profile 约束重新导入/发布。S1 结果和限制见 [VALIDATION](VALIDATION.md)，运行入口见 [评估说明](../evals/README.md)。

## 离线记忆身份检查

原生 Turso 新库始终包含完整身份 schema。#60 将身份列并入 oc_assets，删除身份独立表；旧布局只读检查失败，不迁移、不修复。删除资产仍保留身份占位，同版本备份恢复需使用完整当前格式。详见[身份布局合同](contracts/identity-asset-layout.md)。

`identity-check` 对已有数据库执行
只读同版本检查，包括格式标记和完整 authority schema，不创建或修复任何表。

1. 正常停止宿主及所有离线客户端，备份整个数据目录。
2. 执行 `origence --data-dir DATA_DIR --offline identity-check`。
3. 合法同版本库输出 `{"feature":"memory-identity-v1","valid":true}`；检查失败以非零状态退出。

缺文件、缺表、未标记旧库、弱约束或同名视图占用均拒绝，不安装、不转换。
命令不加载模型、LanceDB、图或 Blob；依赖 OS 目录权限并要求独占路径锁。
不存在的 context.db 不会被新建。旧 `memory-identity-upgrade` 命令与其
`--dry-run` 参数已移除；只读检查没有升级状态或写入模式。
库调用方改用 `TursoStore::check_memory_identity(path)`，成功返回 `()`。

## 失败与恢复

| 现象 | 处理 |
| --- | --- |
| 锁被占用 | 使用 HTTP CLI/MCP；确需离线管理时先正常停止宿主 |
| 模型/解析失败 | 修正配置或输入；有效来源的 failed/cancelled 作业可显式 retry |
| 短暂存储错误 | 作业进入 retry_wait，最多 5 次；错误持久化，耗尽后 failed |
| Worker 或宿主退出 | 修复存储错误后重启；processing 在独占启动时恢复，无需等待租约超时 |
| 取消/删除发生在模型处理中 | 迟到提交复核 generation/权限/来源，不能覆盖或复活；外部孤儿由串行清理移除 |
| 原生写完成、Turso 发布未提交 | 保存的发布计划和账本用于重放；启动先清除无 owner 产物，再幂等写入并发布 |
| cleanup 失败 | 读取仍被墓碑阻断；修复存储后重试失败的 cleanup，或重启触发全范围对账 |
| schema 不兼容 | 停止并保留现场，不自动 ALTER；使用匹配版本或新目录，不覆盖旧数据 |

已受理任务的 embedding profile 与 Worker 配置必须一致；改模型不会自动重建已有向量。强退发生在发布计划保存前，模型调用可能重复。发布保证幂等，不保证外部供应商计费恰好一次。

## 图存储与来源边界

图存储使用 `graph.db`，沿用 `GraphStore`、确定性对象 ID 和 Turso owner/跨存储账本。所有连接启用外键，实体删除级联移除入边、出边和自环；有向遍历最多三跳、去重并排除起点。新文件自动创建完整结构，已有不兼容结构拒绝打开，不自动修复。

本版本不加载旧 graph/kuzu.db，也不迁移旧 SQLite 图文件。实体、关系与来源
归属由 Turso 保存，跨文件发布仍通过权威 owner/账本控制读取。
只支持新建或同版本完整目录；不要删除图文件或清空账本来尝试迁移或恢复。
原始来源与既有备份必须保留，不能把“新图文件可创建”理解为“旧图已恢复”。

## 备份与旧基线

1. 正常停止宿主，确认进程结束且无离线客户端持有数据目录。
2. 将**整个** `OC_DATA_DIR` 复制到备份位置，包含 Turso 及其 WAL、全文/向量、图、blobs 和关联文件；记录二进制版本和非敏感模型配置。
3. 恢复到独立目录，先用同版本离线工具检查，再启动并检查 ready、样本 get/search 和 job 状态。

不要在线分别复制数据库或只备份 context.db；当前没有在线跨库快照协议。历史专用测试库曾完成停机整库备份恢复，生产备份恢复及断电仍待独立验收。逻辑删除保留原文，备份也包含原文。

PG 基线代码和测试可在 Git 提交 `72fb5aa` 查阅。M5 移除了 `src/db.rs`、PG 初始化/授权 SQL、Apalis 依赖及 PG 专用测试；新本地进程套件替代其核心应用验收。没有 PG 数据自动迁移功能，不应将 PG 数据目录当作 `OC_DATA_DIR`。

## AML 适配启用与批次任务

仅使用独立新库。新 schema 不创建 oc_aml_namespaces、oc_aml_users、oc_aml_adds；发现旧 AML 表/视图或缺少新通用约束的库明确拒绝打开，不删除、自动升级、转换或静默补建。不得向业务库手工复制 DDL。正常同版本重启/停机备份恢复仍受支持；历史身份命令不再安装旧库缺表。

1. 用现有 init 创建或配置明确的 root workspace。`OC_ENABLE_AML=true` / `serve --enable-aml` 显式启用路由，默认关闭。Compose 显式透传该变量；`.env` 中未设置时仍为 false。
2. root admin 使用通用 `/admin/keys` 的 `allow_owned_workspaces:true` 或 `key-create <root UUID> --role writer --allow-owned-workspaces` 签发受信服务端 key；每次轮换必须再次显式授予。仅知道 user_id 或目标 UUID 不构成授权。
3. root key 按 reader/writer 权限访问该 root 全部所属用户 workspace，不向终端用户分发；接入服务负责终端用户身份和作用域选择。
4. 配置 embedding 后使用 Add；固定 raw-only 原文 vector，不启用摘要/抽取/图。每批来源、asset、原 job 及命令回执在一个事务内受理，消息解析/预分词与保存计划解码在 spawn_blocking、事务外运行，仍由既有单 Worker 串行 prepare/publish。

轮换先排空原 key 未完成任务再撤销。新 key 可命中相同回执，但 created_by 不变；紧急撤销接受旧任务被中断，新 key 不能接管，也不能为已失去权限的原 creator 显式 retry。已发布数据按当前读权限、有效来源、版本和索引判断，不因读取 key 更换而失效。

接受规格含 parser/version、chunking_version、raw_only 与 embedding profile，保存在原 job payload。同键换规格返回冲突；重启/恢复/retry 不采用新默认值覆盖，运行模型 profile 不匹配会失败。更换 root 或 request_id 不是规避内容冲突/权限限制的方法。

Add 等待上限仍为 25 分钟，上游模型单次调用仍为 45 秒；网关需允许长连接，或用相同 request_id/内容重放。队列等待也计入上限，输入上限不是吞吐/SLO 保证。超时/断线只结束观察，不取消 blocking 闭包或持久任务，无新增 Worker、自动重试、硬时间/内存隔离。优雅停止等待当前 Worker；已存在 Add 观察请求可能等待到 25 分钟结束。

模型失败仍为 failed；相同 Add 返回原失败任务并给出 503。合法库调用方用通用 receipt.job_id，在 lookup_owned_workspace 返回的 AuthContext 内调用 Service::job_action(...,"retry")；没有额外 AML job 管理 HTTP 入口，root 的 /v1/jobs 不跨 workspace 查询。临时存储故障沿用原 Worker retry_wait。显式 retry 的新 generation 可改变未发布 chunk ID，但保持 job/source/asset；同 generation 的恢复保持证据 ID。重放不保证上游计费一次。

审计/日志不输出消息、query、options、外部 user_id 或模型诊断。原文仍在来源/版本/发布计划内持久保存，墓碑不等于物理擦除。停机整库备份边界见[备份与旧基线](#备份与旧基线)；删除来源、工作目录或备份不证明介质物理擦除，副本及模型供应商的保留边界需单独管理。官方 Smoke/Full 与真实模型质量尚未验收。


## 检索分段诊断

设置 RUST_LOG=error,origence::search_timing=debug 可记录成功 Search 的 authorization_ms、embedding_ms、eligibility_ms、vector_ms、branches_ms、verification_ms、total_ms，以及 vector_batches/native_count/returned_count。日志不含 query、证据、用户/资产 ID、路径或凭据。默认 info 不输出这些 debug 事件。

branches_ms 包含候选装载、融合及已启用的辅助分支；verification_ms 是最终新权限快照及身份复核。total_ms 从输入校验后计到结果序列化前后的一部分，不包含外层 AML 用户作用域派生/授权查询、HTTP 编码或网络；不能当 HTTP 端到端延迟。失败和身份不存在的提前返回不生成该完成事件，故不能用这些事件统计错误率。完整 HTTP 耗时须另行测量。本次只增加计时，Worker、超时、取消、关闭、隔离和读取权限保证不变。

## 显式停机压实向量文件

频繁的小批次 Add 会形成许多 LanceDB 碎片。停机维护命令只整理现有向量表的物理布局，使用原生 Compact，不执行 All/Prune、不创建索引、不删除历史版本、不更新模型、关系记录或来源原文。它不作为 Search、Add 或 Worker 的自动步骤。

先按上文停止宿主并完成整库备份，再查看和执行：

~~~powershell
origence --data-dir <已有数据目录> --offline vector-compact --dry-run
origence --data-dir <已有数据目录> --offline vector-compact
~~~

命令不需要 API key，不加载模型；授权来自数据目录的 OS 访问权限，范围是该库的所有向量 profile。必须有兼容的 context.db 和已初始化的 vectors 目录。dry-run 仍获取独占锁，但不改变向量数据或版本；缺失目录/库不会被初始化。维护期间，已有进程和同进程新开的宿主都被拒绝；维护退出后释放锁。不要绕过宿主锁直接并发写 LanceDB。

所有表的命名与 schema 在首次压实前验证；输出按稳定排序的 table_index 给出版本、行数、碎片数和当前表字节数，不输出路径、凭据、源文本或底层错误详情。每表独立提交；中途失败时较早的表可能已经完成，可在检查磁盘和停机状态后显式重试。已是单碎片的表不重写。

每次只处理一个表，使用两个库级压实任务，目标每片 4096 行。Lance 保留索引覆盖边界，已索引与未索引数据不能合并，因此小表也可能保留多个片段；维护不隐式刷新索引。这个目标不是硬内存上限；没有硬超时或硬取消保证。维护需要额外磁盘空间并占用 IO/CPU；保留历史文件意味着它不是磁盘回收或物理擦除。强退/断电中的维护恢复尚未独立验收，不承诺任意断电安全。需要结束维护时应优先等待命令完成，再启动宿主。Worker、上传/解析的取消和关闭保证没有改变。

成功后正常启动宿主。检索仍在候选产生前按 scope/profile/generation/授权 ID 过滤，并执行关系库最终可见性复核；读一致性间隔保持为零。物理碎片重新增长后可能需要再次停机维护，未实现后台压实调度。历史维护对照记录见 [VALIDATION](VALIDATION.md#2026-10-11-完整公开库持续写入与再次压实)，不据此设置固定维护周期；多 profile 正确性与单 profile 真实模型性能分别验收。

更细的成功查询计时可设置 RUST_LOG=error,origence::search_timing=debug,origence::vector_timing=debug：vector_timing 提供 list_ms/open_ms/prepare_ms/execute_ms/collect_ms/decode_ms/total_ms 和 returned_count。execute 含规划/启动，collect 含流执行与读取，不应把它全部当作物理磁盘等待。日志不含查询、scope 或过滤表达式。


## S2 语义目录与发布边界

S2 只对新建数据库安装 oc_memory_properties；旧库开机不 ALTER，不接管旧 fact_key 或既有 identity。当前启动要求完整同版本 schema，缺表或不兼容库拒绝打开。离线 identity-check 只检查完整当前 schema，不安装 I1 或 S2；旧库迁移/导入暂未提供，勿手工补表后宣称已验收升级。

注册属性为管理员显式操作，定义不可变。必须在分配该属性的首个身份之前完成注册。当前阶段不支持修改已注册的属性定义，也不为此开发迁移方案；只有明确调整需求后才另行设计，不允许改表绕过约束。

语义写入复用现有单 durable Worker、发布计划、账本、权限重检、版本竞争、取消和失败重试；没有新增后台循环、子进程、硬期限或内存隔离保证。每次任务最多 32 个成员，每个 quote 最多 8192 字节，来源读取受既有文本上限约束；这些是应用输出限制，不是 parser 内存预算。graceful shutdown 仍完成当前任务。

semantic_publish 发布前后复核完整来源支持及逐字区间。多值沿用整快照可见性：任何支持来源撤回即隐藏整版，并禁止通用 restore 绕过语义；若当前发布来源仍有效，下一次显式 supplement 可重建仍有支持的集合；若当前发布来源本身撤回，后续写入须重新提供事实，不从历史自动补回。追加快照的原生索引账本仍归属本次发布事件，历史成员原始来源保存在不可变 locator；支持来源撤回先阻断读取，不承诺立即物理擦除所有后续快照副本。物理清理/备份保留的既有边界仍适用。

确定性工程回归不能证明调用方或模型提供的 operation/value 语义正确。自由文本抽取、自动别名归一化和通用语义理解仍需独立验收；不要把显式契约通过作为这些能力的证明。


## S3 对话语义捕获运行边界

新增 `semantic_extract` 作业，HTTP/Service 受理后由现有单一串行 Worker 执行。只支持新库，本轮不开发迁移或升级。复用现有 `OC_EXTRACTION_MODEL`、模型端点配置、45 秒单请求超时和有界响应读取；可不配 embedding，此时发布 keyword 能力。调用方明确提交 targets/expected_version，并按需要为单值目标启用 allow_update；没有自动循环、会话监听或新增 worker。

抽取调用在存储事务外；模型 JSON 解析/候选校验及分词通过 await spawn_blocking 完成。S2 路由在短事务中保存 `semantic_capture_plan`（原候选、固定身份/版本、路由、原始成员来源），之后再做 embedding/native 写入。模型输出不充当系统指令或发布授权。所有候选的关系发布原子提交，失败不发布有效子集。

计划已保存时，重试或重开复用它，不重新提取；显式 retry 的新 generation 只重建对应代次的发布产物。计划提交前崩溃或提取本身失败，重试可能再次调用模型，不承诺外部模型恰好一次。尚无计划时 Worker 的提取配置 fingerprint 必须匹配受理配置；已保存计划可继续使用其原 provenance，embedding profile 仍必须匹配。变更模型后不要把旧候选重新标成新模型结果。

恢复和最终提交重新校验权限、任务 generation/run_token、资产版本及所有原始支持来源。撤回本次捕获来源会取消相关作业；撤回其他集合成员来源会阻断待发布计划，已发布集合整体退出检索。已有计划可能保留在任务内部；逻辑撤回不等于磁盘、备份或模型供应商副本物理擦除。

取消和撤权阻止发布，不能保证已经发送的远程模型调用停止；spawn_blocking 不是硬取消。优雅停机继续等待当前作业，沿用原恢复/清理语义；本轮未增加进程隔离或硬截止时间。job 的空 memories 或 duplicate 结果是合法完成；superseded 需重读版本再提交新请求。工程验证使用确定性 loopback 模型桩，不等于真实模型分类或 Agent 效果验收。


## Turso / LanceDB 运行边界

本节描述已合入的 Turso/LanceDB 后端；实际验证状态以 VALIDATION 与精确 head CI 为准。

- 权威数据库使用原生 Rust Turso 0.8.2，不使用 SQLx SQLite 或 libSQL。
  数据目录仍包含 context.db、graph.db、vectors 与 blobs；原文 Blob 不改动。
- 新建库写入引擎/角色/schema 格式标记。既有非空文件只读检查后才可写开；
  不兼容或缺对象的库拒绝打开，不安装缺表、不迁移、不覆盖。旧名称类型别名
  仅保留调用兼容，不表示继续使用 SQLite 引擎。
- identity-check 执行离线只读同版本检查；有效库报告 valid=true，
  缺表/未标记库拒绝。不提供旧库安装、修复或迁移能力。
- 每个连接池最多五个租借；未提交或取消的连接归还前完成一次异步 rollback。
  清理保留连接额度与宿主 OS 锁，失败弃连接并使池不可用；shutdown 等清理结束。
  这不是硬时限：没有新增业务 worker/重试循环，也没有宣称强制终止文件 IO。
  原生 query（包括 RETURNING）先完整读取结果，再归还连接。
  SQL 错误或取消后的租借拒绝后续 SQL 与 commit；事务须 rollback 或 drop，
  其他租借须 drop 后清理，不允许其后写入意外变成自动提交。
- chunk full_text 与 vector 分别有发布账本；原生写全部成功后才在权威事务中
  发布版本、所有者、索引就绪及任务成功。失败后重用持久化计划与完整身份重放。
  撤回先改变权威可见性，再清理派生索引，残留原生行不能恢复已撤回来源。
- 当前有效发布需要的 FTS 表/索引或向量 profile 表/schema 缺失或不匹配时，
  启动、ready 与可检索回执明确失败。健康检查按当前未删除资产和有效根来源
  的精确已提交账本决定必需表；撤回/非当前版本的残留账本不单独阻止启动清理。
  此健康检查仅核根来源元数据和物理 schema，不替代语义支持闭包或逐行检索校验。
  检索前按固定原生 snapshot 校验全部候选的 id/source/version，缺失、重复、
  错配明确失败，不静默返回不完整结果。此检查读取身份元数据，增加查询开销；
  不能把 T0 的耗时比直接当成生产性能结果。没有自动重建或自动修复损坏数据。
- Search/Context 最终权威快照重新核对全部已参与原生排名的完整发布身份。
  排名期间撤回返回可重试的不可用错误；活跃对象的来源/版本/代次/索引状态
  错配返回冲突，不静默缩短 TopK 或沿用旧分数。没有无界自动重试，
  也没有承诺 Turso 与 LanceDB 之间存在一个共同事务快照。
- LanceDB 的 remote feature 仅因 0.40 发布包编译缺陷开启；连接只使用绝对本地
  文件路径，不配置远端地址、凭据或付费服务。
- ANN 使用每物理 segment 64 分区 IVF_FLAT/L2 与固定 32 probes；只读校验
  实际分区数，并拒绝会触发上游统计接口自动迁移的残缺/重叠覆盖元数据。满足冻结候选数量/过滤比例
  才选择；其他情况 exact。该阈值来自有界合成验证，不保证任意生产分布召回率
  或 SLO。同分截止集合可变，RRF 前仍按完整 scoped 身份验证、去重及排序。
- 停服备份应保留整个数据目录及 WAL/派生索引，不能只复制主 .db 文件。
  在线跨库备份与任意断电恢复不在当前已证明范围内。
