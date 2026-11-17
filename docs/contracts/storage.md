# 存储不变量

本文维护当前代码必须保留的数据语义。组件分工见[实现架构](../design/current-implementation.md)，
初始化、故障处理和备份步骤见 [OPERATIONS](../OPERATIONS.md)。
完整字段与约束以 [authority schema](../../src/storage/sqlite/schema.rs) 及其引入的 DDL 为准，
不另行维护逐字段副本或物理表数量。

## SQLite 与最小元数据

权威数据和任务队列统一使用 bundled SQLite 3.54.1（rusqlite 0.43.0，关闭默认
features，仅启用 bundled），不引入 ORM 或独立异步数据库依赖。
`SqliteStore` / `SqliteTx` 是唯一的本地关系实现；引擎标记为 `origence-sqlite-v1`，
没有旧引擎的适配器、别名或回退路径。

只持久化无法安全重建的业务事实、授权依据和恢复状态：

- 版本正文的 `content_hash` 在读取时按原始 UTF-8 字节计算，不再另存一列或由写入方指定。
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

## 身份占位

- `oc_assets.identity_key` 与 `identity_json` 成对为空，或在 memory 资产上同时存在。
  `oc_memory_identity_slot` 按完整 scope 保持唯一，删除后的占位仍保留。
- `fact_key` 是独立的事实槽位字段。正常结构化写入会写入同一规范身份键，
  但不能因此合并两列或将旧 fact_key 解释为完整身份。
- 身份绑定随资产保持稳定，历史版本恢复不改变绑定；删除不释放身份供重新占用。
- 选中的结构化身份必须通过类型、业务字段、规范 key 与正文一致性校验；损坏不自动修复。
  身份不匹配先于墓碑处理，不能把不一致资产当成普通删除冲突。
  结构化身份损坏返回 Unavailable，原生列类型错误保留 Backend；缺少身份仍表示未绑定。
- 精确身份槽读取依赖 schema 保证的 `oc_memory_identity_slot` 索引；不在索引缺失时静默回退。
  批量读取在同一事务内关联，避免以逐行查询代替原有批量路径。

## 独立生命周期

| 对象 | 必须保留的意义 |
| --- | --- |
| 原始事件 / Blob | 不可变输入、文件字节与独立的来源撤回 |
| 资产 / 版本 | 稳定资产身份、当前版本指针与追加式历史正文 |
| 命令 / 作业 | 调用幂等与异步执行、取消、重试及 creator 重新授权 |
| artifact owner | 精确来源/版本对产物的归属；共享对象可有多个 owner |
| index entry | 产物、field、profile、generation 的索引就绪及维度信息 |
| publication ledger | 来源、版本、surface、generation 的外部发布状态与恢复身份 |
| 私有会话 / Run / Skill | 逻辑 key 归属、不可变事件/修订、审阅授权与发布版本 |

`index entry` 和 `publication ledger` 的键、状态与生命周期不同，不能互相替代。
ready + committed 仍不足以证明可见：读取还要检查权限、当前资产版本、来源、owner
以及语义快照或 Learning 支持。作业 generation、业务版本、模型 profile 和执行
run_token 各自独立，不得相互充当身份。

发布前保存完整计划与 pending 账本，事务外写入投影，再在权威事务中复核并提交。
已准备的 MessageBatch 计划与轻量回执摘要一起保存；同代恢复复用并核对摘要，
缺失或损坏明确失败。摘要不替代实际索引及账本检查，也不会完整重扫向量计划。
账本不承担作业队列的尝试数与错误文本职责；重试上限和执行 fencing 仍由作业管理。

## 来源与有效性

历史 locator、parser 标记、页码、UTF-8 字节范围、模型 provenance 与原始支持来源保留。
资产版本、原始来源版本和快照根来源分别表达，不能按当前解析器重新解释历史偏移。
撤回先在权威数据中阻断读取，再清理投影；逻辑删除不等于物理擦除。

所有关联查询遵守 [AGENTS](../../AGENTS.md#sql-query-constraints) 的单表读取和同事务
Rust 关联约束。模型、解析、文件和远端索引 IO 在权威事务外执行；返回及提交前复核授权。

## 初始化与恢复

只创建当前完整 schema；格式标记不能替代完整 DDL 校验。不兼容结构拒绝打开，
不自动补表、迁移、改写或删除既有数据。更换模型/索引目标不重解释已持久化计划。
同版本重开、原计划重试与停机整目录备份恢复保留；AnySeek collection 另行备份，
本地目录和远端索引没有共同事务或在线一致快照。

原始字段盘点、合表评审和测量留在
[固定历史版本](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/contracts)，
不作为当前 schema 或性能结论。
