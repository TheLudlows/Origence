# Origence · 基于 AnySeek 的 Claude Code 第一版

日期：2026-11-16。状态：M1 可选 AnySeek 全文切片已实现，并通过本地真实引擎、公开 Service、HTTP/stdio MCP 与工程回归；真实 Claude Code 客户端尚未联调。

本文记录新的产品主线、两仓职责、实施顺序和验收条件。当前运行行为仍以
[API](../../API.md)、[OPERATIONS](../../OPERATIONS.md)和代码为准；实现进度见
[STATUS](../../STATUS.md)，执行证据见 [VALIDATION](../../VALIDATION.md)。

## 已确认的产品决策

- 长期服务企业内部多个 Agent，共享受权限控制、可追溯的业务数据和上下文。
- 第一版只支持 Claude Code，通过标准 MCP 接口接入。
- 首个建议场景是开发者结合本地代码和企业内部规范、接口文档、设计决策完成开发任务；实际试点项目与资料尚待选定。
- 第一版聚焦知识搜索、正文读取、有效引用、访问隔离和更新删除的一致性。
- 当前阶段继续只面向新建数据库，不新增旧库升级或迁移要求，也不删除、覆盖既有数据。

例如：开发者要求 Claude Code 按公司鉴权规范增加接口，Claude Code 读取本地仓库，
通过 Origence 查找规范、按需读取原文，再修改代码并说明所依据的资料。

## 已核对的仓库基线

下表保留规划开始时的固定版本；M1 后续实现进度以本文实施清单、STATUS 和 VALIDATION 为准。

| 项目 | 固定版本 | 已有能力及边界 |
| --- | --- | --- |
| Origence | [`8de5817a3adf857f6224e248a57f62990ff55a8f`](https://github.com/TheLudlows/Origence/tree/8de5817a3adf857f6224e248a57f62990ff55a8f) | Rust；Turso 权威数据与任务、LanceDB 全文/向量、Turso 图、原始 Blob；只读 stdio MCP 和 HTTP 宿主转发已存在；尚未接入 AnySeek |
| AnySeek | [`6614bbf29bad0041c65738831bc1a16da4c69660`](https://github.com/TheLudlows/anyseek/tree/6614bbf29bad0041c65738831bc1a16da4c69660) | Rust HTTP 记录与检索服务；全文/向量/混合、元数据过滤、稳定身份与版本、幂等重试和整组原子替换；模型推理、解析切块和来源治理由应用负责 |

Origence 的四个现有 MCP 工具是 `context_search`、`context_get`、
`context_resolve`、`context_bundle`，见 [src/mcp.rs](../../../src/mcp.rs)。
讨论中的 `search/read` 是产品动作，第一版优先复用 `context_search/context_get`，
不据此另建同义接口；现有 resolve/Bundle 的代码和契约继续保留。

Origence 目前有 tenant/workspace、角色和 key 授权，没有企业身份接入和文档 ACL。
AnySeek 普通 key 绑定 Tenant，同租户 key 的集合访问范围一致；它的元数据过滤
不等于企业员工授权。两边现有身份能力均不能直接表述为企业权限体系已完成。

AnySeek 的现状依据固定版本的[能力边界](https://github.com/TheLudlows/anyseek/blob/6614bbf29bad0041c65738831bc1a16da4c69660/docs/features.md)
和[HTTP 服务指南](https://github.com/TheLudlows/anyseek/blob/6614bbf29bad0041c65738831bc1a16da4c69660/anyseek/src/server/README.md)。
以上是代码与文档核对，不是这两个版本的运行或质量验收。

## 目标架构与数据责任

```text
Claude Code：本地代码、任务推理和修改
    ↓ 现有只读 stdio MCP，转发到 Origence HTTP 宿主
Origence：员工/项目范围、解析、来源与版本、发布账本、引用、最终可见性
    ↓ 服务端 AnySeek HTTP 适配器
AnySeek：记录、全文/向量索引、通用过滤、候选检索、版本及写入回执
```

AnySeek 接入后，Origence 继续负责业务事实和有效证据的判定。第一批保留
Turso 权威数据、现有单 Worker、原始 Blob 和来源定位，通过适配器逐步把全文、
随后向量检索交给 AnySeek。图和会话学习不扩展为第一版前置能力。

AnySeek 持久保存其接收的 Record；这些 Record 是 Origence 的检索投影。
AnySeek 写入成功不能单独代表 Origence 已完成业务发布，两者之间没有共同事务。
检索命中仍需经 Origence 的权限、当前版本、来源撤回与发布账本复核。

### Origence 的工作

| 工作 | 第一版交付 |
| --- | --- |
| MCP 与 Claude Code 体验 | 复用现有 MCP/宿主转发；提供 Claude Code 接入说明，验证工具发现、搜索、读取和引用 |
| 身份与授权 | 开发期复用 reader key；试点前补员工与授权范围映射；服务端派生可信范围，密钥撤销按当前契约生效 |
| 项目关联 | 先以配置关联仓库、workspace 和知识源，默认项目只缩小查询范围，不授予访问权；管理后台后置 |
| 文档接入与模型 | 复用解析、切块、原文保存及 embedding；先用已授权的 Markdown/文本验证，连接器按实际数据源增加 |
| AnySeek 适配 | 建立字段、稳定 ID、版本与回执映射，接入索引发布、搜索、清理及重启恢复 |
| 引用与读取 | 保留 asset/version/chunk/source/locator；从权威数据读取有效正文，展示可核验来源；来源链接和更新时间的现有支持需逐字段确认 |
| 可观测与验收 | 记录耗时、工具/后端错误和必要证据标识；复用安全日志规则，不记录原文、查询或凭据 |

### AnySeek 的工作

| 工作 | 第一版交付 |
| --- | --- |
| 稳定 HTTP 能力 | 复用 collection、query、Record 和 group 接口，按固定版本对接 |
| 全文/向量/混合检索 | 提供通用检索及过滤；先验证全文切片，向量与融合随后接入 |
| 写入与可见性 | 保留稳定 Record 身份、版本条件、原 operation_id 重试、同步/异步各自保证 |
| 整组更新 | 复用已合入的有界 group replace；验证文档分块替换、空组清除、冲突、重试和恢复 |
| 通用过滤与资源边界 | 验证可信范围在候选截断前参与过滤，记录过滤规模、请求/响应上限及延迟；缺少的通用能力单独补齐 |

AnySeek 不承担文档解析、连接器业务、员工授权决策、来源依赖、记忆策略或 MCP
产品协议。需要扩展时优先增加通用引擎能力，企业语义留在 Origence。

## 第一批适配合同

1. **接口复用**：AnySeek 搜索使用 `POST /api/v1/index/{id}/query`；
   `GET /api/v1/index/{id}/doc/{record_id}` 可用于投影核对，但 MCP 正文读取
   继续通过 Origence 的权威读取和来源校验。该计划不新增公开 HTTP 路由。
2. **映射**：服务端配置绑定 Origence tenant/workspace 与 AnySeek
   tenant/collection。定义稳定 chunk/record 和 asset/group 映射，记录
   asset_id、asset_version、chunk_id、source_event_id、locator、发布 generation，
   向量另含 profile/dimension。AnySeek Record/group 技术版本与业务来源版本分别保存。
3. **授权**：AnySeek key 仅由 Origence 宿主持有。请求范围由认证结果产生；
   项目筛选与该范围取交集。workspace 和当前可见候选在检索截断前限定，
   返回前再次向 Origence 权威数据校验；直接读取和缓存遵守相同范围。
4. **更新与发布**：组大小和字节数在后端上限内时，用
   `POST /api/v1/index/{id}/group/replace` 与 `expected_group_version` 替换完整分块；
   空列表清除成员。超限在第一切片明确拒绝，不能拆成多次请求后宣称整份原子更新。
   `/batch` 与 `/ingest` 保持各自非原子合同。整组引擎原子性不代替 Origence 发布账本。
5. **失败与恢复**：复用现有持久发布计划和 Worker，不建立第二条业务发布路径。
   在发送前保存完整写入请求与 operation_id；超时和结果未知按原身份核对/重试，
   操作窗口到期后的未决结果保持阻断并显式处置，不能换 ID 猜测补写。
   撤回在权威侧立即隐藏，后端清理可重试，迟到任务不能复活来源。
6. **检索语义**：核对现有预分词、AND 匹配、候选范围、score/rank 和融合语义。
   AnySeek 的分词与 BM25/RRF 不默认等价于 LanceDB；适配差异须明确记录并验证。
   保持 model profile/dimension 隔离，存储或权限错误不得伪装成空结果。
7. **引用**：完整保留不可变来源版本、UTF-8 范围、PDF 一基页码和 parser 标记。
   更新不重新解释历史定位；返回内容作为外部证据，不提升为系统指令。

现有 `LocalEngine` 和 `Service::open` 具体绑定 LanceDB，全文与向量共用
`VectorStore`，并非新增一个文件即可切换。第一切片应先收敛最小后端选择边界，
再经现有公开 Service/HTTP/MCP 路径验证，避免只完成独立客户端测试。
参考 [storage](../../../src/storage/mod.rs)、[capabilities](../../../src/storage/capabilities.rs)、
[service](../../../src/service.rs)和[retrieval](../../../src/retrieval.rs)。

## 实施顺序与启动条件

### M0：范围与基线

- [x] 确认企业内部多 Agent 方向，第一版客户端仅 Claude Code。
- [x] 核对上述固定版本的 MCP、存储、检索和 AnySeek HTTP 文档，登记实际缺口。
- [x] 记录职责、发布/权限边界、实施顺序和验收条件。

M0 的勾选只表示范围和静态核对完成，不表示运行测试或适配实现完成。

### M1：AnySeek 全文与 Claude Code 最小闭环

- [x] 固定 collection schema、作用域/ID 映射、预分词匹配和错误转换合同。
- [x] 收敛后端选择，完成全文写入/清理/搜索适配，保持关系权威和发布检查。
- [x] 接入 group replace 与持久发布 epoch；以真实引擎覆盖丢失响应、64 块上限、身份冲突和原计划重开恢复。
- [x] 用独立新库和合成 Markdown/文本，跑通公开 Service 及 HTTP/stdio MCP 的 ingest → search → get → 引用。
- [ ] 基于现有 stdio 宿主转发给出 Claude Code 配置，使用真实客户端验证工具调用。

**M1 首个全文切片已完成本地验证。** 实际接口、作用域/分块映射、词项编码、
64 块/2 MiB 上限与恢复合同见 [API](../../API.md)，配置和 Claude Code 示例见
[OPERATIONS](../../OPERATIONS.md#anyseek-全文与-claude-code)，精确结果见 [VALIDATION](../../VALIDATION.md)。
下一项是在可用的真实 Claude Code 环境完成客户端联调；工程与 stdio MCP 测试不替代该验收，
企业试点及真实开发任务仍按 M2/M3 推进。

### M2：内部受控试点

- [ ] 确定一个项目和一到两个知识源，补仓库与知识空间关联、更新/删除同步。
- [ ] 落实员工身份与授权映射；不同文档权限混存前完成并验收 ACL 传递和读取复核。
      初始可用权限一致的项目 workspace 限定试点范围，不能据此宣称文档 ACL 已实现。
- [ ] 验证失效凭据、跨 workspace、无权文档和伪造项目/过滤参数均无法扩大访问。
- [ ] 验证来源撤回、权限收缩、缓存失效及后端失败时的可见性与错误行为。
- [ ] 按需要接入 embedding、向量及 hybrid，验证模型/维度隔离与排序差异。

### M3：真实开发任务验收

- [ ] 用冻结任务比较 Claude Code 原有工作方式与 Origence 接入后的任务完成情况。
- [ ] 检查引用是否真正支持代码决策，更新后能否读到正确版本，无资料时是否明确缺失。
- [ ] 记录检索与任务耗时、调用量、成本和接入体验，基于基线确定可接受目标。
- [ ] 按最终代码 SHA 记录适用工程门禁和真实客户端结果；桩测试不替代产品效果。

## 第一版完成标准与后置范围

员工在 Claude Code 中提交依赖内部资料的开发任务，能够在自己的授权范围内搜索、
读取当前有效内容、完成修改并给出可核验引用；更新、删除、撤权和重启不破坏该保证。
只接通 MCP、单独调用 AnySeek 或跑通合成 fixture 均不能独立满足完成标准。

本轮后置：其他 Agent 客户端、新的业务知识图谱、跨 Agent 编排、自动共享记忆、
完整 SaaS 控制面和通用连接器平台。既有记忆/Bundle/图能力保留其原有合同和验收边界。
完整企业 SSO 可随实际接入方式选型，文档权限有差异时的授权隔离属于试点门槛。
原可靠性待办继续登记，不把旧版本或旧后端的验证结果外推到 AnySeek 适配。

后续实现 PR 关联本计划中的具体 M1–M3 项，同步 STATUS、API/OPERATIONS 中实际
发生的接口/运行变化和 VALIDATION 的精确结果；计划中的接口与配置不得写成现有功能。
