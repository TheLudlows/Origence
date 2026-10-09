# 本地存储与可扩展接口实施计划

修订：2026-10-09。唯一设计来源是 [整体设计](../specs/2026-09-22-memory-knowledge-platform-design.md)，存储契约集中在 [A2](../specs/2026-09-22-memory-knowledge-platform-design.md#storage-design)，数据归属见 A5。M0–M5 的本地接口与宿主基础已交付；应用层完整厂商解耦仍欠账，当前代码的原生/平台验收见 [VALIDATION](../../VALIDATION.md) 最新快照，不能将历史勾项解释为任意后端可替换或全部平台已通过。

## 目标与范围

交付 SQLite（关系与队列）+ LanceDB（向量）+ Kuzu（图）+ 本地文件的闭环；各存储面通过领域接口访问，保留未来扩展 PostgreSQL/pgvector 的能力。本地交付不依赖 PG 服务、不提供 PG 配置开关、不实现历史库升级或数据搬运。

M0–M5 已接入本地默认运行栈，当前代码与验收映射见 [M5 本地交付记录](2026-09-28-local-host-delivery.md)。Windows 基线推进完成，跨平台/release 等未验证项仍保留；自动发布作为独立产品阶段已于 2026-09-29 完成。旧 PG 基线从活动树裁剪，提交 `72fb5aa` 保留历史参考，不要求先开发 PG 适配器。

现有自动发布、图谱计划与本计划共享里程碑，不各自建立数据库连接或另一套存储接口。本地基础与独立自动发布阶段均已完成；P1/P2 不阻塞本地存储交付。

## 阶段依赖与交付物

| 阶段 | 前置条件 | 交付物 | 设计依据 |
| --- | --- | --- | --- |
| M0 本地可行性 | 无 | 构建/访问验证记录，确定进程边界 | A2.3、A2.4、A2.8 |
| M1 接口与事务 | M0 | scope、领域事务、队列和各 Store 的接口及契约测试 | A2.2、A5 |
| M2 SQLite 核心 | M1 | 自动初始化、业务读写、队列、生命周期测试 | A2.3–A2.5 |
| M3 账本与对账 | M2 | 幂等外部写、取消隔离、清理重放 | A2.6 |
| M4 向量和图 | M1、M3 | LanceDB/Kuzu 实现及图谱/检索集成 | A2.4、A2.6 |
| M5 本地发布验收 | M2–M4 | 本地启动、CLI/HTTP/MCP 闭环与发布文档 | A2.7、A2.8 |

## M0：验证构建与访问方式

已交付：`tools/storage-probe/` 的独立 Cargo.toml/Cargo.lock、直接调用 Rust 库的程序、Windows 构建脚本与子进程测试，结果见 [VALIDATION](../../VALIDATION.md)。这是独立历史可行性探针；主应用随后在 M5 切换。

- [x] SQLite/LanceDB/Kuzu 在 Windows x64/MSVC、Rust 1.88.0 的完整构建、初始化、重复打开、读写和删除通过。
- [x] 真实临时目录验证 scope、同 ID 跨 scope、读写进程竞争及强退。SQLite/LanceDB 可双进程访问；Kuzu 写宿主排斥第二个进程打开（包括只读）。这是存储访问探针，尚非完整 API/Worker 集成测试。
- [x] 验证 Kuzu 单宿主命令访问、共享实例多线程连接、事务前后快照及强退回滚；整体设计第 3 节和 A2.4 已改为本地同进程 API/Worker。
- [x] 记录候选依赖、构建修正、命令、14 项实际检查与限制；`cxx-build` 固定为 1.0.138，Kuzu 保留默认特性，LanceDB 关闭默认云端特性。
- [ ] 扩展至 Linux/macOS/release 的构建与运行验证；对应平台通过前不能宣称支持该平台，Windows 基线可继续 M1。

完成标准：三种后端均有可复现的读写结果与进程访问方案；未解决项不能以“预期支持”标为通过。

## M1：接口与事务契约

拟新增：src/storage/mod.rs、scope.rs、traits.rs、tests/storage_contract.rs。接入点：src/lib.rs、service.rs、worker.rs、retrieval.rs、graph.rs。文件名是实施建议，接口责任以 A2.2 为准。

- [x] 定义 Scope、授权上下文、领域事务、RelationalStore、JobQueue、VectorStore、GraphStore、BlobStore 和 StorageEngine。
- [x] 明确同宿主引擎实例的所有权、阻塞调用边界及关闭顺序；业务模块共享适配器，不各自打开 Kuzu 数据库文件。
- [x] Store/DomainTx 定义 begin/commit/rollback、幂等、来源/版本、enqueue 和 audit 的领域签名；事务对象携带 scope，接口不暴露裸 SQL。
- [ ] 应用层完全使用领域事务而非厂商事务。当前 Service 的 read/write/command 仍返回 SqliteTx，retrieval 使用其扩展方法，完整后端解耦尚未完成；不要求为本地版本先实现第二后端。
- [x] enqueue 是同一关系事务上的领域操作；JobQueue 消费者负责 claim/ack/retry，禁止另开连接造成业务成功而入队失败。
- [x] 图/向量操作显式接收 scope、来源版本；向量查询额外指定 profile、dimension、generation。Blob key 受 scope 和根目录约束。
- [x] 用调用示例覆盖写入提交、任一步失败共同回滚、外部模型 IO 在事务外执行、提交时权限复核。
- [x] 定义可复用契约测试与能力声明，未来 PG 可实现同一接口；不先包装 PG，也不添加返回成功的空 PG 实现。

完成标准：业务层与厂商类型分离，事务/入队边界明确，身份和来源有效性不能被能力降级绕过。

## M2：SQLite 关系库、初始化和队列

拟新增：src/storage/sqlite.rs、src/storage/sqlite-schema.sql、src/storage/local_blob.rs。修改：service.rs、worker.rs、main.rs、api.rs、tests/initialization.rs、tests/local_app.rs（统一经 tests/local.rs 编译）。

- [x] 按 A5 创建完整新库结构；保留当前候选/审核语义，自动发布另由对应计划调整，不与存储切换混在一起。
- [x] 逐连接设置 foreign_keys/busy_timeout，启用 WAL，初始化串行化；已有不兼容结构拒绝，不创建升级历史或 ALTER 链。
- [x] SQL 全部留在适配器，业务读写显式绑定 tenant/workspace；token 查找、平台管理和全局 claim 使用独立特权接口。
- [x] 定义 UUID、JSON、时间、布尔和复合 PK/FK 编码；通过真实 SQLite 读取验证，不只替换 SQL 占位符。
- [x] 关键词写入 Jieba 预分词 `search_terms`，查询精确匹配词项并限定当前 scope/版本；中文召回随 M5 验收。
- [x] 业务、幂等、审计和作业登记同事务；写事务以 IMMEDIATE 开始，模型/文件/原生 IO 在事务外，撤销与短写提交串行化。
- [x] OS 文件锁限制单 Worker；实现 processing 回收、有限次 retry_wait、next_retry_at、取消、generation/run_token 与提交复核。
- [x] 本地文件适配器检查 scope、路径、来源状态，保持逻辑删除与原文保留策略。
- [x] 用临时目录跑初始化、跨 tenant/workspace、并发提交、强退恢复、撤销 key 和重复执行测试。

完成标准：现有核心生命周期在 SQLite 上成立，业务与入队共同回滚；本阶段不宣布完整图/向量功能可用。

## M3：跨存储账本与对账

拟新增：src/storage/ledger.rs、tests/storage_recovery.rs；修改 SQLite 初始化定义和 Worker 编排。

- [x] 把 owners、index_entries、artifact_ledger 的权威记录放在 SQLite，主键包含 scope、来源版本、产物、存储面和 generation。
- [x] 外部写之前登记 pending，使用确定性 ID 幂等写，复核来源/权限/取消/generation 后确认 committed 并发布。M5 Worker 在保存计划和最终发布时复核取消/权限/来源/版本。
- [x] 实现 retry_wait/orphan、attempt、last_error、next_retry_at 和重启对账；不能只把失败吞掉。M5 启动先恢复作业、对账并执行清理。
- [x] 删除先提交墓碑，检索立即排除；后台按依赖顺序清理摘要/chunk、owner/关系/实体和向量。M5 按 owner 规划向量精确 ID 删除和图孤儿 diff。
- [x] 注入外部写成功后崩溃、关系提交失败、迟到结果、取消与共享来源删除，验证重放不重复发布且可收敛。M5 覆盖真实强退、持久化外部写边界重放及迟到/取消/共享来源场景。

完成标准：跨库失败可观测且可恢复，检索可见性不依赖清理及时完成。

## M4：LanceDB、Kuzu 与检索

拟新增：src/storage/lancedb.rs、src/storage/kuzu.rs；修改 retrieval.rs、graph.rs、worker.rs、tests/storage_contract.rs。图业务细节见 [图谱计划](2026-09-22-knowledge-graph-core.md)，不重复实现同一适配器。

- [x] 按 M0 验证方案实现两个后端的自动初始化与进程访问，不重新假设双进程安全。Kuzu 由一个实例持有，经串行阻塞执行器访问；宿主在 M5 同进程装配。
- [x] LanceDB 写入/检索按 scope、profile、dimension、generation 过滤，声明真实的 ANN/精确查询能力。显式绕过向量索引；校验声明维度、向量长度和 profile 表维度；幂等键保留来源版本与 generation。
- [x] Kuzu 保存带 scope 的实体/关系和必要来源投影；SQLite owner/版本状态作为最终可见性权威。投影删除不提供共享 owner 语义。
- [x] M5 接入 M3 账本、持久化发布计划、共享 owner 清理及启动重放；保留原语测试并增加真实应用验收。
- [x] keyword/vector/summary 分支映射回原文证据并融合；图扩展返回可追溯证据，resolve 统一计算预算。
- [x] 每条图/向量命中返回前复核有效来源、发布版本和资产墓碑；验证撤回后立即不可检索。

完成标准：真实后端跑通导入→加工→检索→撤回→清理，跨 scope 无泄漏且共享证据不误删。

适配器交付记录见 [M4 存储切片](2026-09-27-local-vector-graph.md)。该切片是底层历史记录；G3/G4 的检索及清理集成已与 M5 宿主装配共同接入，验收以 M5 记录为准。

## M5：装配与本地交付

修改：main.rs、api.rs、mcp.rs、Cargo.toml、Dockerfile、compose.yaml、.github/workflows/ci.yml、README.md、docs/STATUS.md、docs/VALIDATION.md。

- [x] 固定装配 SQLite/LanceDB/Kuzu/本地文件，提供 OC_DATA_DIR；全部初始化和检查通过后才 ready 或领取任务。
- [x] 本地宿主同进程启动 API 与单 Worker，共享 Kuzu 实例；CLI/MCP 对运行中数据集经宿主访问，离线库模式独占打开。覆盖第二宿主拒绝、请求转发鉴权、关闭与重启恢复。
- [x] 本地构建和运行不依赖 PG 服务、DATABASE_URL、PG 角色、pgvector 扩展或 Apalis PG 队列；解除其对本地发布产物的强制依赖。
- [x] PG 基线代码的归档/裁剪在接入时明确记录，不为保留旧代码增加本地依赖，不新增可选 PG 交付承诺。
- [x] CLI、HTTP、MCP 和 CI 使用本地后端跑初始化、权限、生命周期、恢复、删除、预算测试；普通测试可用临时目录直接运行。
- [x] Windows 历史 M5/A1 基线完成 fmt、clippy 与本地集成测试并记录限制；证据只适用于当时 commit/平台。
- [ ] 当前 main 默认后端完整 Windows/Linux/macOS、release/container 验收收口。轻量 27 项 lib/58 项无原生集成不替代当前原生用例；以 VALIDATION 当前 job 结果判断。

完成标准：干净环境从首次启动到检索和重启恢复可复现；未运行的检查明确标为未验证。

## 与其他计划的关系

- 图谱计划细化 M4 的抽取、摘要、证据和删除验收；M1–M3 不以现有 PG 图实现作为完成依据。
- 自动发布计划已在 M5 本地基线成立后完成，修改领域治理与新库初始化定义，不重建存储层。
- P1 会话记忆与 P2 服务化能力按整体设计推进；未来 PG 扩展不属于本地里程碑。
- 旧 PG 初始化改动未验证部分保留在历史记录；不属于当前本地交付，也不新增 PG 适配器前置阶段。

