# 本地运维说明

适用单宿主 SQLite/LanceDB/SQLite 图。按操作阅读：[启动与凭据](#初始化与凭据) · [模型](#本地-embedding-服务) · [恢复](#失败与恢复) · [备份](#备份与旧基线) · [身份安装](#显式启用记忆身份) · [AML](#aml-适配启用与批次任务) · [向量压实](#显式停机压实向量文件)。当前进度及验证见 [STATUS](STATUS.md)、[VALIDATION](VALIDATION.md)。

## 当前阶段范围

当前开发与验收以新建数据库为目标，不建设或验收版本升级、旧库 schema 兼容及数据迁移能力；只有后续明确调整需求才重新考虑，见 [STATUS](STATUS.md#当前阶段范围)。本文保留已有身份安装命令和旧基线说明，供识别既有行为与安全边界，不代表当前升级/迁移路线图。

同版本运行中的持久化、重启/重试恢复和备份恢复要求不变。不兼容 schema 仍应拒绝打开；使用新目录不意味着可以删除、覆盖或静默转换旧数据，原始来源和备份继续按既有要求保留。

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

不要在线分别复制数据库或只备份 context.db；当前没有在线跨库快照协议。已完成本地专用测试库的停机整库备份恢复，生产备份恢复及断电仍待验收，范围见 [AML_DRILL](AML_DRILL.md)。逻辑删除保留原文，备份也包含原文。

PG 基线代码和测试可在 Git 提交 `72fb5aa` 查阅。M5 移除了 `src/db.rs`、PG 初始化/授权 SQL、Apalis 依赖及 PG 专用测试；新本地进程套件替代其核心应用验收。没有 PG 数据自动迁移功能，不应将 PG 数据目录当作 `OC_DATA_DIR`。

## AML 适配启用与批次任务

仅使用独立新库。新 schema 不创建 oc_aml_namespaces、oc_aml_users、oc_aml_adds；发现旧 AML 表/视图或缺少新通用约束的库明确拒绝打开，不删除、自动升级、转换或静默补建。不得向业务库手工复制 DDL。正常同版本重启/停机备份恢复仍受支持；本次不扩展既有显式身份安装机制。

1. 用现有 init 创建或配置明确的 root workspace。`OC_ENABLE_AML=true` / `serve --enable-aml` 显式启用路由，默认关闭。Compose 显式透传该变量；`.env` 中未设置时仍为 false。
2. root admin 使用通用 `/admin/keys` 的 `allow_owned_workspaces:true` 或 `key-create <root UUID> --role writer --allow-owned-workspaces` 签发受信服务端 key；每次轮换必须再次显式授予。仅知道 user_id 或目标 UUID 不构成授权。
3. root key 按 reader/writer 权限访问该 root 全部所属用户 workspace，不向终端用户分发；接入服务负责终端用户身份和作用域选择。
4. 配置 embedding 后使用 Add；固定 raw-only 原文 vector，不启用摘要/抽取/图。每批来源、asset、原 job 及命令回执在一个事务内受理，消息解析/预分词与保存计划解码在 spawn_blocking、事务外运行，仍由既有单 Worker 串行 prepare/publish。

轮换先排空原 key 未完成任务再撤销。新 key 可命中相同回执，但 created_by 不变；紧急撤销接受旧任务被中断，新 key 不能接管，也不能为已失去权限的原 creator 显式 retry。已发布数据按当前读权限、有效来源、版本和索引判断，不因读取 key 更换而失效。

接受规格含 parser/version、chunking_version、raw_only 与 embedding profile，保存在原 job payload。同键换规格返回冲突；重启/恢复/retry 不采用新默认值覆盖，运行模型 profile 不匹配会失败。更换 root 或 request_id 不是规避内容冲突/权限限制的方法。

Add 等待上限仍为 25 分钟，上游模型单次调用仍为 45 秒；网关需允许长连接，或用相同 request_id/内容重放。队列等待也计入上限，输入上限不是吞吐/SLO 保证。超时/断线只结束观察，不取消 blocking 闭包或持久任务，无新增 Worker、自动重试、硬时间/内存隔离。优雅停止等待当前 Worker；已存在 Add 观察请求可能等待到 25 分钟结束。

模型失败仍为 failed；相同 Add 返回原失败任务并给出 503。合法库调用方用通用 receipt.job_id，在 lookup_owned_workspace 返回的 AuthContext 内调用 Service::job_action(...,"retry")；没有额外 AML job 管理 HTTP 入口，root 的 /v1/jobs 不跨 workspace 查询。临时存储故障沿用原 Worker retry_wait。显式 retry 的新 generation 可改变未发布 chunk ID，但保持 job/source/asset；同 generation 的恢复保持证据 ID。重放不保证上游计费一次。

审计/日志不输出消息、query、options、外部 user_id 或模型诊断。原文仍在来源/版本/发布计划内持久保存，墓碑不等于物理擦除。演练、停机整库恢复与清理边界见 [AML_DRILL](AML_DRILL.md)，官方 Smoke/Full 与真实模型验收未由本次工程回归完成。


## 检索分段诊断

设置 RUST_LOG=error,origence::search_timing=debug 可记录成功 Search 的 authorization_ms、embedding_ms、eligibility_ms、vector_ms、branches_ms、verification_ms、total_ms，以及 vector_batches/native_count/returned_count。日志不含 query、证据、用户/资产 ID、路径或凭据。默认 info 不输出这些 debug 事件。

branches_ms 包含候选装载、融合及已启用的辅助分支；verification_ms 是最终新权限快照及身份复核。total_ms 从输入校验后计到结果序列化前后的一部分，不包含外层 AML 用户作用域派生/授权查询、HTTP 编码或网络；不能当 HTTP 端到端延迟。失败和身份不存在的提前返回不生成该完成事件，故不能用这些事件统计错误率。性能工具另测完整 HTTP 耗时。本次只增加计时，Worker、超时、取消、关闭、隔离和读取权限保证不变。

本地实验命令及结果见[时间与性能诊断](../evals/aml/temporal-v1/README.md)。

## 显式停机压实向量文件

频繁的小批次 Add 会形成许多 LanceDB 碎片。停机维护命令只整理现有向量表的物理布局，使用原生 Compact，不执行 All/Prune、不创建索引、不删除历史版本、不更新模型、关系记录或来源原文。它不作为 Search、Add 或 Worker 的自动步骤。

先按上文停止宿主并完成整库备份，再查看和执行：

~~~powershell
origence --data-dir <已有数据目录> --offline vector-compact --dry-run
origence --data-dir <已有数据目录> --offline vector-compact
~~~

命令不需要 API key，不加载模型；授权来自数据目录的 OS 访问权限，范围是该库的所有向量 profile。必须有兼容的 context.db 和已初始化的 vectors 目录。dry-run 仍获取独占锁，但不改变向量数据或版本；缺失目录/库不会被初始化。维护期间，已有进程和同进程新开的宿主都被拒绝；维护退出后释放锁。不要绕过宿主锁直接并发写 LanceDB。

所有表的命名与 schema 在首次压实前验证；输出按稳定排序的 table_index 给出版本、行数、碎片数和当前表字节数，不输出路径、凭据、源文本或底层错误详情。每表独立提交；中途失败时较早的表可能已经完成，可在检查磁盘和停机状态后显式重试。已是单碎片的表不重写。

每次只处理一个表，使用两个库级压实任务，目标每片 4096 行。这个目标不是硬内存上限；没有硬超时或硬取消保证。维护需要额外磁盘空间并占用 IO/CPU；保留历史文件意味着它不是磁盘回收或物理擦除。强退/断电中的维护恢复尚未独立验收，不承诺任意断电安全。需要结束维护时应优先等待命令完成，再启动宿主。Worker、上传/解析的取消和关闭保证没有改变。

成功后正常启动宿主。检索仍在候选产生前按 scope/profile/generation/授权 ID 过滤，并执行关系库最终可见性复核；读一致性间隔保持为零。物理碎片重新增长后可能需要再次停机维护，未实现后台压实调度。[完整库与追加写入对照](../evals/aml/maintenance-scale-v1/README.md)验证再次维护可恢复性能，但不据此设置固定维护周期；多 profile 正确性与单 profile 真实模型性能分别验收。实测、被拒绝的列投影方案及范围见[向量性能对照](../evals/aml/vector-perf-v1/README.md)。

更细的成功查询计时可设置 RUST_LOG=error,origence::search_timing=debug,origence::vector_timing=debug：vector_timing 提供 list_ms/open_ms/prepare_ms/execute_ms/collect_ms/decode_ms/total_ms 和 returned_count。execute 含规划/启动，collect 含流执行与读取，不应把它全部当作物理磁盘等待。日志不含查询、scope 或过滤表达式。


## S2 语义目录与发布边界

S2 只对新建数据库安装 oc_memory_properties；旧库开机不 ALTER，不接管旧 fact_key 或既有 identity。缺表旧库可继续使用原功能，S2 注册/写入不可用。现有身份安装命令只安装 I1，不能用于启用 S2；旧库迁移/导入暂未提供，勿手工补表后宣称已验收升级。

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


## Turso / LanceDB 接入分支的运行边界

本节描述 PR #58 的新后端；实际验证状态以 VALIDATION 与精确 head CI 为准。

- 权威数据库使用原生 Rust Turso 0.8.2，不使用 SQLx SQLite 或 libSQL。
  数据目录仍包含 context.db、graph.db、vectors 与 blobs；原文 Blob 不改动。
- 新建库写入引擎/角色/schema 格式标记。既有非空文件只读检查后才可写开；
  不兼容或缺对象的库拒绝打开，不安装缺表、不迁移、不覆盖。旧名称类型别名
  仅保留调用兼容，不表示继续使用 SQLite 引擎。
- memory-identity-upgrade 历史命令仅保留离线同版本检查；有效新库报告
  already_enabled，缺表/未标记库拒绝。它不再提供旧库安装或迁移能力。
- 每个连接池最多五个租借；未提交或取消的连接归还前完成一次异步 rollback。
  清理保留连接额度与宿主 OS 锁，失败弃连接并使池不可用；shutdown 等清理结束。
  这不是硬时限：没有新增业务 worker/重试循环，也没有宣称强制终止文件 IO。
  原生 query（包括 RETURNING）先完整读取结果，再归还连接。
- chunk full_text 与 vector 分别有发布账本；原生写全部成功后才在权威事务中
  发布版本、所有者、索引就绪及任务成功。失败后重用持久化计划与完整身份重放。
  撤回先改变权威可见性，再清理派生索引，残留原生行不能恢复已撤回来源。
- 有已提交全文账本时丢失 FTS 表/索引会使启动、ready 与可检索回执失败。
  检索前按固定原生 snapshot 校验全部候选的 id/source/version，缺失、重复、
  错配明确失败，不静默返回不完整结果。此检查读取身份元数据，增加查询开销；
  不能把 T0 的耗时比直接当成生产性能结果。没有自动重建或自动修复损坏数据。
- LanceDB 的 remote feature 仅因 0.40 发布包编译缺陷开启；连接只使用绝对本地
  文件路径，不配置远端地址、凭据或付费服务。
- ANN 使用 64 分区 IVF_FLAT/L2 与固定 32 probes，满足冻结候选数量/过滤比例
  才选择；其他情况 exact。该阈值来自有界合成验证，不保证任意生产分布召回率
  或 SLO。同分截止集合可变，RRF 前仍按完整 scoped 身份验证、去重及排序。
- 停服备份应保留整个数据目录及 WAL/派生索引，不能只复制主 .db 文件。
  在线跨库备份与任意断电恢复不在当前已证明范围内。
