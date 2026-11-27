# 存储不变量

**简体中文** · [English](storage.en.md)

本文维护当前代码必须保留的数据语义。组件分工见[实现架构](../design/current-implementation.md)，
初始化、故障处理和备份步骤见 [OPERATIONS](../OPERATIONS.md)。
完整字段与约束以 [authority schema](../../src/storage/sqlite/schema.rs) 及其引入的 DDL 为准，
不另行维护逐字段副本或物理表数量。

## AnySeek 连接边界

本地模式在宿主进程内通过 Rust SDK 打开 `OC_DATA_DIR/anyseek`。云上模式配置远端 endpoint 和服务间凭据。两者都由已鉴权 `Scope` 自动路由到规范 UUID 字符串的 tenant_id 和 `ws_<workspace_id.simple()>` 集合，无默认命名空间、集合配置或跨 workspace 回退。远端数据请求用 `X-AnySeek-Tenant = tenant_id` 和 `apikey`。启动时打开本地 Catalog 或检查远端连通性及凭据，并检查已有发布的 workspace 集合。新 workspace 首次使用时创建固定 schema 集合，创建结果未知时严格回读。已发布集合缺失、不兼容或分析摘要变化必须失败，不能以空集合继续服务。候选仍须通过 Origence 的权威范围、来源和版本复核。`origence-content-backend-v2` 后端绑定固定目标、路由、模型空间和首次使用时的分析摘要，不含服务凭据。已有发布但没有已初始化摘要时拒绝打开。AnySeek 不保存或签发租户 Key。

## SQLite 与最小元数据

权威数据和任务队列统一使用 bundled SQLite 3.54.1（rusqlite 0.43.0，关闭默认
features，仅启用 bundled），不引入 ORM 或独立异步数据库依赖。
`SqliteStore` / `SqliteTx` 是唯一的本地关系实现；引擎标记为 `origence-sqlite-v2`，
没有旧引擎的适配器、别名或回退路径。

SQLite 是可选库能力。`--no-default-features` 的核心接口、解析、模型、Blob 和远端 AnySeek 适配器不编译或访问 SQLite。`--features sqlite` 启用权威适配器，`local-storage` 组合仅连接远端 AnySeek 的完整宿主及 SQLite，默认 `embedded-anyseek` 增加本地 SDK。完整宿主仍用 SQLite 保存原始来源事件、权限、归属、版本指针、幂等、持久任务及恢复事实。AnySeek 保存已发布正文和摘要文本。

请求只读取实际参与操作的元数据：空批次和空检索词直接返回空结果；缺少上游候选时，
不查询下游索引、Summary、来源或 owner。Learning 不存在、前置条件不成立或父任务
已取消时也不继续查询无关记录。非空证据与必需的读写仍严格校验。
完整 DDL 检查在开库及显式健康检查执行，业务空操作不再兼任全库 schema 探测。

服务事务在同一快照中完成一次入口身份/权限查询并登记提交要求，提交前重新检查权限；
不缓存跨请求的权限。关键词请求复用入口事务读取候选；向量/混合模式在模型 IO 前
结束入口事务，保留后续新快照及返回前的发布/撤回/权限复核。

只持久化无法安全重建的业务事实、授权依据和恢复状态：

- 版本及分块的 `content_hash` 由适配器按原始 UTF-8 字节计算并保存，绑定远端不可变正文；
  宿主读取时重新计算比对。版本正文按 ordinal 拼接分块，不另存整篇正文副本。
  HTTP/Skill 证据中的哈希字段保持不变。
- 发布账本使用现有完整主键保证登记幂等，不再保存由这些列拼接的 `idempotency_key`，
  也不要求 Worker 携带该重复字符串。外部发布仍使用冻结计划和原 operation epoch。
- 命令请求摘要、私有 Run/Skill 的快照摘要、原始文件哈希、解析/模型 provenance、
  冻结检索词项、owner/index/ledger 仍各有校验或恢复职责，不能当作镜像删除。

每个可写连接显式使用 WAL、`synchronous=FULL`、外键和 5 秒 busy timeout。
每池最多五个独占租约，SQLite 的同步 IO 在有序 blocking 连接线程执行；取消 await
不硬停 SQL，后续操作拒绝使用该租约，排队回滚在原操作完成后执行，再归还连接。
关闭等待租约、回滚和 SQLite 句柄释放后才释放宿主锁；不另开持久作业 Worker。
已经发出的 COMMIT 可能完成而调用方没有收到确认；这种未知结果仍通过原命令幂等键
和持久状态核对，取消 await 不能撤销已经提交的数据。

停机且没有非空 WAL 时，只读准入使用 immutable 连接，不创建 sidecar；存在提交 WAL
时必须读取 WAL，不使用 immutable。SHM 的 read mark 是 SQLite 的临时协调状态，
不是备份中的业务事实；只读访问也可能更新它，数据库与已提交 WAL 不因此被改写。

## 正文与索引共用 collection

`origence-content-sha256-v1` 每个 Record 对应一个 chunk，`payload` 为存储但不索引的
JSON（原始 UTF-8 正文、完整 locator、可选摘要及模型版本）；同一记录上的 `terms`、
vector 与 graph 维持既有召回方式。没有独立的整篇正文/版本/摘要 Record，不改变 BM25 语料条数。
SQLite 不再保存 `oc_versions.content`、`oc_chunks.content`、`oc_summaries.text`、
`oc_chunks.search_terms` 或 `oc_summaries.model_revision`。分块词项只供 AnySeek 使用，
摘要模型版本随 payload 保存；SQL 仅保留仍参与 Summary 召回的词项。

SQL 保留来源与可见性判断所需的 locator、Summary 冻结词项、Run/Skill 凭据与恢复计划。
AnySeek 私有集合保存完整问答、轮次目录和检索字段。SQLite 保存工作区与逻辑 key 归属、
会话序号分配、请求幂等记录和实际使用的依赖证明。AnySeek 按组删除正文与索引并保留版本防止迟到写入。
数据目录绑定与 collection schema 同时换代。
[私有问答配套恢复](../OPERATIONS.md#私有问答归档与配套恢复)

读取在事务外解析 AnySeek payload，并比对 chunk 哈希和完整 locator；同一 chunk 的残留
代次只允许重复相同 payload，不从冲突内容中挑选一个。完整版本还比对拼接后的版本哈希。
GET、Search、Context 在远端 IO 后使用新事务复核权限及来源/发布身份；
非空 Search 增加一次正文读取后的最终快照，空结果不增加这次读。历史恢复读取原
版本的不可变 chunk，再经过原发布 guard。缺失、截断或不匹配明确失败，不回退到来源重新解析。
语义记忆的内部合并从原始来源的历史字节范围恢复 quote 并核验其哈希，不需要事务内远端 IO。

## 身份占位

- `oc_assets.identity_key` 与 `identity_json` 成对为空，或在 记忆条目上同时存在。
  `oc_memory_identity_slot` 按完整 scope 保持唯一，删除后的占位仍保留。
- `fact_key` 是独立的事实槽位字段。正常结构化写入会写入同一规范身份键，
  但不能因此合并两列或将旧 fact_key 解释为完整身份。
- 身份绑定随内容条目保持稳定，历史版本恢复不改变绑定；删除不释放身份供重新占用。
- 选中的结构化身份必须通过类型、业务字段、规范 key 与正文一致性校验；损坏不自动修复。
  身份不匹配先于墓碑处理，不能把不一致内容条目当成普通删除冲突。
  结构化身份损坏返回 Unavailable，原生列类型错误保留 Backend；缺少身份仍表示未绑定。
- 精确身份槽读取依赖 schema 保证的 `oc_memory_identity_slot` 索引；不在索引缺失时静默回退。
  批量读取在同一事务内关联，避免以逐行查询代替原有批量路径。

## 独立生命周期

| 对象 | 必须保留的意义 |
| --- | --- |
| 原始事件 / Blob | 不可变输入、文件字节与独立的来源撤回 |
| 内容条目 / 版本 | 稳定内容条目身份、当前版本指针与追加式历史正文 |
| 命令 / 作业 | 调用幂等与异步执行、取消、重试及 creator 重新授权 |
| artifact owner | 精确来源/版本对产物的归属；共享对象可有多个 owner |
| index entry | 产物、field、profile、generation 的索引就绪及维度信息 |
| publication ledger | 来源、版本、surface、generation 的外部发布状态与恢复身份 |
| 私有会话 / Run / Skill | 逻辑 key 归属、不可变事件与修订、指定审阅凭据及发布版本 |

`index entry` 和 `publication ledger` 的键、状态与生命周期不同，不能互相替代。
ready + committed 仍不足以证明可见：读取还要检查权限、当前内容条目版本、来源、owner
以及语义快照或 Learning 支持。作业 generation、业务版本、模型 profile 和执行
run_token 各自独立，不得相互充当身份。

所有发布路径在事务外的 blocking 任务中一次生成正文/摘要词项，并随计划冻结；
重试必须使用原词项，缺失字段按计划损坏处理，不重新分词补齐。
发布前保存完整计划与 pending 账本，事务外将正文、摘要、定位信息与索引写入同一 AnySeek Record，再在权威事务中复核并提交。
已准备的 MessageBatch 计划与轻量回执摘要一起保存；同代恢复复用并核对摘要，
缺失或损坏明确失败。摘要不替代实际索引及账本检查，也不会完整重扫向量计划。
账本不承担作业队列的尝试数与错误文本职责；重试上限和执行 fencing 仍由作业管理。

## 来源与有效性

历史 locator、parser 标记、页码、UTF-8 字节范围、模型 provenance 与原始支持来源保留。
内容条目版本、原始来源版本和快照根来源分别表达，不能按当前解析器重新解释历史偏移。
撤回先在权威数据中阻断读取，再清理投影；逻辑删除不等于物理擦除。

所有关联查询遵守 [AGENTS](../../AGENTS.md#sql-query-constraints) 的单表读取和同事务
Rust 关联约束。模型、解析、文件和远端索引 IO 在权威事务外执行；返回及提交前复核授权。

## 初始化与恢复

只创建当前完整 schema，格式标记不能替代完整 DDL 校验。不兼容结构拒绝打开，不自动补表、迁移、改写或删除已有数据。更换模型或索引目标不重解释持久化计划。保留同版本重开、原计划重试及停机备份恢复。嵌入式 Catalog 随整个 Origence 目录备份。独立服务的 AnySeek collection 另行备份，Origence 目录和远端索引没有共同事务或在线一致快照。

原始字段盘点、合表评审和测量留在
[固定历史版本](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/contracts)，
不作为当前 schema 或性能结论。
