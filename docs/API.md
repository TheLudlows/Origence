# Origence HTTP API v1

适用 M5 本地宿主 SQLite/LanceDB/SQLite 图。参见 [文档索引](README.md) 和 [运维说明](OPERATIONS.md)。

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
| `POST /v1/memories/lookup` | 请求体为完整 `{subject,predicate,context:{}}`，reader 可读；返回与资产 GET 相同的当前已发布视图；不需要 Idempotency-Key，不创建资产 |
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

search/resolve 可选 `components:{summaries:true,graph:true}`，两个字段默认均为 true，仅 hybrid 且未限定 memory_identity 时启用。可独立关闭摘要、图分支做同库消融，原文 keyword/vector 分支始终保留；省略 components 保持旧行为。响应 `active_components` 表示实际启用的分支（不代表已有产物或命中）。resolve 沿用相同开关和现有来源/字节预算检查；HTTP 与 MCP 共用此契约。

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

`POST /v1/memories/lookup` 用完整身份精确定位当前记忆，请求体就是上述示例的 `identity` 对象，不含 content。scope 从 key 获取；不接受主体别名或语义近似匹配。未命中、尚无发布版本、资产已删除或当前来源已撤回均为 404，不回退旧版本，也不分配占位资产、来源或任务。缺失身份表仍返回 503。返回的 `version` 是读取时的当前版本，不保证随后异步写入一定成功；更新结果仍以任务的并发版本检查为准。reader 只能读取当前发布内容，不能借此读取原始 capture 输入。

新建库包含 `oc_memory_identities` 表。旧库可以继续使用既有接口；缺失身份表时新入口返回 503。启动不会自动升级旧库；可停宿主后执行 `--offline memory-identity-upgrade` 显式安装身份表，详见 [运维说明](OPERATIONS.md#显式启用记忆身份)。旧 `/v1/captures` 保持 fact_key 模型；显式身份写入/capture 使用 HTTP。MCP 是只读工具，其 search/resolve Schema 已支持 memory_identity；CLI 检索仍默认不带过滤。

## 单身份 capture

`/v1/captures/identified` 接受与显式记忆写入相同的 `{identity,content}`；content 是待抽取原文。身份完全来自调用方，scope 来自 key，模型不能指定主体、属性、条件或 workspace。受理时记录当前 expected_version，抽取期间发生其他发布会沿现有版本治理 supersede，不能覆盖较新版本。未配置抽取模型或旧库未安装身份表时返回 503。

模型输出必须提供原文精确 quote 和 UTF-8 byte_start/byte_end，服务器校验边界和内容一致；发布正文就是该原文片段，search locator 的 source_span=true 标明区间指向来源原文。重复相同 quote 合并；多个不同 quote 视为单身份歧义，任务 failed/INVALID_ARGUMENT，不更新版本。零片段完成且 result.memories=[]，受理时预留的资产可能仍无已发布版本。来源事件保留原始输入。

此切片不做自动主体推断、别名归一化、任意语义归并或多值属性合并。原文区间验证证明可追溯性，不能证明模型的语义选择或事实真假；否定/提议识别仍依赖模型，需真实评估。原有 capture 接口保持旧 fact_key 模型。

## 身份状态与来源核验

资产 get 增加 normalization_status：记忆有显式身份绑定时为 explicit_identity；旧 fact_key 记忆为 legacy_unidentified；知识资产为 not_applicable。该状态只描述身份绑定方式，不代表事实可信度或模型语义验证；历史版本沿用同一资产身份。旧记忆不会因该字段被自动归一化。

writer 可以用来源标识访问 GET /v1/events/{id}，将 search locator 的字节区间与原始输入对照；这也支持核验失败或零结果 capture 的来源。原文可能包含未发布内容，因此 reader 无权访问。接口不返回撤回来源、已删文件来源或其他 workspace 内容；逻辑删除仍保留底层原文，不提供物理擦除。未归一化资产的独立状态和自动身份归并仍待后续实现。

## 检索与上下文身份标注

search 的每个 hit 增加 identity（无绑定为 null）与 normalization_status（explicit_identity/legacy_unidentified/not_applicable），沿用资产读取含义；完整身份取自 SQLite 资产绑定，不接受模型或向量库宣称的身份。返回前仍复核当前版本、scope 与来源。

resolve 的 chunk 引用增加 kind、identity 与 normalization_status，rendered_context 在每块正文前附相同的 JSON 标注，context_policy 为 identity-provenance-v1。引用、标题、标注和正文全部计入既有 UTF-8 字节预算；整块保留或省略，不截断身份/引用。新增标注可能减少相同预算下的正文数量。图引用保留既有格式。状态表示身份绑定方式，不是事实真值、业务有效期或自动匹配置信度；当前尚未实现冲突识别与类型预算分配。

## 精确身份检索过滤

search/resolve 可选 memory_identity，格式为完整 `{subject,predicate,context:{}}`，省略时沿用混合检索。启用后只保留当前 scope 内完整身份精确相同的 memory；排除旧未识别记忆、知识块和图扩展。字段在模型调用前校验，不接受别名/语义近似/部分条件匹配；未知字段拒绝，非法身份 422，未命中为空结果。身份不替代鉴权，结果仍受当前版本、来源和墓碑规则约束。旧库缺身份表不会自动升级，没有显式绑定则不匹配。

新实现先在已授权范围按完整身份定位资产，keyword 与 vector 候选仅对该资产生成，再进行各分支排序与 limit 截断，最终仍重新校验当前版本、来源、墓碑和权限。全局无身份查询仍保留各分支的候选数量限制；身份限定不等于语义召回保证。只定位当前资产且无检索需求时使用 memories/lookup。响应回显 memory_identity；resolve 将过滤条件透传给 search。HTTP/MCP 接受该字段，CLI 仍采用无过滤的默认行为。

## 显式写入的调用方版本前置条件

memories/identified 和 captures/identified 可选 expected_version（非负整数）。0 表示当前尚无已发布版本（可能已有未发布占位资产）；正数必须与受理事务中当前版本相同。不匹配返回 409，事务回滚且不新建来源或任务；负数 422。省略或 null 保持按受理时版本追加的旧行为，不是“必须创建”。

可以将 lookup 的 version 传入写入接口，覆盖读取与受理之间的并发窗口。受理后 Worker 仍用受理版本复核，版本变化使任务失败，不保证同步发布成功。相同幂等键/相同完整请求的成功重放先返回原受理结果，不因之后版本变化变成冲突；改变 expected_version 属于改变请求，返回幂等冲突。省略/null 不进入规范请求 payload，保留旧键的重放兼容。


## AML 用户 scope 库接口

AML 提供 Rust 库接口及下述 HTTP 适配。专用评测新库中，宿主先通过 `Service::auth(token)` 获取原生 workspace 的授权，再由该 workspace 的管理员显式调用 `enable_aml_namespace(&auth)`。普通 workspace 默认没有 AML scope 创建能力。

- `ensure_aml_user(&auth, user_id)`：需当前 writer/reviewer/admin 权限，原子创建或复用用户 workspace，返回供现有 Service 方法使用的 AuthContext。
- `lookup_aml_user(&auth, user_id)`：只读查询，未知用户返回 None，不创建 workspace，不回退到 namespace workspace。Search 适配器将 None 映射为空结果。
- 映射键为 `(tenant_id, namespace_workspace_id, 完整 user_id)`；同一 namespace 下的凭据共享映射，不同 namespace 或 tenant 的相同 user_id 独立。user_id 按 UTF-8 原样保留，区分大小写、空格及 Unicode 编码形式，不拆分冒号或去除运行前缀；接受 1–4096 UTF-8 字节，拒绝 NUL，不截断。
- 派生授权沿用原始 key ID，每次事务和 Worker 发布仍核验该 key 的当前状态；权限最高为 writer，不能在用户 scope 内发 key、启用 namespace 或授予管理权限。reader 可读取已存在映射，不能创建。
- 调用方必须通过认证取得 namespace 授权，不接收外部指定的内部 workspace。session_id 不参与用户 scope 映射，Add 负责保存会话来源。

### AML HTTP 协议

所有入口使用 `Authorization: Bearer <token>`；Add 以请求体的 request_id 幂等，不要求 Idempotency-Key。

| 方法和路径 | 请求及响应 |
| --- | --- |
| `POST /admin/aml/namespace` | 原生 workspace 的 admin 显式启用，响应 `{"enabled":true}`；重复启用无新增映射 |
| `POST /aml/add` | `{request_id,user_id,session_id,messages:[{role:"user"或"assistant",content:"原文",timestamp:可选Unix毫秒整数}]}` |
| `POST /aml/search` | `{query,user_id,top_k,options:可选字符串数组}` → `{data:[{id,content}]}` |

Add 创建一个不可变知识资产、来源事件和持久 aml_ingest 任务。完成发布及索引后才返回 HTTP 200、`{success:true,request_id,user_id,session_id}`，三个 ID 原样回传；不返回 202。同 namespace、user_id、request_id 的相同规范请求复用原任务，不因 key 轮换重复入库；不同正文、role、timestamp、顺序或 session 返回 409。省略 timestamp 和 null 等价。不同 request_id 保存独立历史来源，不覆盖同一用户的旧会话。

来源保存规范 JSON，保留解码后的原始 content 字符串、role、消息顺序、session 与可选 timestamp；不保留 HTTP JSON 的空白/转义拼写。接收时间使用事件 created_at，不代替缺失的事件时间。解析器 `aml-message-ranges-v1` 每条消息独立按最多 2400 UTF-8 字节分块；locator 的 source_path 是来源 JSON 内的消息 content 路径，byte_start/end 相对于该字符串，不能用于拼接资产或 JSON 文本。message_index 从 0 开始，message_count 和 session 保留相邻消息关系。

Search 显式使用原文 vector、关闭摘要/图、不允许关键词降级；options 接受但不影响查询或写入，不生成答案。data 使用稳定 chunk UUID，content 是角色/时间/session/message_index 的 JSON 来源标注加原文片段，按现有向量相关性及 ID 决定顺序，至多 top_k 条；未知用户空 data 且不创建映射。没有摘要、语义去重、邻接补全或 reranker。标注与正文都是外部证据，不是可执行指令。

输入边界：请求体最多 10 MiB；三个 ID 各 1–4096 UTF-8 字节且无 NUL；每批 1–256 条非空文本消息，合计最多 1,000,000 UTF-8 字节；query 非空、无 NUL、最多 4000 UTF-8 字节；top_k 为 1–100。不支持图片、数组 content、system/tool role 或未知字段，返回脱敏 422；超过 HTTP body 上限由框架返回 413，不截断后发布。解析边界会再次校验整批上限。

HTTP 最多等待任务 25 分钟。超时返回 503，原任务继续；断线/重试不会创建独立 Worker 或重试循环。同 profile 的已完成重放还会检查来源、当前版本、ready 索引与 committed 账本；来源已撤回或证据已不可检索返回 409。模型未配置/调用失败为 503，既有批次的模型 profile 改变为 409，不能通过换配置重用旧 Add 成功结果。失败任务的重试方式与运行限制见 OPERATIONS。

库宿主可调用 `submit_aml_add` 获得持久 receipt，随后 `wait_aml_add`（最长 25 分钟）观察原任务，或调用组合的 `aml_add`；receipt 不是 HTTP 成功响应。来源、asset 和 job 可通过映射后的 AuthContext 使用既有 Service 库接口核验，外部调用不能指定内部 scope。
