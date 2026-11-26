# 记忆写入流程调研

调研日期为 2026-11-26。对照 Hindsight、cognee 和 Supermemory，重点跟踪一条新文本如何变成可检索的事实，以及系统如何找到、判断和更新已有记忆。用「住宿报销上限由 100 元改为 80 元」贯穿对照。

**建议 Origence 先补服务端候选发现，并提供只读预览。** 让接入方提交原文和来源，由服务在获授权的工作区内找可能对应的旧记忆。预览展示候选、原文和缺失条件。获准更新时，沿用 Origence 已有的身份、来源、版本和发布检查。召回做错、模型判断做错、发布规则做错，分别验收。

| 系统 | 调研基线 | 证据范围 |
| --- | --- | --- |
| Origence | `78f08ab95494eb9cca9944e3a87fa0181374e1ef` | 主干实现、契约、已有离线接线记录 |
| Hindsight | v1.3.0 `937886199e0e3443f41f4d8180a16b12cc95c547` | 官方文档和该提交的 Python 服务端源码 |
| cognee | v1.6.10 `c8a094375821f2d8c7c4945d3b2202e2e5d6603e` | 官方文档和该提交的 Python SDK/服务端源码 |
| Supermemory | 托管 API v5，公开仓库 `4fdfbe4516535be27ec5a77e0b8e896080079295` | 官方 API、产品流程和公开接口类型。写入候选选择和冲突判定的服务端实现未在所查资料中证实 |

Hindsight 和 cognee 沿用 [#152](https://github.com/TheLudlows/Origence/issues/152) 的冻结版本。Supermemory 纳入这份写入调研。#152 已有的文档和会话接线结果使用原文检索路径，无法据此判断三家的事实抽取、冲突判断或更新准确率。本调研只做文档和源码核查，酒店案例用于说明机制和设计验收，实测结果待记录。 [#152 跨会话接线记录](https://github.com/TheLudlows/Origence/issues/152#issuecomment-7193961142)

## Hindsight 从原始事实到观察整合

1. **接收。** `retain()` 向指定 memory bank 写入内容。可传 `context` 帮助识别发言人，提供时间、标签和 `document_id`。存在同步和异步入口。同一 `document_id` 默认表达替换，`update_mode="append"` 表达追加。符合 delta retain 条件时，系统按块哈希复用不变块，重提取变化块，并在写入时复核基线。独立旧政策和新政策可用不同 `document_id` 保留两份证据。 [Retain API](https://hindsight.vectorize.io/developer/api/retain) · [源码 · 块差分与并发检查](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/retain/orchestrator.py#L4046-L4099)
2. **提取。** 原文切块后，LLM 抽取带上下文的事实、实体和时间。`world` 表示外部世界的事实，`experience` 表示 memory bank 所属 Agent 的行动或经历。事件发生时间和陈述时间分别记录。默认提取路径可用 `retain_mission` 收窄关注范围。`chunks` 模式直接保存切块，适合测试原文召回。 [Retain 指南](https://hindsight.vectorize.io/developer/retain) · [源码 · 事实输出结构](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/retain/fact_extraction.py#L258-L322)
3. **存储和连边。** 事实向量化后进入 memory bank，文档和块提供来源。实体识别使用名称、共现、时间等信号来复用或新建实体。时间、语义和因果链接写入存储。PostgreSQL 的实体检索走事实与实体的关联，单独物化的实体边用于界面展示。实体解析在判断「是不是同一实体」，例如两种叫法指向同一个部门。 [源码 · 实体匹配](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/memories/pg/entity_resolver.py#L1275-L1382) · [源码 · 事实连边](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/retain/orchestrator.py#L930-L994)
4. **发现旧观察。** 启用自动整合时，写入后调度 consolidation。每条新事实的文本作为查询，批量生成查询向量，再使用银行和标签范围、召回预算、来源预算找已有 `observation`。实现以交错方式合并不同召回分支，降低近似旧观察挤出模型上下文的概率。召回失败会使整批留待重试，以免错误地把有旧观察的情况变成「创建新观察」。 [源码 · 整合调度](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/memory_engine.py#L7625-L7657) · [源码 · 按事实召回](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/consolidation/consolidator.py#L2769-L2808) · [源码 · 召回范围和排序](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/consolidation/consolidator.py#L3730-L3811)
5. **更新观察。** 模型读新事实、候选观察和有日期的支持事实，决定创建、更新或删除观察。可选决策模型先处理明显复述或噪声。更新会追踪 `source_memory_ids`，写入时复核支持事实存活，并可保存观察历史快照。实现会给比本批新输入更新的观察加标记，帮助模型把迟到的旧事实记录为历史。原始事实、观察和观察历史有各自的生命周期。 [观察整合](https://hindsight.vectorize.io/developer/observations) · [源码 · 观察更新和历史](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/consolidation/consolidator.py#L3372-L3552) · [源码 · 迟到事实提示](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/engine/consolidation/consolidator.py#L3816-L3850)

**酒店案例。** 两份独立通知都可成为原始事实。consolidation 可以召回「国内差旅住宿每晚 100 元」这一观察，并依据有生效日期的新通知改写为 80 元。这是支持证据上的观察整合。若输入只有「酒店现在最多报 80 元」，真实模型能否正确保留「每晚」未知、避免误合并，仍需测试。

**知识文档走另一条入口。** v1.3.0 的 Knowledge Bank 将原文件拆成带位置的 passages，再嵌入索引。标准路径无需生成式事实抽取。原文入库和记忆整合由不同入口和状态表示，`reflect` 可同时取证。 [1.3.0 发布说明](https://hindsight.vectorize.io/blog/2026/11/24/version-1-3-0) · [源码 · Knowledge Bank 服务](https://github.com/vectorize-io/hindsight/blob/937886199e0e3443f41f4d8180a16b12cc95c547/hindsight-api-slim/hindsight_api/knowledge/service.py#L1434-L1455)

## cognee 从文档建图到冲突审查

1. **接收和暂存。** `remember(data)` 是长期记忆便捷入口，顺序执行 `add()`、`cognify()`，并在启用时执行 `improve()`。`add()` 先解析写入用户和目标 dataset 权限，再存数据和登记元数据。普通内容按 dataset、owner、tenant 下的内容哈希去重，也可指定稳定 `data_id`。原路径或同名上传文件已存在且内容改变时，`add()` 预检拒绝，调用方应走 `update()`。不同的裸文本默认是不同文档。 [数据流](https://docs.cognee.ai/core-concepts/data-flows) · [源码 · `remember` 编排](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/api/v1/remember/remember.py#L2402-L2470) · [源码 · 来源冲突预检](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/tasks/ingestion/refuse_changed_existing_documents.py)
2. **分类、切块、建图。** `cognify()` 加载和切块。LLM 路径提取实体、边和摘要，并将 DataPoints 和向量写入图与向量存储。提取器默认按配置自动选择，有可用 LLM 时选 LLM，无凭据的本地默认配置可走 GLiNER。另有可选 ontology 及自定义 Pydantic 图模型。开启 early chunk indexing 时，原文先达到可检索状态，后续建图复用已有块向量。默认实体的名称可构成稳定节点身份，边以两端节点和关系名识别。仅复用节点或边不能决定两个值谁当前有效。 [Cognify](https://docs.cognee.ai/core-concepts/main-operations/legacy-operations/cognify) · [源码 · 任务序列及预索引](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/api/v1/cognify/cognify.py#L695-L783) · [源码 · 提取器选择](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/modules/cognify/config.py#L168-L201) · [源码 · 实体与边构造](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/modules/graph/utils/expand_with_nodes_and_edges.py)
3. **更新同一来源。** `update()` 依据 `dataset_id` 和 `data_id` 保留文档身份。增量路径先暂存新文件和校验切块计划，再替换变化区段、重建受影响节点/边/向量并发布文档行。无变化、增量、全量重建和失败有不同结果。全量重建会先撤下旧图产物，因此失败后的旧来源、图产物和就绪状态要分别核验。该接口处理的是文档修订，独立文档之间的说法变化进入冲突审查。 [Update API](https://docs.cognee.ai/python-api/update) · [源码 · 增量阶段与发布](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/api/v1/update/incremental.py#L837-L922) · [源码 · 全量回退](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/api/v1/update/update.py#L332-L416)
4. **发现跨来源候选。** `improve()` 的 `review_conflicts` 阶段需要启用审查、有可用 LLM 且打开边证据。首次审查该 dataset 的实体，后续用处理水位找变更实体和需重试项。对于每个主体，按图中的多个值、来源文档和文本陈述挑出待比较的事实。例如值组至少需要两个值和两个不同来源文档。这条明确的候选入口适合独立做召回率测试。原文陈述配对含英文词和字面量启发式，中文覆盖待验证。 [源码 · 阶段开关与水位](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/modules/improve/stages.py#L539-L579) · [源码 · 实体事实候选选择](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/tasks/memify/review_conflicts/select_review_facts.py)
5. **审查和采用。** 模型用程序分配的引用标签建议 `current`、`superseded` 或 `conflicting`。程序拒绝未知或重复的事实标签，核对事实与主体的关系，日期顺序相反则保留冲突。结果写成带来源的 `FactConflict` 和事实标记。用户也可显式选择已有事实，系统保留被驳回的历史证据，后续自动审查会尊重仍有效的人工决定。混合检索的一条实体事实路径会在已入选的结果中后排 `superseded`，具体呈现仍取决于检索入口。 [源码 · 结果校验](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/tasks/memify/review_conflicts/accept_review.py#L126-L177) · [源码 · 人工选择](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/modules/conflicts/methods.py#L127-L165) · [源码 · 混合检索排序](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/modules/retrieval/hybrid/entities.py#L353-L379)

**酒店案例。** 如果两份通知被建成同一主体的两条限额事实，Improve 可以对比来源和时间，标记 80 元当前、100 元历史，或保留未决冲突。实体被拆成两节点、字段口径不同或中文陈述配对失败，都可能使它们进不了同一候选集。若编辑的是同一份手册，则走 `update()` 的文档和派生图重建。

**会话写入另有快路径。** `remember(..., session_id=...)` 的普通会话文本先存会话缓存，随后按自学习配置由 `improve(session_ids=...)` 处理。即时可读的会话内容和整理后的长期图有不同就绪时点。 [数据流](https://docs.cognee.ai/core-concepts/data-flows) · [源码 · 会话与长期入口](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/api/v1/remember/remember.py#L1120-L1155)

## Supermemory 的事实抽取和版本关联

1. **接收。** v5 使用 `POST /ns/{namespace}/document` 或 `client.add(namespace, ...)` 写原文、URL 或文件。接入方可传稳定文档 ID，同 ID 的 `add` 表达追加/差分。`PATCH` 带 `content` 表达替换文本来源并重新处理，只改文档元数据则保持原文。`supportingContext` 会影响 namespace 的后续提取，适合作为受管配置。调用方提交来源文档，无需提供旧事实 ID 或声明 `updates`。 [写入 API](https://supermemory.ai/docs/ingestion/add-memories) · [v5 文档更新语义](https://supermemory.ai/docs/migration/api-v5-document-updates)
2. **原文索引。** 入队后提取原文、按类型切块、嵌入并建立索引。`system.status=done` 表示文档索引完成。`taskType="superrag"` 走原文检索路径，跳过事实抽取、图关联和 profile 更新。 [处理流程](https://supermemory.ai/docs/concepts/how-it-works) · [SuperRAG](https://supermemory.ai/docs/concepts/super-rag)
3. **抽取及关联。** 默认 `taskType="memory"` 进入官方称为 Dreaming 的阶段。`dynamic` 将相关文档成批处理，`instant` 尽早单独处理当前文档。`system.dreamingStatus=done` 表示记忆生成完成或该文档无需生成记忆。该阶段形成原子记忆，并与已有事实建立 `updates`、`extends` 或 `derives` 关系，也可更新 profile。已证实自动关联属于写入阶段。公开读取 API 的 `threshold`、`rerank` 等参数不足以推断写入的候选生成、匹配阈值或图遍历顺序。 [文档状态](https://supermemory.ai/docs/ingestion/document-operations) · [图记忆](https://supermemory.ai/docs/concepts/graph-memory)
4. **公开结果和历史。** `updates` 表示新事实取代旧事实作为当前的召回结果，旧事实保留为历史。`extends` 表示两条事实均有效，`derives` 表示进一步推导。单条记忆读取公开 `version`、`isLatest`，可带 `related` 返回前后版本和关联事实，也可带 `documents` 查来源文档。公开资料不足以确认企业政策的来源优先级、条件缺失时的决策准则，或能否按精确原文区间逐条审查覆盖决定。 [v5 历史读取和范围](https://supermemory.ai/docs/migration/api-v5#retrieve-a-memory-and-its-history)

**酒店案例。** 官方语义允许模型判断一条明确的 80 元新政策 `updates` 100 元旧政策，再由图保存版本关系。对于只写「酒店现在最多报 80 元」或「听说改成了 80 元」的输入，实际结果须等服务处理完后检查最新标记、版本链和来源。官方例子无法证明按晚、按次或不同地区会正确分流。

## 把三种「更新」区分开

| 系统 | 来源文档变了 | 两份来源说法不同 | 决策结果和历史 |
| --- | --- | --- | --- |
| Hindsight | 同一 `document_id` 进行替换或追加，符合条件时按块差分 | consolidation 以每条新事实查询旧观察，由模型决定如何整合 | 原始事实链接到观察，观察可记录历史快照 |
| cognee | 按 `data_id` 更新，增量替换或重建图 | Improve 以变更实体及其有来源的事实找冲突，模型提出状态，用户可裁决 | 冲突节点和事实标记保存来源及先前证据 |
| Supermemory | 文档 ID 上追加/差分或显式替换 | Dreaming 在 namespace 内自动关联，内部候选规则未知 | 版本关系、`version` 和 `isLatest`，可追查来源 |
| Origence | 来源和发布记录支持溯源和撤回 | 语义写入能检查已给定的目标与冲突，待补服务端候选发现 | 精确身份、并发版本条件和确定性发布规则 |

**图的作用随阶段而变。** Hindsight 用实体、语义等连接帮助召回，整合阶段以新事实文本召回旧观察。cognee 的冲突候选实际从主体的图事实及来源中取。Supermemory 的图公开呈现判断后的关系，写入时怎样找到候选仍待确认。Origence 可先用现有结构化条件和检索选候选，再用实验测出图扩展有无增益。

同样要分别核验来源撤回与派生记忆可见性。Hindsight 支持按源文档删除其事实。cognee 可按 data item 清理衍生图和向量，`memory_only=True` 时保留原文以供重建。Supermemory 可先预览语义匹配，再按确切记忆 ID 软删除以固定操作对象。三者这些入口的可见性变化应作为单独测试，尤其要核查多个来源支持同一事实时的结果。 [Hindsight 文档删除](https://hindsight.vectorize.io/developer/api/documents#delete-document) · [cognee 源码 · 忘记数据](https://github.com/topoteretes/cognee/blob/c8a094375821f2d8c7c4945d3b2202e2e5d6603e/cognee/api/v1/forget/forget.py#L36-L103) · [Supermemory 记忆操作](https://supermemory.ai/docs/recall/memory-operations)

### 竞品复测的观测点

在每个系统的隔离范围内依次写两份明确日期、独立来源的合成通知。另开一组，用同一个文档 ID 重交、追加或修订，验证来源更新路径和独立事实更新路径。记录提交后原文、事实以及整合结果三个可读时点，并检查同一查询是否能解释当前值和旧值来自哪里。

| 系统 | 写入后查看的证据 |
| --- | --- |
| Hindsight | 先读取 `world` 事实及文档来源，随后读取 `observation`、支持事实和可用的历史。分别记录事实和观察开始可见的时间。 |
| cognee | 查看文档、原文块及已生成的实体边。等待 `improve()` 中 `review_conflicts` 实际完成，检查冲突的证据、日期和 `current`／`superseded` 标记。把阶段被跳过与无冲突分别记录。 |
| Supermemory | 等待文档及记忆状态分别完成，按 `searchMode="memories"` 查当前条目，再读取 `related` 与 `documents` 检查版本和来源。 |

保持输入措辞和历史顺序一致，分别记录每家的原生输出。比如 Hindsight 的观察改写、cognee 的冲突状态和 Supermemory 的 `updates` 边，都可以解释为当前值变化，但需要保留原生证据以便复核。

## Origence 的优化顺序

### 1. 定义独立的候选发现和发布结果

当前 `/v1/captures/semantic` 接受原文和最多 20 个允许目标。身份由工作区、主体稳定 ID、目录属性和适用条件确定。模型返回目标索引、操作和值的原文引文。没有确定目标时保留未归一化证据。当前 worker 在事务外调用模型，持久化路由计划，并在发布前重新检查已使用的目标、来源和版本。 [受控提取契约](../contracts/semantic-capture.md) · [记忆身份与语义](../contracts/memory.md) · [现有 worker](../../src/worker/semantic_capture.rs)

第一版增加一个**只读预览**。模型根据原文、同段上下文和属性目录提取一条条主张，保留逐字引文、金额单位、肯定或提议状态、已知条件和缺失条件。服务在当前认证范围内，先按可确定的 subject、predicate、context 筛选，再用同义词、词法和向量检索补召回，用有界候选集展示旧值、来源、当前版本、匹配理由与缺项。当前值参与对比，更新案例中不能按金额相等过滤候选。只有近似名称或相关实体时，把关系标为待确认。目标检索失败单独返回失败状态，和确实无候选区分。

预览可先用结构化精确召回、再与混合检索做消融比较。同一看似正确的候选若存在跨工作区、已撤回、过期版本或越权问题，在候选进入模型上下文前就要排除。图扩展若加入，也须满足相同范围约束，记录候选来自哪条路径。

### 2. 复用确定性发布和明确的写入授权

第一版让获授权的写入者确认候选并使用现有确定性路由。预览带回目标版本与证据区间，正式写入复核主体、目录属性、全部必填条件、来源和生效日期、`expected_version` 与更新权。重复事实走幂等路径。同一身份出现两个值且来源关系不明确时，保存冲突证据。只有已确认的同一身份和获准的更正才生成新版本。

后续评估是否为特定管控来源开放自动发布。区分原文声称的生效时间、系统接收时间和发布版本时间，由产品规则决定哪条来源可改变当前值。没有业务生效日时保留未知。旧政策迟到、试行与正式并存、适用范围变化都需要明确决策。沿用现有 worker、持久化计划、重试复用和发布时复验。跟踪重复提交、并发写入、来源撤回及权限撤销，用现有契约约束重放后可见的版本。

### 3. 用同一批案例分别验收抽取、匹配和发布

测试输入应包含同一主体的历史事实和多个高相似的干扰条目。每项独立标注原文引文、候选旧记录、操作、现行值和历史值。抽取与检索测试可先固定候选或事实，隔离故障所在，最后再从原文跑完整链路。

| 输入或故障 | 预期发布结果 |
| --- | --- |
| 已发布「国内差旅住宿每晚 100 元」，授权通知明确写明同一政策于 2026-12-01 调为每晚 80 元 | 识别同一身份。保留旧版本和生效界线，按设计验证生效日前后查询 |
| 旧记忆为「国内差旅住宿每晚 100 元」，只输入「酒店现在最多报 80 元」 | 能找到候选，列出缺失的范围和口径，当前版本保持 |
| 另有外部承包商每次 80 元、另一地区每晚 80 元 | 各条件单独建身份，确保相关候选未被错误合并 |
| 「建议改为 80 元」「听说改为 80 元」或两份同级可靠来源冲突 | 记录提议、未核实或冲突，当前值保持，能查看证据 |
| 换一种说法重复 80 元，随后迟到一份写 100 元的旧通知 | 等价重述不增版本，旧通知可追查，当前值保持 |
| 两个写者同时看到 v1，分别提出 80 元和 90 元，或确认后权限被撤销 | 基于同一旧版本的两个不同更新最多一个生效。权限撤销或版本过期时停止发布 |
| 候选服务超时、同一写入重放、写入后撤回唯一支持来源 | 超时与空结果区分，重放复用原决定，撤回后按现有规则退出读取 |

建议分别记录以下分子和分母。**抽取**统计主张精确率、召回率、原文／金额单位一致率及缺失条件正确标注率。**匹配**统计有旧条目时的目标 Recall@5、中文改写子集、无目标误匹配率。**发布**统计正确更新、误覆盖、漏更新、重复增版及跨范围泄漏，误覆盖和跨范围泄漏分别报告原始计数，一经发现即暂停自动发布验收。**运行**记录从接收到原文可读、候选可审、发布可读的 p50/p95，以及每阶段的模型调用、失败、重试和超时。按 [#152](https://github.com/TheLudlows/Origence/issues/152) 的习惯固定数据、模型、预算和版本，历史修订、安全边界和答题准确率分别给结论。

上述日期例子的按时可见性是后续设计验收项。现有 `expected_version` 表示并发写条件。业务生效日期仍需另行建模和验证。 [Origence 记忆契约](../contracts/memory.md)
