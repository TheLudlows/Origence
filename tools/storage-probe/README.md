# 构建辅助：vendored protoc 定位

隔离的 Rust 工程，唯一的产物是 `probe-protoc`：一个打印 `protoc-bin-vendored` 自带 protoc 可执行路径的小工具。当环境既没有设置 `PROTOC`、也没有 `protoc` 可执行时，[tools/build.ps1](../build.ps1) 用它作为 protoc 兜底，供 lancedb 编译期定位 protoc，避免另行安装。

历史 M0 存储探针（`origence-storage-probe`、`verify.py`、`sqlite`/`vector` 后端）已删除；本地存储验收见 [VALIDATION](../../docs/VALIDATION.md)，日常构建走 [tools/build.ps1](../build.ps1)。

## 构建

需要 Rust 1.98.0（MSVC 目标）：

```powershell
cargo fetch --manifest-path tools/storage-probe/Cargo.toml --locked --target x86_64-pc-windows-msvc
cargo run --manifest-path tools/storage-probe/Cargo.toml --locked --no-default-features --features build-tools --bin probe-protoc
```

`build-tools` feature 拉入 `protoc-bin-vendored`，是该 bin 的唯一所需特性。
