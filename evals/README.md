# 评估起点

`cases.jsonl` 是冻结样例格式，不是已完成的竞品 benchmark。生命周期与安全门槛由 `tests/local_app.rs`（通过 `tests/local.rs` 运行） 可执行验证；检索样例作为后续真实冻结数据集的起点，不把模拟 embedding 的排序算作真实语义质量。

扩充评估时分别记录：检索 Recall@k/来源正确率，未发布产物泄露率/跨 tenant/workspace 泄露率，版本冲突和删除不复活，实际 Agent 引用与任务完成率，p50/p95/p99 和模型成本。正确性门槛不应被平均相关性抵消。

尚未运行 Cognee 或其他 ContextDB 产品对比；“rds contextdb”具体产品身份仍待确认。性能和胜负结论必须附硬件、数据集版本、模型配置、运行命令和原始结果。

当前运行基线为 M5 本地宿主，参见 [文档索引](../docs/README.md)。真实 SQLite/LanceDB/Kuzu 与模型 stub 的测试验证行为，不作为检索质量得分。
