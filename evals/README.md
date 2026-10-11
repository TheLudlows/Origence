# 评估起点

`cases.jsonl` 是冻结样例格式，不是已完成的竞品 benchmark。生命周期与安全门槛由 `tests/local_app.rs`（通过 `tests/local.rs` 运行） 可执行验证；检索样例作为后续真实冻结数据集的起点，不把模拟 embedding 的排序算作真实语义质量。

扩充评估时分别记录：检索 Recall@k/来源正确率，未发布产物泄露率/跨 tenant/workspace 泄露率，版本冲突和删除不复活，实际 Agent 引用与任务完成率，p50/p95/p99 和模型成本。正确性门槛不应被平均相关性抵消。

尚未运行 Cognee 或其他记忆/知识平台的产品对比；“rds contextdb”具体产品身份仍待确认。性能和胜负结论必须附硬件、数据集版本、模型配置、运行命令和原始结果。

当前运行基线为 M5 本地宿主，参见 [文档索引](../docs/README.md)。真实 SQLite/LanceDB/SQLite 图 与模型 stub 的测试验证行为，不作为检索质量得分。

## HTTP 检索评估器（2026-10-08）

`run.py` 使用真实宿主 HTTP API：新临时数据目录 → 新 workspace → 导入知识 → 等待 published → search → 按来源文档去重评分。默认关闭所有模型调用，且不读取已有服务或数据目录。

```sh
python3 -m unittest discover -s evals -p 'test_*.py' -v
python3 evals/run.py --binary target/release/origence --mode keyword --k 5 --commit COMMIT --output target/evals/keyword-run
```

首批 `corpus.jsonl` 为 12 份合成文档，`retrieval.jsonl` 为 24 个样本，覆盖精确词、同义问法、同名范围干扰、多来源、否定和无答案。它们是可执行种子，不是完整 300 用例，也不是已取得效果结果。旧 `cases.jsonl` 保留生命周期/安全样例，不能直接交给检索 runner。

报告包含 manifest、imports、逐题原始响应和 summary；用 source ID → 本次 asset ID 映射评价文档级 Recall@k、MRR@k、二元相关性 nDCG@k。search 请求最多 100 个 chunk，去重后取前 k 个文档；这不是 chunk 级 Recall 或完整无限候选检索。多份必要来源均进入分母；无答案题不进入 Recall 分母。请求失败的可回答题计零分，无答案请求失败单列为错误，不算正确拒答。成功请求延迟单列，同时记录全部请求错误；小样本 p99 不代表生产性能。

vector/hybrid 仅在显式 `--allow-model-calls`、`OC_ENABLE_MODELS=true` 与模型配置齐全时运行；模型可能收费。旧种子默认不启用模型；S1 v2 已用真实本地模型完成实验，provider fee 为 0、CPU 未定价，usage 单独计量。S1 v2 使用独立摘要/图开关，在同一份发布产物上运行六配置矩阵；见 [S1 v2](s1/v2/README.md)。

输出路径须不存在，以免覆盖原始结果；输出可能含授权语料正文，不含 API/模型 key。CI 使用公开合成语料上传结果。真实企业数据的报告需按其授权范围保存，不能复用公开 CI 工件流程。测试 fixture 只验证 adapter 与指标，不能作为 Rust 宿主或语义效果验收。

## 已执行的 S1 实验

[S1 v2 结果](s1/results/local-77d43a8-v2/README.md) 包含 300 题六配置、来源独立留出集、真实 BGE/Qwen 模型配置、原始响应压缩包、失败清单及独立审计。新增 200 题未独立人工复核，无答案误召回仍是明确缺口；完整平台、Agent 和生产验收另行执行。

## AML 本地检索基线

[AML 合成开发基线](aml/README.md)通过真实 Add/Search 评测消息级证据召回、多跳全部证据覆盖与无答案候选；与上面的文档去重指标分别报告。
