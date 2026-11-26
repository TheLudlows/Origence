# #149 中文检索质量复跑

2026-11-26，主干 `620188115c89b20bc1411f5ec1e48b93f68548eb`。向量和原文混合各在 12/12 道无答案题上返回候选与上下文。试验阈值未改善留出结果，产品策略未采用它。

## 冻结环境

- 36 篇合成文档、300 道题。开发集 192 道可答、8 道无答案，留出集 96 道可答、4 道无答案。[S1 v2 数据与哈希](../../v2/manifest.json)
- 全新 SQLite，嵌入式 AnySeek `5626dadfd5414b294a3cf0f6b845f5261760f68b`。一次导入，各模式请求上限 100，按来源去重后取 k=5，resolve 预算 2,000 UTF-8 字节。关闭摘要、图和降级。
- BGE-small-zh-v1.5 `7999e1d3359715c523056ef9478215996d62a620`，512 维、CLS/L2、无查询指令。本地 CPU 推理，供应商费用为零，CPU 成本未定价。[模型文件与哈希](models.json) · [运行环境与二进制摘要](environment.json)

## 基线

来源／版本与原文引句的 Recall@5 在各组数值相同，均与原文覆盖按可答题平均。900 次 search 和 900 次 resolve 均成功。延迟为单机顺序执行的 Debug 样本。[p50／p95／p99 与完整汇总](comparison.json) · [独立复算](audit.json)

| 模式 | 划分 | 来源／版本与引句 Recall@5 | 原文覆盖 | 无答案 search／resolve 返回结果 | search／resolve p95（ms） |
| --- | --- | ---: | ---: | ---: | ---: |
| 关键词 | 开发 | 0.52% | 0.52% | 0/8 · 0/8 | 50.82 / 53.33 |
| 关键词 | 留出 | 0.00% | 0.00% | 0/4 · 0/4 | 40.30 / 40.93 |
| 向量 | 开发 | 96.35% | 97.92% | 8/8 · 8/8 | 201.48 / 207.92 |
| 向量 | 留出 | 100.00% | 100.00% | 4/4 · 4/4 | 204.51 / 198.92 |
| 原文混合 | 开发 | 96.35% | 97.92% | 8/8 · 8/8 | 246.42 / 225.14 |
| 原文混合 | 留出 | 100.00% | 100.00% | 4/4 · 4/4 | 218.55 / 218.42 |

「上线前谁来批准」未召回 `release-policy` v1。「Orion员工总人数是多少？」无答案，语义模式排名前两位为 Iris 预算和 Willow 合同。[逐题失败清单](failure-analysis.json)

## 门槛对照

只用开发集最高向量分选定 `0.5508528947830201`，要求开发集的来源召回、引句召回和原文覆盖各降不超过 1 个百分点。固定阈值后离线重放，向量和原文混合的变化相同。[阈值与输入哈希](gate-lock.json)

| 划分 | 无答案误召回（search 和 resolve 各自） | 来源／版本及引句 Recall@5 | 原文覆盖 |
| --- | ---: | ---: | ---: |
| 开发 | 8/8 → 6/8 | 96.35% → 95.83% | 97.92% → 97.40% |
| 留出 | 4/4 → 4/4 | 100.00% → 98.96% | 100.00% → 98.96% |

留出集错挡一道可答题 `v2-vega-ticket-04`。离线重放沿用原请求延迟。跨工作区和来源撤回后返回空证据，严格向量和混合模式遇到模型故障返回 HTTP 503。[隔离与失败状态检查](retrieval-scope-smoke.txt)

## 复跑

在包含本报告的检出中运行，使用新测试目录。模型服务仅监听 loopback，按清单下载固定权重。

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

解压 [原始归档](raw-results.tar.gz)，将 `--run` 指向其中的 `issue149-baseline` 即可从逐题响应复算。[SHA-256 清单](sha256.txt) · [业务试点与独立签认 #91](https://github.com/TheLudlows/Origence/issues/91)
