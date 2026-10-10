# M4 本地向量与图适配器

> 2026-10-10：本文保留历史实施与验收记录。当前图后端已替换为 SQLite；旧 Kuzu/原生构建描述不再适用于当前代码。见 [SQLite 图替换计划](2026-10-10-replace-kuzu-with-sqlite-graph.md) 与 [当前运维说明](../../OPERATIONS.md)。


修订：2026-09-28。适配器已交付，检索融合、可见性、共享 owner 清理和宿主装配在 [M5](2026-09-28-local-host-delivery.md) 接入。本文保留接口契约与构建决策，移除过时整文件代码草稿；源码及 [VALIDATION](../../VALIDATION.md) 是实际依据。

## 依赖与运行边界

| 组件 | 固定条件 |
| --- | --- |
| LanceDB | `=0.23.1`，关闭默认云端特性；Arrow 56.2 系列 |
| Kuzu | `=0.11.3`，保留默认扩展特性 |
| CXX 生成器 | `cxx-build =1.0.138`，与 Kuzu 所用 cxx 一致，避免 MSVC 链接符号不匹配 |
| Cargo feature | 默认 `local-storage`；细分 `local-vector` 和 `local-graph` 用于库边界验证 |
| 进程 | 单宿主拥有 Kuzu Database，API/Worker 共享；CLI/MCP 通过宿主访问 |

Windows 构建用 [tools/build.ps1](../../../tools/build.ps1)。需要 MSVC C++、CMake、Ninja、protoc；限制并行链接以控制内存。独立 M0 探针的最低 Rust 版本结果不等于主应用最低版本验证。

## LanceDB 契约

`src/storage/lancedb.rs` 实现 VectorStore 和 Lifecycle：

- 所有写、查、删显式接收 scope；profile 对应独立表，查询要求 dimension/generation。
- 校验非零且可表示的维度、向量长度、有限数值、现有表维度；批次先完整校验再写，schema 不兼容明确失败。
- merge 身份包含 scope、artifact、source/version、generation；相同写重放不增加重复对象。
- 精确查询显式绕过 ANN。应用先从 SQLite 获取当前可见候选 ID，在原生 top-k 前过滤，随后再次核验来源和索引状态。
- `delete_source` 提供来源级原语；M5 清理使用 `delete_objects` 精确产物 ID，避免误删仍有效的共享来源产物。

能力声明为精确查询，不宣称已交付 ANN、自动 profile 重建或 reranker。

## Kuzu 契约

`src/storage/kuzu.rs` 实现 GraphStore 和 Lifecycle：

- 节点/关系含 tenant/workspace 与来源投影，实体按规范化名称、关系按端点和谓词确定身份。
- 同步原生调用置于有界 `spawn_blocking`；串行许可由阻塞闭包持有，异步调用者取消不能提前释放许可。
- 实体 upsert、关系确定性身份查写及删除可重放；重新打开后数据持久化。
- `delete_source` 只依据最后写入的来源投影，不提供共享 owner 语义。M5 使用 SQLite owner 集合与 `list_ids/delete_objects` 差集清理。
- hybrid 返回规范化名称/关系及有效原文证据，不信任共享实体的最后写入描述。底层邻域最多三跳，当前应用仅一跳。

## 验收映射

| 模块 | 主要行为 |
| --- | --- |
| `tests/lancedb_store.rs` | 重放、重开、删除、维度/数值拒绝、scope/profile/generation/source-version 隔离、候选 top-k 过滤 |
| `tests/kuzu_store.rs` | 真实实体/关系、邻域、scope 隔离、重复写/删除与重开 |
| Kuzu 单元测试 | 取消后的阻塞操作仍持有串行许可 |
| `tests/local_ledger.rs` | SQLite pending → 原生幂等写 → committed 组合 |
| `tests/local_app.rs` | HTTP/CLI/MCP、图摘要证据、共享 owner、迟到写隔离和恢复 |

存储原语测试不能替代应用可见性验收；当前两层通过同一 `tests/local.rs` 运行。平台范围、未验证项和命令结果统一记录在 [VALIDATION](../../VALIDATION.md)。
