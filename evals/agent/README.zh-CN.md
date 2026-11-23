# S4有界代理比较

**简体中文** · [English](README.md)

**实时模型/产品比较：NOT_RUN。**生成的本地测试运行仅用于确定性测试。它无法建立起源检索结果、模型质量、任务完成度、语义蕴含或生产准备性。未使用付费API、远程模型、模型下载或现有用户数据修改。

## 冻结范围

`freeze.json`在真实推理之前会哈希处理`corpus.json`、`cases.json`、`labels.json`和`config.json`。这十个由作者编写的合成任务涵盖了发布计划、跨会话移交、更新后的决策、条件学习（正面和负面）、未解决的冲突、撤销支持、无答案、环境/实体范围以及被视为数据的外部指导。这些任务并未重复使用S1/AML问题，也没有经过独立的人类评审。脚本化的测试运行并非针对未知质量的评估。在观察实时结果后进行的更改需要新的数据集/版本；切勿重写旧结果或其冻结版本。

这四个代理共享一个代理提示、模型/修订版本生成策略、动作模式、调用上限、输出令牌限制和上下文上限：

- `no_memory`：检索工具不会返回任何证据。
- `simple_hybrid`：实际使用的 `/v1/search`，仅限混合原始数据（摘要/图表模式）。
- `evidence_bundle`：实际使用的 `/v1/context`，内存/知识/学习/会话路径。
- `gold_diagnostic`：由ORACLE选定的冻结原始证据；一个上层上下文诊断工具，不是可部署的检索器。答案别名和评分标准永远不会进入模型提示中。

该模型会选择每个 `retrieve(query)` 或 `finish(answer, citations)` 操作。在做出下一个选择之前，它先观察真实的工具行为。因此，这是一个有界代理循环，而不是一个保存了答案的代理。每一次尝试，包括错误，都会消耗其预算。一个案例最多可以使用四次模型调用，三次工具调用，每次模型调用需要消耗512个输出令牌，24,000个序列化后的UTF-8上下文字节，以及7,000个证据观察字节。携带证据的观察字节包括模型面对的包装层。空的工具记录会消耗上下文预算，但不会消耗证据预算。UTF-8字节并不是模型令牌的精确计数。每条路径的候选者限制并不是全局检索工作预算；模型调用和嵌入成本必须分别报告。

所有模型输入、输出、工具原始响应、交付/遗漏决策、计数器、延迟、源绑定和错误都会被保留下来。当没有完整报告时，模型的使用情况为`null`；未定价的计算/总成本仍然为`null`。对于已经完成的明确本地运行，本地提供商的费用可能为零。每个工件都会记录输入、配置、代码、提示、模式、模型快照和绑定哈希值。Arm命令会在不同的案例之间轮换；这个小的顺序样本不支持p99/SLO声明。

## 在没有模型的情况下运行该工具。

从仓库的根目录开始（仅限Python标准库）：

```sh
python3 -m unittest discover -s evals -p 'test_*.py' -v
python3 evals/agent/runner.py --mode plan --output target/s4-plan
python3 evals/agent/runner.py --mode fixture --output target/s4-fixture
```

输出目录不得已经存在。`plan`会明确写入`NOT_RUN`。`fixture`使用了一个标签盲脚本化适配器，并在内存中进行检索；它只验证管道、边界、评分和归档。fixture的数值指标并没有被描述为代理或产品质量。

现有的 `evals/local_models.py` 将 JSON 对象聊天转换为图提取。不要修改或使用该聊天路由用于 S4。`evals/agent/local_chat.py` 是一个独立的、通用的 JSON 模式聊天适配器，其中系统/用户消息保持不变，真实生成受到限制，采用贪婪的 CPU 推理方式，没有缓存，也不信任远程代码，仅使用现有的本地 safetensor 快照。可选的运行时包遵循仓库中的 `evals/requirements-local.txt` 文件；这里不会发生任何包/模型安装操作。

## 实时前提条件（不执行）

实时运行需要明确的授权，用于本地推理以及所有相关操作：

1. 现有的本地模型快照、模型名称、不可变的修订版本、运行时以及模型文件的哈希值。适配器不会下载任何内容。
2. 使用全新的隔离 Origence 数据库及 workspace，只导入此合成语料。运行器只读，不初始化或改写服务。准备阶段在另行授权后使用公开 `/v1/knowledge`、显式或语义记忆、SessionTurn 以及 Improve API。保留原始语料文字、版本、条件、生命周期与来源到产品的绑定。会话来源含精确 `session_question` 和 `session_answer` 种子字段。Learning 必须是保留条件的真实 Learning，适配器不能把普通知识重新标为 Learning。其来源陈述为冻结正文，条件精确为 `environment=staging` 和 `schema=7`。准备完成后保存实际 Conditions/Statement/Why 包装正文的 SHA。评测前撤回 `revoked-code`。
3. 一个 writer-or-higher 令牌，该令牌与那个合成工作区相关联，因为会话路由是明确请求的。仅将令牌保存在 `S4_ORIGENCE_TOKEN` 中；它永远不会写入报告。在评估过程中不会发生任何归档或反馈写入操作。
4. 一个准备好的绑定文件，包含精确的数据哈希值、产品提交/二进制哈希值，以及本地嵌入模型/版本文件的哈希值。所有当前的源都必须被绑定。每个产品块/轮次都需要其精确的当前内容SHA值、源事件和不可更改的坐标；未绑定或已被撤销的证据将无法通过验证，而是会默默地进入提示框。学习/会话包装器会被验证并保留。
5. 起源配置为使用相同的本地嵌入端点，没有远程备用方案。操作员必须验证这一配置；API不会暴露主机的秘密配置。运行器会在检索之前检查声明的字面循环回端点实际的`/metadata`与绑定信息，并在使用之前或之后进行归档。这种元数据检查并不能证明主机的私有配置。

最小化的绑定形状（使用真实验证过的坐标/哈希值，永远不要使用这些占位符）：

```json
{
  "synthetic_only": true,
  "fresh_database": true,
  "freeze_sha256": "SHA256_OF_FREEZE_JSON",
  "workspace_id": "WORKSPACE_UUID",
  "product_provenance": {
    "commit": "40_HEX_COMMIT",
    "binary_sha256": "64_HEX_BINARY_HASH",
    "embedding_base_url": "http://127.0.0.1:8098",
    "embedding_model": "PINNED_LOCAL_MODEL",
    "embedding_revision": "IMMUTABLE_REVISION",
    "embedding_files_sha256": {"config.json": "64_HEX_FILE_HASH"}
  },
  "sources": [
    {"type":"asset_chunk","source_id":"release-approval","asset_id":"ASSET_UUID","asset_version":1,"chunk_id":"CHUNK_UUID","source_event_id":"EVENT_UUID","content_sha256":"64_HEX_CONTENT_HASH"},
    {"type":"session_turn","source_id":"handover-primary","session_id":"SESSION_UUID","turn_id":"TURN_UUID","source_event_id":"EVENT_UUID","content_sha256":"64_HEX_CONTENT_HASH"}
  ]
}
```

在满足了这些前提条件和审批之后，显式的仅本地命令如下：

```sh
S4_ALLOW_LOCAL_MODEL_CALLS=true python3 evals/agent/local_chat.py \
  --allow-local-inference --model-path /approved/existing/snapshot \
  --model MODEL --revision REVISION --port 8099

S4_ALLOW_LOCAL_MODEL_CALLS=true python3 evals/agent/runner.py --mode live \
  --allow-local-model-calls --chat-url http://127.0.0.1:8099 \
  --model MODEL --model-revision REVISION --origence-url http://127.0.0.1:8080 \
  --bindings /approved/synthetic-bindings.json --output target/s4-live-new-run
```

仅允许使用字面循环回HTTP操作；拒绝代理、重定向、URL凭据/查询秘密以及远程端点URL。HTTP操作有超时限制和有限的数据读取能力；这不是模型处理取消的硬性保证。如果准备就绪/审批失败，则标记为`NOT_RUN`。在尝试之后中断执行并不代表比较已经完成。

简单的基线仅接收实际返回的产品字段，而不是冻结的Oracle条件或定位器。EvidenceBundle自己的`rendered_context`以不变的形式传递，作为一次不可信任的观察结果，同时保留冲突组和源引用。如果剩余的证据/上下文预算不足以容纳更多内容，则保留完整的内容或省略整个内容。后续的检索会使用剩余的证据预算来请求更小的包。渲染器允许保留512字节用于包装器/警告信息；实际观察结果在交付前就已经被测量了。任何剩余的多余部分都会被省略，永远不会被截断。

## 分数解释和分母

每个冻结案例都会出现在每个分支中，包括失败的请求。缺失或重复的行会被拒绝。可纠正的错误被视为零次成功和零次所需来源覆盖。不回答的错误属于错误，永远不会成功。不回答的案例仅从所需来源覆盖的分母中排除（它们需要零个来源），而不是任务成功的分母。

- `task_success_heuristic`：期望的回答别名、所有必需的引用来源ID、有效的引用，以及没有禁止的回答字符串；或者对于不回答的任务，有效的回避行为。这可能会接受不正确的额外声明和有效的改写。
- `citation_source_support_heuristic`：将引用的ID映射到必需的来源ID，除以所有引用的ID，包括虚构的ID。这是来源的相关性，**而不是包含关系**。
- `required_source_coverage`：通过实际提供的ID引用的必需来源，除以所有必需的来源。检索/交付与使用是分开跟踪的。
- 缺失的引用有单独的计数。当没有引用存在时，引用精度为`null`，而未经引用的答案无法通过任务成功。
- 独立的人类评审/语义包含关系：`NOT_EVALUATED`。

使用`gzip.open(path, 'rt', encoding='utf-8')`读取`traces.jsonl.gz`，以检查每一个原始文本。报告只包含合成语料库；不要重复使用他们的公共工件政策来处理私人数据。计划和配置只是工程证据。原始运行工件被保留在本地；发布需要明确的批准，因此它们没有包含在这个PR中。使用上述命令将它们复制到新的输出目录中。
