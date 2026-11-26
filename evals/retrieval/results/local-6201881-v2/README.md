# #149 中文检索质量复跑

2026-11-26，应用主干 `620188115c89b20bc1411f5ec1e48b93f68548eb`。向量与原文混合仍对全部 12 道无答案题返回候选和上下文。开发集锁定的最高向量分门槛未减少留出误召回，还丢失一道可答题。本次交付复跑材料和离线对照，产品检索策略保持现有行为。

## 冻结环境

- 复用 [S1 v2 数据清单](../../v2/manifest.json)的 36 篇合成文档和同一开发／留出划分。语料 SHA-256 为 `115e58b9c92ae4ca3f1162eb25bb8940a6687fb3576609b6e44479bdd8fad84e`，300 题文件为 `fced7f6e4aef6ecea5ff6553e3739b0bb8e4102c9dfa804dfa6dee5269209d1c`。
- 全新 SQLite、嵌入式 AnySeek Catalog `5626dadfd5414b294a3cf0f6b845f5261760f68b`。一次导入，顺次跑关键词、向量及原文混合。请求上限 100，按来源文档去重后取 k=5，resolve 预算 2,000 UTF-8 字节。关闭摘要和图，`allow_partial=false`。
- BGE-small-zh-v1.5 `7999e1d3359715c523056ef9478215996d62a620`，512 维，CLS/L2，无查询指令。主权重 SHA-256 为 `354763b9b1357bc9c44f62c6be2276321081ed2567773608c0d0785b61d5a026`，所有模型文件见 [models.json](models.json)。PyTorch 2.14.1+cpu、Transformers 4.57.6、本地 CPU 两线程。未运行生成模型，供应商费用为零，CPU 成本未定价。[环境与二进制摘要](environment.json)

## 基线

精确来源和版本的 Recall@5、引句 Recall@5 以及实际渲染的原文覆盖按可答题平均。开发集为 192 道可答和 8 道无答案，留出集为 96 道可答和 4 道无答案。无答案误召回列统计至少返回一条候选的题数。命中的语料版本均通过独立复算核对。

| 模式 | 划分 | 来源／版本 Recall@5 | 原文引句 Recall@5 | 原文覆盖 @2,000 B | 无答案 search／resolve 有证据 |
| --- | --- | ---: | ---: | ---: | ---: |
| 关键词 | 开发 | 0.52% | 0.52% | 0.52% | 0/8 · 0/8 |
| 关键词 | 留出 | 0.00% | 0.00% | 0.00% | 0/4 · 0/4 |
| 向量 | 开发 | 96.35% | 96.35% | 97.92% | 8/8 · 8/8 |
| 向量 | 留出 | 100.00% | 100.00% | 100.00% | 4/4 · 4/4 |
| 原文混合 | 开发 | 96.35% | 96.35% | 97.92% | 8/8 · 8/8 |
| 原文混合 | 留出 | 100.00% | 100.00% | 100.00% | 4/4 · 4/4 |

900 次 search 和 900 次 resolve 均成功。下表单位为毫秒，每格依次为 p50／p95／p99。顺序执行的单机 Debug 样本同时包含宿主和检索开销。

| 模式 | 划分 | search p50／p95／p99 | resolve p50／p95／p99 |
| --- | --- | ---: | ---: |
| 关键词 | 开发 | 35.86 / 50.82 / 71.27 | 36.09 / 53.33 / 124.47 |
| 关键词 | 留出 | 32.68 / 40.30 / 79.98 | 33.36 / 40.93 / 44.19 |
| 向量 | 开发 | 157.47 / 201.48 / 288.22 | 157.30 / 207.92 / 294.32 |
| 向量 | 留出 | 162.73 / 204.51 / 239.86 | 161.06 / 198.92 / 254.17 |
| 原文混合 | 开发 | 192.15 / 246.42 / 304.21 | 189.40 / 225.14 / 260.29 |
| 原文混合 | 留出 | 190.57 / 218.55 / 234.44 | 189.81 / 218.42 / 230.83 |

- `s1-002`「上线前谁来批准」应找到 `release-policy` v1。关键词无候选，两个语义模式的前两项是供应商和差旅制度。
- `v2-orion-travel-02`「Orion员工出门办公前需要获得谁同意？」关键词无候选，两个语义模式均覆盖 `orion-travel` v1。
- 无答案题 `s1-097`「Quantum项目负责人是谁」在两个语义模式都召回了 Boreal 和 Atlas 的负责人。留出题 `v2-heldout-no-answer-1`「Orion员工总人数是多少？」的前两条为 Iris 预算和 Willow 合同。[全部失败样例](failure-analysis.json)

## 门槛对照

只在开发集用已获权向量候选的最高原始分校准。预设约束是逐模式保留来源召回、原文召回和上下文覆盖，各项损失至多 1 个百分点。锁定 `0.5508528947830201` 后，在留出集用原始向量分重放同一规则，两个语义模式结果相同。[阈值与原始输入摘要](gate-lock.json)

| 划分 | 无答案误召回（search 和 resolve 各自） | 来源／版本及引句 Recall@5 | 原文覆盖 |
| --- | ---: | ---: | ---: |
| 开发 | 8/8 → 6/8 | 96.35% → 95.83% | 97.92% → 97.40% |
| 留出 | 4/4 → 4/4 | 100.00% → 98.96% | 100.00% → 98.96% |

留出集错挡 `v2-vega-ticket-04`「Vega补交材料后创建时间会改变吗？」。离线对照复用同一次 search／resolve 延迟，没有线上门槛延迟数据。后续需要能核对问题与证据是否匹配的信号，新增困难无答案题验证后再决定是否接入产品。

另用同一应用及模型快照验证三种模式的跨工作区隔离和来源撤回。正常检索能返回原始版本，异工作区和撤回后 search、resolve 均成功返回空结果。停止 loopback 模型服务后，严格向量和混合的 search、resolve 返回 HTTP 503。离线门槛也将这些请求失败保留为 `request_failed`，成功但缺证据记 `insufficient_evidence`。[隔离与失败状态检查](retrieval-scope-smoke.txt)

## 复跑

在包含本报告的检出中执行。评测文件来自同一改动，Rust 源码与主干基线一致。模型服务运行于 loopback，支持从公开源下载清单中固定的权重。以下目录均须为新建的测试路径。

```sh
python3 -m venv /tmp/origence-149-env
/tmp/origence-149-env/bin/pip install torch==2.14.1+cpu --index-url https://download.pytorch.org/whl/cpu
/tmp/origence-149-env/bin/pip install -r evals/requirements-local.txt
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo build --locked -j 4 --bin origence

HF_HUB_DISABLE_XET=1 HF_HUB_DISABLE_IMPLICIT_TOKEN=1 \
  /tmp/origence-149-env/bin/python evals/local_experiment.py --local --original-only \
  --binary target/debug/origence --dataset evals/retrieval/v2 \
  --model-dir /tmp/origence-149-models --download-models \
  --commit 620188115c89b20bc1411f5ec1e48b93f68548eb --output /tmp/origence-149-run

python3 evals/retrieval_gate.py lock --run /tmp/origence-149-run \
  --output /tmp/origence-149-run/gate-lock.json
python3 evals/retrieval_gate.py report --run /tmp/origence-149-run \
  --lock /tmp/origence-149-run/gate-lock.json --output /tmp/origence-149-run/gate-report.json
python3 evals/audit_s1.py --run /tmp/origence-149-run --dataset evals/retrieval/v2
/tmp/origence-149-env/bin/python evals/retrieval_scope_smoke.py \
  --binary target/debug/origence --embedding-path /tmp/origence-149-models/bge-small-zh-v1.5
```

[原始归档](raw-results.tar.gz)内包括 900 条配对 search／resolve 响应、导入映射、分组汇总、锁定门槛和逐题前后变化。解压后将 `--run` 指向其中的 `issue149-baseline` 目录，可从保存的响应独立复算。[SHA-256 清单](sha256.txt) · [汇总](comparison.json) · [独立审计](audit.json)

这批合成题的逐字证据覆盖用于工程回归。业务成功率与独立签认由 #91 跟踪。
