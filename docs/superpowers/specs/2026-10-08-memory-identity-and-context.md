# 记忆身份与上下文策略：P1 前置设计

> **文件定位**：这是 I1–I4 的领域规格（身份、抽取、冲突与上下文边界），不是剩余任务的排期计划。具体实施顺序、未完成任务、每阶段完成标准见 [后续阶段路线图](../plans/2026-10-09-next-stage-roadmap.md)。已验收能力仍以 STATUS/VALIDATION 与实际代码为准。


日期：2026-10-08；状态更新：2026-10-09，验收代码 `8d8d224`。已实现身份 v1 编码、SQLite 绑定、显式写入与单身份 capture、来源读取、绑定状态、精确 lookup、search/resolve 身份标注与过滤，以及调用方 expected_version。现有 CI 已 7/7 通过：27 项 lib、58 项无原生集成，三平台原生 HTTP/Worker、render/预算测试，以及 Linux/macOS release/HTTP smoke、关键词宿主评估、容器及 MSRV check；证据见 [VALIDATION](../../VALIDATION.md)。自动推断、属性目录、未归一化状态、语义冲突、I3 会话及完整 I4 类型策略尚未实现。本文补充 [整体设计](2026-09-22-memory-knowledge-platform-design.md) §4/§5/§7/A3，不改变 A1 自动发布，不提前实现 P2 valid_from/to/as_of。

## 当前缺口与范围

M5/A1 的旧 memory/capture 入口仍使用 workspace 内 fact_key 定位资产，capture 的 key 由模型生成；同义 key 不会自动合并，不同主体相同 key 仍可能错误更新。新显式身份入口已按完整业务身份隔离更新，expected_version 只防并发覆盖。API key 的 principal 是授权主体，不等于被记忆的用户/项目/Agent。知识与记忆共同检索不等于具备一致的事实身份。

当前单身份 capture 只校验原文 quote/区间，否定、提议、主体归属仍依赖模型；不同 quote 拒绝更新是保守歧义规则，不是通用语义冲突检测。精确身份过滤在检索候选产生后执行，仍受分支截断限制，候选下推属于后续工作。图实体名称 ID 与 MemoryIdentity 是两套身份，图谱同名消歧仍需单独评估。

本文冻结完整目标与评估反例；已交付接口以 API 文档为准，未交付目标不能标成现有能力。

## 身份契约

| 字段 | 含义与规则 |
| --- | --- |
| scope | tenant_id/workspace_id，从认证取得，不接受模型指定 |
| subject | kind + stable_id，表示被描述的用户、项目、服务或团队；名字是显示/别名，不作为唯一身份 |
| predicate | 受控事实属性，例如 database.engine、release.approval_required；属性目录按 workspace 管理 |
| context_key | 明确适用条件，如 env=production；采用规范编码，不用自然语言拼 key |
| memory_id | 服务分配的不可变 ID；唯一身份由 scope + subject + predicate + context_key 决定 |
| source/evidence | 原始事件、原文位置与抽取版本；既有 owner/source 有效性规则保留 |
| expected_version | 独立的并发更新条件，不表示事实真假或业务有效时间 |

示例：Atlas 的生产数据库属性与 Boreal 的生产数据库属性必须是不同身份；同一 Atlas 的 database.engine 由 PostgreSQL 改为 Cassandra 应定位同一 memory_id 并追加版本；Atlas 的测试和生产属性不能混在一起。调用方不能用 subject 跨越 workspace 权限。

身份归一化采用受控规则：稳定主体 ID 精确匹配，属性使用已登记名称，context 以规范编码保存。不能对任意用户 ID 大小写折叠；显示名字、别名和语义相似度仅用于候选查找，不能单独授权合并。

## capture 匹配与更新

1. 接收调用方明确的主体/上下文线索，并保存原始输入；scope 永远来自认证。
2. 抽取 factual statement 与属性/条件，不把提议、否定或猜测变成已确定事实；保留原文支持区间。
3. 先按稳定身份查现有记忆；语义召回只提供可能匹配的对象，最终精确身份决定是否复用。
4. 身份明确则按当前 expected_version 写入；重复同一事实不应制造多份资产。
5. 身份无法确定时，不自动选择一个已存在对象进行覆盖。按独立、未归一化且有来源的记忆保存，并标明匹配状态；它仍沿 A1 发布规则运行，不能伪装为已归一化的当前事实。
6. 已识别为单值属性、但输入本身存在互斥且无法解释的断言时，返回显式语义冲突；原文来源保留，当前值不变。多值属性使用集合语义，不能逐条覆盖。

身份未归一化的内容可作为来源证据参与检索，但上下文必须标明不确定性，不按“最新版本”替换确定的当前事实。别名绑定/合并需要确定性服务操作及审计；不引入人工候选审核表，也不由模型直接修改身份表。

## 记忆与知识关联

知识块提供原文证据；记忆提供可复用陈述；会话提供交互经历；Learning 提供有条件的经验。共用 EvidenceBundle 和来源治理，保留各自类型和语义，不强制所有短记忆都进行 LLM 图抽取。

记忆与知识之间的 supports/contradicts/derived_from 关联带来源和版本。知识更新或撤回只影响具体证据关联，不直接重写与其同名的记忆；失去所有有效支持的派生产物退出读取。重新推导以新的任务及版本发布。

若知识与记忆矛盾，默认同时保留有效证据并标出矛盾，不能无条件采用“记忆优先”“文档优先”或最新写入。业务可配置明确权威来源规则，但规则属于检索策略版本，不能改变权限/撤回边界。

## 上下文组装

顺序：授权/可见性过滤 → 各类型检索 → 来源与版本去重 → 冲突标识 → 按任务策略分配预算 → 渲染引用。只以同源、同版本、同范围的重复片段去重，不能因文字相似而丢掉相反证据。

P1 策略分别支持近期会话、当前已识别记忆、知识原文、条件化经验。权重/预算由评估决定，不预先硬编码通用比例。Guidance 是来源内容，不自动升级为系统指令；外部证据保持与 Agent 控制指令的边界。维持当前字节预算兼容，精确 tokenizer 仍为单独增强。

## 实施切片与验收

| 切片 | 交付物 | 核心验收 |
| --- | --- | --- |
| I0 契约/数据 | 本文、评估反例 | 相同 key 跨主体、同义 key、生产/测试、否定、互斥属性 |
| I1 身份领域 | MemoryIdentity 与确定性编码/唯一约束，兼容入口策略 | 两主体不互相覆盖，同身份追加版本，旧 fact_key 不被静默重新解释 |
| I2 抽取匹配 | 可追溯抽取与身份查找，未归一化状态 | 语义相似不能单独触发覆盖；重复内容不多建资产 |
| I3 P1 会话 | session_turns、evidence/feedback、guidance、阶段水位与 improve | 跨会话复用、反馈幂等、经验有条件且可溯源 |
| I4 上下文策略 | 类型/冲突/去重/预算策略及版本 | 预算内保留必要证据，不把冲突隐藏或把提议当确定事实 |

当前不提供历史库自动升级。新增表/列前必须明确 fresh-data 验收与旧数据兼容边界；不可在启动中静默 ALTER/重解释已有 fact_key，也不可声称现有 M5 数据自动兼容。迁移/导入路径如需支持，应单独设计和验收。

P2 保留业务时间与 GraphCompletion。存储接口的厂商解耦仍是欠账，但第二后端不是 I1–I4 的前置条件；避免把产品推进再次变成后端扩展项目。

## I1 首个代码切片：身份类型与 v1 编码

`src/memory_identity.rs` 提供 `MemorySubject`、`SubjectKind`、`MemoryIdentity`、验证和 `key(scope)`。编码使用固定 JSON tuple（版本、tenant、workspace、主体种类、稳定 ID、属性、排序条件映射）后 SHA-256，前缀为 `memory-identity:v1:`。正文、来源、授权 principal 和 expected_version 不进入身份，允许同一身份后续追加业务版本。

本切片不自动纠正大小写、Unicode 或空白；拒绝空值、边界空白、控制字符、超限字段与非法属性名。属性名语法校验不等于已实现属性目录。scope 必须由应用鉴权取得；codec 自身不提供认证或 scope 授权。

八项 Rust 单元测试覆盖主体/属性/环境、tenant/workspace、条件排序、大小写/Unicode、无效字段、超限、版本化 key 与固定编码向量。该 codec 的 8 项测试已通过上一轮 CI；本轮新增持久化测试与发布烟测另行验收。

首个 codec 切片已通过 8 项 Rust 测试。后续持久化/API 范围见下节；capture 的既有 fact_key 语义不因该切片自动改变。

## I1 持久化与显式写入切片

新增 SQLite `oc_memory_identities` 保存 scope、v1 key、资产和完整身份 JSON；scope/key 与 scope/asset 双唯一约束、复合资产外键防止身份绑定漂移。`POST /v1/memories/identified` 在授权短写事务中绑定身份、写来源/审计/幂等响应并入队，Worker 沿用 expected_version 和原有发布治理。已绑定资产拒绝旧 slot 写入；与已有 legacy fact_key 碰撞返回冲突。资产读取在可见性核验后返回身份。

新表只随新建库创建；旧库无表时旧接口仍可用，新入口 503，不在启动时迁移。已新增离线 `memory-identity-upgrade` 定向安装，见下节。CLI 检索仍默认不带身份过滤，MCP 是只读工具且 search/resolve Schema 已支持 memory_identity；显式写入与受限 capture 使用 HTTP。自动抽取身份/匹配尚未实现。新增 SQLite 契约测试与 release/container 发布烟测，验收结果以 VALIDATION 为准。

## 精确身份只读查找

`POST /v1/memories/lookup` 接受完整 MemoryIdentity，在授权读事务中用 scope + v1 key 定位绑定，再复用 asset_view 的当前版本和有效来源过滤。reader 可以读取已发布内容；未命中不创建 slot，未发布、墓碑或撤回返回 404，旧库缺表返回 503。输入校验、绑定一致性校验和提交时权限复核保留。接口不提供别名、语义匹配或跨主体条件的模糊匹配，也不将读取版本当作后续写入的锁。

## I1 兼容库的显式安装边界

只支持通过现有本地基础字段检查的库增加缺失身份表，fresh-data 与安装共用 `memory-identity-schema.sql`。必须停止宿主、独占已有库；dry-run 只报告 required/already_enabled，执行后 enabled，重复执行无变化。安装事务不更新任何业务记录；同名非表对象、未知身份 DDL、基础不兼容和锁占用均拒绝。该切片不引入自动升级历史链或 PG 迁移，后续 schema 变化需另行设计。用户可以选择继续使用旧库旧接口而不安装新表。

## I1/I4 后续已实现契约

PR #10 已合入 main `73fc51a`。显式 memory/capture 写入可选 expected_version：0 表示尚无发布版本，正数必须精确匹配受理时版本；负数拒绝，不匹配返回 409 且受理事务回滚。相同幂等请求成功重放优先返回原受理结果，省略/null 保持旧规范 payload；Worker 保留最终版本复核。实际接口见 [API](../../API.md)。

search/resolve 返回 identity、kind、normalization_status，并将完整标注计入既有字节预算；完整 memory_identity 过滤只保留精确身份 memory。未实现别名/语义匹配、主体级混合证据、冲突识别或类型预算，不能将这些切片标为 I2/I4 全部完成。

