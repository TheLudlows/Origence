# Origence：AML API 适配合同

原始设计日期：2026-10-12；本次文档整理：2026-10-16。

本文保留 [Issue #48](https://github.com/TheLudlows/Origence/issues/48) 的生产设计合同。实现已随 PR #50/#51 合入；当前接口与操作分别以 [API](../../API.md#aml-http-协议) 和 [OPERATIONS](../../OPERATIONS.md#aml-适配启用与批次任务) 为准。原执行计划中的专项测试矩阵、测试向量与执行命令已移除；本页不宣称当前 AML 专项验证覆盖。

## 目标与范围

将 AML 协议语义收敛到 API 适配层。用户 workspace 由适配器确定性计算，不新增用户映射表、不增加 `scope_key` 字段；下层补充能力必须可被非 AML 调用方直接使用。

```text
AML API 适配层
  协议校验 / 确定性 ID / 请求转换 / 等待策略 / 响应转换
                       ↓
Origence 通用应用服务
  workspace 授权与创建 / 消息批次入库 / 命令幂等 / 发布状态 / 检索
                       ↓
Origence 通用存储与单 Worker
  workspaces / api_keys / commands / events / assets / jobs / chunks
```

本轮完成后删除 `oc_aml_namespaces`、`oc_aml_users`、`oc_aml_adds` 的建表和业务实现，移除 `aml_role()`、`AmlIngest` 以及 Worker 对 AML DTO/解析器的依赖。不是仅给表、函数或任务换名字。

不纳入本轮：旧数据库迁移或升级、任意层级 workspace 树、完整 IAM、每用户隐藏密钥、新存储后端、额外 Worker、自动重试策略调整、检索提分、新模型、自动维护、官方 Smoke/Full、真实评测数据导入。新库范围不允许删除或静默转换既有库；同版本重启/恢复仍是验收项。

## 冻结的技术契约

### 3.1 确定性 workspace ID

```text
workspace_id = UUIDv5(
    固定派生命名空间 UUID,
    encode_v1("aml", tenant_id, root_workspace_id, 完整 user_id)
)
```

固定派生命名空间与完整输入编码见下文“确定性编码细节”；它们是生产定位合同的一部分。

`root_workspace_id` 来自已认证密钥绑定的 workspace，不接受请求体自行指定 tenant/root。字段采用无歧义的版本化长度编码；用户 ID 使用完整原始 UTF-8，不截断、不转小写、不自行做 Unicode 归一化。

映射不包含 `session_id`、`request_id`、API key/token、模型配置、进程启动时间。同一 root 下同一用户固定到同一 workspace；不同用户仍隔离，不是所有 AML 用户共用一个固定 workspace。新一轮需要隔离的数据集使用另一个显式配置的根 workspace，不在每次启动时随机改变映射。

Add 计算 ID 后调用通用 `ensure_owned_workspace`（暂定名）；Search 计算同一 ID 后只调用授权查找，不创建 workspace，授权合法且对象不存在时返回空结果。

确定性 ID 只解决定位，不是授权或保密凭证。已有 ID 的 tenant、owner 不匹配必须失败，不得覆盖、重新绑定或因冲突随机生成另一个 ID。未保存完整映射键时不能承诺发现所有同归属哈希碰撞；保留名称型 UUID 的非零碰撞边界，不宣称数学上的绝对无碰撞。

### 3.2 通用的一层 workspace 授权

在现有 `oc_workspaces` 增加可空 `owner_workspace_id`，不保存 AML user ID 或 scope_key。使用同 tenant 的组合外键，禁止自身归属；本轮只允许根 workspace 的一层所属 workspace。创建或检查所属关系时，必须在事务内确认 owner 自身的 `owner_workspace_id IS NULL`；组合外键和禁止自身归属不能代替此检查。直接绑定 child 的 admin key 也不能继续派生下一层。归属创建后不可随意更改，避免改变已有任务的授权含义。

在现有 `oc_api_keys` 增加通用 `allow_owned_workspaces`（暂定名），默认 false。管理员通过通用密钥签发能力显式授予；普通密钥的原有权限不扩大。轮换签发的新 key 同样必须显式获得该能力，不因属于同一 root 或具有 admin 角色而自动继承。不创建新的 ACL/映射表或每用户隐藏凭据。

具备该能力的 root key 是受信服务端凭据，可按其 reader/writer 权限访问 root 下全部所属用户 workspace；不得作为终端用户凭据分发。`user_id` 用于选择数据作用域，不验证终端用户身份。此处沿用既有 AML namespace 的信任模型，终端用户身份校验与访问约束由持有 root key 的受信接入服务负责。

事务内允许两类访问：直接属于目标 workspace 的有效 key；或具备显式所属作用域权限、且目标 workspace 归属于其 root 的有效 key。派生访问限于 reader/writer，不自动获得发钥、删除或继续派生权限。能力开关不得由普通请求自行设置。

并发 ensure 在事务内检查根权限、创建或检查目标归属。不能把 `AuthContext.workspace_id` 改掉就视为授权完成。Worker、重试、最终发布、模型/原生 I/O 后的读取都重查 live key/role/能力；保留原始 `created_by`，不得借新 key 偷换任务创建者。

本轮保留原任务绑定原创建者的执行授权：旧 key 撤销后，其未完成任务不得由新 key 接管或继续发布；即使新 key 命中原回执，也不获得原任务的执行权。正常轮换先排空旧 key 的未完成任务，再撤销旧 key；紧急撤销接受未完成任务被中断。本轮不引入任务身份转移或无缝接管机制。已发布数据仍按当前访问权限和实时来源/索引可见性判断，不因更换读取 key 自动失效。

### 3.3 通用幂等域与持久回执

复用 `oc_commands`，增加显式 `dedup_scope`（暂定名），唯一维度为：

```text
tenant_id + workspace_id + dedup_scope + operation + key
```

`dedup_scope` 由服务端操作契约决定：普通操作保持 `principal:<principal_id>`；通用消息批次导入使用 `workspace`。`principal_id` 继续记录实际首次提交者并用于审计，不伪造一个统一身份来规避权限。

AML API 将完整 request_id 做摘要编码，并加固定的适配器/协议版本前缀（如 `aml:add:v1:<digest>`），适配通用 200 ASCII 字节的键边界；不要截断。前缀与摘要算法在 P0 冻结，避免与同一 workspace 内无关调用方的批次键混用；不让请求自行选择 `dedup_scope`。完整外部批次标识保留在通用批次输入中，参与请求一致性核验。

请求内容、session、消息顺序、角色、时间和接受时的处理配置都参与一致性检查。接受时的处理规格（accepted spec）至少包含 parser 标识/版本、分块版本、raw-only 模式和 embedding profile，并在接受事务内持久保存在原 job 的 payload 中，同时纳入请求指纹；不能只存在于当前进程配置或幂等摘要中。Worker、同版本恢复及显式 retry 使用该已接受规格，不以重启后的默认值覆盖。更换 embedding/profile/parser、分块版本或处理模式时，对同一幂等键返回冲突，不能把模型配置加进键中绕开冲突并新建任务。

source、asset、job、命令回执在同一个写事务中提交。回执保存原任务与发布引用，不保存永久 `success=true`。重放先授权，再匹配内容和配置，取得同一回执后查询实时发布状态。密钥轮换可以命中原回执，但不能越过原任务的撤销/重试限制。

### 3.4 通用消息批次

新增可供非 AML 调用方直接使用的 `MessageBatch`、`SourceMessage`、`IngestReceipt`（暂定名）。批次包含外部批次标识、session、消息序列及角色/正文/时间；不包含 AML Search/Response DTO 或 AML 等待常量。

通用 `role` 是有资源边界的消息标签，不承载授权，也不限定为 AML 的两种角色；非 AML 库入口须覆盖 `system`、`tool` 等标签的原样保存与检索来源。AML 适配器仍只接受现有 `user`/`assistant`。`timestamp` 统一为可选 Unix 毫秒整数，保留缺省状态，不补当前时间，也不据此重排消息；消息顺序始终以输入序列为准。标签边界和时间校验由通用入口及 Worker 一致执行，AML 特有约束留在适配器。

通用服务和 Worker 对输入再次做资源边界检查，不仅依赖 HTTP 校验。通过既有 `events/assets/jobs/chunks` 保存来源和结果；一个 Add 对应一个逻辑批次任务，整批可见，不拆成多次独立发布。

将 `AmlIngest` 替换为真正读取通用消息模型的 `MessageBatchIngest`，复用现有单 Worker、publication 恢复、原生写入账本和 prepare/publish 路径。CPU 密集解析继续放在 `spawn_blocking` 中，在异步执行器线程和存储事务之外运行；不能在持有数据库事务时执行模型或重 CPU 工作。

保留原文、消息边界、顺序、role、timestamp、session，以及相对具体消息的 UTF-8 byte range。来源必须能用 source event + batch + message index + range 唯一定位，同一 session 多批次不能只靠局部 message_index 区分。

使用新的通用 parser 标识并冻结其含义；不重新解释旧证据。原始批次与 SessionTurn/Feedback 档案是不同对象，不为复用而伪造问答反馈。

### 3.5 发布状态与 AML 外部行为

抽出通用发布状态查询/等待能力。本轮围绕单个 `IngestReceipt` 所引用的原 job、asset、source、目标版本与所需 profile 判定批次发布状态，复用既有 generation/账本规则；不扩展为统一所有任务类型或 Extract/Improve 等多产物任务的发布框架。至少区分进行中、完成且可检索、失败、取消/被替代、已不可见。成功要求目标版本和来源有效、所需 profile 的全部分块索引已就绪、账本 generation 匹配；不能只检查 job.state 或随便搜索到一条片段。

复用 `visibility.rs` 中适用的局部规则；当前版本、历史读取和 Learning 传递来源规则继续分开，不能用一个简化谓词替代全部语义。

AML Add 的等待策略和响应转换留在 API 模块；本轮保留现有 25 分钟等待上限和超时重试语义。HTTP 断开不取消、不复制后台任务；不增加适配器自己的任务恢复状态或第二个重试循环。

AML Search 保持原文 vector、禁用摘要/图/重排、`allow_partial=false`，不生成答案。options 保持当前行为，错误映射、输入边界和可观察的 parser/metadata 变化必须写入 API 文档。入库显式选择 raw-only，不能因复用普通 Ingest 意外开启摘要或图。

### 3.6 API 与部署边界

将 AML DTO、派生函数、协议校验、请求/响应转换放在 API 适配子模块，建议 `src/api/aml.rs` 或同层等价模块。路由/宿主的 feature gating 分开处理。

`/aml/add`、`/aml/search` 保留为协议入口。移除 `/admin/aml/namespace` 的专用领域操作，改为通用密钥能力授权；路由启用由宿主的显式开关控制，默认关闭。根 workspace 从认证身份取得，不新建 AML namespace 存储模型。

适配器通过公共通用服务 API 操作数据，不直接写 SQL。通用能力先以可复用库接口交付，不把全面 workspace 管理控制面或大量新 HTTP 入口作为本轮前置。

## 确定性编码细节

- UUIDv5 派生命名空间：`9b6adf90-8252-4a40-83e0-64f14f6febed`。
- 输入先写 ASCII `origence:owned-workspace:v1` 与一个 NUL 字节，再依次写四个 UTF-8 字段：`aml`、tenant UUID 的小写连字符表示、root workspace UUID 的小写连字符表示、完整原始 user_id。每个字段前写其 UTF-8 字节长度（无符号 64 位大端序）。
- key = `aml:add:v1:` + SHA-256(完整 request_id UTF-8) 的 64 位小写十六进制摘要；不截断 request_id，不包括任何处理配置。
- 通用 message role 为 1–64 UTF-8 字节、不含 Unicode 控制字符的原样标签；时间是可选有符号 i64 Unix 毫秒，允许负数，缺省不补值且不重排。AML 仍仅接受 user/assistant。
- 批次命令操作名 `message_batch.ingest` 固定使用 workspace 幂等域，普通操作继续 principal 域。调用方不能自选域。
- accepted spec 将 parser 标识、parser_version、chunking_version、raw_only=true 与 embedding_profile 保存在原 job payload 并参与请求 hash。
- 当前 schema 必须拒绝缺少通用列/约束的旧库，最终 P3 同时拒绝含旧 AML 专用表的库；不删除或转换旧数据。
