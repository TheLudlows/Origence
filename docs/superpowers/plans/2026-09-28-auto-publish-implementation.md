# 自动发布（移除候选审核门）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 去掉「候选 → 审核 → 发布」人工门，writer 的结构化 memory 与 capture 抽取结果直接发布；保留 scope、来源、审计、幂等、版本冲突、删除和恢复约束（整体设计 [A1](../specs/2026-09-22-memory-knowledge-platform-design.md)）。

**Architecture:** 复用 M5 本地宿主与 SQLite 事务：`memory` 接受后直接入队 `publish` 任务（payload 携带 `expected_version`，Worker 从来源事件读取正文）；`capture` 的 `extract` 任务在事务外调用模型后，对每条抽取结果按 `fact_key` 定位资产槽并逐条经账本发布（`Publication.memories` 多记忆计划，保存后可重放）。发布任务按 Write 权限复核提交；`restore` 等管理动作保留 reviewer/admin 授权。新库不再创建 `oc_candidates`/`oc_reviews` 和 `oc_versions.review_id`，不升级已有库。

**Tech Stack:** Rust (edition 2024)、axum、sqlx/SQLite、LanceDB、Kuzu、tokio；测试经 `tests/local.rs` 统一编译。

**上游计划：** [自动发布 P1–P4](2026-09-22-auto-publish.md)（本文件是其可执行细化；该计划的 `tests/lifecycle.rs`、`tests/processes.rs` 是 PG 时代旧名，对应本仓库的 `tests/local_app.rs`、`tests/sqlite_domain.rs`）。

## Global Constraints

- 不引入 PG/Apalis/RLS；全部 SQL 留在 SQLite 适配器内，业务层只经 `DomainTx`（A2.2）。
- 不提供已有库升级或迁移；schema 只在新库创建；`check` 遇缺列拒绝启动、不自动修复（A2.3）。
- 业务写、幂等、审计、入队同一 `BEGIN IMMEDIATE` 事务；模型/文件/原生 IO 在事务外；提交时按 live role 重新校验权限（A2.2）。
- 变更请求需 `Idempotency-Key`（1–200 ASCII）；同键同请求重放返回缓存响应。
- 外部写（向量/图）必须先登记 pending 账本、使用确定性产物 ID；只有 SQLite 最终提交后才是有效证据（A2.6）。
- 每个 Task 结束时 `cargo fmt --all`、`cargo clippy --locked --all-targets -j 1 -- -D warnings`、`cargo test --locked -j 1` 必须通过（Windows x64/MSVC、Rust 1.96.0；链接内存压力用 `-j 1`）。
- 提交信息用 conventional commits（参照 git log 风格），末尾加 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- 文档（STATUS/VALIDATION/README/API 等）统一在 Task 5 更新，前四个 Task 不改文档。

---

### Task 1: Worker 发布计划重构为多记忆结构（纯重构，无语义变化）

把 `Publication` 从「单 asset 单 version + chunks」改为 `memories: Vec<PublishedMemory>`；publish/restore/ingest 仍各构造一条记忆，行为不变。为 Task 3 的 extract 多事实发布铺路。

**Files:**
- Modify: `src/worker.rs`（结构体、`process_claim` 非 extract 分支、`prepare`、`ledger`、`publish_prepared`）
- Test: `tests/local_app.rs:859`（`saved_publication_recovers_after_external_graph_write` 手工构造的发布计划 JSON）

**Interfaces:**
- Consumes: 现有 `DomainTx` 方法、`LedgerEntry`/`LedgerKey`、`graph::entity_id`/`relation_id`。
- Produces（均为 `worker.rs` 私有类型，后续 Task 依赖其字段名）:
  - `struct DraftMemory { asset: Uuid, fact_key: Option<String>, expected_version: Option<i32>, chunks: Vec<Chunk> }`
  - `struct PublishedMemory { asset: Uuid, fact_key: Option<String>, expected_version: Option<i32>, version: i32, chunks: Vec<PublishedChunk> }`
  - `struct Publication { generation: i64, profile: Option<String>, summary_model: Option<String>, graph: Option<GraphExtraction>, memories: Vec<PublishedMemory> }`
  - `prepare(service, claim, profile, drafts: Vec<DraftMemory>) -> Result<Publication>`（version 由 `expected_version.unwrap_or(0) + 1` 计算）
  - 保存计划 JSON 新形状：`{"generation","profile","summary_model"?,"graph"?,"memories":[{"asset","fact_key","expected_version","version","chunks":[...]}]}`

- [ ] **Step 1: 替换 worker.rs 的计划结构体**

把现有 `PublishedChunk` 之后的 `Publication` 定义替换为（`PublishedChunk` 本身不变）：

```rust
#[derive(Clone, Serialize, Deserialize)]
struct PublishedMemory {
    asset: Uuid,
    #[serde(default)]
    fact_key: Option<String>,
    #[serde(default)]
    expected_version: Option<i32>,
    version: i32,
    chunks: Vec<PublishedChunk>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Publication {
    generation: i64,
    profile: Option<String>,
    #[serde(default)]
    summary_model: Option<String>,
    graph: Option<GraphExtraction>,
    memories: Vec<PublishedMemory>,
}
/// One asset's pending publication built before model IO, version still to be
/// verified against the live asset at commit time.
struct DraftMemory {
    asset: Uuid,
    fact_key: Option<String>,
    expected_version: Option<i32>,
    chunks: Vec<Chunk>,
}
```

- [ ] **Step 2: 重写 `prepare`（含多记忆 chunk ID 去碰撞）**

替换整个 `prepare` 函数。chunk ID 必须包含记忆下标，否则 extract 多事实时不同资产的同序 chunk 会得到相同确定性 ID：

```rust
async fn prepare(
    service: &Service,
    claim: &ClaimedJob,
    profile: Option<String>,
    drafts: Vec<DraftMemory>,
) -> Result<Publication> {
    if drafts.is_empty() || drafts.iter().any(|d| d.chunks.is_empty()) {
        return Err(AppError::Conflict("no source chunks available".into()));
    }
    let mut memories = Vec::new();
    for (memory_index, draft) in drafts.into_iter().enumerate() {
        let mut out = Vec::new();
        for (ordinal, chunk) in draft.chunks.into_iter().enumerate() {
            let embedding = if profile.is_some() {
                Some(service.models.embed(&chunk.content).await?)
            } else {
                None
            };
            let summary = if claim.kind == "ingest" && service.models.extraction_enabled() {
                service.models.summarize(&chunk.content).await?
            } else {
                String::new()
            };
            let id = graph::entity_id(&format!(
                "chunk:{}:{}:{}:{}",
                claim.job_id, claim.generation, memory_index, ordinal
            ));
            out.push(PublishedChunk {
                id,
                chunk,
                embedding,
                summary,
            });
        }
        memories.push(PublishedMemory {
            asset: draft.asset,
            fact_key: draft.fact_key,
            expected_version: draft.expected_version,
            version: draft.expected_version.unwrap_or(0) + 1,
            chunks: out,
        });
    }
    let text = memories
        .iter()
        .flat_map(|m| m.chunks.iter())
        .map(|c| c.chunk.content.as_str())
        .collect::<Vec<_>>()
        .join("");
    let graph = if claim.kind == "ingest" && service.models.extraction_enabled() {
        Some(service.models.extract_graph(&text).await?)
    } else {
        None
    };
    Ok(Publication {
        generation: claim.generation,
        profile,
        summary_model: service.models.summary_model().map(str::to_owned),
        graph,
        memories,
    })
}
```

- [ ] **Step 3: 重写 `ledger`（逐记忆 source version；图产物挂在首条记忆）**

```rust
fn ledger(claim: &ClaimedJob, publication: &Publication) -> Vec<LedgerEntry> {
    fn push(
        items: &mut Vec<LedgerEntry>,
        claim: &ClaimedJob,
        source: SourceVersion,
        artifact_id: Uuid,
        artifact_type: &str,
        surface: Surface,
    ) {
        let key = LedgerKey {
            scope: claim.scope,
            source,
            artifact_id,
            surface,
            generation: claim.generation,
        };
        items.push(LedgerEntry {
            key,
            artifact_type: artifact_type.into(),
            idempotency_key: ledger_idempotency_key(&key),
        });
    }
    let mut items = Vec::new();
    for memory in &publication.memories {
        let source = SourceVersion {
            source_id: claim.source.unwrap(),
            version: memory.version,
        };
        for chunk in &memory.chunks {
            if chunk.embedding.is_some() {
                push(&mut items, claim, source, chunk.id, "chunk", Surface::Vector);
            }
        }
    }
    // Only knowledge ingest produces a graph, and ingest is single-memory.
    if let (Some(g), Some(first)) = (&publication.graph, publication.memories.first()) {
        let source = SourceVersion {
            source_id: claim.source.unwrap(),
            version: first.version,
        };
        for e in &g.entities {
            push(
                &mut items,
                claim,
                source,
                graph::entity_id(&e.name),
                "entity",
                Surface::Graph,
            );
        }
        for r in &g.relations {
            push(
                &mut items,
                claim,
                source,
                graph::relation_id(
                    graph::entity_id(&r.source),
                    &r.predicate,
                    graph::entity_id(&r.target),
                ),
                "relation",
                Surface::Graph,
            );
        }
    }
    items
}
```

- [ ] **Step 4: 重写 `process_claim` 的非 extract 路径（构造单条 DraftMemory）**

`extract` 与 `cleanup` 分支保持原样；profile 校验、saved-plan 分支及之后的 fresh-plan 构造替换为：

```rust
    let profile: Option<String> = decode(claim.payload["embedding_profile"].clone())?;
    if profile.is_some() && profile != service.models.profile {
        return Err(AppError::Unavailable(
            "worker embedding profile differs from accepted job".into(),
        ));
    }
    let saved = claim
        .payload
        .get("publication")
        .filter(|p| p["generation"] == claim.generation);
    let publication = if let Some(saved) = saved {
        let p: Publication = decode(saved.clone())?;
        tx.commit().await?;
        p
    } else {
        let expected: Option<i32> = decode(claim.payload["expected_version"].clone())?;
        let asset = claim.asset.ok_or(AppError::NotFound)?;
        let chunks = match claim.kind.as_str() {
            "publish" => {
                let candidate = tx
                    .candidate_for_review(decode(claim.payload["candidate_id"].clone())?)
                    .await?;
                let chunks =
                    parsing::chunks(candidate["content"].as_str().ok_or(AppError::NotFound)?, "text")?;
                tx.commit().await?;
                chunks
            }
            "restore" => {
                let chunks = tx
                    .chunks_for_version(
                        asset,
                        decode(claim.payload["target_version"].clone())?,
                    )
                    .await?
                    .into_iter()
                    .map(|(content, locator)| Chunk { content, locator })
                    .collect();
                tx.commit().await?;
                chunks
            }
            "ingest" => {
                let file = tx
                    .event_file(claim.source.ok_or(AppError::NotFound)?)
                    .await?;
                let format = claim.payload["format"]
                    .as_str()
                    .unwrap_or("text")
                    .to_string();
                tx.commit().await?;
                if let Some(id) = file["file_id"].as_str() {
                    if file["media_type"] != format {
                        return Err(AppError::Invalid(
                            "file format does not match ingest request".into(),
                        ));
                    }
                    service
                        .file(&a, Uuid::parse_str(id).map_err(anyhow::Error::from)?)
                        .await?;
                    let key =
                        Service::blob_key(&a, file["hash"].as_str().ok_or(AppError::NotFound)?);
                    parsing::parse_file(&service.engine.blobs().path_for(&key)?, &format).await?
                } else {
                    parsing::chunks(file["content"].as_str().unwrap_or(""), &format)?
                }
            }
            _ => return Err(AppError::Invalid("unknown job operation".into())),
        };
        prepare(
            service,
            claim,
            profile,
            vec![DraftMemory {
                asset,
                fact_key: None,
                expected_version: expected,
                chunks,
            }],
        )
        .await?
    };
    publish_prepared(service, claim, publication).await
```

注意：原 `ingest` 分支末尾的 `return publish_prepared(...)` 移除，统一落到函数尾部；原 fresh-plan 末尾的单独 `tx.commit()` 已并入各分支（ingest 分支在文件 IO 前提交，publish/restore 在读完后提交）。

- [ ] **Step 5: 重写 `publish_prepared`（逐记忆提交复核、插入、审计）**

```rust
async fn publish_prepared(
    service: &Service,
    claim: &ClaimedJob,
    publication: Publication,
) -> Result<()> {
    let a = auth(claim);
    let entries = ledger(claim, &publication);
    let mut tx = service.write(&a, permission(claim)).await?;
    guard(&mut tx, claim).await?;
    tx.save_publication(claim.job_id, &json!(publication))
        .await?;
    for entry in &entries {
        tx.register_pending(entry.clone()).await?;
        tx.resume_pending(entry.key).await?;
    }
    tx.commit().await?;
    let mut vectors = Vec::new();
    for memory in &publication.memories {
        let source = SourceVersion {
            source_id: claim.source.ok_or(AppError::NotFound)?,
            version: memory.version,
        };
        for c in &memory.chunks {
            if let Some(embedding) = &c.embedding {
                vectors.push(VectorEntry {
                    id: c.id,
                    embedding: embedding.clone(),
                    profile: publication.profile.clone().unwrap_or_default(),
                    dimension: embedding.len(),
                    generation: claim.generation,
                    source,
                });
            }
        }
    }
    service.engine.vector().upsert(claim.scope, vectors).await?;
    if let (Some(g), Some(first)) = (&publication.graph, publication.memories.first()) {
        let source = SourceVersion {
            source_id: claim.source.ok_or(AppError::NotFound)?,
            version: first.version,
        };
        service
            .engine
            .graph()
            .upsert_entities(claim.scope, source, g.entities.clone())
            .await?;
        service
            .engine
            .graph()
            .upsert_relations(claim.scope, source, g.relations.clone())
            .await?;
    }
    let mut tx = service.write(&a, permission(claim)).await?;
    guard(&mut tx, claim).await?;
    for memory in &publication.memories {
        let current = tx.asset_current_version(memory.asset).await?;
        if current != memory.expected_version {
            return Err(AppError::Conflict(
                "publication superseded by another version".into(),
            ));
        }
        let source = SourceVersion {
            source_id: claim.source.ok_or(AppError::NotFound)?,
            version: memory.version,
        };
        let content = memory
            .chunks
            .iter()
            .map(|c| c.chunk.content.as_str())
            .collect::<Vec<_>>()
            .join("");
        tx.insert_version(
            memory.asset,
            memory.version,
            &content,
            &hash(content.as_bytes()),
            source.source_id,
            decode(
                claim
                    .payload
                    .get("target_version")
                    .cloned()
                    .unwrap_or(Value::Null),
            )?,
            claim.payload["title"].as_str(),
        )
        .await?;
        tx.attach_review(
            memory.asset,
            memory.version,
            decode(claim.payload["review_id"].clone())?,
        )
        .await?;
        for (ordinal, c) in memory.chunks.iter().enumerate() {
            tx.insert_chunk(
                c.id,
                memory.asset,
                memory.version,
                ordinal as i32,
                &c.chunk.content,
                &c.chunk.locator,
                &parsing::lexical(&c.chunk.content),
            )
            .await?;
            tx.register_owner(source, "chunk", c.id, Some(c.id))
                .await?;
            if !c.summary.is_empty() {
                let id = graph::entity_id(&format!("summary:{}", c.id));
                tx.insert_summary(
                    id,
                    c.id,
                    &c.summary,
                    publication.summary_model.as_deref().unwrap_or(""),
                    &parsing::lexical(&c.summary),
                )
                .await?;
                tx.register_owner(source, "summary", id, Some(c.id))
                    .await?;
            }
            if let Some(e) = &c.embedding {
                tx.index_ready(
                    c.id,
                    publication.profile.as_deref().unwrap_or(""),
                    e.len(),
                    claim.generation,
                )
                .await?;
            }
        }
        tx.update_asset_version(memory.asset, memory.version, claim.payload["title"].as_str())
            .await?;
    }
    for entry in entries {
        if entry.key.surface == Surface::Graph {
            tx.register_owner(entry.key.source, &entry.artifact_type, entry.key.artifact_id, None)
                .await?;
        }
        tx.confirm_committed(entry.key).await?;
    }
    if let Some(id) = claim.payload["candidate_id"].as_str() {
        tx.mark_candidate_published(Uuid::parse_str(id).map_err(anyhow::Error::from)?)
            .await?;
    }
    for memory in &publication.memories {
        tx.audit(
            "asset.published",
            memory.asset,
            json!({"version":memory.version,"source_event_id":claim.source,"job_id":claim.job_id}),
        )
        .await?;
    }
    let first = publication
        .memories
        .first()
        .ok_or(AppError::Conflict("publication without memories".into()))?;
    let mut capabilities = vec!["keyword"];
    if publication.profile.is_some() {
        capabilities.push("vector");
    }
    if publication
        .memories
        .iter()
        .any(|m| m.chunks.iter().any(|c| !c.summary.is_empty()))
    {
        capabilities.push("summary");
    }
    if publication.graph.is_some() {
        capabilities.push("graph");
    }
    tx.settle_completed(claim.job_id,"published",&json!({"asset_id":first.asset,"version":first.version,"readiness":"ready","index_capabilities":capabilities})).await?;
    tx.commit().await?;
    Ok(())
}
```

（`mark_candidate_published`/`attach_review`/candidate 读取在 Task 4 移除；`hash` 已在 worker 作用域。）

- [ ] **Step 6: 更新 `saved_publication_recovers_after_external_graph_write` 的手工计划 JSON**

`tests/local_app.rs:859` 的 `save_publication` 调用替换为：

```rust
    tx.save_publication(claim.job_id,&json!({"generation":claim.generation,"profile":null,"memories":[{"asset":claim.asset,"fact_key":null,"expected_version":null,"version":1,"chunks":[{"id":chunk_id,"chunk":{"content":"Atlas evidence","locator":{"start":0,"end":14}},"embedding":null,"summary":""}]}],"graph":{"entities":[entity],"relations":[]}})).await.unwrap();
```

- [ ] **Step 7: 全量验证**

```sh
cargo fmt --all
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
```
Expected: 与基线相同——68 项全过、0 失败（行为未变）。

- [ ] **Step 8: Commit**

```sh
git add src/worker.rs tests/local_app.rs
git commit -m "refactor(worker): publication plans carry per-memory versions

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: memory 直接发布（去掉候选中转，publish 任务降为 Write 权限）

`POST /v1/memories` 接受后直接入队 `publish` 任务；Worker 的 publish 分支改从来源事件读取正文（不再查候选）；`guard` 移除候选状态检查；publish/extract 任务按 Write 权限复核。

**Files:**
- Modify: `src/service.rs`（`memory` 方法）、`src/worker.rs`（`permission`、`guard`、publish 分支）
- Test: `tests/local_app.rs:629-795`（`local_transactions_versions_review_retraction_and_idempotency` 中段）

**Interfaces:**
- Consumes: Task 1 的 `DraftMemory`/`prepare`。
- Produces:
  - `Service::memory` 响应：`{"asset_id":UUID,"source_event_id":UUID,"job_id":UUID,"state":"accepted","conflict":bool}`（`job_id` 恒为字符串；`candidate_id` 不再返回）。
  - memory `publish` 任务 payload：`{"expected_version":null或整数,"format":"text","embedding_profile":...}`（无 `candidate_id`/`review_id`）。
  - 审计动作 `memory.direct_published`（受理时落库，target=asset）。
  - Worker `permission(claim)`：仅 `restore` → `Permission::Publish`，其余（publish/extract/ingest）→ `Permission::Write`。

- [ ] **Step 1: 先改测试表达新契约**

`tests/local_app.rs` 中 `local_transactions_versions_review_retraction_and_idempotency`：

(a) 645-666 行（writer proposal + reader 拒绝候选）替换为：

```rust
    let writer_key = store.issue_key(scope, "writer").await.unwrap();
    let writer = s.auth(&writer_key.token).await.unwrap();
    let proposal = s
        .memory(
            &writer,
            "writer",
            MemoryInput {
                fact_key: "writer".into(),
                content: "Writer proposal".into(),
                publish_if_authorized: true,
            },
        )
        .await
        .unwrap();
    assert!(proposal["job_id"].is_string());
    assert_eq!(proposal["state"], "accepted");
    let reader_key = store.issue_key(scope, "reader").await.unwrap();
    let reader = s.auth(&reader_key.token).await.unwrap();
    // Job state can expose unpublished input, so it stays behind write permission.
    assert!(s.job(&reader, id(&proposal, "job_id")).await.is_err());
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&writer, id(&proposal, "asset_id"), None).await.unwrap()["version"],
        1
    );
```

(b) 711-749 行（update 候选 + review 块）替换为：

```rust
    let update = s
        .memory(
            &a,
            "update",
            MemoryInput {
                content: "Release review required".into(),
                ..input
            },
        )
        .await
        .unwrap();
    assert!(update["job_id"].is_string());
    assert_eq!(update["conflict"], true);
    assert!(opencontext::worker::process_next(&s).await.unwrap());
    assert_eq!(
        s.get(&a, id(&first, "asset_id"), None).await.unwrap()["version"],
        2
    );
```

（`first` 之后的 restore/retract/搜索段保持不变。）

- [ ] **Step 2: 运行确认失败**

```sh
cargo test --locked -j 1 --test local local_app::local_transactions
```
Expected: FAIL（`proposal["job_id"].is_string()` 断言失败——旧流程 writer 无 Publish 权限时 `job_id` 为 null）。

- [ ] **Step 3: 实现 `service.rs::memory`**

替换整个 `memory` 方法：

```rust
    pub async fn memory(&self, a: &AuthContext, key: &str, input: MemoryInput) -> Result<Value> {
        parsing::validate_text(&input.content)?;
        if input.fact_key.trim().is_empty()
            || input.fact_key.len() > 256
            || input.fact_key.contains('\0')
        {
            return Err(AppError::Invalid("invalid fact_key".into()));
        }
        let (mut tx, cached) = self
            .command(a, Permission::Write, "memory", key, &json!(input))
            .await?;
        if let Some(v) = cached {
            return Ok(v);
        }
        let source = tx.create_event("structured", &input.content, None).await?;
        let (asset, version) = tx.slot(&input.fact_key).await?;
        let job = Self::enqueue(
            &mut tx,
            "publish",
            json!({"expected_version":version,"format":"text","embedding_profile":self.models.profile}),
            Some(asset),
            Some(source),
        )
        .await?;
        tx.audit(
            "memory.direct_published",
            asset,
            json!({"content_hash":hash(&input.content),"expected_version":version,"job_id":job}),
        )
        .await?;
        Self::finish(
            tx,
            json!({"asset_id":asset,"source_event_id":source,"job_id":job,"state":"accepted","conflict":version.is_some()}),
        )
        .await
    }
```

- [ ] **Step 4: 实现 worker 侧变更**

(a) `permission` 函数替换为：

```rust
fn permission(claim: &ClaimedJob) -> Permission {
    if claim.kind == "restore" {
        Permission::Publish
    } else {
        Permission::Write
    }
}
```

(b) `guard` 中删除整个 `if claim.kind == "publish" { ... candidate_for_review ... }` 块（保留 `active_job`、`valid_targets`、非 extract 的 `expected_version` 预检）。

(c) `process_claim` 的 `"publish"` 分支（Task 1 版本）替换为从事件读正文：

```rust
            "publish" => {
                let text = tx
                    .event_content(claim.source.ok_or(AppError::NotFound)?, true)
                    .await?
                    .ok_or(AppError::NotFound)?;
                let chunks = parsing::chunks(&text, "text")?;
                tx.commit().await?;
                chunks
            }
```

- [ ] **Step 5: 运行确认通过**

```sh
cargo test --locked -j 1 --test local local_app
```
Expected: PASS（3 项 app 测试全过；test 1 的 review 流程仍经事件正文发布，行为不变）。

- [ ] **Step 6: fmt/clippy + 提交**

```sh
cargo fmt --all
cargo clippy --locked --all-targets -j 1 -- -D warnings
git add src/service.rs src/worker.rs tests/local_app.rs
git commit -m "feat(memory): writers publish structured memories directly

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: capture 抽取结果直接发布（extract 任务多记忆发布）

`extract` 不再落候选：模型抽取（事务外）→ 短写事务逐条 `slot` → `prepare` 逐条生成计划（含 embedding）→ 经 `publish_prepared` 账本发布；任务结果为逐记忆的已发布清单。`capture` 入队时携带 `embedding_profile`。

**Files:**
- Modify: `src/service.rs`（`capture` 的 payload）、`src/worker.rs`（`process_claim` extract 分支、profile 校验前移、`publish_prepared` settle 结果）
- Test: `tests/local_app.rs:267-292`（test 1 的 capture/审核段）

**Interfaces:**
- Consumes: Task 1 的 `DraftMemory`/`prepare`/`publish_prepared`、Task 2 的 `permission`。
- Produces:
  - `extract` 任务 payload：`{"embedding_profile":...}`（新增字段；无它时向量能力缺失）。
  - `extract` 完成结果：`outcome="published"`，`result={"memories":[{"asset_id":UUID,"fact_key":string,"version":int}],"readiness":"ready","index_capabilities":[...]}`；空抽取返回 `memories:[]` 且任务成功。
  - 其余单资产任务（publish/restore/ingest）结果保持 `{"asset_id","version","readiness","index_capabilities"}`。

- [ ] **Step 1: 先改测试**

`tests/local_app.rs:267-292` 替换为：

```rust
    // Extraction publishes under the capture creator's own write authority.
    let capture = post(
        &http,
        &base,
        token,
        "/v1/captures",
        json!({"content":"Proposed policy"}),
    )
    .await;
    let extracted = job(&http, &base, token, id(&capture, "job_id"), "completed").await;
    assert_eq!(extracted["outcome"], "published");
    let claim = &extracted["result"]["memories"][0];
    assert_eq!(claim["fact_key"], "model.claim");
    assert_eq!(claim["version"], 1);
    let recall = post(
        &http,
        &base,
        token,
        "/v1/search",
        json!({"query":"proposal","mode":"keyword"}),
    )
    .await;
    assert_eq!(recall["hits"][0]["asset_id"], claim["asset_id"]);
    let second = &extracted["result"]["memories"][1];
    assert_eq!(second["fact_key"], "model.second");
    assert_eq!(second["version"], 1);
    // A duplicate fact_key in one extraction drops instead of breaking the job.
    assert_eq!(extracted["result"]["memories"].as_array().unwrap().len(), 2);
    // Zero extracted memories still completes with an empty published list.
    let empty = post(
        &http,
        &base,
        token,
        "/v1/captures",
        json!({"content":"EMPTY-EXTRACT"}),
    )
    .await;
    let none = job(&http, &base, token, id(&empty, "job_id"), "completed").await;
    assert_eq!(none["outcome"], "published");
    assert_eq!(none["result"]["memories"], json!([]));
```

测试文件顶部的 stub `extract` handler（`async fn extract`）完整替换为（system+user 消息拼接，`models.extract` 发送 `[{system},{user}]` 两条消息；空抽取触发；默认返回两条不同 fact_key 的记忆加一条重复 fact_key，覆盖 N≥2 发布与去重）：

```rust
async fn extract(Json(v): Json<Value>) -> Json<Value> {
    let prompt = format!(
        "{}{}",
        v["messages"][0]["content"].as_str().unwrap_or(""),
        v["messages"][1]["content"].as_str().unwrap_or("")
    );
    let content = if prompt.contains("Summarize") {
        "Atlas summary evidence".to_string()
    } else if prompt.contains("EMPTY-EXTRACT") {
        json!({"memories":[],"entities":[],"relations":[]}).to_string()
    } else {
        json!({"memories":[
            {"fact_key":"model.claim","content":"Model proposal requires review","publish_if_authorized":true},
            {"fact_key":"model.second","content":"Second extracted fact","publish_if_authorized":true},
            {"fact_key":"model.claim","content":"Duplicate claim ignored","publish_if_authorized":true}
        ],"entities":[{"name":"Atlas","entity_type":"service","description":"payments"},{"name":"Team","entity_type":"team","description":"owner"}],"relations":[{"source":"Atlas","predicate":"owned_by","target":"Team"}]}).to_string()
    };
    Json(json!({"choices":[{"message":{"content":content}}]}))
}
```

- [ ] **Step 2: 运行确认失败**

```sh
cargo test --locked -j 1 --test local local_app::local_host_api
```
Expected: FAIL（`extracted["outcome"]` 是 `"candidates_created"`）。

- [ ] **Step 3: 实现**

(a) `service.rs::capture` 的 enqueue 行改为：

```rust
        let job = Self::enqueue(
            &mut tx,
            "extract",
            json!({"embedding_profile":self.models.profile}),
            None,
            Some(source),
        )
        .await?;
```

(b) `worker.rs::process_claim`：把 profile 解码与校验移到 extract 分支之前（紧跟 `guard` 之后、`if claim.kind == "extract"` 之前）：

```rust
    let profile: Option<String> = decode(claim.payload["embedding_profile"].clone())?;
    if profile.is_some() && profile != service.models.profile {
        return Err(AppError::Unavailable(
            "worker embedding profile differs from accepted job".into(),
        ));
    }
```

（原位置的同一段删除；extract 分支内可直接使用 `profile`。）

(c) extract 分支整体替换为：

```rust
    if claim.kind == "extract" {
        // A saved plan replays without re-calling the extraction model.
        if let Some(saved) = claim
            .payload
            .get("publication")
            .filter(|p| p["generation"] == claim.generation)
        {
            let p: Publication = decode(saved.clone())?;
            tx.commit().await?;
            return publish_prepared(service, claim, p).await;
        }
        let text = tx
            .event_content(claim.source.ok_or(AppError::NotFound)?, true)
            .await?
            .ok_or(AppError::NotFound)?;
        tx.commit().await?;
        let memories = service.models.extract(&text).await?;
        // Slot every fact under a short write tx; versions are re-verified at
        // commit, so a race only supersedes the job without overwriting.
        let mut tx = service.write(&a, Permission::Write).await?;
        guard(&mut tx, claim).await?;
        let mut drafts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in &memories {
            // One memory per fact key per extraction; slot is SELECT-then-INSERT,
            // so a duplicate would collide with itself at the version recheck.
            if !seen.insert(m.fact_key.clone()) {
                continue;
            }
            let (asset, expected) = tx.slot(&m.fact_key).await?;
            drafts.push(DraftMemory {
                asset,
                fact_key: Some(m.fact_key.clone()),
                expected_version: expected,
                chunks: parsing::chunks(&m.content, "text")?,
            });
        }
        tx.commit().await?;
        let publication = prepare(service, claim, profile, drafts).await?;
        return publish_prepared(service, claim, publication).await;
    }
```

(d) `publish_prepared` 末尾的 settle 段替换为（audit 循环之后的 capabilities 计算保留，仅 settle 结果分流）：

```rust
    let mut result = json!({"readiness":"ready","index_capabilities":capabilities});
    if claim.kind == "extract" {
        result["memories"] = json!(
            publication
                .memories
                .iter()
                .map(|m| json!({
                    "asset_id": m.asset,
                    "fact_key": m.fact_key,
                    "version": m.version,
                }))
                .collect::<Vec<_>>()
        );
    } else {
        let first = publication
            .memories
            .first()
            .ok_or(AppError::Conflict("publication without memories".into()))?;
        result["asset_id"] = json!(first.asset);
        result["version"] = json!(first.version);
    }
    tx.settle_completed(claim.job_id, "published", &result).await?;
```

（同时删除 Task 1 在 capabilities 计算之前加入的 `let first = publication.memories.first().ok_or(AppError::Conflict("publication without memories".into()))?;` 一行——`first` 移入 else 分支内取得，空抽取才能以 `memories:[]` 完成而非报 Conflict。另改 `prepare` 开头检查：`drafts.iter().any(|d| d.chunks.is_empty())` 时报 Conflict，`drafts.is_empty()` 不再报错——空集直接产出零记忆计划、无外部写、settle `memories:[]`。）

- [ ] **Step 4: 运行确认通过**

```sh
cargo test --locked -j 1 --test local local_app
cargo test --locked -j 1 --test local saved_publication
```
Expected: PASS。

- [ ] **Step 5: fmt/clippy + 提交**

```sh
cargo fmt --all
cargo clippy --locked --all-targets -j 1 -- -D warnings
git add src/service.rs src/worker.rs tests/local_app.rs
git commit -m "feat(capture): extracted memories publish directly

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 移除候选/审核存储面与 API

新库不再创建 `oc_candidates`/`oc_reviews` 与 `oc_versions.review_id`；删除 review 服务方法、`/v1/candidates*` 路由、`DomainTx` 候选方法及内存契约 double 对应实现；既有库不迁移（多余表/列不阻断 `check`）。

**Files:**
- Modify: `src/storage/sqlite-schema.sql`、`src/storage/sqlite.rs`、`src/storage/sqlite/app.rs`、`src/storage/traits.rs`、`src/worker.rs`、`src/service.rs`、`src/api.rs`、`src/types.rs`、`src/mcp.rs`、`src/models.rs`
- Modify: `tests/storage_contract.rs`、`tests/sqlite_domain.rs`、`tests/initialization.rs`、`tests/local_app.rs`

**Interfaces:**
- Consumes: Task 2/3 后已无生产代码依赖候选方法。
- Produces: `DomainTx` 删除 `insert_candidate`、`candidate_for_review`、`apply_review`、`withdraw_affected_candidates`、`candidate_view`、`candidates_view`；`SqliteTx` 删除 `attach_review`、`mark_candidate_published`；`Service` 删除 `review`、`candidate_get`、`candidates`；`types.rs` 删除 `ReviewInput`；API 删除 3 条候选路由。schema `oc_versions` 无 `review_id`。

- [ ] **Step 1: schema 与结构检查**

(a) `src/storage/sqlite-schema.sql`：删除 `CREATE TABLE oc_candidates (...)` 与 `CREATE TABLE oc_reviews (...)` 两个整块；`oc_versions` 定义中删除 `review_id BLOB, `（一行内字段），即：

```sql
CREATE TABLE oc_versions (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, asset_id BLOB NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0), content TEXT NOT NULL, content_hash TEXT NOT NULL,
    source_event_id BLOB NOT NULL, restored_from INTEGER, title TEXT NOT NULL,
    created_by BLOB NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, asset_id, version),
    FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, source_event_id) REFERENCES oc_events(tenant_id, workspace_id, id)
);
```

(b) `src/storage/sqlite.rs` 的 `REQUIRED_PROJECTIONS`：删除 `("oc_candidates", ...)` 与 `("oc_reviews", ...)` 两项；`oc_versions` 投影改为 `"tenant_id,workspace_id,asset_id,version,content,content_hash,source_event_id,restored_from,title,created_by,created_at"`。

- [ ] **Step 2: 适配器删除候选实现**

(a) `sqlite.rs`：删除 `candidate_json` 函数、`insert_candidate`、`candidate_for_review`、`apply_review`、`withdraw_affected_candidates`、`candidate_view`、`candidates_view` 的 `impl DomainTx for SqliteTx` 实现。

(b) `sqlite.rs::insert_version`：SQL 去掉 `review_id` 列与 `NULL` 值：

```rust
        sqlx::query(
            "INSERT INTO oc_versions(tenant_id, workspace_id, asset_id, version, content, content_hash, source_event_id, restored_from, title, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, COALESCE(?, (SELECT title FROM oc_assets WHERE tenant_id = ? AND workspace_id = ? AND id = ?)), ?, ?)",
        )
```

（绑定参数顺序与数量不变。）

(c) `src/storage/sqlite/app.rs`：删除 `attach_review` 与 `mark_candidate_published` 两个方法。

- [ ] **Step 3: trait 与契约 double**

(a) `src/storage/traits.rs`：从 `DomainTx` 删除 Step Interfaces 列出的 6 个方法及其文档注释；模块与 `DomainTx` 文档注释中的 "candidates, reviews" 改为不含候选（例如 "events, memory/knowledge assets, versions, chunks, summaries, files and jobs"）。

(b) `tests/storage_contract.rs`：删除内存 double 中 `insert_candidate`、`candidate_for_review`、`apply_review`、`withdraw_affected_candidates`、`candidate_view`、`candidates_view` 六个 impl 块。

- [ ] **Step 4: 业务层删除 review 流**

(a) `src/service.rs`：删除 `review`、`candidate_get`、`candidates` 三个方法；`delete` 方法中删除 `tx.withdraw_affected_candidates().await?;` 一行。

(b) `src/api.rs`：删除路由 `/v1/candidates`、`/v1/candidates/{id}`、`/v1/candidates/{id}/review` 及 `candidates`、`candidate`、`review` 三个 handler。

(c) `src/types.rs`：删除 `ReviewInput` 结构体。

(d) `src/worker.rs`：`publish_prepared` 中删除 `tx.attach_review(...)` 块与 `if let Some(id) = claim.payload["candidate_id"]... mark_candidate_published` 块。

(e) `src/models.rs::extract`：删除 `item.publish_if_authorized = false;` 一行（字段已无语义）。

(f) `src/mcp.rs:37`：`context_search` 描述改为 `"Search published workspace memory and knowledge with source citations. Only currently published versions with valid sources are returned."`

- [ ] **Step 5: 测试更新**

(a) `tests/sqlite_domain.rs`：
- 头部注释改为 `//! events, memory/knowledge assets, versions, chunks, summaries, files and jobs`。
- `memory_lifecycle_publishes_an_asset`：删除 `let candidate = tx.insert_candidate(...)` 与 `let _ = candidate;` 两段，保留 slot/insert_version。
- 整体删除 `review_approves_a_candidate` 与 `candidates_view_lists_open_candidates` 两个测试。
- `withdrawal_and_cancellation_only_touches_transaction_scope` 重写为：

```rust
#[tokio::test]
async fn cancellation_only_touches_transaction_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("scope.db"))
        .await
        .unwrap();
    let a = provision(&store, "a").await;
    let b = provision(&store, "b").await;
    let mut tx = store.begin(b.auth.clone()).await.unwrap();
    tx.check_permission(Permission::Write).await.unwrap();
    let source = tx
        .create_event("structured", "evidence", None)
        .await
        .unwrap();
    let (asset, _) = tx.slot("fact").await.unwrap();
    let job = Uuid::new_v4();
    tx.enqueue(WorkItem {
        job_id: job,
        kind: "publish".into(),
        payload: json!({}),
        asset: Some(asset),
        source: Some(source),
    })
    .await
    .unwrap();
    tx.retract_event(source).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(a.auth).await.unwrap();
    tx.check_permission(Permission::Delete).await.unwrap();
    tx.cancel_affected_jobs().await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(b.auth).await.unwrap();
    assert_eq!(tx.job_view(job).await.unwrap()["state"], "pending");
    tx.cancel_affected_jobs().await.unwrap();
    assert_eq!(tx.job_view(job).await.unwrap()["state"], "cancelled");
    tx.commit().await.unwrap();
}
```

(b) `tests/initialization.rs` 追加：

```rust
#[tokio::test]
async fn fresh_schema_has_no_review_tables_or_column() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let store = SqliteStore::open(dir.path().join("oc.db")).await?;
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('oc_candidates','oc_reviews')",
    )
    .fetch_one(store.pool())
    .await?;
    assert_eq!(tables, 0);
    assert!(
        sqlx::query("SELECT review_id FROM oc_versions LIMIT 0")
            .execute(store.pool())
            .await
            .is_err()
    );
    // Legacy databases keep extra tables; the projection check tolerates them.
    sqlx::query("CREATE TABLE oc_candidates(x)")
        .execute(store.pool())
        .await?;
    store.check().await?;
    Ok(())
}
```

(c) `tests/local_app.rs`：测试 2 更名为 `local_transactions_versions_retraction_and_idempotency`，头部注释（如有）同步去掉 review 字样。

- [ ] **Step 6: 全量验证**

```sh
cargo fmt --all
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
```
Expected: PASS（数量较基线少：sqlite_domain 删 2 测试、initialization 增 1 测试）。

- [ ] **Step 7: Commit**

```sh
git add -A
git commit -m "feat(governance): remove candidate review gate and tables

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: 文档、计划勾选与验收记录

**Files:**
- Modify: `README.md`、`docs/API.md`、`docs/STATUS.md`、`docs/README.md`、`docs/OPERATIONS.md`、`docs/VALIDATION.md`、`docs/superpowers/plans/2026-09-22-auto-publish.md`、`docs/superpowers/specs/2026-09-22-memory-knowledge-platform-design.md`

**Interfaces:**
- Consumes: Task 2–4 的最终行为契约（memory 响应、extract 结果、角色矩阵、schema）。
- Produces: 文档与实际行为一致；VALIDATION 记录本轮实测命令与结果。

- [ ] **Step 1: README.md**

(a) 第 5 段改为：`数据按 tenant/workspace 隔离；writer 的结构化记忆与 capture 抽取结果直接进入发布任务，提交时重新核验身份、权限、来源与预期版本；检索只返回当前已发布、资产未删除且来源仍有效的证据。会话记忆、企业 SaaS 仍属于后续阶段。`

(b) 第 47 行段改为：`创建响应只代表受理；任务达到 \`completed\` 且 \`outcome=published\` 后才表示发布完成。已有事实的修改通过 expected_version 乐观校验追加版本，冲突时任务 superseded 且旧值保留。恢复追加新版本，删除资产不可恢复，撤回来源使所有引用该来源的版本不可读。完整契约见 [API](docs/API.md)。`

(c) 配置表 `OC_EXTRACTION_MODEL` 行：`可选候选抽取、知识摘要和实体关系抽取模型` → `可选记忆抽取、知识摘要和实体关系抽取模型`。

(d) 当前边界第 111 行：`原文件、事件、版本、候选及审计暂保留` → `原文件、事件、版本及审计暂保留`。

- [ ] **Step 2: docs/API.md**

(a) 角色表：
- writer 行：`reader + 上传/原文件下载/知识入库/结构化记忆发布/capture/任务查询；取消、重试自己任务`
- reviewer 行：`writer + 版本恢复/操作其他主体任务`
- 表下段落：`原始文件和任务可能涉及未发布数据，reader 不能读取。记忆与 capture 由 writer 授权直接发布，发布任务提交时重新核验权限与来源。`

(b) 接口表：
- `POST /v1/memories` 行：`\`{fact_key,content}\`（\`publish_if_authorized\` 已废弃，兼容接受但无作用）→ asset/source/job 标识，\`state:"accepted"\`，\`conflict\` 表示追加已有事实`
- `POST /v1/captures` 行：`\`{content}\` → source/job；抽取结果逐条直接发布，结果见任务 \`result.memories\``
- 删除 `GET /v1/candidates`、`GET /v1/candidates/{id}`、`POST /v1/candidates/{id}/review` 三行。

(c) 「审核例子」整节替换为：

```markdown
## 版本冲突

memory 的更新按 `expected_version` 乐观校验：受理时记录资产当前版本，Worker 提交前复核；期间发生其他发布则任务变为 `superseded`，旧值保留，不发生覆盖。此时重新读取现状并再次提交。发布计划与账本保证重放不产生重复版本。
```

(d) 「任务与读取可见性」中 `completed + outcome=candidates_created` 一句删除，改为：`只有 completed + outcome=published 表示该次版本及索引已原子提交；capture 抽取的 result.memories 列出每条事实的 asset/version。`

- [ ] **Step 3: docs/STATUS.md**

(a) 已交付列表中「记忆候选、审核、授权首次发布、知识入库…」一条改为：`writer 结构化记忆与 capture 抽取直接发布（A1 自动发布）：无候选/审核表，expected_version 乐观并发，账本化多事实发布；知识入库、不可变版本、历史标题、追加恢复、文件上传和文本 PDF 解析。`

(b) 「接下来」删除第 1 条（自动发布），后续条目重新编号。

(c) 文首更新说明补一句：`2026-09-28 已实施自动发布（A1），候选/审核门移除，详见 [自动发布计划](superpowers/plans/2026-09-22-auto-publish.md) 与 [验收](VALIDATION.md)。`

- [ ] **Step 4: 其余文档**

(a) `docs/README.md:23`：`M5 保留候选审核语义；自动发布、会话记忆、经验蒸馏、多租户 SaaS 不属于此次已完成范围。` → `自动发布已实施：候选/审核门已移除，writer 记忆与抽取直接发布。会话记忆、经验蒸馏、多租户 SaaS 不属于已完成范围。`

(b) `docs/OPERATIONS.md:11`：表行中 `候选、` 删除（`权限、事件、版本、幂等、任务、owner、索引元数据、账本、审计`）。

(c) `docs/superpowers/plans/2026-09-22-auto-publish.md`：头部定位注记改为 `已实施（2026-09-28，见 [实施计划](2026-09-28-auto-publish-implementation.md) 与 [验收](../../VALIDATION.md)）`；P1–P4 全部条目勾选为 `[x]`。

(d) `docs/superpowers/specs/2026-09-22-memory-knowledge-platform-design.md` 头部「实现对照」注记：把 `仍保留候选审核语义。本文的自动发布、P1/P2 是目标设计` 改为 `自动发布（A1）已于 2026-09-28 实施：记忆/capture 直接发布，候选/审核门移除。P1/P2 仍是目标设计`。

- [ ] **Step 5: 全量质量门并记录 VALIDATION**

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
cargo check --locked --no-default-features --lib -j 4
```

在 `docs/VALIDATION.md` 顶部新增 `## 2026-09-28：自动发布验收` 一节，记录：环境行、上表四项命令的实际结果（通过数/失败数/忽略数）、本轮新增覆盖（writer 直接发布、expected_version 冲突 superseded、extract 多事实发布与重放、候选面删除后旧库兼容、`/v1/candidates` 404）、以及「未验证」沿用既有清单。实际数字以运行为准，不得预先填写。

- [ ] **Step 6: 文档链接与提交**

检查所有编辑文档的本地相对链接可解析（无工具，逐个目检），然后：

```sh
git add -A
git commit -m "docs: auto-publish delivery records

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## 验收对照（A1 / P1–P4 覆盖）

| 要求 | 落点 |
| --- | --- |
| A1.1 移除候选→审核→发布门 | Task 2/3/4 |
| A1.2 新库无 candidates/reviews/review_id；不升级已有库 | Task 4 Step 1 |
| A1.3 memory 直接 publish job（asset/source/expected_version/slot） | Task 2 |
| A1.3 capture → extract → 逐条直接 publish | Task 3 |
| A1.4 Prepared 多记忆结果（PublishMemories 语义） | Task 1/3（`Publication.memories`） |
| A1.5 保留审计/溯源/删除级联/幂等 | Task 2（audit）、既有 owner/账本路径不变 |
| P1 write 权限、publish≠reviewer、状态语义 | Task 2（permission()、state:"accepted"） |
| P2 fact_key 槽、expected_version 乐观、多事实独立资产、幂等重放 | Task 2/3 + Task 1 保存计划 |
| P3 提交复核、失败可观测、撤回不可复活 | guard + commit 复核不变；cancel_affected_jobs 保留 |
| P4 API/MCP/测试/文档 | Task 4/5 |
