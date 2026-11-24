# Origence · 当前进度

**简体中文** · [English](STATUS.en.md)

更新于 2026-11-19，整合基线为 main [`2094f81`](https://github.com/TheLudlows/Origence/commit/2094f81c990aaf7d69d4ff09295c1a2a837c4ba1)。
Origence 是面向智能体的统一数据底座，涵盖记忆、知识、Skill 和 Trace。首版目标见[产品路线图](ROADMAP.md)与[架构规划](design/agent-data-foundation.md)。

AML 评测协议的 HTTP 入口与启动开关已移除。所属 workspace 和消息批次通过[通用 Rust 库接口](API.md#通用所属-workspace-与消息批次库接口)继续提供。

Claude Code 与 Codex 已通过同 key 私有归档互读和独立 Run 验收。双向 Skill 复用已在公开合成题上跑通，正式未见题试点及独立业务签认待执行。[#87 双客户端接入](https://github.com/TheLudlows/Origence/issues/87) · [#91 试点及验证记录](../evals/pilot/README.md)

| 范围 | 已实现 | 尚未完成 |
| --- | --- | --- |
| 记忆 | 显式写入与版本化更正、身份条件与精确筛选、受控提取、来源引用与撤回。见[记忆契约](contracts/memory.md) | P0 真实任务验收 |
| 知识 | Text、Markdown、PDF 入库，版本读取与比较、发布与撤回；更新失败保留原发布版本。见 [API](API.md#接口) | P0 真实任务验收 |
| Skill | [选定经历 CLI](SKILL-CANDIDATE-CLI.zh-CN.md)、选定审阅包、精确批准、不可变发布、在线下载及两种客户端目录格式导出。Windows 公开样例已验证两端原生加载及来源撤回后拒绝新导出。见[实测记录](../evals/pilot/native-skill.windows.json) | 两场景未见题跨端复用与独立核验、受管在线重载 |
| Trace | 私有 Run、上下文准备回执、HTTP/MCP 显式上报、按项目启用的 Codex 和 Claude Code Hook 与采集缺口、所有者转录的外部核验、精确版本引用与归档 PDF 页内定位。见 [Run 契约](contracts/runs.zh-CN.md) | 两端真实 Hook 加载与实际事件覆盖、端到端证据核对及独立签认 |
| 检索与上下文组装 | AnySeek 全文、向量、一跳图候选与权威复核；按预算组装证据。真实引擎本地回归通过 | 当前后端的模型检索质量、容量与延迟验收 |
| 私有归档与反馈 | 双客户端显式归档互读、逻辑 key 隔离与轮换撤销已验证；支持显式反馈和 Improve → Learning | 在 P0 双场景任务中核对归档和反馈证据 |
| Node.js 管理界面 | 五页 UI，授权新建子工作区、受限目录、异步入库、固定版本与来源、按当前 key 的 Run 创建数及趋势。已有 Chromium 定向验证 | 实际屏幕阅读器、实体触屏、生产计量与告警。见 [管理界面 #76](https://github.com/TheLudlows/Origence/issues/76) |

精确测试环境、提交和结果见 [VALIDATION](VALIDATION.md)。

## Origence 本地与云上启动

`origence serve --local` 通过 Rust SDK 在同一进程打开 AnySeek 本地 Catalog，目录为 `OC_DATA_DIR/anyseek`。启动恢复、已发布集合核验及正常关闭由同一宿主管理。Windows 开发脚本调用此入口，再按需启动 Node UI。云上镜像使用 `origence serve` 连接独立部署的 AnySeek，地址与凭据由部署环境提供。[运维说明](OPERATIONS.md#anyseek-全文与向量)

## #105 客户端 Hook 本地切片

Codex 用 `turn_id` 记录轮次，Claude Code 用 `prompt_id` 记录单个提示上下文。
两者通过同一私有队列和 Run API 上报可识别的元数据。MCP 仅在调用方显式传递
同一 Run 标识时合用。接入、事件覆盖和中断边界见[客户端接入](CLIENTS.md#按项目启用私有-run-hook)。
本地夹具覆盖公开 CLI、HTTP 和 MCP。真实客户端 Hook、模型与正式试点仍待验收。
自动会话正文归档保留在 [#105 后续规划](https://github.com/TheLudlows/Origence/issues/105)。

## 多 workspace API 凭据

同一逻辑 key 可在同租户的多个 workspace 获得各自的角色。HTTP 支持显式 workspace
选择与授权枚举，在线 MCP 支持在连接启动时指定 workspace。私有归档仍按 workspace 和
逻辑 key 隔离。原生客户端会话内切换、插件自动配置及归档采集仍待接入。见 [API](API.md#身份与密钥管理)。

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
简化候选读取为 ID 集合。保留来源判断、发布指针、内容哈希、摘要召回词项和恢复状态。
正文及摘要词项统一在事务外生成并冻结，重试不再以重新分词兼容缺失字段的旧计划。
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
真实 Chromium 已验证 Run → 固定 v1 → 来源撤回、旧引用再次点击及另一个 key 拒绝；
精确范围见 [VALIDATION](VALIDATION.md)。仅本地验证，#76 的其余规格仍未完成。

## 下一步

1. 继续 [#76](https://github.com/TheLudlows/Origence/issues/76) 的屏幕阅读器和实体触屏验收，并按现有验收清单补足证据。
2. 按 [#91](https://github.com/TheLudlows/Origence/issues/91) 冻结客户端、模型和任务材料，验证归档、Skill 复用和 Trace，并完成独立签认。

## 范围与证据

只面向新建数据库；同版本持久化、重启/重试与备份恢复仍是要求。
原始会话、Run 和未发布候选按逻辑 key 私有隔离；workspace 成员身份不授予他人原始归档访问权。
本工作区获授权的成员可以复用已发布的记忆、知识和技能，支持来源仍需有效；Skill 的
私有会话依据可能限制其他 key 的审阅或分发，详见 [Run 契约](contracts/runs.zh-CN.md)。
首批采用成员授权范围一致的 workspace；企业身份与差异文档 ACL 尚未实现。
平台只提供归档/查询，不恢复客户端工作现场。Skill 发布不授予工具执行权限。

## 本地 Windows 开发联启入口

基于 main `c70a1aa`，新增 PowerShell 7.4+ 的 AnySeek → Origence → 可选 Node UI
前台联启脚本，使用已初始化目录及当前环境凭据；首次初始化独立说明。
Linux pwsh 的模拟进程回归和真实服务启动/停止/重开已验证，**真实 Windows 尚未实测**。
命令、readiness 与停止限制见 [Windows 开发说明](WINDOWS-DEV.md) 和 VALIDATION。
仅本地交付，不改变部署、生产恢复或现有 CI 范围。
