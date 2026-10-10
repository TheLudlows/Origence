# Replace Kuzu Graph Backend with SQLite

更新：2026-10-10。本文以最终实现替代实施前的代码草稿；精确验证证据见 [VALIDATION](../../VALIDATION.md)。

## 目标与边界

按现有图谱设计，用 SQLx/SQLite 的 `SqliteGraphStore` 替换原生图引擎。保持 `GraphStore`、`StorageEngine`、领域调用、确定性对象 ID、账本可见性和 hybrid snapshot JSON 契约。关系库继续持有单宿主 OS 锁，API/Worker 共享引擎；图后端不另设文件锁。

SQLite 依赖原已存在。本次不增加原生图依赖；SQLite/LanceDB 构建仍需要 C/C++ 编译器与 protoc。Rust MSRV 为 1.98，开发工具链 1.98.1。

新路径为 `OC_DATA_DIR/graph.db`，旧 `graph/kuzu.db` 不再被加载。图数据为可重新发布的派生投影，无旧图文件迁移；已有关系 owner/已提交账本不会自动补回空图。切换须按 [OPERATIONS](../../OPERATIONS.md) 备份并在新目录重新导入/发布来源。

## 已实施

- [x] 新增 `src/storage/sqlite-graph-schema.sql`：scope 实体、关系、来源标签、确定性关系唯一键，双端点外键 `ON DELETE CASCADE`，scope/source/head/tail/id 索引。
- [x] 新增 `src/storage/sqlite_graph.rs`：六个 GraphStore 方法、snapshot、Lifecycle；逐连接 foreign_keys/busy_timeout，WAL、五连接池。
- [x] LocalEngine 与 Service 改为 SQLite 图，账本集成测试接入同一后端。
- [x] 删除 `src/storage/kuzu.rs`、`tests/kuzu_store.rs`、旧探针 `src/graph.rs`、Kuzu/CXX 直接依赖及 `local-graph` feature。
- [x] Cargo 自动更新应用/探针锁文件，移除 Kuzu/CMake/CXX 依赖链；保留包的版本与 checksum 未变。
- [x] CI、Docker、两个 Windows 构建脚本移除 Kuzu 专用 CMake/Ninja 配置；保留 MSVC、protoc 和 LanceDB 工具链。
- [x] 更新 README/API/运维/状态/路线图/平台设计；历史方案与原始 CI 证据保留并标明已被替换。

最终 feature：

```toml
[features]
default = ["local-storage"]
local-vector = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema"]
local-storage = ["local-vector"]
```

SQLite 图模块不依赖 local-vector/local-storage，可在 no-default-features 的轻量 CI 验证图契约。

## 行为契约与草稿修正

| 操作 | 最终行为 |
| --- | --- |
| ID/作用域 | 使用 `crate::graph::entity_id/relation_id`；实体 uid 为 tenant/workspace/ID 组合，不改变名称归一化算法 |
| 实体 upsert | last-writer-wins，包括来源、版本及实体投影 |
| 关系 upsert | first-writer-wins；唯一键保证并发重放幂等；缺少同 scope 任一端点时 no-op，不创建悬空关系 |
| traverse | 有向递归 CTE、scope 过滤、最大三跳、去重、排除起点；`UNION` 限制重汇合路径重复展开 |
| delete_objects | 仅删除所给 scope 对象；实体删除原子级联入边、出边、自环 |
| delete_source | 匹配 scope/source/version 的实体级联删除，再删除其余对应来源关系；共享 owner 最终权威仍是关系账本 |
| snapshot/list_ids | 以短读事务获取一致快照；snapshot 形状不变，名称/predicate trim+lowercase，按 ID 排序 |
| initialize/check | 空库在短 IMMEDIATE 事务内创建完整结构；检查已知 DDL、键、级联和索引；残缺结构、同名 view、缺失约束均拒绝，失败不修复 |
| 写入/恢复 | 对象写入独立提交；不持跨库长事务。启动 cleanup、未发布孤儿回收和保存计划重放仍使用现有账本流程 |

实施前示例只做列投影检查，且会插入缺少端点的关系、带环遍历会返回起点。最终实现按计划契约补齐这些边界，并用外键级联替代多条手工 detach SQL。旧引擎特有的取消后阻塞许可测试随阻塞执行器删除。离线身份升级测试改为确认 `graph.db` 不被创建，避免继续用旧目录作为检查目标。

## 验证清单

- [x] `tests/sqlite_graph_store.rs` 经 `tests/local.rs` 编入现有集成可执行程序：原五项契约加八项边界回归，无新增原生 Job。
- [x] 两个锁文件不存在 Kuzu/CMake/CXX 依赖；活动 Rust、测试、探针、构建配置不存在旧符号。
- [x] format、默认/no-default Clippy `-D warnings`。
- [x] 默认完整套件及 no-default lib/local；重点核验 vector/hybrid、共享来源删除、保存图发布恢复与孤儿清理。
- [x] build、HTTP smoke、固定 keyword 种子评估；keyword 得分不代替真实 hybrid 质量。
- [x] `probe-protoc` 以 `--locked --no-default-features --features build-tools` 构建。
- [x] PR 实现提交 `d3222e5` 的 fast-check/Linux/MSRV CI：run `38030946507` 全部实际执行项成功；Windows/macOS/Docker 按现有策略待 main 验收。

锁文件仅通过 `cargo update --workspace` 更新，未手工修改。其余构建、检查、测试均使用 `--locked`。
