# S1 检索质量基线（数据集 v1）

**简体中文** · [English](README.md)

本文记录 S1 v1 的数据输入与标注协议。对应固定代码和后端的历史结果见 [S1 v2 报告](results/local-77d43a8-v2/README.md)。合成数据集供开发使用，不代表生产基准。

## 标注协议

- 此数据版本内各文档不可变。`source_id`、`source_version` 和 `locator` 构成稳定证据键，v1 的 `locator` 指向完整合成源文档。
- `relevant_source_ids` 列出回答所需的全部来源。多来源题必须召回所有列出的来源才算完整覆盖。
- `relevant_source_ids` 为空表示语料中无答案。此类题不进入 Recall/MRR/nDCG 分母，另计误召回。
- `evidence_quote` 是引用来源的逐字摘录，供审阅者直接审计。它不是生成的回答或模型判断。
- 查询人工根据源文本编写，刻意加入同义表达，以揭示词法检索的局限。不保证每种检索模式均可回答。
- 评分运行后不要原地修改数据。创建新版本，记录原因，并保留此前哈希及结果。

## 冻结输入

- `corpus.jsonl` 是源文档副本，各文档版本均为 `1`。
- `cases.jsonl` 包含 100 个带文档级证据标签及逐字引文的问题。
- `manifest.json` 保存 SHA-256 哈希和标注范围。

当前运行器按文档级证据评分，不评估精确论断支持、答案生成、Agent 任务成功率或统一 Token 预算下的 Evidence Coverage。v2 使用 300 题并设有独立留出集。

## 复现

构建 release 二进制，然后用文档中的适配器运行关键词基线。向量和混合模式须显式启用 embedding 服务，并记录模型、版本、维度和成本。固定向量的 S0 回归模型不能用来衡量语义质量。

```sh
python3 evals/run.py --binary target/release/origence --corpus evals/retrieval/corpus.jsonl --cases evals/retrieval/cases.jsonl --mode keyword --k 5 --commit 47d1375 --output target/evals/retrieval-v1-keyword
```

输出目录不得已存在。一起保留 manifest、逐题结果、导入及汇总。
