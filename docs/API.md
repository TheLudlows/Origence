# Origence API 与库接口

适用本地宿主 SQLite 控制状态以及 AnySeek 正文、摘要、全文、向量与一跳图投影。本文负责 HTTP v1 与公开库接口契约；进度见 [STATUS](STATUS.md)，操作步骤见 [OPERATIONS](OPERATIONS.md)，精确版本验证见 [VALIDATION](VALIDATION.md)。

按需阅读：[库接口](#rust-库接口) · [接口表](#接口) · [任务与可见性](#任务与读取可见性) · [检索](#检索) · [显式身份](#显式记忆身份)。

所有 `/v1/*` 请求需 `Authorization: Bearer TOKEN`。workspace 来自 key，客户端不能指定 workspace。原有命令式修改接口需 1–200 个 ASCII 字符的 `Idempotency-Key`；Run/Skill 使用下文的稳定 ID、修订号与摘要；幂等范围为 workspace + key 主体 + 操作/目标 + 幂等键。同请求复用结果，同键不同内容返回 409。幂等响应只包含标识/状态，删除后重放不会再次写入或返回已删正文。

## 私有会话、Trace 与 Skill

`GET /v1/session` 从认证派生逻辑 key 与服务端 session ID（首次可创建绑定）。已有
`/v1/sessions/{session_id}/turns`、反馈和 Improve 接口只接受当前 key 的归档，
并保留 writer 权限。发布后的 Learning 正文和条件可被同 workspace 召回，但归档、
Improve 和支持记录的私有指针仅返回给原逻辑 key。管理员可以对当前空间的活跃凭据调用
`POST /admin/keys/{id}/rotate`，新凭据延续原逻辑 key 并撤销旧凭据；响应中的新 token
应安全保存。独立签发的新 key 不能读取原归档。客户端通过
[`context_session` / `context_turn_archive` / `context_turn_get`](CLIENTS.md) 使用同一边界。

Trace 以私有 Run 存储。Run 入口从认证取得逻辑 key，按当前权限和来源有效性处理。

| Run 接口 | 输入与结果 |
| --- | --- |
| `POST /v1/runs` | writer 提交稳定的 `run_id` 与脱敏 `summary`，绑定当前归档 |
| `POST /v1/runs/{id}/events` | 稳定 `event_id`、时间、报告方与具名事件；相同重试返回原序号 |
| `POST /v1/runs/{id}/context` | `event_id` 和 `context`（EvidenceBundle 请求）；返回 bundle 和服务端准备回执 |
| `GET /v1/runs/{id}?after=0&limit=50` | 当前 key 的分页追溯，包含明确的采集边界 |
| `GET /v1/runs/{id}/prefix?through_sequence=N` | 当前 key 的不可变前缀摘要，可作为 Skill 依据 |

服务端回执证明已准备精确上下文，不证明客户端收到或模型使用。客户端的工具、加载、
结果、反馈和工件引用只是上报；未上报即未知。任一来源失效时引用复核停发。
外部核验记录由 Run 所有者针对精确前缀通过 HTTP 提交，保留转录来源与核验身份的声明状态。
详情见 [Run 契约](contracts/runs.md)和[外部核验契约](contracts/run-verification.md)。

| Skill 接口 | 输入与结果 |
| --- | --- |
| `PUT /v1/skill-candidates/{id}` | `{expected_revision,candidate}`；0 创建，修改产生新修订 |
| `GET /v1/skill-candidates/{id}` | 仅创建者读取候选、私有备注和精确依据 |
| `POST /v1/skill-candidates/{id}/reviews/{review}` | 选定审阅者、修订、发布目标和脱敏依据 |
| `GET /v1/skill-reviews/{review}` | 选定审阅包与决定，不扩展到原会话或 Run |
| `POST /v1/skill-reviews/{review}/decision` | 指定审阅者使用 `{digest,approve}` 批准或驳回精确内容 |
| `POST /v1/skill-reviews/{review}/publish` | 有发布权者按 `{digest}` 发布已批准的不可变版本 |
| `GET /v1/skills/{id}/versions/{version}/package` | 核对作用域、版本和来源后返回仅包含获准工件的包 |
| `DELETE /v1/skills/{id}/versions/{version}` | 有发布权的角色撤回版本，阻断后续在线导出 |

候选修订、审阅包内容或发布目标变化需要新的批准。原始会话和审阅依据不随包分发。
调用方提交候选，也可用选定经历 CLI 将手工填写的方法绑定到一个精确依据。包格式和边界见
[Skill 契约](contracts/skills.md)和[选定经历 CLI](SKILL-CANDIDATE-CLI.md)。

## 检索诊断

Search、Resolve 与 EvidenceBundle 返回 `retrieval:{requests,omitted_requests}`。
每条是一次真实 AnySeek `/query` HTTP 尝试，包含 Origence/后端请求标识、路由、
实际模式（browse/lexical/vector/hybrid）、客户端往返微秒、HTTP 状态、安全错误码、
返回候选数量及可获得的后端诊断。browse 包括候选预检，不能当作语义召回。

后端诊断格式为 v2，含服务版本、分析配置摘要、embedding 空间 SHA256（未配置为 null）、
查询耗时与 `visible_seq`，以及查询后另行采样的
committed/lexical/vector 进度、失败与恢复标志。配置还记录核对过的容量限制和 embedding
空间摘要。每次查询均比较返回的分析与空间摘要，拒绝启动后发生的空间变化。
序号属于 AnySeek collection，不是 workspace 序号、SQLite 版本或跨存储快照。
客户端往返和服务端耗时边界不同，均不证明客户端消费或模型实际使用。

每次调用只保留最近 128 次尝试，较早省略数量显式返回。诊断不保存查询原文、向量、
过滤值、正文、URL、密钥或原始后端错误；未知/未收到字段保持 null。畸形或关联不符的
后端诊断拒绝使用。上下文内容与引用的确定性不包含各次调用变化的标识和耗时。

`POST /v1/runs/{id}/context` 将成功诊断与精确 bundle 摘要一起写入私有 Run；
通过认证、归属和输入校验后发生的准备失败记录 `context_preparation_failed`，并标为
server_observed。首次成功时另存私有上下文快照；相同请求重放先检查事件和请求摘要，
复核当前权限、原始引用及私有会话有效性后返回原 bundle/回执，不再调用模型或 AnySeek，
后端暂时不可用或语料更新不会改写原结果。快照不在分页 Run trace 中重复返回。
请求摘要改变返回冲突；来源撤回或权限失效仍阻断重放。
失败事件同样不可覆盖，恢复后新的准备尝试使用新的 event_id。普通 MCP/Search 请求
不会自动加入某个 Run，仍需显式调用准备接口；未采集活动保持未知。

## AnySeek 统一检索

每个 workspace 的全文、向量和一跳图候选使用同一 AnySeek collection。适配器将 Origence 预分词
编码为固定 SHA-256 词项并保留 AND 语义；向量由 Origence 生成，模型 profile 与维度
由目录绑定固定。AnySeek 保存发布后的分块正文和摘要，SQLite 保存权限、发布版本、来源和内容哈希，Blob 保存原始文件。返回前，Origence 核验正文、权限、版本与来源。

发布在外部 IO 前保存原 operation epoch。每个不可变发布至多 64 个 chunk Record，编码后请求至多 2 MiB；
图元数据附在其中一个确定的 chunk 上，不新增 Record。同一发布通过一个组提交，新业务版本使用独立组。网络结果未知时检查完整组，
作业重用持久计划，撤回后不因残留索引而复活。查询按作用域、完整来源身份和已授权
chunk IDs 筛选，返回前再复核权威状态。向量候选核对额外包含模型 profile 和维度。
依赖故障返回不可用，不转换为空召回。

部署及目录绑定见 [OPERATIONS](OPERATIONS.md#anyseek-全文与向量)，验证结果见 [VALIDATION](VALIDATION.md)。

## Rust 库接口

- 无元数据库核心：`default-features = false`，提供类型、解析、模型、Blob 与 AnySeek 接口。
- SQLite 权威适配器：`default-features = false, features = ["sqlite"]`。
- 完整 HTTP/MCP 宿主：默认 `local-storage`，自动启用 `sqlite`。

核心独立检查用 `cargo test --no-default-features`；原 authority-only 检查现在显式使用
`cargo test --no-default-features --features sqlite`，仍运行其完整授权/队列/恢复回归。

宿主通过 `Service::open_with_anyseek(dir, models, config)` 组装服务；通用调用方使用
公开 Service/DTO，事务与关联查询留在存储适配器。完整签名以 [lib.rs](../src/lib.rs)
导出的模块和源码为准。

旧的独立图存储接口和图遍历已移除；
`StorageEngine::new` 不再接收图存储，`SqliteStore::cleanup_plan` 只返回待清理的 chunk ID。
现有 HTTP/MCP 图检索契约不变。

- `SqliteStore::check_memory_identity(path)` 对已有路径执行只读、独占锁下的完整同版本
  schema 检查，不创建或修复表；CLI 为 `--offline identity-check`。
- 命令幂等、作业执行和 artifact owner/index/ledger 各有独立职责，见[存储不变量](contracts/storage.md)。
- 所属 workspace 与消息批次接口见下文；外部协议字段不能进入通用存储或授权判断。

## 角色

| 角色 | 权限 |
| --- | --- |
| reader | 已发布内容 get/search/resolve |
| writer | reader + 上传/原文件下载/知识入库/结构化记忆发布/capture/会话归档与反馈/任务查询；取消、重试自己任务 |
| reviewer | writer + 版本恢复/操作其他主体任务 |
| admin | reviewer + 资产删除/来源撤回/文件删除/失败清理任务重试 |

原始文件和任务可能涉及未发布数据，reader 不能读取。记忆与 capture 由 writer 授权直接发布，发布任务提交时重新核验权限与来源。

## 接口

### 管理界面只读目录

- `GET /v1/assets?kind=memory|knowledge&limit=25&after=<UUID>`：reader 以上，
  workspace 来自认证；按资产 UUID 升序扫描当前已发布、未删除的候选，逐条复用
  普通资产读取的完整来源/语义/可见性检查。返回 `{items,next_after}`，items 仅为
  资产元数据，不包含正文或语义 quote。来源失效的候选被排除，其他存储错误明确失败。
- `GET /v1/runs?limit=25&after=<UUID>`：reader 以上，只返回当前逻辑 key 的 Run
  ID、脱敏 summary 与创建时间，不授予管理员读取其他 key 私有记录的权限。
  获授权的凭据轮换延续逻辑身份；响应含 `scope:"current_logical_key"`。

两个接口默认 limit=50、范围 1–100；省略 after 从头开始，未知查询字段拒绝。
按 UUID 而非创建时间排序；`next_after` 是本页最后扫描的候选标识，不是总数。
资产页可能因来源失效而少于 limit，甚至为空；只要 next_after 非 null 仍可继续。
分页是逐次授权的实时读取，不宣称跨请求冻结快照。读取不创建会话或任何发布任务。

运行总览使用独立的 `GET /v1/runs/overview?window=24h|7d`，默认 `24h`。
仅统计当前 tenant、workspace 和逻辑 key 的持久化 Run 创建记录。
响应包含 `scope:"current_logical_key"`、`window`、`timezone:"UTC"`、
`start_ms`、`end_ms`、`created_runs` 和 `series`。
`series` 按时间升序返回 24 个一小时桶或 7 个 24 小时桶，每桶
含 `start_ms`、`end_ms`、`created_runs`，包括零计数。
起止为 Unix 毫秒，窗口和桶均左闭右开，以本次服务端读取时刻为终点向前滚动。
此计数随授权轮换保留，事件、重试和证据撤回不会新增或抹去创建记录。
窗口参数不能覆盖鉴权范围。详情见 [Run 合同](contracts/runs.md#run-creation-overview)。
该接口使用新建的 SQLite 权威库。新版拒绝缺少创建时间索引的旧库，旧版也拒绝
含新索引的库。两个方向均在修改数据库前拒绝。

记忆目录支持额外的 `memory_filter=<URL 编码 JSON>`，例如：
`{"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"release.rule","context":{"env":"production"}}`。
库入口为 `Service::list_memories(auth, &MemoryCatalogFilter, after, limit)`。

- `subject` 可省略；提供时必须包含现有 `kind` 与精确业务 `stable_id`，不推断名称或 key。
- `predicate` 可省略；提供时按现有身份规范校验并精确匹配。
- `context` 可省略表示不限；提供时比较完整字符串键值集合，键顺序无关，大小写/Unicode
  原样比较。`{}` 只匹配空条件，不是通配符，也不是子集条件。
- `identified` 可省略表示不限，`true` 只匹配已保存的显式身份，`false` 只匹配无身份资产；
  `false` 不能同时指定主体/属性/条件。未知身份不会推断。未知字段或非法组合拒绝。
- 仅 `kind=memory` 接受此字段，JSON 字符串最大 16,384 字节。

筛选目录仍是实时、有界的候选游标接口，而不是全量计数：单次按 scoped 主键从 after
检查最多 100 个候选（另取一个 lookahead），包含非记忆、未发布或隐藏候选；在返回前
完成精确筛选及正常资产来源/语义可见性复核。`limit` 是返回上限；可能少于 limit 或为空，
只要 `next_after` 非 null 就应继续，变更筛选须清空游标。不会为凑满一页无限扫描。
未指定 `memory_filter` 的既有目录行为不变。记忆 UI 通过此服务端筛选分页，不以本页
文本过滤或搜索命中冒充目录；知识页的“筛选已加载的资产”仍明确是本地筛选。

Node.js UI 只通过这些及下表既有 HTTP 接口访问服务；不直接打开数据库或执行 CLI。

| 方法和路径 | 请求 / 结果 |
| --- | --- |
| `GET /health/live` | 进程存活，无鉴权 |
| `GET /health/ready` | 单 Worker 正在运行且本地存储检查通过；不是外部模型健康检查 |
| `POST /v1/memories` | `{fact_key,content}`（`publish_if_authorized` 已废弃，兼容接受但无作用）→ asset/source/job 标识，`state:"accepted"`，`conflict` 表示追加已有事实 |
| `POST /v1/memories/identified` | `{identity:{subject:{kind,stable_id},predicate,context:{}},content}` → asset/source/job；同 scope 下相同身份复用资产；`conflict` 表示已有版本 |
| `POST /v1/memories/lookup` | 请求体为完整 `{subject,predicate,context:{}}`，reader 可读；返回与资产 GET 相同的当前已发布视图；不需要 Idempotency-Key，不创建资产 |
| `POST /v1/captures/semantic` | `{content,targets:[{identity,expected_version,allow_update:false}]}` → source/job；受限模型分类并复用 记忆语义，详见下节 |
| `POST /v1/captures` | `{content}` → source/job；抽取结果逐条直接发布，结果见任务 `result.memories` |
| `POST /v1/captures/identified` | `{identity,content}` → asset/source/job；完整身份由调用方指定，模型只抽取原文片段；需要 extraction model |
| `POST /v1/knowledge` | `{title,content或file_id,format:"text"或"markdown"或"pdf",asset_id:null或UUID,expected_version:null或整数}` |
| `POST /v1/files?name=...&format=...` | 原始二进制请求体，非 multipart；返回 file_id；文件名仅作元数据 |
| `GET /v1/files/{id}` | 校验 hash 后返回 attachment/octet-stream，存储路径由 scope 和内容 hash 生成 |
| `GET /v1/assets/{id}?version=N` | 已发布当前/历史版本，含 content/hash/title/source/restored_from |
| `POST /v1/assets/{id}/restore` | `{target_version,expected_version,reason}` → 恢复 job；完成后新增版本 |
| `DELETE /v1/assets/{id}` | 永久逻辑墓碑，阻断包括历史版本在内的读取 |
| `GET /v1/events/{id}` | writer 及以上读取有效来源原文 `{event_id,kind,content,file_id,created_at}`；reader 403，跨 scope/已撤回/文件已删除 404 |
| `DELETE /v1/events/{id}` | 撤回来源，阻断所有引用该来源的版本，不自动回退到旧版本 |
| `DELETE /v1/files/{id}` | 阻断原文件，撤回所有引用该文件的事件 |
| `GET /v1/jobs/{id}` | operation/state/generation/outcome/result/error_code |
| `POST /v1/jobs/{id}/cancel` | 标记 cancelled，增加 generation/run_token，旧执行不能提交；已完成任务不能取消，cleanup 不能取消 |
| `POST /v1/jobs/{id}/retry` | 仅 failed/cancelled 且来源、资产仍有效；新 generation 原子入队；cleanup 重试需 admin |
| `POST /v1/sessions/{session_id}/turns` | `{turn_id,question,answer,used_evidence:[]}` → 同步归档标识；writer 及以上，详见会话归档节 |
| `GET /v1/sessions/{session_id}/turns/{turn_id}` | writer 及以上读取完整有效问答、归档引用与反馈 |
| `POST /v1/sessions/{session_id}/turns/{turn_id}/feedback` | `{feedback_id,target,rating,comment?}` → 同步记录标识；不调权、不触发模型 |
| `GET /v1/sessions/{session_id}/turns/{turn_id}/feedback` | writer 及以上读取该有效轮次的反馈列表 |
| `POST /v1/search` | `{query,limit:10,mode:"keyword",allow_partial:false}`；mode 支持 keyword/vector/hybrid |
| `POST /v1/resolve` | `{query,budget_tokens:2000,mode:"keyword",allow_partial:false}` → rendered_context 和 sources |

知识更新只在异步发布成功后改变当前版本及标题。文件和正文必须二选一；PDF 必须使用 file_id。每个历史版本保留自己的标题。恢复内容和标题，同时新增 `restored_from`，不会覆盖历史版本或跳过来源状态检查。

## 身份与密钥管理

| 方法和路径 | 语义 |
| --- | --- |
| `GET /v1/whoami` | 返回已认证 key 的 id、tenant_id、workspace_id、role |
| `POST /admin/keys` | admin 提交 `{workspace_id,role}`；workspace 必须等于当前 token 所属 workspace，返回 `{key_id,token}` |
| `DELETE /admin/keys/{id}` | admin 撤销同 workspace 的 key，允许撤销自身；后续调用拒绝 |

`/admin/keys` 同样要求 Bearer token，但不使用业务幂等缓存，避免保存明文 token。发行结果只返回一次，重复 POST 会创建不同 key；若响应丢失，需要离线管理核对数据库中的 key ID 并撤销多余凭据。离线 workspace/key 管理通过 `--offline` 和操作系统目录权限授权，宿主运行时拒绝打开。

## 版本冲突

更新受理时记录资产当前版本，Worker 提交前复核；期间发生其他发布则任务为 superseded，不覆盖先发布的版本，应重新读取后再提交。发布计划/账本保证重放不重复发布。显式身份入口还支持[调用方版本前置条件](#显式写入的调用方版本前置条件)。

## 任务与读取可见性

`pending → processing → completed/failed/superseded`；短暂存储故障为 `processing → retry_wait → processing`；取消为 `cancelled`。只有 completed + outcome=published 表示该次版本及索引已原子提交；capture 抽取的 result.memories 列出每条事实的 asset/version。之后仍可能被更新、删除或来源撤回。

删除响应为 `{id,blocked:true,cleanup_job_id,originals_retained:true}`。逻辑删除事务完成后新读取被阻断，索引清理是否完成不影响这一规则。已经发出的数据无法收回；在删除前已开始的请求可能先完成。

所有业务写事务以 SQLite `BEGIN IMMEDIATE` 开始，原子提交业务、幂等、审计和入队。模型/文件/原生索引 IO 在关系事务外执行。最终提交再次校验权限、墓碑、来源、预期版本、generation 和 run_token；密钥撤销与写事务串行化。此本地适配器无 RLS，scope 由适配器显式绑定。

发布计划和 pending 账本先落 SQLite，全文/向量/图写入后，版本、owner、ready 索引及 committed 账本与任务完成一起提交。外部产物不等于已发布证据。短暂存储错误进入 `retry_wait`，最多尝试 5 次；模型/输入失败需显式重试。独占宿主重启时恢复 processing 并提升 run_token，先对账清理再接收请求。

cleanup 的 `completed + outcome=ok` 表示本轮派生数据清理完成；原始事件、版本和文件仍保留，不等于原文物理擦除。

## 检索

返回 hits 含 asset_id/version/chunk_id/source_event_id/locator/score。locator 给出原文 UTF-8 字节区间、行号，PDF 另有页号。只取当前已发布版本，显式历史版本走 get。

私有 Run 的固定 PDF 引用可用 `GET /v1/runs/{id}/pdf-source?sequence=N&evidence_ref_id=UUID`
读取已核对的页内提取文本。该接口要求 Run 所有权和现有原件权限，返回精确资产版本、
已知页码及可验证的文本片段，或明确无页内定位。`original=true` 获取该引用的原件。
详情见 [Run 契约](contracts/runs.md#pdf-page-preview-from-a-private-run)。

向量必须与 profile 一致，不混用不同模型/维度。模型查询失败且 `allow_partial=true` 才退回关键词，同时返回 `effective_mode` 和 warnings；否则 503。纯关键词发布的历史内容没有向量，不会出现在 vector 结果中，hybrid 的关键词分支仍可召回。无 profile 自动补齐或 reranker；向量查询在 AnySeek 中对授权候选排名；不承诺对原后端或生产分布具有同等排序与性能。

resolve 返回 `tokenizer=utf8-bytes-upper-bound-v1`、`count_is_estimate=true`；count 为 rendered_context 的 UTF-8 字节数。预算 0–32000，超出预算的整块被跳过，不生成没有正文的引用。返回文本是外部证据，不能当成系统指令。

原文关键词在 AnySeek 中按词项编码执行 AND 匹配（查询词项全部出现），限定当前 scope、发布代和完整来源版本。Summary 保留同 chunk 跨摘要行聚合后的 AND 语义。候选身份缺失或不一致会明确失败，不静默返回少召回结果。hybrid 将 keyword/vector/summary 映射为 chunk 证据后 RRF 融合，并按实体名种子扩展一跳关系；每个图对象包含 `evidence`（asset_id/version/chunk_id/source_event_id/locator）。SQLite owner 和 committed 账本决定图证据是否有效；不会返回已撤回来源的描述投影。

search/resolve 可选 `components:{summaries:true,graph:true}`，两个字段默认均为 true，仅 hybrid 且未限定 memory_identity 时启用。可独立关闭摘要、图分支做同库消融，原文 keyword/vector 分支始终保留；省略 components 保持旧行为。响应 `active_components` 表示实际启用的分支（不代表已有产物或命中）。resolve 沿用相同开关和现有来源/字节预算检查；HTTP 与 MCP 共用此契约。

图分支用 AnySeek 元数据过滤找名称包含查询词的实体，再按任一初始实体 ID 查询入边或出边。候选会按来源、版本和 generation 经 SQLite 复核，撤回或未发布的投影不能当作证据。关系在响应中保留原有方向；扩展出来的实体不会继续扩展。图字段不作为全文词项或向量，不增加参与 BM25 统计的记录。候选超过服务端上限会明确报错，不静默截断。图候选预算在引擎侧独立于响应的最多 20 个实体和 20 条关系；此路径不改变 `/v1/context`，也不新增独立图查询 API。

只有知识 ingest 在配置抽取模型时生成摘要和图；短事实 publish 与 restore 不自动生成图。启用的加工步骤失败时整个任务失败，不宣称部分图已就绪。查询 embedding 失败可按 allow_partial 降级；存储错误不会通过降级绕过权限或来源验证。

## 错误

领域错误使用 `{error:{code,message}}`，不暴露 SQL、密码、模型响应或文档正文：401 未认证，403 无权限，404 不存在或不可见，409 版本/幂等冲突，422 输入无效，503 模型等依赖不可用，500 内部/数据库错误。Axum 在进入 handler 前产生的 JSON/路径/请求大小错误使用框架默认格式和状态（例如 400/413/422）。

## 显式记忆身份

`POST /v1/memories/identified` 示例：

```json
{"identity":{"subject":{"kind":"service","stable_id":"billing"},"predicate":"release.approval","context":{"environment":"production"}},"content":"生产发布需要两人审批"}
```

subject kind 支持 user/agent/project/service/team；stable_id 是业务稳定标识，不是 API key 主体。身份包含鉴权 scope、主体、predicate 和排序后的 context，不包含内容或版本。未知字段拒绝，predicate 采用小写 ASCII 命名；条件值按原样比较，不做别名或语义归一化。相同身份追加版本；不同主体、条件或 workspace 分开。并发写入沿用 expected_version 乐观冲突检查，最终状态以 job 为准。

资产读取增加 `identity` 字段；旧记忆/知识为 null。身份绑定资产且不可修改，删除后同身份不能重建。旧 fact_key/capture 写入不能修改已绑定身份的资产，返回 409；旧事实键若恰好与新身份编码碰撞，新入口返回 409，不自动转换。

lookup 接收上述 identity 对象（无 content），在 key 的 scope 内精确查找，不接受别名/语义近似。未命中、未发布、已删除或来源撤回均为 404，不回退旧版本或创建任何资产/来源/任务。缺失身份表不是合法的未命中或空结果。返回 version 不是后续写入锁，最终以 job 的版本检查为准；reader 不能借此读取原始 capture 输入。

新建库始终包含完整身份 schema；启动拒绝缺表或不兼容库，不提供缺表时仅开放旧接口的降级模式，也不自动安装或升级。只读检查步骤见 [OPERATIONS](OPERATIONS.md#离线记忆身份检查)。旧 captures 仍用 fact_key；显式写入/capture 使用 HTTP。只读 MCP 支持 memory_identity 检索，CLI 默认不带该过滤。

## 单身份 capture

`/v1/captures/identified` 接受与显式记忆写入相同的 `{identity,content}`；content 是待抽取原文。身份完全来自调用方，scope 来自 key，模型不能指定主体、属性、条件或 workspace。受理时记录当前 expected_version，抽取期间发生其他发布会沿现有版本治理 supersede，不能覆盖较新版本。未配置抽取模型时返回 503；缺失身份 schema 的库在启动时拒绝打开。

模型输出必须提供原文精确 quote 和 UTF-8 byte_start/byte_end，服务器校验边界和内容一致；发布正文就是该原文片段，search locator 的 source_span=true 标明区间指向来源原文。重复相同 quote 合并；多个不同 quote 视为单身份歧义，任务 failed/INVALID_ARGUMENT，不更新版本。零片段完成且 result.memories=[]，受理时预留的资产可能仍无已发布版本。来源事件保留原始输入。

此切片不做自动主体推断、别名归一化、任意语义归并或多值属性合并。原文区间验证证明可追溯性，不能证明模型的语义选择或事实真假；否定/提议识别仍依赖模型，需真实评估。原有 capture 接口保持旧 fact_key 模型。

## 身份状态与来源核验

资产 get 增加 normalization_status：记忆有显式身份绑定时为 explicit_identity；记忆语义/受控提取 保留的冲突、否定、提议和歧义证据为 unnormalized；旧 fact_key 记忆为 legacy_unidentified；知识资产为 not_applicable。该状态只描述身份绑定方式，不代表事实可信度或模型语义验证；历史版本沿用同一资产身份。旧记忆不会因该字段被自动归一化。

writer 可以用来源标识访问 GET /v1/events/{id}，将 search locator 的字节区间与原始输入对照；这也支持核验失败或零结果 capture 的来源。原文可能包含未发布内容，因此 reader 无权访问。接口不返回撤回来源、已删文件来源或其他 workspace 内容；逻辑删除仍保留底层原文，不提供物理擦除。记忆语义 已提供未归一化资产的独立状态；自动身份归并尚未实现。

## 检索与上下文身份标注

search 的每个 hit 增加 identity（无绑定为 null）与 normalization_status（explicit_identity/unnormalized/legacy_unidentified/not_applicable），沿用资产读取含义；完整身份取自 SQLite 资产绑定，不接受模型或向量库宣称的身份。返回前仍复核当前版本、scope 与来源。

resolve 的 chunk 引用及正文前 JSON 标注包含 kind、identity、normalization_status，context_policy=identity-provenance-v1；图引用格式不变。引用/标题/标注/正文均计入字节预算，整块保留或省略，因此标注可能减少正文容量。状态只表示身份绑定，不表示真值、有效期或置信度；冲突识别和类型预算未实现。

## 精确身份检索过滤

search/resolve 可选 memory_identity，格式为完整 `{subject,predicate,context:{}}`，省略时沿用混合检索。启用后只保留当前 scope 内完整身份精确相同的 memory；排除旧未识别记忆、知识块和图扩展。字段在模型调用前校验，不接受别名/语义近似/部分条件匹配；未知字段拒绝，非法身份 422，未命中为空结果。身份不替代鉴权，结果仍受当前版本、来源和墓碑规则约束。完整当前 schema 中没有显式绑定则不匹配；缺失身份表属于存储错误，不作为空结果返回，也不自动升级。

完整身份先定位授权资产，keyword/vector 候选限定于该资产后排序和截断，最终重新校验版本、来源、墓碑及权限。无身份查询仍有各分支候选上限；身份限定不保证语义召回。只需定位当前资产时用 lookup。响应回显 memory_identity，resolve 透传过滤；HTTP/MCP 支持，CLI 默认不带过滤。

## 显式写入的调用方版本前置条件

memories/identified 和 captures/identified 可选 expected_version（非负整数）。0 表示当前尚无已发布版本（可能已有未发布占位资产）；正数必须与受理事务中当前版本相同。不匹配返回 409，事务回滚且不新建来源或任务；负数 422。省略或 null 保持按受理时版本追加的旧行为，不是“必须创建”。

可以将 lookup 的 version 传入写入接口，覆盖读取与受理之间的并发窗口。受理后 Worker 仍用受理版本复核，版本变化使任务失败，不保证同步发布成功。相同幂等键/相同完整请求的成功重放先返回原受理结果，不因之后版本变化变成冲突；改变 expected_version 属于改变请求，返回幂等冲突。省略/null 不进入规范请求 payload，保留旧键的重放兼容。

## 通用所属 workspace 与消息批次库接口

以下能力通过公开 Rust Service API 使用。

- `issue_key_with_capabilities(auth, role, KeyCapabilities { allow_owned_workspaces: true })`：只有目标 root 的当前 admin 可签发；既有 `issue_key` 默认 false，admin 或同 root 新 key 不自动继承。直接绑定 child 的 key 不可获得继续派生能力。
- `ensure_owned_workspace(auth, target_id, name)`：当前有显式能力的 root writer 及以上，在同一事务创建或检查一层所属 workspace。目标 tenant/owner 不匹配返回冲突；不改绑或随机改 ID。返回的 AuthContext 保留原 key，只提供 reader/writer 权限。
- `lookup_owned_workspace(auth, target_id)`：先核对 root 的当前身份、能力与 read 权限；合法但未知目标返回 None，不创建。普通 key 的原权限不扩大。
- `submit_message_batch(auth, key, MessageBatch, AcceptedMessageSpec)`：接收外部 batch_id、session_id 和有序 SourceMessage 数组，原子保存 source、asset、job 和 `oc_commands` 回执。每批固定使用 workspace 范围的 `message_batch.ingest` 幂等；普通命令仍使用 principal 范围。key 为 1–200 ASCII 字节，调用方不能自选去重域。
- MessageBatch 的 batch_id/session_id 为 1–4096 UTF-8 字节、无 NUL；每批 1–256 条消息，正文合计不超过 1,000,000 UTF-8 字节，正文遵守通用文本校验。role 是 1–64 UTF-8 字节、非空白且无控制字符的原样标签，例如 system/tool；它不代表权限。timestamp 是可选有符号 i64 Unix 毫秒，缺省/null 不补当前时间，不据此重排消息。
- `AcceptedMessageSpec::raw_only(profile)` 固定 `message-batch-ranges-v1`、parser_version=1、chunking_version=1、raw_only=true 及 embedding profile；接受事务把完整规格写入原 job payload。完整批次和规格参与请求 hash，重复键的内容/session/顺序/角色/时间/parser/profile/mode 变化返回冲突，不另建任务。恢复和显式 retry 使用原规格；运行模型 profile 不匹配时拒绝处理。
- `IngestReceipt` 保存原 asset/source/job/目标版本和规格，不保存永久 success。`message_batch_status` / `wait_message_batch` 重新授权并观察这一批次，区分 Pending、Retrievable、Failed、CancelledOrSuperseded、Invisible。只有确切目标版本、有效来源、全部目标 profile 的原分块索引和账本 generation 一致，才是 Retrievable。
- 同一 workspace 的合法新 key 可命中同一回执，但 job.created_by 不变。原 key 撤销/降权/关闭能力后，新 key 不接管原消息发布任务；Worker、重试、最终发布及 I/O 后读取都重新验证实时权限。正常轮换先排空旧任务，再撤销旧 key。原有 cleanup 是特权回收任务：仍允许当前 admin 在原 creator 撤销后显式重试清理，但不转移 created_by，也不发布数据。
- 原文来源 JSON 保留完整 batch/session/role/timestamp 和输入顺序；分块定位包含 source_event_id、external_batch_id、message_index、source_path、消息内 UTF-8 byte_start/end 及 parser 规格。同 session 的多批次不会仅由局部 message_index 区分。原始批次不伪造 SessionTurn/Feedback。

派生能力 root key 是受信服务端凭据，可以访问 root 下全部所属 workspace。调用方负责核对终端用户身份并选择获授权的目标 workspace。root key 仅供受信服务使用。

## 显式记忆语义

新数据库提供以下入口；旧数据库不自动安装记忆语义表。所有 scope 来自认证，写操作需要 Idempotency-Key。

- POST /admin/memory-properties：仅 admin，注册不可变属性，例如 {"predicate":"database.engine","cardinality":"single","required_context":["env"]}。cardinality 为 single 或 multi；required_context 最多 32 个合法条件名。相同定义可重放；改变定义或接管已有显式身份的属性返回 409。
- GET /v1/memory-properties/{predicate}：reader 可读取本 workspace 的已登记定义，未登记返回 404。
- POST /v1/memories/semantic：writer 及以上；重复事实同步返回 `{state:"duplicate",asset_id,version,source_event_id}`，保存本次来源但不新建版本或任务，不含 job_id。需要发布时返回 `{state:"accepted",asset_id,source_event_id,job_id,semantic_status,normalization_status}`，由现有单 Worker 发布，只有任务 completed 后才能宣称新版本已发布。已登记属性不能经 /identified 或 /captures/identified 写入（409）。未登记的旧显式身份入口和旧 fact_key 语义保持原样。

语义请求示例：

~~~json
{
  "identity": {"subject":{"kind":"service","stable_id":"Atlas"},"predicate":"database.engine","context":{"env":"production"}},
  "content": "Atlas production uses PostgreSQL.",
  "operation": "assert",
  "expected_version": 0,
  "statements": [{"value":"postgresql","quote":"Atlas production uses PostgreSQL.","byte_start":0,"byte_end":33}]
}
~~~

content 是不可变原始来源；quote 必须逐字匹配 UTF-8 字节半开区间。最多 32 条 statement/集合成员，value 为 1–256 字节且无边界空白/控制字符，quote 最多 8192 字节；content 继续受既有文本限制。值以精确字符串区分，不自动折叠同义词或 Unicode。identity 的完整稳定主体、属性及条件共同定位资产；必需条件缺失或属性未登记返回 422。

| operation | 行为 |
| --- | --- |
| assert | 首次建立事实；相同值返回 duplicate 且不创建版本/任务；单值与当前值相反时发布独立 conflict 证据。多值已有集合只接受其子集作为重复，新增值须用 supplement |
| update | 只用于单值，显式更新并追加版本 |
| supplement | 只用于多值，按精确值取并集，保留旧成员的原始来源与定位 |
| negate / propose / ambiguous | 发布独立、未归一化证据；不绑定或覆盖当前身份 |

单值请求含多个不同值时标为 conflict，不选择最后一条。省略 identity 同样保留为 ambiguous（negate/propose 保留具体状态），expected_version 必须为 0。有 identity 的不确定输入只把身份作为建议，仍检查已登记属性/必需条件，但 expected_version 不用于覆盖，因为没有目标事实更新。接口不推断别名，不增加审核队列；操作与 value 是调用方的显式分类，逐字 quote 不证明语义真假。

确定事实必须提供非负 expected_version（0 表示尚未发布；小于 i32::MAX），接收和发布阶段都检查；过期请求返回 409，受理后竞争失败的任务为 superseded。同 key 同请求返回原响应；异请求返回 409。重复事实仍保存原始来源和审计，但不会把新输入追加为已有版本的额外支持。

不确定证据在 search/resolve/get 标为 normalization_status=unnormalized，locator.semantic_status 为 proposed/negated/ambiguous/conflict；建议身份仅在 locator.suggested_identity 中，不被当作绑定身份。确定事实保留 explicit_identity 标记。精确 memory_identity 检索不会包含建议身份证据。

语义 locator 使用 parser=memory-semantics:v1，包含 semantic_value、原始 source_event_id/source_version、byte_basis=source_utf8、byte_start/end 和完整 support_sources。SearchHit.source_event_id 指向该 quote 的原始事件；hit.version 是当前资产快照版本，原始来源版本看 locator.source_version。get/lookup 的 semantic_values 列出各成员的来源，不把快照 content 的拼接偏移当作原文偏移。

多值采用整快照有效性：任一支持来源或本次操作来源撤回，整个快照立即退出 search/resolve/get/lookup，不回退历史。后续显式 supplement 可从仍有效的成员重建新版本；这种修复即使值重复也会产生发布任务。若撤回的是当前快照自身的发布来源，既有清理会移除该版 chunk；后续写入必须重新提供要保留的事实，不从历史自动补回。通用 restore 禁止恢复语义资产，应提交新的语义写入。旧版本原文与定位不改写。此规则偏保守，不代表逐成员独立发布或自动恢复。

## 对话语义捕获

新库可调用 `POST /v1/captures/semantic`，或公开库 `Service::semantic_capture`。writer 及以上权限，必须配置 extraction model；只进行显式请求驱动的捕获，不后台监听会话。调用方提供保留说话人和适用条件的原始文本，以及允许匹配的完整身份列表；稳定主体 ID、属性目录和授权不能由原文或模型创建。

```json
{
  "content": "用户：Atlas 在 production 使用 PostgreSQL。",
  "targets": [{
    "identity": {
      "subject": {"kind": "service", "stable_id": "Atlas"},
      "predicate": "database.engine",
      "context": {"env": "production"}
    },
    "expected_version": 0,
    "allow_update": false
  }]
}
```

首先由 admin 注册相应属性目录。每个 target 的属性必须已注册且包含所有必需条件；expected_version 必填，0 表示无已发布版本。受理时检查所有目标的版本，任一不匹配即返回 409，且不创建来源或任务。相同身份不能重复列入 targets。allow_update 默认 false，只能为单值属性打开；即便打开，也只在模型分类为明确 update 时生效。模型把不同值分类为 assert 时，仍保留为 conflict 证据。

受理后不持续锁定整个 targets 列表：进入记忆语义的确定性候选在路由时核对所用目标版本；计划保存后，Worker 在重试/恢复及最终提交前复核实际路由资产的版本、来源与可见性，包含 duplicate 结果是否仍为有效当前事实。未被候选使用的 target，以及未归一化证据仅作为 suggested_identity 保留的目标，不作为最终提交的身份版本前置条件。否定、提议、歧义和未获 allow_update 授权而降级的 update 不更新建议身份；确定性 assert 路由为 conflict 前仍检查目标版本，之后独立发布 conflict 证据。实际确定事实版本竞争失败时，整个任务 superseded，不部分发布。

content 上限 64 KiB UTF-8，最多 20 个 targets；空 targets 合法，但所有结果只能是未归一化证据。模型输出最多 20 个候选、总计 32 个 statements，每个非空目标最多一个候选（冲突 statements 合并在同一候选中）。引用上限 8192 字节、value 上限 256 字节、集合最终上限 32 成员。模型只能返回目标索引/null、operation 和带精确原文区间的 statements；未知字段、重复目标、非法区间或超限导致整批失败，不部分发布。调用参数错误返回 422，版本/幂等冲突返回 409，模型未配置返回 503；受理后的模型失败记入 job，不在错误响应或日志中输出原文/模型诊断。

模型分类为否定、提议、歧义或无法匹配目标时，使用记忆语义的未归一化路由。缺少 allow_update 授权的 update 降为 ambiguous，保留 quote，不能覆盖当前值。multi 用 supplement 追加；single 的 supplement 等不兼容操作使任务失败。重复值不增加版本，但保存本次输入来源。模型提取只是在调用方允许范围内进行判断；精确引用、身份限制和版本检查不证明模型的语义判断正确。

受理返回 `{state:"accepted",source_event_id,job_id}`。任务完成时 `result.memories` 按模型候选顺序返回 asset_id、version、target、requested_operation、state（published/duplicate）、semantic_status、identity 和 normalization_status。空候选返回空数组；全部重复也可 completed，不代表产生了新版本。lookup/get/search/resolve 继续使用既有接口，搜索身份过滤字段为 `memory_identity`。

locator 沿用记忆语义原始来源与字节范围，自动提取的新成员附加 `extraction:{parser:"semantic-capture:v1",model:"配置的模型名称"}`；集合中的旧成员保留自己的来源版本及 extraction，不标成新模型/新输入。来源撤回沿用记忆语义整快照隐藏、无自动历史回退。检索跨调用方/跨会话可复用同 workspace 已发布记忆，但本接口不保存结构化 SessionTurn/Feedback，也不是 Improve/Learning 经验蒸馏接口。

## 会话归档与幂等反馈

SessionTurn 是不可变的问答档案，由调用方在回答完成后明确提交。`state:"archived"` 表示归档已提交，`state:"recorded"` 表示反馈已提交。学习需要显式调用下文的 [Improve](#显式-improve--learning)。

归档轮次和反馈的读写均要求 writer 或更高权限，并且 `session_id` 必须是认证逻辑 key 的绑定会话。tenant、workspace 与逻辑 key 均来自认证，读写和重试都检查凭据是否仍然有效。session_id、turn_id、feedback_id 与证据 UUID 必须非 nil。POST 要求 `Idempotency-Key`。未知字段、缺少必要字段、未知枚举均拒绝。

`POST /v1/sessions/{session_id}/turns` 示例：

```json
{
  "turn_id": "11111111-1111-4111-8111-111111111111",
  "question": "Atlas 使用哪个数据库？",
  "answer": "使用 SQLite。",
  "used_evidence": [{
    "kind": "chunk",
    "id": "22222222-2222-4222-8222-222222222222",
    "asset_id": "33333333-3333-4333-8333-333333333333",
    "asset_version": 2,
    "chunk_id": "22222222-2222-4222-8222-222222222222"
  }]
}
```

- question、answer 均须非空白，各不超过 65,536 UTF-8 字节。used_evidence 必填，显式 `[]` 表示未使用证据；缺省不是空数组。最多 64 条，不接受完全相同的重复引用。只提交实际用于回答的子集。
- 引用 kind 为 chunk/entity/relation。chunk 的 id 必须等于 chunk_id；图引用的 id 是实体/关系 ID，chunk_id 由调用方选择为该次实际使用的支持片段。asset_version 是正整数的精确历史资产版本，不是当前版本别名。
- 服务端核验片段、资产版本、根来源、所有记忆语义快照支持来源与所属 scope。图引用还需匹配 kind/id/根来源/版本的 owner 和 committed graph ledger。当前图所有权是来源/版本级；所选片段必须属于同一支持来源/版本，本接口不声称已有逐片段图抽取对齐证明。
- 支持仍有效的旧版本在更新到新版本后仍可归档，不要求等于 current_version。归档保存完整原 locator（含 parser、字节范围、extraction 与记忆语义support_sources），不重新解释历史偏移。
- GET 的 used_evidence 元素为 `{evidence_ref_id,reference,source_event_id,source_version,version_source_event_id,locator}`。reference 为上述调用方引用，evidence_ref_id 由服务端首次归档分配并持久化。source_event_id/source_version 是记忆语义成员的原始来源；version_source_event_id 是该资产快照根来源，reference.asset_version 是实际使用的资产版本，三者不能互相替代。

归档成功 HTTP 200 返回 `{session_id,turn_id,source_event_id,state:"archived"}`。唯一领域键为 tenant/workspace/session_id/turn_id。在同一个获授权的逻辑 key 下，相同正文和有序 `used_evidence` 返回原标识，更换 `Idempotency-Key` 或获准轮换凭据仍可重试。改变内容返回 409，原归档保留。SQLite 事务原子保存问答来源事件、档案、实际引用、审计与命令回执，任一引用失败则回滚。

反馈请求示例（针对原轮次或 GET 返回的确切归档引用）：

```json
{"feedback_id":"44444444-4444-4444-8444-444444444444","target":{"type":"turn"},"rating":"helpful","comment":"回答解决了当前问题"}
```

```json
{"feedback_id":"55555555-5555-4555-8555-555555555555","target":{"type":"evidence","evidence_ref_id":"66666666-6666-4666-8666-666666666666"},"rating":"unhelpful"}
```

rating 只接受 helpful/unhelpful/neutral，表示调用方判断，不推断事实真伪。comment 可省略或 null，最多 4096 UTF-8 字节。引用目标必须属于路径中的已归档轮次，不能反馈未使用的检索命中、其他轮次或其他 scope。每轮最多 256 条不可变反馈；已达到上限时新记录返回 409，完全相同的重放仍可成功，不静默截断。领域唯一键为 tenant/workspace/principal/feedback_id；同键同语义请求复用结果，改变轮次/目标/评分/评论返回 409。反馈不覆盖、修订或删除已有评分，也不直接影响排序与权重。POST 返回 `{session_id,turn_id,feedback_id,principal_id,feedback_sequence,state:"recorded"}`，GET 返回 `{session_id,turn_id,feedback:[...]}`；反馈元素包含原请求及 created_by/created_at，轮次 GET 同样含 feedback。

治理采用保守的整轮次失效：可通过既有 admin `DELETE /v1/events/{source_event_id}` 撤回问答；任何实际引用的资产删除、根来源撤回、记忆语义 任一支持来源撤回或文件删除，同样立即阻断整轮问答与全部反馈读取和新增反馈（404）。原始 archive source 的 GET 也执行依赖检查，不能绕过此规则。清理任务尚未运行时规则即生效；历史档案不依赖可被清理的 chunk 外键。读与重放均重新校验有效性，命令缓存只存标识/状态，已失效档案不会因同键重放、换键或重开进程恢复可见。后续学习只能使用通过同一有效性门槛的档案；显式学习入口见下节 Improve → Learning。逻辑撤回保留原始持久化记录，不等于物理擦除。

## 显式 Improve → Learning

仅新建数据库；现有 extraction model 配置提供提议与独立核验，不增加 provider、自动触发、排序改权或身份覆盖。调用方显式发起：

- `POST /v1/sessions/{session_id}/improve`（writer+，Idempotency-Key）：`{improve_id,conditions:["仅在 staging 发布时"],max_feedback:32}`。improve_id 非 nil 且正文不可变；conditions 必填，1–8 个互异非空项，每项最多 256 UTF-8 字节；max_feedback 默认 32，范围 1–32。
- `GET /v1/improvements/{improve_id}`（writer+）：返回状态、父任务、冻结水位、数量、提议/核验保存状态、Learning 子任务及安全错误码，不返回冻结问答或模型原始响应。依赖撤回后仍可读取这些管理信息。
- `GET /v1/learnings/{learning_id}`（仅原逻辑 key）：返回仍有效的已发布 Learning 详情，包含公开声明、条件及私有支持指针，不含原始问答 quote。普通 workspace 成员通过 Search/Bundle 复用公开正文与条件。

反馈使用 workspace 内事务分配的单调 feedback_sequence，身份仍是 tenant/workspace/principal/feedback_id。时间戳相同或回退不影响批次。每次冻结最多 32 条反馈、128 KiB JSON 输入；首条过大报错，不消费水位。提议与核验的每次完整序列化模型输入也限制为 128 KiB；核验加入候选后若超界，在发出请求前失败且不消费水位，可取消后缩小批次重试。冻结时已失效的轮次只保存跳过标识，不保留隐藏正文。同一 session 最多一个 accepted/publishing run；重放相同命令或 improve_id 复用领域结果，改变请求返回 409。

Worker 先提议、持久化，再独立核验、持久化，最后一次事务登记知识资产、Learning 支持及普通 Ingest 子任务。最多 8 个候选，statement/why 分别不超过 8192/4096 UTF-8 字节，每个候选 1–32 条精确支持，每条 quote 最多 4096 UTF-8 字节。支持绑定冻结批次内的 turn_id、principal_id、feedback_id、question/answer 字段与 UTF-8 quote 范围；模型 conditions 必须与调用方逐项完全相同。核验 reason 只接受有限安全代码。helpful 不证明事实，unhelpful 不授权否定或覆盖记忆。

所有 accepted 子任务发布完成后，才原子推进完成水位；空候选、拒绝与精确重复均有明确结果。精确 novelty 只比较 statement 与 conditions，不声称语义级去重。跨 session 并发或已保存核验的重放，还在登记事务中检查有效的 accepted/published 预留；冲突时整笔登记回滚、父任务为 superseded、水位不前移。显式 cancel 该未完成 run 后用新的 improve_id 再发起即可，不能通过 generic retry 绕开预留。每个 Learning 独立原子发布，多个 Learning 不承诺同时可见。失败子任务保持原 asset/source/job 标识，按现有 `/v1/jobs/{id}/retry` 重试；持久化的阶段结果复用。远端模型响应返回与本地提交之间若进程崩溃，调用可能重试，不承诺模型网络调用 exactly-once。

Improve 的父模型任务完成后，run 可能仍为 publishing。此时对父任务调用既有 cancel 会取消未完成子任务并释放 run，水位不前移；已发布 Learning 保留有效性与可见性。取消的 run/子任务不能 retry；以新的 improve_id 再次显式发起，精确已发布重复不再创建新资产。已完成 run 不能取消。来源永久失效时可采用此取消/新请求路径，不因临时失败静默消费反馈。

Learning 是独立 knowledge，不是 memory identity。每个 chunk 的 locator.learning 保存 conditions 和支持标识，resolve 将条件与片段一起渲染并计入预算；当前只为 Learning 生成 keyword/vector，不生成可能丢失适用条件的 summary/graph，其他知识的配置不变。普通 knowledge 更新与 restore 不能重写 Learning。

所有支持轮次必须整体有效，包括未被候选引用的实际证据、精确历史版本、根来源及记忆语义全部原始成员来源。提议/计划重放/索引/最终提交及 search、get、resolve、graph、原始来源读取均重新检查；候选预算在失效过滤后使用。Learning 支持另一个 Learning 的轮次时进行有界传递校验，最多 64 个不同 Learning/轮次节点，循环或超界失败而非截断接受。撤回立即隐藏，先于异步原生清理。新版本/restore 不会复活历史失效支持。

此闭环的确定性夹具只能证明工程流程；真实模型核验准确率、事实质量与 Agent 收益仍待独立验收。统一上下文见下节 EvidenceBundle；真实 Agent 对照仍需单独验收。

## EvidenceBundle：统一上下文

`POST /v1/context` 是只读请求，无幂等键；对应 MCP `context_bundle`，与
`Service::evidence_bundle` 共用 `EvidenceInput` / `EvidenceBundle`。原有 search、
resolve 合约不变。最小输入为 `{"query":"Atlas"}`。

可选字段：`mode` 为 keyword（默认）/vector/hybrid，`allow_partial` 默认 false，
`budget` 默认 4000、范围 0–32000（实际 UTF-8 字节，**不是 token**）；`routes`
为唯一的路由配置列表，每项 `route` 为 memory/knowledge/learning/session，
`limit` 默认 20、范围 1–100，`max_bytes` 默认 4000、范围 0–32000。
默认启用前三项，session 默认关闭。未知字段、重复路由、非法预算返回输入错误。

`memory_identity` 只限制 memory，使用精确主体/属性/条件；不改写知识或 Learning
范围。`applicability_context` 为最多 32 个非空、每项最多 256 字节的调用方条件
字符串。Learning 的每个条件与其逐字精确匹配才标记 `caller_context_exact_match`，
否则保留并标记 `unverified`；两者都不代表系统验证了真实适用性。

启用 session 必须 writer 以上，否则 403；该权限在任何模型调用前校验。
`session_id` / `exclude_session_id` 可限制会话，必须同时启用 session。会话路由
扫描最多 512 条近期关系型记录，完整来源/传递证据有效性先于保留条数限制，
返回 `scanned` / `scan_exhausted`；扫描耗尽明确提示可能存在未扫描的更早记录。
这不是全历史语义检索，不能把读者权限扩大为读取原始问答。

返回 `rendered_context`、`items`、`conflicts`、`warnings`、scope、policy、
requested/effective mode、总预算/实际字节及各路由候选/保留/省略计数。
每个 item 保留类型、检索分支、条件/适用状态、语义身份、得分种类和稳定引用。
`provenance.type=asset_chunk` 明确区分 asset_version、原始 source_version、
version_root_source_event_id；`session_turn` 保留真实 session_id/turn_id/归档来源
与历史 used_evidence，不伪造 chunk。来源及转引完整可见性在预算前和最终渲染
快照中核验，撤回不等待物理清理。有效性仅表示通过该读取快照的可见性检查。

仅对明确单值身份、同条件下的 confirmed/conflict 值做确定性冲突分组；组内所有
候选与警告整体保留或整体省略。普通文本、不同条件及多值集合不推断互斥。
冲突检查仅覆盖召回候选，不能据此声称跨文档事实无冲突。只对同一精确证据
位置去重，合并 keyword/vector 路径，不合并相似文字。

所有标题、引用、条件、来源、冲突警告和分隔符都计入实际 UTF-8 上下文预算；
整块保留，不截断中文/emoji，不保留孤立引用。三个资产路由共享一次 embedding，
各自独立候选过滤/预算。此版只用原文 chunk，不引入 summary/graph 路径。
稳定轮询与各路由上限是中性工程默认，不代表已验证的最佳质量权重。

**归档限制**：资产 item 可转换为原有 chunk used_evidence。session_turn 引用
目前不能作为 `SessionTurn.used_evidence`（该合约只支持 chunk/entity/relation）；
响应与会话正文标记 `unsupported_session_turn_reference`，调用方不得伪造 chunk
或声称此类引用已完整归档。Learning 发布给 reader 的引用只有支持标识/范围，
不返回支撑轮次的原始问答或 quote。

详见 [EvidenceBundle 契约](contracts/evidence-bundle.md)。真实 Agent
对照需独立执行；fixture 仅验证工具/模型交互框架，不是任务收益结果。

### 资产固定版本的已记录 Run 引用

`GET /v1/assets/{asset_id}/runs?version=1&limit=25&after={run_uuid}`：`version` 必填且为正数；
`limit` 为 1–100，默认 25；`after` 可省略，按 Run UUID 稳定翻页。仅当前 logical key，
管理员没有他人的 Run 读取特权。返回 `asset_id`、`asset_version`、`items`（仅 `run_id`、
`created_at_ms`）、`next_after`、`scope: current_logical_key`、`coverage: recorded_references`。
空页仍可能有下一游标；源或资产不可读时拒绝访问。索引定位首条已归档引用并复核权威事件，
不可读的关联保守省略，不承诺完整采集或整个 Run 的其他引用可读。
详见 [Run 合同](contracts/runs.md#exact-asset-version-recorded-associations)。
