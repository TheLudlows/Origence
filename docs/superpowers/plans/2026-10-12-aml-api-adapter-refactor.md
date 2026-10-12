# Origence：AML API 适配收敛执行计划

日期：2026-10-12  
核对基线：[`main edd058e646d2fd66ab60f0e0262559e5b3fcba05`](https://github.com/TheLudlows/Origence/tree/edd058e646d2fd66ab60f0e0262559e5b3fcba05)  
状态：**实施中，尚未验收**。本文件冻结 Issue #48 的批准范围；未勾选项不代表已完成实现或验收。执行前须核对届时 main 与本计划基线的差异。  
建议计划入库位置：`docs/superpowers/plans/2026-10-12-aml-api-adapter-refactor.md`

## 执行跟踪

- [x] P0：冻结技术契约、确定性 ID 测试向量与行为基线。
- [x] P1：完成通用作用域、授权和命令幂等域。
- [x] P2：完成通用消息批次、持久回执和发布状态。
- [x] P3：AML API 一次切换并删除旧实现；实现 head 9c492317 经独立审查与适用 CI。
- [x] P4：同版本恢复、最终实现/Compose 配置验收、独立审查与证据归档完成（efcb1ddd，CI 38238050316）；本归档提交仍受同一精确 head 合入门禁约束。

所有阶段都携带各自回归测试，不能把必要安全测试延后到 P4。

## 1. 目标与范围

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

## 2. 当前代码核对结论

| 当前点 | 已确认的行为 | 改造要求 |
| --- | --- | --- |
| `src/storage/sqlite/domain.rs` | `live_role()` 回退到 `aml_role()`；命令去重包含 `principal_id` | 改为通用作用域授权和显式幂等域 |
| `src/storage/sqlite/aml.rs` | 专用授权、Add 回执读写、可检索性检查 | 职责通用化后删除 |
| `src/aml/service.rs` | 批次入库、profile 校验、等待、AML 响应均在专用 Service 实现中 | 分离协议行为与通用入库/发布能力 |
| `src/storage/sqlite/visibility.rs` | 已分离当前检索、历史版本和局部来源支持 | 复用适用部分，不重写另一份可见性体系 |
| `Cargo.toml` | `uuid` 当前启用 `v4`、`serde`，未启用 `v5` | 使用库的 UUIDv5 实现，增加必要 feature；Cargo 生成锁文件 |
| `.github/workflows/ci.yml` | PR 有 fast-check、Linux；main 还有 Windows；含 HTTP、MCP、AML 恢复测试 | 延续现有范围，不恢复 macOS/Docker 自动 CI |
| `AGENTS.md` | 当前只面向新库；要求来源、持久化、恢复、边界与同版本兼容检查 | 所有步骤保持这些约束 |

上述是指定基线的源码核对结果，不是重新测试的结论。引用定位与标准见第 10 节。

## 3. 冻结的技术契约

### 3.1 确定性 workspace ID

```text
workspace_id = UUIDv5(
    固定派生命名空间 UUID,
    encode_v1("aml", tenant_id, root_workspace_id, 完整 user_id)
)
```

固定派生命名空间常量在实现提交中一次确定并冻结，跨语言测试向量必须写出实际输入字节和实际 UUID，不能只保留示意公式。

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

将 AML DTO、派生函数、协议校验、请求/响应转换放在 API 适配子模块，建议 `src/api/aml.rs` 或同层等价模块。纯协议与 ID 测试不应依赖原生存储；路由/宿主的 feature gating 分开处理。

`/aml/add`、`/aml/search` 保留为协议入口。移除 `/admin/aml/namespace` 的专用领域操作，改为通用密钥能力授权；路由启用由宿主的显式开关控制，默认关闭。根 workspace 从认证身份取得，不新建 AML namespace 存储模型。

适配器通过公共通用服务 API 操作数据，不直接写 SQL。通用能力先以可复用库接口交付，不把全面 workspace 管理控制面或大量新 HTTP 入口作为本轮前置。

## 4. 修改清单

| 编号 | 修改 | 涉及位置 | 完成定义 |
| --- | --- | --- | --- |
| M1 | 固定 UUIDv5 派生常量、编码及映射函数 | API 适配模块；`Cargo.toml`、Cargo 生成的 `Cargo.lock` | 固定向量跨重启、跨语言一致；核心不识别 AML 规则 |
| M2 | 并发安全创建/查找和一层所属作用域授权 | 通用 workspace 服务；`sqlite/relational.rs`、`domain.rs`、scope/types/traits | 直接/派生权限明确；撤销、降权、错误归属失败 |
| M3 | 通用幂等域、回执与请求配置冲突 | `service.rs` 的通用命令入口；`sqlite/domain.rs`、命令类型 | 原有操作保持 principal 范围；批次跨 key 去重 |
| M4 | 通用批次来源模型及入库服务 | 新的通用消息模块；通用 Service；`jobs.rs`、`worker.rs` | 非 AML 输入可直接走同一持久化发布流程 |
| M5 | 通用发布状态和等待 | 通用服务；`sqlite/visibility.rs` 及适用的来源/索引查询 | 完整来源和全部必需索引可见，才确认成功 |
| M6 | AML 路由薄适配和配置切换 | `api.rs` 及其 AML 子模块；宿主配置 | 只有协议、ID、参数和响应转换，无专用 SQL/任务恢复 |
| M7 | 删除 AML 存储与任务特例 | `sqlite/aml.rs`；两份 AML schema；`sqlite/schema.rs`；模块注册和旧任务类型 | 新库不创建 AML 表，下层不依赖 AML DTO/业务函数 |
| M8 | 工具、测试、文档和工程证据 | `tests/`、`tools/`、当前 eval 执行器、`.github/workflows/ci.yml`、相关 docs | 行为回归完整，历史实验不被改写 |

拟删除的文件为 `src/storage/aml-schema.sql`、`src/storage/aml-add-schema.sql`、`src/storage/sqlite/aml.rs`；`src/aml.rs` 与 `src/aml/service.rs` 在协议和通用职责分别迁出后删除，不留下双实现。

既有表变更仅限必要的通用结构：`oc_workspaces.owner_workspace_id`、`oc_api_keys.allow_owned_workspaces`、`oc_commands.dedup_scope` 与相关约束/查询。上述名称是本计划建议名称，不表示现已存在。

## 5. 执行顺序与提交边界

```text
P0 契约与测试设计
        ↓
P1 通用作用域/授权/幂等
        ↓
P2 通用消息批次/发布状态
        ↓
P3 AML 一次切换 + 专用路径清理
        ↓
P4 最终同版本验收与证据归档
```

所有阶段均携带其针对性测试；P4 是整体验收，不是第一次补安全测试。

### P0：冻结契约与基线

核对执行时 main 与本计划基线差异，避免覆盖后续已合入修改。固定 ID 编码、根 workspace 来源、权限能力、幂等域、原文定位、失败与等待规则；记录真实 UUID 测试向量和 schema 拒绝条件。

产物：此计划进入仓库、契约测试清单、输入/响应基线与本轮可观察变化说明。不导入私有评测题，不重算历史质量报告。

### P1：通用作用域、授权和幂等

先修改已有表的新库 DDL/约束与 schema 检查，再实现通用服务与事务验证。普通 workspace/key/command 行为仍保持原契约。实现确定性 ID 的 API 纯函数及测试，但不在半完成状态切换线上 AML 路由。

产物：通用 workspace 与 command 能力、并发/撤销/越权/跨 key 去重回归。中间提交可短暂保留旧适配供现有路径使用，但禁止双写、影子发布、自动回退；该过渡不能作为最终完成状态。

### P2：通用消息批次与发布可见性

完成通用消息模型、原子入库、任务编解码、既有 Worker 分支与计划恢复。移出可复用的状态查询，按需要调用共享可见性片段。非 AML 公共库入口直接跑完入库 → Worker → Search → 重试/恢复。

产物：通用 ingest/receipt/publication API、完整来源测试、受控模型故障测试。无需启用抽取/图等高级功能。

### P3：一次切换并清理

在同一完整变更中，将 AML API 指向通用路径，更新启用方式、工具和 active fixtures，删除三张 AML 表、AML SQL、AmlIngest、专用权限和旧 Service 编排。消息 parser 标识或配置契约变化同时更新文档。

不把“删表但仍用旧授权/回执”的状态合入。构建时检查下层不再引用 AML DTO/函数；必要的历史 schema 拒绝识别、文档及历史报告可保留，不等于兼容运行旧路径。

产物：端到端可工作的唯一 AML 适配路径；普通产品路径回归；新库 schema 不含 `oc_aml_*`。

### P4：最终验收、记录与合入

在同一个最终 head 上执行完整适用 CI 和定向回归，记录 commit、run、测试计数、跳过项及失败项。复查 main 合入后的实际检查结果，不拿旧提交成功记录替代。

交付说明明确新库要求、旧库拒绝行为、启用与授权方法以及未运行的真实模型/官方评测。测试数据使用专用临时目录；不得删除业务库。

建议提交组织：一份文档契约提交；一个通用基础能力 PR（P1/P2 按风险分提交）；一个完整切换清理 PR（P3）；P4 随最终 head 验收完成，不单独延后必要回归。

## 6. 验收矩阵

| 编号 | 场景 | 必须满足 |
| --- | --- | --- |
| T01 | 同一 tenant/root/user 重复派生 | ID 固定；session、request、key、模型变化不影响定位 |
| T02 | 不同 user/root/tenant；大小写、Unicode、多字节与含分隔符 ID | 不截断、不错误合并；固定测试向量通过 |
| T03 | 首次并发 Add | 只有一个目标 workspace，原文批次不重复 |
| T04 | 已有 ID 的 tenant/owner 不匹配 | 冲突失败，不覆盖、不改绑、不随机 fallback |
| T05 | 未知用户 Search | 先通过合法身份与接入权限检查；空结果且无 workspace/任务创建 |
| T06 | 普通 key、reader、伪造目标、跨 root/tenant、递归派生及未授权的新 key | 未获授权的操作失败；owner 自身必须为 root，直接绑定 child 的 admin key 也不能递归派生；普通产品 key 不扩权，新 key 不自动继承能力 |
| T07 | I/O 前后撤销/降权/关闭派生能力；Worker 重启 | 最终读取/发布重新验证，不靠旧 AuthContext 授权 |
| T08 | 同批次并发/超时/换 key 重放；撤销旧 key 后换 key 重放 | 相同 workspace 幂等域复用同一 job/asset/source；creator 不被偷换；旧 key 撤销后仍命中原回执，不复制任务，不恢复其未完成任务的发布权 |
| T09 | 相同 request_id 改正文、session、顺序、时间或 accepted spec；重启后配置变化 | 明确冲突；无新批次或静默旧结果；原 job 持久保留 parser/分块版本/raw-only/profile，恢复不以当前默认值覆盖 |
| T10 | 其他普通命令的同键不同 principal | 保留原有 principal 幂等隔离 |
| T11 | 中文/emoji/组合字符/长消息、同 session 多个 Add 批次、缺省及乱序 timestamp | 原文可重建；每片段定位到确切批次/消息/字节范围；可选 Unix 毫秒时间不补值，消息按输入序列保序 |
| T12 | 通用库入口绕过 HTTP 直接调用，包含非 AML 的 system/tool 消息 | 同样有输入/资源/角色与发布检查；通用 role 标签及来源原样保留，完整路径成功；AML 入口仍拒绝非 user/assistant 角色 |
| T13 | 模型失败、索引失败、发布前来源撤回 | 不部分发布，不提前成功 |
| T14 | HTTP 断开、处理中强退、同版本备份恢复 | 原任务可观察和恢复，无重复版本/分块；不宣称断电保证 |
| T15 | completed 后索引/profile/来源/当前版本不再满足 | 幂等重放不返回过时 success；只读状态不恢复被撤回数据 |
| T16 | vector/keyword/hybrid、历史读取、S2/S3/Learning/Bundle | 本次作用域与命令改造不削弱原有权限与可见性 |
| T17 | 新库与旧 schema | 新库无 AML 表；旧 schema 明确拒绝，不自动升级/清空/转换 |
| T18 | 无默认 feature、默认 feature、HTTP/MCP/CLI 回归 | 新依赖/模块边界可编译，现有接入行为保持 |
| T19 | 同库同 generation 的重放/重启，以及显式 retry 进入新 generation | 同一 generation 的证据 ID 保持稳定；显式 retry 保留原 job/source/asset，未发布 chunk 的 ID 可随新 generation 改变，不改写已发布证据；不同独立新库不要求随机资产/分块 UUID 相等 |
| T20 | 下层独立性 | 无 AML 特殊表、任务、DTO 或授权函数；非 AML 调用方复用同一实现 |

新旧实现使用独立新库对照时，以用户、批次、session、消息索引、原文范围匹配证据；不要把原本依赖随机 job ID 的 chunk UUID 强行要求跨库相等。排序算法与 tie-break 规则保持；对因新库 UUID 不同造成的同分顺序差异单独说明，不据此宣布检索质量变化。

## 7. 工程检查

按现有 Rust 1.98.0 和项目 CI 执行，不新增并行 Worker 或重新启用已取消的平台作业。以下是待执行命令，不是已通过结果。

```bash
cargo fmt --all -- --check
cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings
cargo test --locked --no-default-features -j 2 --no-fail-fast
cargo clippy --locked --all-targets -j 2 -- -D warnings
cargo test --locked -j 2 --no-fail-fast
cargo build --locked -j 2

python3 -m unittest discover -s tools -p 'test_*.py' -v
python3 -m unittest discover -s evals -p 'test_*.py' -v
AML_TEST_BINARY=target/debug/origence python3 -m unittest discover -s tools -p 'test_aml_recovery.py' -v
python3 tools/smoke.py --binary target/debug/origence
python3 tools/mcp_smoke.py --binary target/debug/origence
```

集成测试继续挂入既有 `tests/local.rs`；仓库 `autotests=false`，不能只新增测试文件而没有注册进实际执行目标。定向测试筛选须核对实际执行数量，不能以零测试退出码为通过。

CI 执行器若引用旧启用端点、旧 parser、`oc_aml_adds` 或 `aml_ingest`，必须改为验证通用命令/任务/来源，而不是删除整个恢复场景。正式历史实验工件及原始分数不批量改写。

UUIDv5 只开启必要的 `uuid` feature；锁文件由 Cargo 更新并审查差异，保留最新主干已有的 MCP 依赖修复，不借此回退或无关升级依赖。

## 8. 文档与工具同步

更新 `docs/API.md`：协议入口、输入边界、来源/parser、Add 等待/错误、通用批次和幂等范围；明确通用 role 与 AML 角色边界、可选 Unix 毫秒 timestamp 的保序语义、accepted spec 和单批次回执的发布判定范围。

更新 `docs/OPERATIONS.md`、`docs/AML_DRILL.md`：宿主开关、根 workspace、通用授权发钥、评测隔离、恢复及新库边界；明确 root key 的受信服务端用途、新 key 能力须显式授予、正常轮换先排空，以及紧急撤销不接管原未完成任务的限制。

更新 `docs/AGENT_MEMORY_LEADERBOARD.md`：以通用能力为基础的接入方式；不冒称完成官方 Smoke/Full。

更新 `docs/STATUS.md`、`docs/VALIDATION.md` 和当前路线图：分开记录计划、实现、实测与未验收项。历史报告只增加必要的“历史版本”导航，不重新生成漂亮分数。

在 `AGENTS.md` 加入明确依赖约束：外部评测协议不得渗入存储 schema、通用任务和权限判断；新增下层能力必须由非 AML 入口验证。

## 9. 最终完成标准

- [x] 固定命名空间下的同一 AML 用户由计算获得固定 workspace，无专用映射表或 scope_key。
- [x] 三张 AML 专表及业务使用路径全部移除；没有同义替代表、双写或自动 fallback。
- [x] 通用权限仍覆盖撤销、派生作用域、后台任务和 I/O 后复核；普通 key 不扩权。
- [x] 通用幂等和原子批次保留跨 key 重试、配置冲突、恢复和索引可见后确认成功。
- [x] 正式 AML 仍是相同检索策略；协议或 parser 可观察变化均明确记录。
- [x] 所有功能只依赖通用工作区、命令、来源、资产、任务、分块和索引；非 AML 调用方有实际测试。
- [x] 最终 head 的适用测试与 CI 有可核对证据；真实模型/官方评测未执行时明确保持 NOT_RUN。

## 10. 核对来源

源码均以 `TheLudlows/Origence@edd058e646d2fd66ab60f0e0262559e5b3fcba05` 为基线：

- [`src/storage/sqlite/domain.rs`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/src/storage/sqlite/domain.rs)：授权回退、begin_command/finish_command。
- [`src/storage/sqlite/aml.rs`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/src/storage/sqlite/aml.rs)：AML role、Add record、Add visibility。
- [`src/aml/service.rs`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/src/aml/service.rs)：原子接受、profile 冲突、等待和 Search 配置。
- [`src/storage/sqlite.rs`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/src/storage/sqlite.rs)、[`src/storage/sqlite/visibility.rs`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/src/storage/sqlite/visibility.rs)：当前模块布局、共享可见性边界。
- [`AGENTS.md`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/AGENTS.md)：新库范围、模块边界、恢复、来源和验证纪律。
- [`.github/workflows/ci.yml`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/.github/workflows/ci.yml)：当前工具链、平台和工程测试范围。
- [`Cargo.toml`](https://github.com/TheLudlows/Origence/blob/edd058e646d2fd66ab60f0e0262559e5b3fcba05/Cargo.toml)：uuid feature、Rust 版本与显式 tests/local 目标。
- [RFC 9562 §5.5](https://www.rfc-editor.org/rfc/rfc9562.html#section-5.5)：名称型 UUIDv5；UUID 作为标识的唯一性和安全边界按该标准理解。

本计划的原始批准来源为 [Issue #48](https://github.com/TheLudlows/Origence/issues/48)（2026-10-12 09:07:45 UTC）。执行时 main 仍为上述基线，没有额外差异。最终命令和 CI 证据须另行记录。


## P0 冻结细节

- UUIDv5 派生命名空间：`9b6adf90-8252-4a40-83e0-64f14f6febed`。
- 输入先写 ASCII `origence:owned-workspace:v1` 与一个 NUL 字节，再依次写四个 UTF-8 字段：`aml`、tenant UUID 的小写连字符表示、root workspace UUID 的小写连字符表示、完整原始 user_id。每个字段前写其 UTF-8 字节长度（无符号 64 位大端序）。
- key = `aml:add:v1:` + SHA-256(完整 request_id UTF-8) 的 64 位小写十六进制摘要；不截断 request_id，不包括任何处理配置。
- 通用 message role 为 1–64 UTF-8 字节、不含 Unicode 控制字符的原样标签；时间是可选有符号 i64 Unix 毫秒，允许负数，缺省不补值且不重排。AML 仍仅接受 user/assistant。
- 批次命令操作名 `message_batch.ingest` 固定使用 workspace 幂等域，普通操作继续 principal 域。调用方不能自选域。
- accepted spec 将 parser 标识、parser_version、chunking_version、raw_only=true 与 embedding_profile 保存在原 job payload 并参与请求 hash。
- 固定跨语言向量见 [JSON](../../contracts/aml-workspace-v1-vectors.json)；实现必须使用 uuid crate 的 v5 功能。向量包含实际编码字节及实际 UUID，独立 Python RFC 9562 算法生成，配套 Rust 测试核对（执行结果另记）。
- 当前 schema 必须拒绝缺少通用列/约束的旧库，最终 P3 同时拒绝含旧 AML 专用表的库；不删除或转换旧数据。

### 执行环境与证据边界

本轮云端编辑环境没有 Rust/Cargo。Cargo.lock 只能由 Rust 1.98.0 的 Cargo 生成；临时只读 GitHub Actions 工具工作流提供锁文件与 rustfmt 补丁，并在合入前移除。其生成结果不是测试通过证据。所有适用检查以最终 head 的 PR CI 和合入后 main CI 为准；真实模型、真实评测导入与官方 Smoke/Full 均 NOT_RUN。
