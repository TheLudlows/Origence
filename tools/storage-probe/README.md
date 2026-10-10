# 本地存储 M0 验证

> 文档定位（2026-09-28）：M0 独立探针，保留 Rust 1.88 可行性证据；M5 应用已完成本地装配，日常构建使用 [tools/build.ps1](../build.ps1)，当前验收见 [VALIDATION](../../docs/VALIDATION.md)。

隔离的 Rust 工程，直接调用 `sqlx`、`lancedb`，不连接数据库服务、不进入应用运行路径。依赖固定在 Cargo.toml 和 Cargo.lock。图后端已替换为 SQLite，探针不再编译旧图引擎；图契约由 `tests/sqlite_graph_store.rs` 与本地应用/账本套件验证。LanceDB 关闭默认云端特性，使用本地文件接口。

## Windows 构建

需要 Rust 和 MSVC C++ Build Tools，在 MSVC 开发环境运行。Lance 所需的 protoc 由可选构建辅助 crate 提供，不要求另行安装，也不是运行时服务。

在仓库根目录执行：

```powershell
cargo fetch --manifest-path tools/storage-probe/Cargo.toml --locked --target x86_64-pc-windows-msvc
./tools/storage-probe/build.ps1
python tools/storage-probe/verify.py
```

最低 Rust 版本为 1.98，使用 `1.98.1` 工具链验证（原 1.88 测试记录保留为历史证据）：

```powershell
./tools/storage-probe/build.ps1 -Toolchain 1.98.1 -TargetDirectory target/storage-probe-msrv -Jobs 8
python tools/storage-probe/verify.py --binary target/storage-probe-msrv/debug/origence-storage-probe.exe
```

若 Windows 默认执行策略禁止 `.ps1`，可仅对本次构建进程使用 `powershell -NoProfile -ExecutionPolicy Bypass -File tools/storage-probe/build.ps1`，在后面追加上面的工具链参数；不需要修改系统执行策略。

只检查某个后端可加 `--backend sqlite`、`--backend lancedb`。检查结果和临时数据库保留在 `target/storage-probe/runs/run-*/`，不访问应用数据库。

## 检查范围

- 重复初始化、读写、关闭后重新打开、按 tenant/workspace 删除，以及同 ID 跨 scope（含引号字符串）。
- LanceDB 按 scope 预过滤的精确向量 top-1 查询，长连接读取其他进程提交的数据，两个写进程的并发追加。
- SQLite WAL 下读写并发、写锁竞争、杀死未提交事务后的恢复。
- 两种后端在已确认写入后被强制杀死，重新打开验证数据。

实际通过项、构建失败和未验证项以 [验证记录](../../docs/VALIDATION.md) 与运行生成的 report.json 为准。脚本失败即返回非零状态；不会把预期支持当作已验证。

## 范围限制

探针只有最小表结构；重复初始化不等于生产结构完整性检查。LanceDB 的 `put` 是追加，用唯一测试 ID 验证并发，尚未实现业务幂等 upsert。向量测试未创建 ANN 索引；SQLite 的 `search` 只是 scoped 查询，未验证 FTS。没有实现生产 scope 授权、来源有效性检查、跨库账本、队列或 Worker OS 文件锁。这些属于 M1–M4，不能用本探针替代验收。

Rust 1.98、其他 OS 和 release 构建须单独记录实测结果；Cargo 的版本解析成功不能代替最低 Rust 版本编译。
