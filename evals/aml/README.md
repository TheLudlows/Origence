# 本地 AML 检索质量基线 v1

本目录全部为自行编写的合成开发数据，不含官方 AML 数据。用于验证真实 Add/Search 路径和证据排序，不运行 Answer/Eval，也不是官方 Smoke。

## 数据和指标

固定的 corpus.json 包含两个用户、每用户 8 个 session/72 条短消息，共 144 条消息和 20 道题（16 可答、4 无答案）。两个用户使用相同题型及不同个人事实，只有 10 个题型模板，不能当作 20 个独立语义样本。覆盖历史/当前状态、跨 session、多跳、多语言、精确编号、偏好和无答案；另有重复的日常干扰消息。无独立留出集或人工复核，尚不代表长上下文难度。

标签仅存在于评测文件的 required 字段，不进入写入消息或查询文本。通过响应里的 session_id/message_index 对齐来源，再核对完整原文、role、timestamp；未知来源、跨用户来源、重复 chunk ID 或原文改变会使整次运行失败。

一次 Search 请求 top_k=100，在实际有序响应的前 1/5/10/100 条 chunk 上计算指标，不按来源去重后扩充候选。当前每条短消息只有一个 chunk：Recall 是必要消息命中比例，all_evidence 要求多跳的所有必要消息都出现，MRR 只衡量首个相关结果。无答案不进入 Recall 分母，单列非空候选数；非空不等于最终回答错误。请求/契约错误使运行 failed，不产出成功汇总；已完成逐题响应仍保存。

## 本地运行

先按 [AML_DRILL](../../docs/AML_DRILL.md) 设置本地 BGE-M3 的四个模型环境变量，再执行：

~~~powershell
python tools/aml_quality.py --binary target/debug/origence.exe --report target/aml-quality-new.json
python -m unittest discover -s tools -p 'test_aml_quality.py' -v
~~~

报告路径必须不存在，避免覆盖证据。runner 复用演练工具 Host，只启动自身临时宿主，使用 loopback 随机端口和专用 workspace；最终停止子进程并删除自身新建临时数据树。业务实现不新增子进程。报告可能包含完整合成原文，不包含 host key 或模型 key；不能将此公开工件流程直接用于真实企业或官方受限数据。

## 已运行结果

[本地 v1 结果](results/local-v1/README.md)保留原始响应压缩包、模型/机器清单和限制。后续修改语料或排序策略应建立新版本，保留当前工件。优先补充非模板化近似干扰、完整长消息分块与独立留出集，再评估固定 Answer/Eval 的拒答能力；不要根据这 20 题调阈值并声称泛化。
