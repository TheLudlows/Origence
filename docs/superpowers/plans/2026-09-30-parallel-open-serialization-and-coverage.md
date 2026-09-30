# 并行初始化串行化与验证覆盖补强实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复测试套件默认并行执行下 `initialization::sqlite_concurrent_initialization_is_serialized` 稳定失败（SQLite `code: 5 database is locked`，2026-09-30 当日全量 4/4 复现、隔离通过）的同进程并发打开竞态；同时消掉 auto-publish 周期遗留的代码级 Minor（`claim.source.unwrap()` panic 路径、去重静默丢弃）并补齐 2026-09-29 验收记录中声明的未验证断言。

**Architecture:** `SqliteStore::open` 的进程内锁（`PROCESS_LOCKS`）目前只做引用计数，同进程第二个 `open` 与第一个在连接期 `PRAGMA journal_mode=WAL` 与 `BEGIN IMMEDIATE` 初始化上直接竞态。修复为每路径增加一个跨 `connect + initialize` 全程持有的 `tokio::sync::Mutex`（跨进程语义不变，仍由 OS 文件锁拒绝）。Worker 侧新增 `source_for` 帮助函数统一 5 处 `SourceVersion` 构造（unwrap 改为 `NotFound` 错误），去重丢弃处补 `tracing::warn!`。测试补 4 组断言/用例（accept 字段、extract 结果字段、已删除路由 404、确定性竞态 superseded），套件 67 → 68。

**Tech Stack:** Rust（edition 2024）、sqlx/SQLite、tokio（`full` 特性已含 `sync`）；测试经 `tests/local.rs` 统一编译。

## Global Constraints

- 不引入新依赖；`tokio::sync::Mutex` 由现有 `tokio = { features = ["full"] }` 提供（Cargo.toml:29）。
- 不改 schema、不迁移数据；`REQUIRED_PROJECTIONS` 与 sqlite-schema.sql 不动。
- 跨进程语义不变：第二个进程打开同一路径仍被 OS 文件锁拒绝（`tests/sqlite_lock.rs` 既有用例必须保持通过）。
- 每个 Task 结束时 `cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -j 1 -- -D warnings`、`cargo test --locked -j 1 --no-fail-fast` 必须通过（Windows x64/MSVC、Rust 1.96.0；链接内存压力用 `-j 1`）。
- cargo 的 `-j 1` 只限制编译并行；test harness 默认仍全核并行跑测试——本计划的并行判定命令以 `--test local`（默认 test 线程）为准。
- 提交信息用 conventional commits（参照 git log 风格），末尾加 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- 文档（STATUS/VALIDATION/台账）统一在 Task 4 更新，前三个 Task 不改文档。
- VALIDATION 诚实规则：只记录实测断言的内容；数字取自实际运行输出。
- 若采用子代理派发：本地 API 代理拒绝 Agent 工具的模型别名，派发必须省略 model 参数（沿用 auto-publish 台账 ENV NOTE）。

## 执行前置

- 从 main（`2f2f29f`）创建分支 `fix/parallel-open-serialization-and-coverage`。
- 提交本计划文档：`docs: plan parallel open serialization and coverage hardening`。
- 红灯证据已在案（2026-09-30 控制器会话）：`cargo test --locked -j 1 --test local` 默认并行 4/4 失败于 `initialization::sqlite_concurrent_initialization_is_serialized`，错误 `backend error: error returned from database: (code: 5) database is locked`；`-- --exact` 隔离运行 0.05s 通过；`-- --test-threads=1` 全量 59/59 通过。

---

### Task 1: 同进程并发 SqliteStore::open 串行化

`tests/initialization.rs:27` 的测试用 `tokio::join!` 在同进程并发打开同一路径两次。`acquire_lock`（`src/storage/sqlite.rs:187`）对同进程只做引用计数，两个 `open` 随后在 `SqlitePoolOptions::connect_with`（连接期执行 `PRAGMA journal_mode=WAL`）与 `initialize()`（`BEGIN IMMEDIATE`）上竞态；全套件并行时其他测试抢占 CPU/IO，竞态窗口命中即 `SQLITE_BUSY`。修复：每路径一个跨 `connect + initialize` 全程持有的异步互斥，同进程 `open` 全程串行；跨进程仍由 OS 文件锁拒绝。

**Files:**
- Modify: `src/storage/sqlite.rs:18`（导入 `Arc`）
- Modify: `src/storage/sqlite.rs:128-131`（`open` 持有串行锁）
- Modify: `src/storage/sqlite.rs:170-202`（`HeldLock` 增字段、`acquire_lock` 返回串行锁）
- Test: 既有 `tests/initialization.rs:27`（`sqlite_concurrent_initialization_is_serialized`，不改）

**Interfaces:**
- Consumes: 现有 `PROCESS_LOCKS`/`HeldLock`/`LockGuard`（A2.5）；`SqliteStore::open` 对外签名不变。
- Produces: `fn acquire_lock(path: &Path) -> StorageResult<(LockGuard, Arc<tokio::sync::Mutex<()>>)>`（本文件私有，仅 `open` 一个调用点）；`HeldLock` 新增字段 `opening: Arc<tokio::sync::Mutex<()>>`。

- [x] **Step 1: 复现确认（RED）**

Run: `cargo test --locked -j 1 --test local 2>&1 | grep -E "test result|FAILED"`

Expected: `test initialization::sqlite_concurrent_initialization_is_serialized ... FAILED`、`test result: FAILED. 58 passed; 1 failed`。若本次意外全绿，最多重跑 3 次；仍绿则记录在案继续（修复依据为当日 4/4 失败记录与设计分析），但 Step 4 的 5 连绿判定不变。

- [x] **Step 2: 实现串行化（4 处编辑）**

编辑 1 —— `src/storage/sqlite.rs:18` 导入 `Arc`：

```rust
use std::sync::{LazyLock, Mutex};
```

替换为：

```rust
use std::sync::{Arc, LazyLock, Mutex};
```

编辑 2 —— `open()` 内 `acquire_lock` 调用处（约 128-131 行）：

```rust
        // Take the single-writer lock before touching the database: a second
        // process that opens the same path fails fast here instead of racing
        // the running worker (A2.5). Reentrant within this process.
        let lock = acquire_lock(&path)?;
```

替换为：

```rust
        // Take the single-writer lock before touching the database: a second
        // process that opens the same path fails fast here instead of racing
        // the running worker (A2.5). Reentrant within this process.
        let (lock, opening) = acquire_lock(&path)?;
        // Same-process opens serialize end to end: the OS lock only rejects
        // other processes, so a reopen would otherwise race the first open's
        // WAL transition and schema transaction (SQLITE_BUSY under load).
        let _serialized = opening.lock().await;
```

编辑 3 —— `HeldLock` 结构体（约 170-179 行，含文档注释）：

```rust
/// One in-process holder of the OS lock and its open-store count. The OS lock
/// lives on the single `file` handle; in-process reopens bump `refs` without
/// re-taking the OS lock, so the API and Worker (one process) share one holder
/// while a second process is still rejected (A2.5).
struct HeldLock {
    /// Owned solely to hold the OS lock until the last in-process store drops.
    #[allow(dead_code)]
    file: File,
    refs: usize,
}
```

替换为：

```rust
/// One in-process holder of the OS lock and its open-store count. The OS lock
/// lives on the single `file` handle; in-process reopens bump `refs` without
/// re-taking the OS lock, so the API and Worker (one process) share one holder
/// while a second process is still rejected (A2.5). `opening` serializes
/// same-process opens end to end so a reopen cannot race the first open's
/// connect and initialize (SQLITE_BUSY under load).
struct HeldLock {
    /// Owned solely to hold the OS lock until the last in-process store drops.
    #[allow(dead_code)]
    file: File,
    refs: usize,
    /// Held across connect + initialize for every in-process open of this path.
    opening: Arc<tokio::sync::Mutex<()>>,
}
```

编辑 4 —— `acquire_lock`（约 185-202 行，含文档注释）：

```rust
/// Take the single-writer lock for `path`, returning a guard that releases it
/// on drop. Reentrant within this process (A2.5).
fn acquire_lock(path: &Path) -> StorageResult<LockGuard> {
    let key = lock_path(path);
    let mut locks = PROCESS_LOCKS
        .lock()
        .expect("process lock registry poisoned");
    if let Some(held) = locks.get_mut(&key) {
        held.refs += 1;
        return Ok(LockGuard(key));
    }
    let file = File::create(&key)?;
    file.try_lock_exclusive().map_err(|e| {
        StorageError::Conflict(format!("store lock is held by another process: {e}"))
    })?;
    locks.insert(key.clone(), HeldLock { file, refs: 1 });
    Ok(LockGuard(key))
}
```

替换为：

```rust
/// Take the single-writer lock for `path`, returning a guard that releases it
/// on drop plus the per-path mutex that serializes in-process opens. Reentrant
/// within this process (A2.5).
fn acquire_lock(path: &Path) -> StorageResult<(LockGuard, Arc<tokio::sync::Mutex<()>>)> {
    let key = lock_path(path);
    let mut locks = PROCESS_LOCKS
        .lock()
        .expect("process lock registry poisoned");
    if let Some(held) = locks.get_mut(&key) {
        held.refs += 1;
        return Ok((LockGuard(key), held.opening.clone()));
    }
    let file = File::create(&key)?;
    file.try_lock_exclusive().map_err(|e| {
        StorageError::Conflict(format!("store lock is held by another process: {e}"))
    })?;
    let opening = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(
        key.clone(),
        HeldLock {
            file,
            refs: 1,
            opening: opening.clone(),
        },
    );
    Ok((LockGuard(key), opening))
}
```

死锁与引用计数推演（实现者自查）：等待方在 `acquire_lock` 内先递增 `refs` 再等待 `opening`，故持有条目在等待期间不会被 `release_lock` 移除；`_serialized` 守卫在 `open` 返回时释放（store 仅持有 `LockGuard`），使用期并发不受影响；`open` 中途失败时两个守卫都随作用域释放，无残留状态。

- [x] **Step 3: 隔离运行该测试**

Run: `cargo test --locked -j 1 --test local initialization::sqlite_concurrent_initialization_is_serialized -- --exact`

Expected: `test result: ok. 1 passed`（注意 `--exact` 必须带完整测试名，见 auto-publish 台账的 flake 命令勘误）。

- [x] **Step 4: 默认并行全量回归 5 连跑**

Run（连续 5 次）: `cargo test --locked -j 1 --test local 2>&1 | grep -E "test result|FAILED"`

Expected: 每次均 `test result: ok. 59 passed; 0 failed`。任何一次失败即修复不完整，停下重新分析（不得继续后续 Task）。

- [x] **Step 5: 三重门槛**

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
```

Expected: 全部通过；总数 67（8 单元 + 59 集成）。

- [x] **Step 6: 提交**

```bash
git add src/storage/sqlite.rs
git commit -m "fix(storage): serialize same-process concurrent store opens

In-process reopens only bumped the lock refcount, so a concurrent open
raced the first open's WAL transition and schema transaction and hit
SQLITE_BUSY under parallel test load. A per-path async mutex now spans
connect + initialize; cross-process opens are still OS-lock rejected.

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Worker source_for 统一来源构造与去重观测

`ledger()`（`src/worker.rs:325`）是不可失败函数，但内部两处 `claim.source.unwrap()`（350、369 行）是 panic 路径，与 `publish_prepared` 三处 `ok_or(AppError::NotFound)`（418、437、461 行）不对称；`claim.source` 类型为 `Option<Uuid>`（`src/storage/traits.rs:371`）。extract 去重丢弃（176-178 行）无任何日志观测。本任务：新增 `source_for` 帮助函数统一 5 处构造、`ledger()` 改为返回 `Result`、去重处补 `tracing::warn!`。行为不变（所有可发布任务的 `claim.source` 均为 `Some`；防御路径由 panic 变为 job 落 `failed`）。

**Files:**
- Modify: `src/worker.rs:171-178`（去重 tracing）
- Modify: `src/worker.rs:325-398`（`source_for` + `ledger` 可失败化）
- Modify: `src/worker.rs:405, 417-420, 436-439, 460-463`（调用点）
- Test: 既有全套件回归（行为不变，无新测试；tracing 与防御路径不引入可断言的对外行为）

**Interfaces:**
- Consumes: `ClaimedJob.source: Option<Uuid>`、`SourceVersion { source_id, version }`、`AppError::NotFound`、`Result`（均在 worker.rs 现有作用域）。
- Produces: `fn source_for(claim: &ClaimedJob, version: i32) -> Result<SourceVersion>`（worker.rs 私有）；`fn ledger(claim: &ClaimedJob, publication: &Publication) -> Result<Vec<LedgerEntry>>`。

- [x] **Step 1: 去重处补 tracing（176-178 行）**

```rust
            if !seen.insert(m.fact_key.clone()) {
                continue;
            }
```

替换为：

```rust
            if !seen.insert(m.fact_key.clone()) {
                tracing::warn!(
                    job_id=%claim.job_id,
                    fact_key=%m.fact_key,
                    "duplicate extracted fact_key dropped"
                );
                continue;
            }
```

- [x] **Step 2: 新增 source_for 并改 ledger 签名**

在 `prepare` 结束（324 行 `}`）与 `fn ledger`（325 行）之间插入：

```rust
fn source_for(claim: &ClaimedJob, version: i32) -> Result<SourceVersion> {
    Ok(SourceVersion {
        source_id: claim.source.ok_or(AppError::NotFound)?,
        version,
    })
}
```

`ledger` 签名行（325 行）：

```rust
fn ledger(claim: &ClaimedJob, publication: &Publication) -> Vec<LedgerEntry> {
```

替换为：

```rust
fn ledger(claim: &ClaimedJob, publication: &Publication) -> Result<Vec<LedgerEntry>> {
```

`ledger` 体内第一处构造（348-352 行，4 空格缩进）：

```rust
    for memory in &publication.memories {
        let source = SourceVersion {
            source_id: claim.source.unwrap(),
            version: memory.version,
        };
```

替换为：

```rust
    for memory in &publication.memories {
        let source = source_for(claim, memory.version)?;
```

`ledger` 体内第二处构造（368-371 行）：

```rust
        let source = SourceVersion {
            source_id: claim.source.unwrap(),
            version: first.version,
        };
```

替换为：

```rust
        let source = source_for(claim, first.version)?;
```

`ledger` 返回（397 行）：

```rust
    items
}
```

替换为：

```rust
    Ok(items)
}
```

- [x] **Step 3: publish_prepared 调用点**

405 行：

```rust
    let entries = ledger(claim, &publication);
```

替换为：

```rust
    let entries = ledger(claim, &publication)?;
```

417-420 行（向量循环，4 空格缩进）：

```rust
    for memory in &publication.memories {
        let source = SourceVersion {
            source_id: claim.source.ok_or(AppError::NotFound)?,
            version: memory.version,
        };
```

替换为：

```rust
    for memory in &publication.memories {
        let source = source_for(claim, memory.version)?;
```

436-439 行（图写入）：

```rust
        let source = SourceVersion {
            source_id: claim.source.ok_or(AppError::NotFound)?,
            version: first.version,
        };
```

替换为：

```rust
        let source = source_for(claim, first.version)?;
```

460-463 行（二次复核事务内，8 空格缩进）：

```rust
        let source = SourceVersion {
            source_id: claim.source.ok_or(AppError::NotFound)?,
            version: memory.version,
        };
```

替换为：

```rust
        let source = source_for(claim, memory.version)?;
```

完成后全文件 grep 确认：`claim.source.unwrap()` 0 处；`source_for(claim` 5 处（ledger 2 + publish_prepared 3）；`SourceVersion {` 字面构造仅剩 `source_for` 内 1 处。

- [x] **Step 4: 三重门槛**

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
```

Expected: 全部通过，67/67（8 单元 + 59 集成）。

- [x] **Step 5: 提交**

```bash
git add src/worker.rs
git commit -m "fix(worker): fallible ledger source construction and dedup tracing

- source_for helper replaces five inline SourceVersion constructions;
  the two unwrap() panic paths in ledger() now surface NotFound
- duplicate extracted fact keys log a warning instead of vanishing

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: 验证覆盖补强（4 组断言/用例）

补齐 2026-09-29 验收「未验证」清单：竞态 superseded、已删除路由 404；以及台账 Minor 中的覆盖缺口：accept 期 `conflict:false` 与 `source_event_id`、extract 结果 `readiness`/`index_capabilities`。全部为对既有行为的回归钉（预期立即通过）；竞态 superseded 用确定性手法构造（两次 accept 抢同一 expected_version，逐个 process_next）。

**Files:**
- Modify: `tests/local_app.rs:730-731`（lifecycle 测试 accept 字段断言）
- Modify: `tests/local_app.rs:286-287`（host 测试 extract 结果字段断言）
- Modify: `tests/local_app.rs:316-317` 之间（host 测试 404 断言块）
- Modify: `tests/local_app.rs:803-805` 之间（新增竞态 superseded 测试）
- Test: 即上述文件本身；套件 59 → 60 集成用例（总数 67 → 68）

**Interfaces:**
- Consumes: 既有辅助 `id(v,k)`（local_app.rs:18）、`job()` HTTP 轮询（:83）、service 方法 `s.memory/s.job/s.get`、`opencontext::worker::process_next`、accept 响应形状 `{"asset_id","source_event_id","job_id","state":"accepted","conflict"}`（service.rs:175）、job 状态映射 `AppError::Conflict → "superseded"`（worker.rs:113）。
- Produces: 新测试 `async fn racing_expected_versions_supersede_the_loser()`；无新辅助函数。

- [x] **Step 1: lifecycle 测试 accept 字段断言**

`tests/local_app.rs:730-731` 现状：

```rust
    let first = s.memory(&a, "once", input.clone()).await.unwrap();
    assert_eq!(first, s.memory(&a, "once", input.clone()).await.unwrap());
```

在其后追加两行（置于 `let mut different` 之前）：

```rust
    assert_eq!(first["conflict"], false);
    assert!(first["source_event_id"].is_string());
```

- [x] **Step 2: host 测试 extract 结果字段断言**

`tests/local_app.rs:286-287` 现状：

```rust
    let extracted = job(&http, &base, token, id(&capture, "job_id"), "completed").await;
    assert_eq!(extracted["outcome"], "published");
```

在其后追加（host 已启用 embedding mock，能力为 keyword+vector；capture 无摘要/图谱）：

```rust
    assert_eq!(extracted["result"]["readiness"], "ready");
    assert_eq!(
        extracted["result"]["index_capabilities"],
        json!(["keyword", "vector"])
    );
```

- [x] **Step 3: 已删除路由 404 断言**

`tests/local_app.rs:316`（`assert_eq!(none["result"]["memories"], json!([]));`）与 317 行注释（`// Two sources own the same graph objects...`）之间插入：

```rust
    // The removed candidate-review surface no longer resolves.
    for (method, path) in [
        (reqwest::Method::GET, "/v1/candidates"),
        (
            reqwest::Method::GET,
            "/v1/candidates/00000000-0000-0000-0000-000000000000",
        ),
        (
            reqwest::Method::POST,
            "/v1/candidates/00000000-0000-0000-0000-000000000000/review",
        ),
    ] {
        let r = http
            .request(method, format!("{base}{path}"))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), reqwest::StatusCode::NOT_FOUND);
    }
```

（不使用 `request` 辅助——它断言成功状态；此处直接用 `http` 客户端断言 404。）

- [x] **Step 4: 新增确定性竞态 superseded 测试**

在 `local_transactions_versions_retraction_and_idempotency` 结束（803 行 `}`）与 `#[tokio::test]`（805 行）之间插入：

```rust
#[tokio::test]
async fn racing_expected_versions_supersede_the_loser() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(dir.path(), Models::disabled()).await.unwrap();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    let store = s.engine.relational();
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "local")
        .await
        .unwrap();
    let key = store.issue_key(scope, "writer").await.unwrap();
    let a = s.auth(&key.token).await.unwrap();
    let input = MemoryInput {
        fact_key: "racer".into(),
        content: "Race base".into(),
        publish_if_authorized: true,
    };
    let base = s.memory(&a, "base", input.clone()).await.unwrap();
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&base, "asset_id"), None).await.unwrap()["version"],
        1
    );
    // Both updates accept against the same current version; the first commit
    // passes the recheck, the loser settles superseded without a new version.
    let left = s
        .memory(
            &a,
            "left",
            MemoryInput {
                content: "Race left".into(),
                ..input.clone()
            },
        )
        .await
        .unwrap();
    let right = s
        .memory(
            &a,
            "right",
            MemoryInput {
                content: "Race right".into(),
                ..input.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(left["conflict"], true);
    assert_eq!(right["conflict"], true);
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    let left_state = s.job(&a, id(&left, "job_id")).await.unwrap()["state"].clone();
    let right_state = s
        .job(&a, id(&right, "job_id"))
        .await
        .unwrap()["state"]
        .clone();
    // Queue order between same-instant jobs is not guaranteed; assert the
    // outcome pair, not which one won.
    let states = [left_state.as_str(), right_state.as_str()];
    assert!(states.contains(&"completed"));
    assert!(states.contains(&"superseded"));
    assert_eq!(
        s.get(&a, id(&base, "asset_id"), None).await.unwrap()["version"],
        2
    );
    s.engine.shutdown().await.unwrap();
}
```

- [x] **Step 5: 运行新断言**

Run: `cargo test --locked -j 1 --test local racing_expected_versions_supersede_the_loser -- --exact`

Expected: `test result: ok. 1 passed`。若失败，先看失败输出定位（superseded 映射在 worker.rs:113，复核冲突在 worker.rs:455-458），不得为绿改弱断言。

Run: `cargo test --locked -j 1 --test local local_app -- --nocapture 2>&1 | grep -E "test result|FAILED"`

Expected: 4 个 local_app 测试全过（含新用例）。

- [x] **Step 6: 三重门槛**

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
```

Expected: 全部通过；**68 通过**（8 单元 + 60 集成）。

- [x] **Step 7: 提交**

```bash
git add tests/local_app.rs
git commit -m "test(app): pin accept fields, extract readiness, removed routes, race supersession

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 文档与台账收尾

记录 2026-09-30 验收（新 VALIDATION 节）、修订 2026-09-29 节的未验证清单（两项已补齐）、STATUS 更新戳、关闭 auto-publish 台账（Task 5 勾选 + 合并后复核记录 + 本轮修复波记录）、归档 M3 时代遗留的未跟踪台账文件。

**Files:**
- Modify: `docs/VALIDATION.md:1-3`（新节插入在 `# 验证记录` 之后、`## 2026-09-29` 之前）
- Modify: `docs/VALIDATION.md:22`（2026-09-29 节未验证行修订）
- Modify: `docs/STATUS.md:3`（更新戳与本轮一句）
- Modify: `.superpowers/sdd/progress.md:13`（Task 5 勾选）与文末（追加记录；锚定完整最后一行）
- Move: `.superpowers/sdd/task-6-brief.md`、`task-6-report.md`、`final-fix-report.md` → `.superpowers/sdd/archive/`（M3 时代未跟踪文件，非 git 操作）

**Interfaces:**
- Consumes: Task 1-3 的实测结果（并行 5 连绿、套件 68、各断言）、本会话 2026-09-30 的 4/4 失败记录。
- Produces: VALIDATION 新节 `## 2026-09-30：并行初始化串行化与覆盖补强`。

- [x] **Step 1: VALIDATION 新节**

在 `# 验证记录`（1 行）与 `## 2026-09-29：自动发布验收`（3 行）之间插入（`__秒__` 处填 Task 3 Step 6 输出中的实测集成耗时，保留"非性能基准"措辞）：

```markdown
## 2026-09-30：并行初始化串行化与覆盖补强

环境：Windows x64/MSVC、Rust/Cargo 1.96.0，锁定仓库依赖。本轮修复测试套件默认并行执行下 `initialization::sqlite_concurrent_initialization_is_serialized` 稳定失败（SQLite `code: 5 database is locked`，当日 4/4 复现、隔离执行通过）的问题：同进程并发 `SqliteStore::open` 现按路径经异步互斥串行完成连接与初始化（跨进程仍由 OS 文件锁拒绝）；同时补齐 2026-09-29 声明的未验证断言与遗留代码 Minor。实施映射见 [并行初始化串行化计划](superpowers/plans/2026-09-30-parallel-open-serialization-and-coverage.md)。

| 检查 | 实测结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --locked --all-targets -j 1 -- -D warnings` | 通过 |
| `cargo test --locked -j 1 --no-fail-fast` | **68 通过，0 失败，0 ignored**；8 项单元 + 60 项集成（新增竞态 superseded 用例 1 项），集成执行约 __秒__，非性能基准 |
| 并行回归 | 修复后 `cargo test --locked -j 1 --test local`（默认并行 test 线程）连续 5 次全绿；修复前同命令当日 4/4 失败于并发初始化用例 |
| `cargo check --locked --no-default-features --lib -j 4` | 通过 |

本轮新增/调整的验收覆盖：

- 同进程并发打开同一数据库：两个并发 `SqliteStore::open` 串行完成连接与初始化，默认并行套件下不再出现 `database is locked`；跨进程打开仍被 OS 锁拒绝。
- accept 响应字段：首次受理 `conflict:false` 与 `source_event_id` 存在性显式断言。
- extract 任务结果：`readiness:"ready"` 与 `index_capabilities`（keyword/vector）显式断言。
- 已删除路由：`GET /v1/candidates`、`GET /v1/candidates/{id}`、`POST /v1/candidates/{id}/review` 断言 404。
- 竞态 superseded：同一 expected_version 的两个发布任务，先提交者发布、后提交者在提交复核处 superseded，不产生第三个版本。

未验证：沿用既有清单（Linux/macOS/release、容器镜像构建和运行、远端 CI、主应用最低 Rust 1.88、在线备份、断电恢复、长期压力/规模与真实模型语义效果）。

```

- [x] **Step 2: 修订 2026-09-29 节未验证行**

`docs/VALIDATION.md:22` 现状：

```markdown
未验证：竞态导致的 `superseded` 任务与已删除路由 `/v1/candidates` 的 404 无专门断言；其余沿用既有清单（Linux/macOS/release、容器镜像构建和运行、远端 CI、主应用最低 Rust 1.88、在线备份、断电恢复、长期压力/规模与真实模型语义效果）。
```

替换为：

```markdown
未验证：其余沿用既有清单（Linux/macOS/release、容器镜像构建和运行、远端 CI、主应用最低 Rust 1.88、在线备份、断电恢复、长期压力/规模与真实模型语义效果）。竞态 superseded 与已删除路由 404 的断言已于 2026-09-30 补齐，见上方 2026-09-30 节。
```

- [x] **Step 3: STATUS 更新戳**

`docs/STATUS.md:3` 的「2026-09-29 已实施自动发布（A1）……与 [验收](VALIDATION.md)。」句后追加：

```markdown
2026-09-30 同进程并发打开串行化并补强验收覆盖，见 [验收](VALIDATION.md)。
```

并将行首「更新：2026-09-29。」改为「更新：2026-09-30。」。

- [x] **Step 4: 台账收尾（progress.md）**

`.superpowers/sdd/progress.md:13`：

```markdown
- [ ] Task 5: 文档、计划勾选与验收记录
```

替换为：

```markdown
- [x] Task 5: 文档、计划勾选与验收记录
```

在文件末尾（锚定第 53 行完整最后一行「Task 5 (ac26d60 detail)…」整行）追加：

```markdown
- Task 5 COMPLETE (ff09353..2f2f29f). Post-merge close-out (2026-09-30, controller): branch fast-forward merged to main (main == origin/main == 2f2f29f); Task 2 MERGE CONDITION confirmed satisfied (Task 4 landed in-branch before merge — the extract-content window never shipped as a main state); residual candidate/review reference sweep clean (remaining "candidate" strings are the audited keeps: vector_candidates pool, extraction prompt wording); API.md documents publish_if_authorized as deprecated no-op. Ledger Minors dispositioned by the 2026-09-30 fix wave below.
- 2026-09-30 fix wave (branch fix/parallel-open-serialization-and-coverage, plan docs/superpowers/plans/2026-09-30-parallel-open-serialization-and-coverage.md): (1) same-process concurrent SqliteStore::open serialized via per-path async mutex — parallel full-suite runs failed 4/4 on initialization::sqlite_concurrent_initialization_is_serialized (database is locked), 5x green after fix; (2) worker source_for helper removes both claim.source.unwrap() panic paths + dedup drop now traced; (3) coverage: conflict:false, source_event_id, extract readiness/index_capabilities, removed-routes 404, deterministic race-superseded (suite 67→68); (4) docs: VALIDATION 2026-09-30 section, 2026-09-29 未验证 line amended, STATUS stamp, M3-era scratch files archived to .superpowers/sdd/archive/. Carried Minors NOT fixed by policy: old-format saved-plan decode across upgrade (dev-stage accepted), legacy oc_candidates residue for pre-upgrade queued jobs (A2.3 no-migration).
```

- [x] **Step 5: 归档 M3 遗留文件**

```sh
mkdir -p .superpowers/sdd/archive
mv .superpowers/sdd/task-6-brief.md .superpowers/sdd/task-6-report.md .superpowers/sdd/final-fix-report.md .superpowers/sdd/archive/
```

（三者均为 M3 时代的未跟踪本地文件，历史事实已由 git 跟踪的计划/STATUS/VALIDATION 记录。）

- [x] **Step 6: 文档链接与门槛**

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
git diff --check
```

并核对 `docs/VALIDATION.md`、`docs/STATUS.md` 中新增的相对链接（`superpowers/plans/2026-09-30-parallel-open-serialization-and-coverage.md`）指向存在。

Expected: 全部通过；68/68。

- [x] **Step 7: 提交**

```bash
git add docs/VALIDATION.md docs/STATUS.md
git commit -m "docs: record 2026-09-30 validation and close out auto-publish ledger

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

（progress.md 与 archive 移动均不入库——.superpowers 未被 git 跟踪。）

---

## Self-Review 记录

- **Spec 覆盖**：用户指令「全部修复」清单 ↔ 任务映射——并行 flake → Task 1；`claim.source.unwrap()` panic 路径 + 去重 tracing → Task 2；覆盖缺口（conflict:false、source_event_id、readiness/index_capabilities、/v1/candidates 404、竞态 superseded）→ Task 3；台账收尾（Task 5 勾选、合并后复核、M3 文件归档）+ 文档 → Task 4。两项明确不修（政策豁免）：旧格式已存计划跨升级解码失败（dev 阶段接受）、遗留库预升级任务残留行（A2.3 无迁移约束）——已记入台账处置行。
- **占位符扫描**：唯一运行时占位 `__秒__` 为执行期实测数据（VALIDATION 诚实规则要求实测值），非设计内容；其余步骤均含完整代码/命令。
- **类型一致性**：`acquire_lock` 新签名 `(LockGuard, Arc<tokio::sync::Mutex<()>>)` 与 open() 解构一致；`HeldLock.opening` 字段名两处一致；`source_for(claim: &ClaimedJob, version: i32) -> Result<SourceVersion>` 与 5 个调用点参数一致（memory.version/first.version 均为 i32，与 PublishedMemory.version 类型一致）；测试用例复用既有辅助（id/job/post/s.memory/s.job/s.get），无新符号。
- **测试计数推演**：59 + 1（racing_expected_versions_supersede_the_loser）= 60 集成；8 + 60 = 68 总数；Task 1/2 门槛仍为 67，Task 3/4 为 68——与各 Step 预期一致。
- **风险与回退**：Task 1 若 5 连跑仍失败，唯一候选原因是串行化未覆盖到的路径（如懒连接期 PRAGMA），停下分析不得进入 Task 2;回退 = revert 单提交。Task 2 行为不变（防御路径 panic→failed）；Task 3 全部钉既有行为，失败即暴露真缺陷。
