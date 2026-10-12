# Origence HTTP API v1

适用本地宿主 SQLite/LanceDB/SQLite 图。本文只描述已实现行为；进度见 [STATUS](STATUS.md)，操作步骤见 [OPERATIONS](OPERATIONS.md)。

按需阅读：[接口表](#接口) · [任务与可见性](#任务与读取可见性) · [检索](#检索) · [显式身份](#显式记忆身份) · [AML](#aml-http-协议)。

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
| `POST /v1/captures/semantic` | `{content,targets:[{identity,expected_version,allow_update:false}]}` → source/job；受限模型分类并复用 S2，详见下节 |
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

更新受理时记录资产当前版本，Worker 提交前复核；期间发生其他发布则任务为 superseded，不覆盖先发布的版本，应重新读取后再提交。发布计划/账本保证重放不重复发布。显式身份入口还支持[调用方版本前置条件](#显式写入的调用方版本前置条件)。

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

lookup 接收上述 identity 对象（无 content），在 key 的 scope 内精确查找，不接受别名/语义近似。未命中、未发布、已删除或来源撤回均为 404，不回退旧版本或创建任何资产/来源/任务；缺身份表为 503。返回 version 不是后续写入锁，最终以 job 的版本检查为准；reader 不能借此读取原始 capture 输入。

新建库含 oc_memory_identities；旧库缺表时原接口可用，新身份入口 503。启动不自动升级，安装步骤见 [OPERATIONS](OPERATIONS.md#显式启用记忆身份)。旧 captures 仍用 fact_key；显式写入/capture 使用 HTTP。只读 MCP 支持 memory_identity 检索，CLI 默认不带该过滤。

## 单身份 capture

`/v1/captures/identified` 接受与显式记忆写入相同的 `{identity,content}`；content 是待抽取原文。身份完全来自调用方，scope 来自 key，模型不能指定主体、属性、条件或 workspace。受理时记录当前 expected_version，抽取期间发生其他发布会沿现有版本治理 supersede，不能覆盖较新版本。未配置抽取模型或旧库未安装身份表时返回 503。

模型输出必须提供原文精确 quote 和 UTF-8 byte_start/byte_end，服务器校验边界和内容一致；发布正文就是该原文片段，search locator 的 source_span=true 标明区间指向来源原文。重复相同 quote 合并；多个不同 quote 视为单身份歧义，任务 failed/INVALID_ARGUMENT，不更新版本。零片段完成且 result.memories=[]，受理时预留的资产可能仍无已发布版本。来源事件保留原始输入。

此切片不做自动主体推断、别名归一化、任意语义归并或多值属性合并。原文区间验证证明可追溯性，不能证明模型的语义选择或事实真假；否定/提议识别仍依赖模型，需真实评估。原有 capture 接口保持旧 fact_key 模型。

## 身份状态与来源核验

资产 get 增加 normalization_status：记忆有显式身份绑定时为 explicit_identity；旧 fact_key 记忆为 legacy_unidentified；知识资产为 not_applicable。该状态只描述身份绑定方式，不代表事实可信度或模型语义验证；历史版本沿用同一资产身份。旧记忆不会因该字段被自动归一化。

writer 可以用来源标识访问 GET /v1/events/{id}，将 search locator 的字节区间与原始输入对照；这也支持核验失败或零结果 capture 的来源。原文可能包含未发布内容，因此 reader 无权访问。接口不返回撤回来源、已删文件来源或其他 workspace 内容；逻辑删除仍保留底层原文，不提供物理擦除。未归一化资产的独立状态和自动身份归并仍待后续实现。

## 检索与上下文身份标注

search 的每个 hit 增加 identity（无绑定为 null）与 normalization_status（explicit_identity/legacy_unidentified/not_applicable），沿用资产读取含义；完整身份取自 SQLite 资产绑定，不接受模型或向量库宣称的身份。返回前仍复核当前版本、scope 与来源。

resolve 的 chunk 引用及正文前 JSON 标注包含 kind、identity、normalization_status，context_policy=identity-provenance-v1；图引用格式不变。引用/标题/标注/正文均计入字节预算，整块保留或省略，因此标注可能减少正文容量。状态只表示身份绑定，不表示真值、有效期或置信度；冲突识别和类型预算未实现。

## 精确身份检索过滤

search/resolve 可选 memory_identity，格式为完整 `{subject,predicate,context:{}}`，省略时沿用混合检索。启用后只保留当前 scope 内完整身份精确相同的 memory；排除旧未识别记忆、知识块和图扩展。字段在模型调用前校验，不接受别名/语义近似/部分条件匹配；未知字段拒绝，非法身份 422，未命中为空结果。身份不替代鉴权，结果仍受当前版本、来源和墓碑规则约束。旧库缺身份表不会自动升级，没有显式绑定则不匹配。

完整身份先定位授权资产，keyword/vector 候选限定于该资产后排序和截断，最终重新校验版本、来源、墓碑及权限。无身份查询仍有各分支候选上限；身份限定不保证语义召回。只需定位当前资产时用 lookup。响应回显 memory_identity，resolve 透传过滤；HTTP/MCP 支持，CLI 默认不带过滤。

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

Search 显式使用原文 vector、关闭摘要/图、不允许关键词降级；options 接受但不影响查询或写入，不生成答案。data 使用稳定 chunk UUID，content 是角色/时间/session/message_index/message_count、parser、source_path、byte_basis 与 byte_start/end 的 JSON 来源标注加原文片段，按现有向量相关性及 ID 决定顺序，至多 top_k 条；未知用户空 data 且不创建映射。没有摘要、语义去重、邻接补全或 reranker。byte_start/end 为原消息 UTF-8 的半开区间；这里只暴露已有 locator，不改变分块、原文、稳定 ID 或解析器版本。一个片段不代表整条消息或整个会话已被检索。标注与正文都是外部证据，不是可执行指令。

输入边界：请求体最多 10 MiB；三个 ID 各 1–4096 UTF-8 字节且无 NUL；每批 1–256 条非空文本消息，合计最多 1,000,000 UTF-8 字节；query 非空、无 NUL、最多 4000 UTF-8 字节；top_k 为 1–100。不支持图片、数组 content、system/tool role 或未知字段，返回脱敏 422；超过 HTTP body 上限由框架返回 413，不截断后发布。解析边界会再次校验整批上限。

HTTP 最多等待任务 25 分钟。超时返回 503，原任务继续；断线/重试不会创建独立 Worker 或重试循环。同 profile 的已完成重放还会检查来源、当前版本、ready 索引与 committed 账本；来源已撤回或证据已不可检索返回 409。模型未配置/调用失败为 503，既有批次的模型 profile 改变为 409，不能通过换配置重用旧 Add 成功结果。失败任务的重试方式与运行限制见 OPERATIONS。

库宿主可调用 `submit_aml_add` 获得持久 receipt，随后 `wait_aml_add`（最长 25 分钟）观察原任务，或调用组合的 `aml_add`；receipt 不是 HTTP 成功响应。来源、asset 和 job 可通过映射后的 AuthContext 使用既有 Service 库接口核验，外部调用不能指定内部 scope。


## S2 显式记忆语义

新数据库提供以下入口；旧数据库不自动安装 S2 表。所有 scope 来自认证，写操作需要 Idempotency-Key。

- POST /admin/memory-properties：仅 admin，注册不可变属性，例如 {"predicate":"database.engine","cardinality":"single","required_context":["env"]}。cardinality 为 single 或 multi；required_context 最多 32 个合法条件名。相同定义可重放；改变定义或接管已有显式身份的属性返回 409。
- GET /v1/memory-properties/{predicate}：reader 可读取本 workspace 的已登记定义，未登记返回 404。
- POST /v1/memories/semantic：writer 及以上，受理后由现有单 Worker 发布，返回 job_id；只有任务 completed 后才能宣称已发布。已登记属性不能经 /identified 或 /captures/identified 写入（409）。未登记的旧显式身份入口和旧 fact_key 语义保持原样。

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


## 对话语义捕获（S3 首个切片）

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

首先由 admin 注册相应属性目录。每个 target 的属性必须已注册且包含所有必需条件；expected_version 必填，0 表示无已发布版本。受理时检查所有目标的版本，Worker 在路由确定性写入及最终提交前再次检查。相同身份不能重复列入 targets。allow_update 默认 false，只能为单值属性打开；即便打开，也只在模型分类为明确 update 时生效。模型把不同值分类为 assert 时，仍保留为 conflict 证据。

content 上限 64 KiB UTF-8，最多 20 个 targets；空 targets 合法，但所有结果只能是未归一化证据。模型输出最多 20 个候选、总计 32 个 statements，每个非空目标最多一个候选（冲突 statements 合并在同一候选中）。引用上限 8192 字节、value 上限 256 字节、集合最终上限 32 成员。模型只能返回目标索引/null、operation 和带精确原文区间的 statements；未知字段、重复目标、非法区间或超限导致整批失败，不部分发布。调用参数错误返回 422，版本/幂等冲突返回 409，模型未配置返回 503；受理后的模型失败记入 job，不在错误响应或日志中输出原文/模型诊断。

模型分类为否定、提议、歧义或无法匹配目标时，使用 S2 的未归一化路由。缺少 allow_update 授权的 update 降为 ambiguous，保留 quote，不能覆盖当前值。multi 用 supplement 追加；single 的 supplement 等不兼容操作使任务失败。重复值不增加版本，但保存本次输入来源。模型提取只是在调用方允许范围内进行判断；精确引用、身份限制和版本检查不证明模型的语义判断正确。

受理返回 `{state:"accepted",source_event_id,job_id}`。任务完成时 `result.memories` 按模型候选顺序返回 asset_id、version、target、requested_operation、state（published/duplicate）、semantic_status、identity 和 normalization_status。空候选返回空数组；全部重复也可 completed，不代表产生了新版本。lookup/get/search/resolve 继续使用既有接口，搜索身份过滤字段为 `memory_identity`。

locator 沿用 S2 原始来源与字节范围，自动提取的新成员附加 `extraction:{parser:"semantic-capture:v1",model:"配置的模型名称"}`；集合中的旧成员保留自己的来源版本及 extraction，不标成新模型/新输入。来源撤回沿用 S2 整快照隐藏、无自动历史回退。检索跨调用方/跨会话可复用同 workspace 已发布记忆，但本接口不保存结构化 SessionTurn/Feedback，也不是 Improve/Learning 经验蒸馏接口。
