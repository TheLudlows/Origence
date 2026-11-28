# Origence · 当前进度

**简体中文** · [English](STATUS.en.md)

更新于 2026-11-27，本地实现基于 main `a542ff0`。
Origence 是面向智能体的统一数据底座，涵盖记忆、知识、Skill 和 Trace。首版目标见[产品路线图](ROADMAP.md)与[架构规划](design/agent-data-foundation.md)。

AML 评测协议的 HTTP 入口与启动开关已移除。所属 workspace 和消息批次通过[通用 Rust 库接口](API.md#通用所属-workspace-与消息批次库接口)继续提供。

Claude Code 与 Codex 已通过同 Key 会话互读和独立 Run 验收。双向 Skill 复用已在公开合成题上跑通，正式未见题试点及独立业务签认待执行。[#87 双客户端接入](https://github.com/TheLudlows/Origence/issues/87) · [#91 试点及验证记录](../evals/pilot/README.md)

| 范围 | 已实现 | 尚未完成 |
| --- | --- | --- |
| 记忆 | 显式写入与版本化更正、身份条件与精确筛选、受控提取、来源引用与撤回。已有单值的不同提取结果另存为证据，显式确认可更新已发布值。见[记忆契约](contracts/memory.md)。[长期记忆](contracts/memory-consolidation.md)统一接收事实和经验来源并一起召回。 | P0 真实任务验收、归纳的真实模型质量 |
| 知识 | Text、Markdown、PDF 入库，版本读取与比较、发布与撤回；更新失败保留原发布版本。见 [API](API.md#接口) | P0 真实任务验收 |
| Skill | [从 Run 详情选取依据、起草及提交审阅](SKILL-CANDIDATE-CLI.zh-CN.md#在控制台从-run-起草)、[选定经历 CLI](SKILL-CANDIDATE-CLI.zh-CN.md)、选定审阅包、精确批准、不可变发布、在线下载。writer 可审阅与发布。历史 Windows 公开样例验证了两端原生加载及撤回来源后的访问限制。见[实测记录](../evals/pilot/native-skill.windows.json) | 两场景未见题跨端复用与独立核验、受管在线重载 |
| Trace | 私有 Run、上下文准备回执、HTTP/MCP 显式上报、Codex 和 Claude Code 插件的默认 Run Hook、所有者转录的外部核验、精确版本引用与归档 PDF 页内定位。见 [Run 契约](contracts/runs.zh-CN.md) | 两端完整事件覆盖、端到端证据核对及独立签认 |
| 检索与上下文组装 | AnySeek 全文、向量、一跳图候选与权威复核。默认关键词检索要求查询词项全部出现。真实引擎本地回归通过。日常上下文按预算组装带业务日期、时间线和固定引用的归纳事实，控制台可核对原始来源。[证据包契约](contracts/evidence-bundle.zh-CN.md) | 当前后端的模型检索质量、容量与延迟验收 |
| 会话与反馈 | AnySeek 保存新轮次的正文和目录，按逻辑 Key 隔离。控制台「会话」显示已保存的问答。支持按 ID 与序号读取、关键词与向量召回、显式反馈。成员授权空间按权限删除问答，单 Key 工作空间保留原始会话。经验可用人工确认或固定文档版本直接提交，也可走 Improve。 | 在 P0 双场景任务中核对会话和反馈证据 |
| Node.js 管理界面 | 工作空间 Key 登录、部署管理员跨空间查看和全局知识审核，以及受限目录、异步入库、固定版本与来源、Run 列表与创建趋势。运行总览显示可选中复制的当前工作区 ID。现有成员授权空间仍可使用账号登录。 [#76](https://github.com/TheLudlows/Origence/issues/76) 已结项。 | 冻结任务的端到端追溯由 [#91](https://github.com/TheLudlows/Origence/issues/91) 验收。 |

精确测试环境、提交和结果见 [VALIDATION](VALIDATION.md)。

## 发布与经验副本精简

无模型的常规发布在等待重试时只保存代次、操作纪元和摘要，从固定输入重建并核对同一批内容。模型任务保留精确待发布结果，发布成功后在同一 SQLite 事务清除大字段。消息批次保留精确回执元数据。Improve 完成后只保留提案计数及核验、子任务回执；经验来源正文从不可变候选生成。详见[发布和清理设计](design/current-implementation.md#storage-consistency)。

Run 准备重放从固定证据重新拼接文本并校验原摘要。Run 所有者可只保存外部核验记录的引用。Skill 发布引用获审阅的同一份包文件。

SQLite 仍保存不可变经验候选，以及 Run 精确回放所需的普通证据项正文。私有会话证据在重放时从 AnySeek 读取。

独立 AnySeek 部署可使用已取消 Record 数量上限的集合 Info，继续校验显式声明的旧版上限及查询候选限额。具体固定版本和实测见 [VALIDATION](VALIDATION.md#2026-11-27--独立-anyseek-集合限额兼容)。

## 全局知识正文

独立全局版本的分块正文和关键词索引由 AnySeek 保存。SQLite 保存每次发布的来源和审核状态，以及校验固定分块与 Run 引用所需的摘要和范围。待审、驳回和下架的版本不会进入公开候选。

## 单 Key 工作空间

新库可离线创建单 Key 工作空间。该 Key 可用于 Agent 和控制台登录，拥有本空间的业务权限，可以删除记忆。原始会话支持追加和读取，归档成功后由现有持久 Worker 自动提取记忆。重放沿用同一任务，删除的记忆不会因重放复活。轮换延续空间、归档和提取任务。初始化的部署管理员账号可创建平级工作区、管理各空间的 Key、查看会话和 Run，以及审核固定版本的全局知识。已批准的共享副本参与本空间与全局知识的联合召回。见 [部署管理与全局知识接口](API.md#部署管理员)。

## Origence 本地与云上启动

默认构建包含完整宿主和嵌入式 SDK。云上镜像关闭默认特性并启用 `host`，单独使用 SQLite 权威适配器仍可选 `sqlite`。

`origence serve --local` 通过 Rust SDK 在同一进程打开 AnySeek 本地 Catalog，目录为 `OC_DATA_DIR/anyseek`。启动恢复、已发布集合核验及正常关闭由同一宿主管理。Windows 开发脚本调用此入口，再按需启动 Node UI。云上镜像使用 `origence serve` 连接独立部署的 AnySeek，地址与凭据由部署环境提供。[运维说明](OPERATIONS.md#anyseek-全文与向量)

`origence` 公开 `serve`（兼容别名 `api`）。首次启动登记部署管理员。管理员通过控制台或 HTTP 创建工作空间。`origence-deploy` 管理本地 API 和控制台，`origence-setup` 使用已有工作区 Key 接入 Agent。两者独立卸载并保留数据。[安装说明](INSTALLER.md)

## Streamable HTTP MCP

Claude Code 和 Codex 插件通过 Origence 服务端的 `/mcp` 检索和调用工具。检索无需安装本地 Origence。服务端按请求校验凭据和工作区权限，未指定工作区时使用凭据的默认工作区。
两个插件共用 Node.js Hook，负责 Run 上报和可选归档。见[客户端接入](CLIENTS.md#直连-http-mcp)与[验证](VALIDATION.md)。

工作空间 Key 和成员授权工作区的管理员可在控制台「Agent 连接」查看本空间正在处理的 MCP 请求和最近五分钟的活动，
按凭据汇总并显示握手识别的客户端类型。观测限于当前服务进程。见 [Agent 连接接口](API.md#agent-连接)。

## #105 客户端 Hook 本地切片

Codex 用 `turn_id` 记录轮次，Claude Code 用 `prompt_id` 记录单个提示上下文。
两端插件默认安装检索 MCP 和私有 Run Hook。Hook 按 MCP 的工作区选择使用有写权限的凭据，
并通过有界私有队列和 Run API 上报元数据。MCP 仅在调用方显式传递同一 Run 标识时合用。
接入与事件边界见[客户端接入](CLIENTS.md#验证默认-run-hook)。原生 Windows runner 已通过合成数据的安装、跨工作区检索、
历史版本读取和默认 Hook，见 [VALIDATION](VALIDATION.md)。正式冻结因题集、模型授权和独立角色缺失而阻塞。
`origence-setup` 使用可写 Key 时默认[自动归档当前项目的会话](CLIENTS.md#按项目自动归档会话)。

应用直接驱动的任务还可使用[受管 Run 上报](MANAGED-RUNS.md)。Codex App Server 提供命令状态及可用的退出码和耗时，Claude Agent SDK 提供取消回执及对应轮次的结果。

## 多 workspace API 凭据

同一逻辑 key 可在同租户的多个 workspace 获得各自的角色。HTTP 支持显式 workspace
选择与授权枚举，在线 MCP 支持在连接启动时指定 workspace。会话记录仍按 workspace 和
逻辑 key 隔离。原生客户端会话内工作区切换仍待接入。见 [API](API.md#身份与密钥管理)。

## AnySeek 服务凭据与 workspace 集合

只配置 `OC_ANYSEEK_URL` 与 `OC_ANYSEEK_API_KEY`；已鉴权 tenant UUID 直接作为
AnySeek 命名空间，workspace UUID 对应 `ws_<无连字符 UUID>` 集合，无须另配租户或集合。
首次使用按固定 schema 自动初始化，并发及创建响应丢失后严格回读；已有发布的集合在
启动时检查，丢失/配置漂移不会静默建空。绑定 `origence-content-backend-v2` 固定正文格式、路由及模型配置，首次集合使用时
持久化分析摘要。AnySeek 不签发租户 Key；Origence 用户权限和私有会话保持原语义。
验证见 VALIDATION。

## AnySeek 正文与索引统一存储

正文、摘要文本及其 provenance 与全文/向量/图索引共用同一 chunk Record。SQLite 移除
版本/分块正文与摘要文本列；复核后进一步删除未被使用的分块词项及重复摘要模型版本，
简化候选读取为 ID 集合。SQLite 保留来源判断、发布指针、内容哈希和恢复状态。
摘要词项移到 AnySeek 同一 chunk Record 的独立字段，由混合检索的摘要分支查询。
历史读取和恢复使用远端不可变正文，缺失或被改写明确失败；不从原始事件静默重算。
读取期间不持有 SQL 事务，返回前重新检查权限、来源与全部原生排序参与者。
格式为 `origence-sqlite-v2` / `origence-content-sha256-v1`，仅新目录/集合；精确验证见 VALIDATION。

## SQLite 权威存储与最小元数据

进一步将 SQLite/文件锁依赖设为可选：无默认特性的核心库可独立使用，纯核心与
Blob/PDF 测试不加载元数据库；SQLite authority 单独通过 `--features sqlite` 验证。
服务端合并事务入口的重复权限查询，关键词路径复用授权/候选快照，空读集合及没有
候选的后续路径不访问额外元数据表。提交与返回前仍核验当前授权和发布身份。

本地关系数据和持久任务只使用 rusqlite + bundled SQLite；旧引擎实现、依赖与公开别名
已移除。关闭驱动默认 features，不引入 ORM/异步数据库库；锁文件净减少 112 个包。
保留账本复合主键幂等；正文迁出后恢复持久化版本哈希，并为 chunk 保存内容哈希，供跨存储核验。
授权、来源、版本历史、冻结发布计划与恢复状态仍是持久事实，详见[存储契约](contracts/storage.md)。
本次在 PR 的 Linux/Windows CI 验证真实 SQLite 的锁、WAL、事务取消与恢复；
精确本地结果、远端 CI 及未验收边界统一见 VALIDATION 和 PR。

## #76 关联 Run 本地切片

已同步 main `f62168b`（#81 SQLite）：内容详情可读取具体版本的当前 logical key 已记录 Run 引用。
有界索引与事件同事务写入，读取再次核验权威引用、内容条目、来源和所有权，不返回 Run 正文；
固定 v1 往返、另一管理员隔离、撤回、分页与重开已验证，见 [VALIDATION](VALIDATION.md)。
索引保留首条引用证明，不替代完整采集或 trace 可读性；新 schema fresh-only，旧库拒绝而不迁移。
本切片已合入 #83。随后 #99 实现 PDF 页内文本定位，#100 实现 Run 创建数与趋势。

## 检索模块拆分（已合入 PR #79）

已同步 main `3d474d9`（含 #77 UI、#78 诊断）。已核验分路的排序/融合位于
`retrieval::rank`，最终发布及返回复核由 SQLite 事务方法负责；Search 与 EvidenceBundle
保持各自召回、原有快照边界和外部 IO 顺序，同时保留主干诊断采集及 Run 重放保护。
合并后的验证与各历史提交的结果分别见 [VALIDATION](VALIDATION.md)。

## 检索配置与诊断

当前切片基于 main `44e06df`：核对分析配置与 embedding 空间并持久绑定摘要；
Search/Context 返回有界查询诊断，Run 准备路径保存成功与失败观察。
成功准备的重放在核验原权限和来源后读取私有快照，不再依赖模型或 AnySeek；
查询诊断 v2 逐次核对 embedding 空间摘要，阻断启动后的空间变化。
最近 128 次尝试和明确省略数不构成完整客户端采集或跨存储快照。字段见 [API](API.md#检索诊断)。
CI 保留 Node UI 构建、真实 UI 门禁和固定夹具缓存，缓存与检出共用诊断 v2 后端的精确 SHA。
最终检查与真实 AnySeek 联调结果统一记录到 VALIDATION，历史 CI 结果按原提交保留。

## 本地 Run UI 定向验证

基于 `01d4f82` 的本地改动区分准备成功、服务端观察到准备失败与客户端上报。
真实 Chromium 已验证 Run → 固定 v1 → 来源撤回、旧引用再次点击及另一个 key 拒绝。
此处记录当时的本地验证。精确范围见 [VALIDATION](VALIDATION.md)。

## 下一步

按 [#91](https://github.com/TheLudlows/Origence/issues/91) 冻结客户端、模型和任务材料，验证归档、Skill 复用，以及从 Run 经上下文或 Skill 到结果的追溯，并完成独立签认。

## 范围与证据

正常启动要求当前 schema。同版本重启、重试和[配套备份恢复](OPERATIONS.md#私有问答归档与配套恢复)仍是要求。
原始会话和未发布候选按逻辑 key 私有隔离。工作区管理员可查看本区的 Run 元数据、上报状态、创建统计和已记录的详细链路。Run 写入、重放和私有来源读取继续校验所属 Key。
本工作区获授权的成员可以复用已发布的记忆、知识和技能，支持来源仍需有效；Skill 的
私有会话依据可能限制其他 key 的审阅或分发，详见 [Run 契约](contracts/runs.zh-CN.md)。
企业身份与细粒度文档 ACL 尚未实现。
平台只提供归档/查询，不恢复客户端工作现场。Skill 发布不授予工具执行权限。

## 本地 Windows 开发联启入口

基于 main `c70a1aa`，新增 PowerShell 7.4+ 的 AnySeek → Origence → 可选 Node UI
前台联启脚本，使用已初始化目录及当前环境凭据；首次初始化独立说明。
Linux pwsh 的模拟进程回归和真实服务启动/停止/重开已验证，**真实 Windows 尚未实测**。
命令、readiness 与停止限制见 [Windows 开发说明](WINDOWS-DEV.md) 和 VALIDATION。
仅本地交付，不改变部署、生产恢复或现有 CI 范围。
