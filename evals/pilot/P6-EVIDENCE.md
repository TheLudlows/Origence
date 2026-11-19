# P6 独立签认与 Run 核验证据

逐题、逐 Run 使用一个受限证据包。以[空白关联表](p6-case-link.template.json)登记题号、场景、客户端、基线/复用臂和 Run。证据包沿用[执行手册的索引](RUNBOOK.md#真实闭环与精确加载证据)，并将下列材料关联到同一份封存索引。

1. **固定 Run 依据。** 由有权的 Run owner 按 [#89 契约](../../docs/contracts/run-verification.md)读取指定位置的不可变前缀，保留回执及引用事件。将 Run ID、`through_sequence`、前缀回执的 `digest`（填作 `prefix_digest`）和 `basis_sequences` 抄入索引。逐条核对依据序号在该前缀内且为正整数、互不重复，并保留事件与资产精确版本及来源的对应关系。只在获授权的范围内提供证据。
2. **封存原始索引字节。** 索引登记上述四个字段和材料的相对路径、SHA-256、字节数、UTC 时间、采集人、证据类型、适用条款、执行项和题号。逐项核对字节后，对最终索引文件的原始字节计算 SHA-256，填写 `evidence_index.sha256`。保留原始文件及换行。索引摘要与 Run 前缀摘要分别核对。
3. **保留独立签认原件。** 独立核对人使用已有签认方式，在原件或原件明确引用的附件中确认同一 Run ID、序号、前缀摘要、依据序号及索引 SHA-256。原件同时保留其身份、独立性、核验时间、核验范围、判断和理由。按已有受限渠道向获授权者核对原件来源，并登记原件引用和自身 SHA-256。摘要用于核对字节，独立身份和判断依据由核对人及原件确认。
4. **分别记录 owner 转录和回执。** 有写权限的 Run owner 按契约转录结论与依据。核对回执顶层 `run_id` 及 `assessment` 内的 `verification_id`、`through_sequence`、`prefix_digest`、`basis_sequences`，保留回执引用、摘要和撤回状态。原件、索引摘要和文件引用留在受限证据包，提交沿用现有严格 DTO。回执应保留 `evidence_origin:owner_transcribed_external`、`verifier_authentication:unverified_claim`、`platform_verification:not_performed`。单列服务准备、客户端自报、可观察的实际使用与独立判断。服务准备 v2 或客户端自报成功时，实际使用版本仍可能为 UNKNOWN。
5. **按原标准登记 P6。** 将受限关联表的引用写进[签认表](signoff.template.json)的 `matrix.P6.evidence_refs`。核对索引、独立原件与 owner 回执是否绑定同一 Run 依据，记录缺失、拒绝访问、撤回和不一致。前缀、文件或摘要不符的条目无法支持原结论。独立人按既有 P6 口径汇总全部应核验题，PRD A6 与 P7 各按其既有条款单独验收。样例或单件证据只对应列出的 Run。

索引只列签认前的依据文件。独立原件、回执与关联表在同一证据包中单独保存，关联表用各自的引用和摘要绑定它们。文件引用相对证据包根目录。封存后改动依据时另立索引及复核记录，保留历史。签认、文件存取及 Run 读取仍按各自授权。读取被拒或来源撤回时，登记可见状态并保持内容受限。

尚未执行的环节登记 `NOT_RUN`。执行后无法核实的观察登记 `UNKNOWN` 并说明原因。独立人有证据后给出的 `inconclusive` 则是 #89 的一种结论。独立业务结果、核验覆盖及签认范围继续按[执行矩阵](RUNBOOK.md#执行矩阵)和 [PRD 口径](PRD-MAPPING.md#计量差异必须披露)记录。

## 公开合成样例

[关联表](examples/p6-synthetic/case-link.json)和[索引](examples/p6-synthetic/index.json)演示精确摘要与字段的关联。Run UUID、前缀摘要和序号均为合成占位；[依据文本](examples/p6-synthetic/basis.txt)仅用于检验文件哈希。样例无独立签认原件或 owner 回执，两栏均为 `NOT_RUN`，不进入任何试点分母。

在仓库根目录用以下 Python 标准库命令核对样例：

```sh
python - <<'PY'
import hashlib
import json
from pathlib import Path

root = Path("evals/pilot/examples/p6-synthetic")
link = json.loads((root / "case-link.json").read_bytes())
index_bytes = (root / link["evidence_index"]["ref"]).read_bytes()
assert hashlib.sha256(index_bytes).hexdigest() == link["evidence_index"]["sha256"]
index = json.loads(index_bytes)
assert index["run_prefix"] == link["run_prefix"]
for item in index["files"]:
    data = (root / item["path"]).read_bytes()
    assert len(data) == item["bytes"]
    assert hashlib.sha256(data).hexdigest() == item["sha256"]
print("P6 synthetic evidence linkage: OK")
PY
```
