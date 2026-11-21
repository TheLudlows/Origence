# P6 独立签认与 Run 核验证据

逐题、逐 Run 使用一个受限证据包。以[空白关联表](p6-case-link.template.json)登记题号、场景、客户端、基线/复用臂和 Run。证据包沿用[执行手册的索引](RUNBOOK.md#真实闭环与精确加载证据)，并将下列材料关联到同一份封存索引。

正式核验从[签认表](signoff.template.json)的 `freeze_sha256` 开始。独立核验人先核对它与独立保管人提供的冻结记录原始字节 SHA-256 一致，再核对该记录的 `data.tasks_sha256` 与实际 `tasks.json` 原始字节一致。按[题集与评分表](RUNBOOK.md#题集与评分表)核对观察表与同一题集的绑定。在该观察表内用题号、场景和臂定位唯一一行。按评分器的 `input_digest` 重算证据包内实际交付的题目输入，与该行 `input_sha256` 比对，并从交付记录核对同一 Run。题号与臂可以在不同批次复用。

1. **固定 Run 依据。** 由有权的 Run owner 按 [#89 契约](../../docs/contracts/run-verification.md)读取指定位置的不可变前缀，保留回执及引用事件。将 Run ID、`through_sequence`、前缀回执的 `digest`（填作 `prefix_digest`）和 `basis_sequences` 抄入索引。逐条核对依据序号在该前缀内且为正整数、互不重复，并保留事件与资产精确版本及来源的对应关系。只在获授权的范围内提供证据。
2. **封存原始索引字节。** 索引登记上述四个字段和材料的相对路径、SHA-256、字节数、UTC 时间、采集人、证据类型、适用条款、执行项和题号。逐项核对字节后，对最终索引文件的原始字节计算 SHA-256，填写 `evidence_index.sha256`。保留原始文件及换行。索引摘要与 Run 前缀摘要分别核对。
3. **保留独立签认原件。** 独立核对人使用已有签认方式，在原件或原件明确引用的附件中确认同一 Run ID、序号、前缀摘要、依据序号及索引 SHA-256。原件同时保留其身份、独立性、核验时间、核验范围、判断和理由。按已有受限渠道向获授权者核对原件来源，并登记原件引用和自身 SHA-256。摘要用于核对字节，独立身份和判断依据由核对人及原件确认。
4. **分别记录 owner 转录和回执。** 有写权限的 Run owner 按契约转录结论与依据。核对回执顶层 `run_id` 及 `assessment` 内的 `verification_id`、`through_sequence`、`prefix_digest`、`basis_sequences`，并将 `assessment.conclusion` 与独立原件核对。保留回执引用、摘要和撤回状态。原件、索引摘要和文件引用留在受限证据包，提交沿用现有严格 DTO。回执应保留 `evidence_origin:owner_transcribed_external`、`verifier_authentication:unverified_claim`、`platform_verification:not_performed`。单列服务准备、客户端自报、可观察的实际使用与独立判断。服务准备 v2 或客户端自报成功时，实际使用版本仍可能为 UNKNOWN。
5. **按原标准登记 P6。** 将同一受限关联表的引用写进已核对观察行的 `evidence` 与签认表的 `matrix.P6.evidence_refs`。交给独立核验人时，保留对可信冻结记录、同批题集及该观察行的受限引用。绑定缺失或不一致的条目不得纳入正式 P6。核对索引、独立原件与 owner 回执是否绑定同一 Run 依据，记录缺失、拒绝访问、撤回和不一致。独立人按既有 P6 口径汇总全部应核验题，PRD A6 与 P7 各按其既有条款单独验收。样例或单件证据只对应列出的 Run。

索引只列签认前的依据文件。独立原件、回执与关联表在同一证据包中单独保存，关联表用各自的引用和摘要绑定它们。文件引用相对证据包根目录。封存后改动依据时另立索引及复核记录，保留历史。签认、文件存取及 Run 读取仍按各自授权。读取被拒或来源撤回时，登记可见状态并保持内容受限。

尚未执行的环节登记 `NOT_RUN`。执行后无法核实的观察登记 `UNKNOWN` 并说明原因。独立人有证据后给出的 `inconclusive` 则是 #89 的一种结论。独立业务结果、核验覆盖及签认范围继续按[执行矩阵](RUNBOOK.md#执行矩阵)和 [PRD 口径](PRD-MAPPING.md#计量差异必须披露)记录。

## 公开合成样例

[关联表](examples/p6-synthetic/case-link.json)和[索引](examples/p6-synthetic/index.json)演示精确摘要与字段的关联。Run UUID、前缀摘要和序号均为合成占位；[依据文本](examples/p6-synthetic/basis.txt)仅用于检验文件哈希。样例无独立签认原件或 owner 回执，两栏均为 `NOT_RUN`，不进入任何试点分母。

在仓库根目录用 Python 标准库检查关联表、索引和证据文件：

```sh
python evals/pilot/p6_check.py evals/pilot/examples/p6-synthetic/case-link.json
```

成功只表示 `INDEX_LINKAGE_ONLY`。工具检查原始字节的 SHA-256、文件字节数、案号和 P6 项、重复文件及一致的 Run 前缀字段；不会从服务读取或重算 Run 前缀，不核对冻结题、签认原件、owner 回执或身份，也不进行评分。公开样例另外标记 `public_synthetic: true`，必须保持 `NOT_RUN`，不计入正式结果。

以关联表所在目录作为已封存证据包根目录，引用采用 `/` 分隔的相对路径；不接受绝对路径、`..`、链接或 reparse point。仅对已停止写入、无并发替换且可信来源的本地离线副本使用。文件匹配不等于内容真实，不能替代正式人工核验。

## 逐题冻结绑定的离线结构检查

只有在已有交付记录、独立原件和 owner 回执后才能运行此项。用此前通过可信渠道取得的冻结记录 SHA-256，不要从待核对证据包自行现算并充作可信摘要。明确阻塞或已污染的冻结批次会被拒绝；匹配摘要不代表冻结条件已获批准。冻结记录、题集和观察表由操作者显式指定；其余引用相对关联表目录。命令只读已封存的本地离线副本：

```sh
python evals/pilot/p6_bind.py /path/to/package/case-link.json \
  --freeze /path/to/freeze.json --freeze-sha256 "$TRUSTED_FREEZE_SHA256" \
  --tasks /path/to/tasks.json --observation /path/to/observations.json \
  --delivery-ref delivery.json
```

- 将所选题实际交付的 `input` JSON 对象与 `case_id`、`scenario`、`arm`、`client`、`run_id` 一并保存在交付记录中。不要以计划输入替代实际交付记录。将其作为 `type: delivered_input` 的文件列入封存索引，使用实际记录的字节数和 SHA-256。复做端按场景固定为数据库排障 Codex、报销复核 Claude Code；前期产出 Skill 的首做端不等于同题的无 Skill 对照端；按手册保持同题两臂客户端、模型与预算一致，由独立人另行核对，脚本只检查单行。
- 在确有执行记录时，观察表顶层 `product_execution` 登记 `RECORDED`（不表示其他题已完成，也不是业务判定）。选中行记录 `COMPLETED`、`ERROR` 或 `TIMEOUT`，并在其 `evidence` 填入此包内关联表的文件名。核验人从实际交付记录和已核对的关联表分别登记该行 `run_id`、原始关联表字节的 `case_link_sha256`；不要从待校验的候选材料盲目回填。重跑同一题也不能混用观察行。脚本检查唯一题号/臂、原始冻结记录与题集摘要、归一化输入摘要和交付的 Run。
- 在材料齐备时，关联表及其 `independent_original`、`owner_transcription` 两节的 `status` 登记 `RECORDED`。按原格式保存独立原件与 #89 单条回执 JSON，分别绑定原始字节摘要，不把这两份文件混入先前封存的索引。只有保存的回执 `withdrawal` 明确为 null 时，其转录栏才登记 `withdrawal_status: active`。撤回、缺失、未知或未执行不产生结构成功结果。

命令成功只输出 `STRUCTURAL_BINDING_ONLY`。它核对回执的 Run/核验 ID、前缀元组、来源标记及所保存字节中的撤回字段；`passed`、`failed` 和 `inconclusive` 都是转录主张，不改变检查结果。#89 回执本身不含独立原件的摘要；工具分别核对两份文件的原始摘要，不解读原件中的结论、身份或签字，也不验证签认表上的引用、回执的最新状态、当前源可见性或权威前缀内容。仓库中的公开 `NOT_RUN` 样例只能使用上一节的索引检查。正式人工核验、评分与独立签认仍按前述流程执行。
