# HTTP API v1

适用 M5 本地宿主 SQLite/LanceDB/Kuzu。参见 [文档索引](README.md) 和 [运维说明](OPERATIONS.md)。

所有 `/v1/*` 请求需 `Authorization: Bearer TOKEN`。workspace 来自 key，客户端不能指定 workspace。修改接口还需 1–200 个 ASCII 字符的 `Idempotency-Key`；幂等范围为 workspace + key 主体 + 操作/目标 + 幂等键。同请求复用结果，同键不同内容返回 409。幂等响应只包含标识/状态，删除后重放不会再次写入或返回已删正文。

## 角色

| 角色 | 权限 |
| --- | --- |
| reader | 已发布内容 get/search/resolve |
| writer | reader + 上传/原文件下载/知识入库/结构化记忆发布/capture/任务查询；取消、重试自己任务 |
| reviewer | writer + 版本恢复/操作其他主体任务 |
| admin | reviewer + 资产删除/来源撤回/文件删除/失败清理任务重试 |

原始文件和任务可能涉及未发布数据，reader 不能读取。记忆与 capture 由 writer 授权直接发布，发布任务提交时重新核验权限与来源。

## 接口

| 方法和路径 | 请求 / 结果 |
| --- | --- |
| `GET /health/live` | 进程存活，无鉴权 |
| `GET /health/ready` | 单 Worker 正在运行且本地存储检查通过；不是外部模型健康检查 |
| `POST /v1/memories` | `{fact_key,content}`（`publish_if_authorized` 已废弃，兼容接受但无作用）→ asset/source/job 标识，`state:"accepted"`，`conflict` 表示追加已有事实 |
| `POST /v1/memories/identified` | `{identity:{subject:{kind,stable_id},predicate,context:{}},content}` → asset/source/job；同 scope 下相同身份复用资产；`conflict` 表示已有版本 |
| `POST /v1/captures` | `{content}` → source/job；抽取结果逐条直接发布，结果见任务 `result.memories` |
| `POST /v1/captures/identified` | `{identity,content}` → asset/source/job；完整身份由调用方指定，模型只抽取原文片段；需要 extraction model |
| `POST /v1/knowledge` | `{title,content或file_id,format:"text"或"markdown"或"pdf",asset_id:null或UUID,expected_version:null或整数}` |
| `POST /v1/files?name=...&format=...` | 原始二进制请求体，非 multipart；返回 file_id；文件名仅作元数据 |
| `GET /v1/files/{id}` | 校验 hash 后返回 attachment/octet-stream，存储路径由 scope 和内容 hash 生成 |
| `GET /v1/assets/{id}?version=N` | 已发布当前/历史版本，含 content/hash/title/source/restored_from |
| `POST /v1/assets/{id}/restore` | `{target_version,expected_version,reason}` → 恢复 job；完成后新增版本 |
| `DELETE /v1/assets/{id}` | 永久逻辑墓碑，阻断包括历史版本在内的读取 |
| `DELETE /v1/events/{id}` | 撤回来源，阻断所有引用该来源的版本，不自动回退到旧版本 |
| `DELETE /v1/files/{id}` | 阻断原文件，撤回所有引用该文件的事件 |
| `GET /v1/jobs/{id}` | operation/state/generation/outcome/result/error_code |
| `POST /v1/jobs/{id}/cancel` | 标记 cancelled，增加 generation/run_token，旧执行不能提交；已完成任务不能取消，cleanup 不能取消 |
| `POST /v1/jobs/{id}/retry` | 仅 failed/cancelled 且来源、资产仍有效；新 generation 原子入队；cleanup 重试需 admin |
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

memory 的更新按 `expected_version` 乐观校验：受理时记录资产当前版本，Worker 提交前复核；期间发生其他发布则任务变为 `superseded`，旧值保留，不发生覆盖。此时重新读取现状并再次提交。发布计划与账本保证重放不产生重复版本。

## 任务与读取可见性

`pending → processing → completed/failed/superseded`；短暂存储故障为 `processing → retry_wait → processing`；取消为 `cancelled`。只有 completed + outcome=published 表示该次版本及索引已原子提交；capture 抽取的 result.memories 列出每条事实的 asset/version。之后仍可能被更新、删除或来源撤回。

删除响应为 `{id,blocked:true,cleanup_job_id,originals_retained:true}`。逻辑删除事务完成后新读取被阻断，索引清理是否完成不影响这一规则。已经发出的数据无法收回；在删除前已开始的请求可能先完成。

所有业务写事务以 SQLite `BEGIN IMMEDIATE` 开始，原子提交业务、幂等、审计和入队。模型/文件/原生索引 IO 在关系事务外执行。最终提交再次校验权限、墓碑、来源、预期版本、generation 和 run_token；密钥撤销与写事务串行化。SQLite 无 RLS，scope 由适配器显式绑定。

发布计划和 pending 账本先落 SQLite，向量/图写入后，版本、owner、ready 索引及 committed 账本与任务完成一起提交。外部产物不等于已发布证据。短暂存储错误进入 `retry_wait`，最多尝试 5 次；模型/输入失败需显式重试。独占宿主重启时恢复 processing 并提升 run_token，先对账清理再接收请求。

cleanup 的 `completed + outcome=ok` 表示本轮派生数据清理完成；原始事件、版本和文件仍保留，不等于原文物理擦除。

## 检索

返回 hits 含 asset_id/version/chunk_id/source_event_id/locator/score。locator 给出原文 UTF-8 字节区间、行号，PDF 另有页号。只取当前已发布版本，显式历史版本走 get。

向量必须与 profile 一致，不混用不同模型/维度。模型查询失败且 `allow_partial=true` 才退回关键词，同时返回 `effective_mode` 和 warnings；否则 503。纯关键词发布的历史内容没有向量，不会出现在 vector 结果中，hybrid 的关键词分支仍可召回。无 profile 自动补齐、ANN 或 reranker。

resolve 返回 `tokenizer=utf8-bytes-upper-bound-v1`、`count_is_estimate=true`；count 为 rendered_context 的 UTF-8 字节数。预算 0–32000，超出预算的整块被跳过，不生成没有正文的引用。返回文本是外部证据，不能当成系统指令。

关键词使用 Jieba 预分词字段的精确词项匹配（查询词项全部出现），限定当前 scope。hybrid 将 keyword/vector/summary 映射为 chunk 证据后 RRF 融合，并按实体名种子扩展一跳关系；每个图对象包含 `evidence`（asset_id/version/chunk_id/source_event_id/locator）。SQLite owner 和 committed 账本决定图证据是否有效；不会返回已撤回来源的描述投影。

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

新建库包含 `oc_memory_identities` 表。旧库可以继续使用既有接口；缺失身份表时新入口返回 503。启动不会自动升级旧库；可停宿主后执行 `--offline memory-identity-upgrade` 显式安装身份表，详见 [运维说明](OPERATIONS.md#显式启用记忆身份)。capture、CLI/MCP 仍使用既有写入模型。

## 单身份 capture

`/v1/captures/identified` 接受与显式记忆写入相同的 `{identity,content}`；content 是待抽取原文。身份完全来自调用方，scope 来自 key，模型不能指定主体、属性、条件或 workspace。受理时记录当前 expected_version，抽取期间发生其他发布会沿现有版本治理 supersede，不能覆盖较新版本。未配置抽取模型或旧库未安装身份表时返回 503。

模型输出必须提供原文精确 quote 和 UTF-8 byte_start/byte_end，服务器校验边界和内容一致；发布正文就是该原文片段，search locator 的 source_span=true 标明区间指向来源原文。重复相同 quote 合并；多个不同 quote 视为单身份歧义，任务 failed/INVALID_ARGUMENT，不更新版本。零片段完成且 result.memories=[]，受理时预留的资产可能仍无已发布版本。来源事件保留原始输入。

此切片不做自动主体推断、别名归一化、任意语义归并或多值属性合并。原文区间验证证明可追溯性，不能证明模型的语义选择或事实真假；否定/提议识别仍依赖模型，需真实评估。原有 capture 接口保持旧 fact_key 模型。
