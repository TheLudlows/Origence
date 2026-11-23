# S1 检索实验 v2

**简体中文** · [English](README.md)

检索推理前冻结 300 道题（开发 200、留出 100）、36 篇合成来源文档、288 道可答和 12 道无答案题。每道可答题都有来源、版本、整文档定位以及逐字支持引文。开发和留出集的金标来源不交叉，但均搜索相同的 36 篇语料。评分前在 `manifest.json` 固定哈希、模型修订、k=5、UTF-8 字节预算=2000 及六套配置。运行后不能修改题目或金标。需要订正标签时另立数据版本。

保留此前 100 道人工标注的 v1 题目。新增的 200 道题及 24 篇合成来源由助手编写并与原文核对，未独立人工审阅。这是合成工程基准，不代表生产数据，未获独立人工标注认证。留出集独立于开发来源和检索调参，不独立于作者。

## 配置和指标

六种配置共用一次新 Origence 发布，分别为关键词、向量、仅原始分块的混合、混合加摘要、混合加图、混合加摘要与图。语料、embedding、候选上限、k 和上下文预算相同。导入时运行一次真实 embedding 和提取，分支开关只影响查询。只开图的混合配置用于隔离图相对摘要的收益。关键词、向量模式忽略仅用于混合的开关。

报告保留 search 和 resolve 原始响应、导入记录、数据集和模型文件哈希、CPU 运行时版本、投影生成响应和逐题指标。`summary.json` 分别报告两个子集及各类别，包含文档 Recall/MRR/nDCG@5、原文逐字证据 Recall@5、Coverage@2000 字节、完整覆盖题占比、无答案误召回、search/resolve p50/p95/p99 和模型用量。只有图来源不能算作原文引句。这些指标不能认证语义蕴含、生成答案引用的正确性或 Agent 任务成功。

本地供应商费用为零。CPU 成本未定价，不能声称为零。导入用量单列，查询用量按配置记录。语义评分不使用固定向量回归模型。未根据留出题调向量拒答阈值，因此向量检索可能给无答案题返回无关文档。记录这种失败，不能把零请求错误视为正确拒答。

## 复现

Python 模型可选依赖不改变应用的 Cargo 依赖。

```sh
python3 -m venv /tmp/origence-model-env
/tmp/origence-model-env/bin/pip install torch==2.14.1+cpu --index-url https://download.pytorch.org/whl/cpu
/tmp/origence-model-env/bin/pip install -r evals/requirements-local.txt
cargo build --locked -j 2
HF_HUB_DISABLE_XET=1 /tmp/origence-model-env/bin/python evals/local_experiment.py \
  --binary target/debug/origence --model-dir /tmp/origence-models \
  --download-models --dataset evals/retrieval/v2 --commit "$(git rev-parse HEAD)" \
  --output /tmp/origence-s1-v2-run
```

loopback 服务与评测宿主位于同一进程树。运行时只从本地文件加载模型。可选下载步骤会拉取数据清单指定的精确快照。输出必须是新目录。真实提取在应用导入前离线准备。当地供应商使用 greedy float32 CPU 生成、两阶段 JSON schema（先实体，再生成端点必须取自已生成名称的关系），每个数组最多八项。保留两阶段原始输出和提示。这些约束不认证语义正确。按完整提示词、模型修订和推理策略缓存，因此消融组不会各用不同摘要或图。准备时间和用量单列，导入计时不含生成。Embedding 不缓存。耗时来自单轮串行 CPU 执行，不代表生产延迟 SLO。

原 24 题、12 文档的种子仍冻结在 `evals/corpus.jsonl` 和 `evals/retrieval.jsonl`，对应的八个关键词未命中单独记录。语料和题目分布不同，分数不能直接比较。

## 模型实现参考

- [BGE 模型卡](https://huggingface.co/BAAI/bge-small-zh-v1.5)说明 CLS 池化、归一化与对称无指令策略。
- [Qwen 模型卡](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct)说明本地 Transformers 推理。
- [LM Format Enforcer](https://github.com/noamgat/lm-format-enforcer)提供 JSON schema Token 约束。格式和端点一致性不能认证事实得到支持。

这 300 道检索题不能替代完整平台协议中的记忆生命周期、Agent 任务、治理与故障、重跑不确定性或生产负载。各项仍由 S2–S4 和运维验收单独覆盖。

## 已执行结果

[本地 CPU 实验 77d43a8](../results/local-77d43a8-v2/README.md)保留六套结果、开发/留出报告、配对消融差值、全部失败、独立引用与预算审计，以及带 SHA-256 的压缩原始响应。新增标签仍待独立人工审阅。
