# 受限查询扩展与重排实验

目标：分开测量候选缺失和候选排序的影响。业务 Rust/API 不变，实验只通过现有公开 AML Add/Search 和已授权模型网关处理自编合成开发数据。

## 固定对照

- vector：原始 Search top-100 的前 5 条。
- rerank_only：原始前 40 条，让聊天模型只选择至多 5 个合法候选索引。
- expanded_rrf：模型观察问题与首轮前 8 条，生成至多 3 个补充查询；各取 20 条，与原始前 40 条按 RRF(60) 合并，最多保留 40 条，取前 5。
- expanded_rerank：按各查询排名轮流取候选，去重后最多保留 40 条，再选择至多 5 条原始证据。为新关系保留位置，避免重复命中的干扰投票挤出第二跳。允许空结果，但不允许模型生成替代证据。

开发集为 v2 的 15 道 workshop 问题；已观察的 gallery 不再作为未见测试集。规划器与重排器看不到 required、rationale、类别和答案。每次 Search 均核对同一用户的原文和 metadata，融合去重，模型只能引用合法且不重复的候选索引。证据中的指令视作不可信数据，但提示词不构成完整注入防护证明。

## 运行和预算

沿用本地 BGE-M3 环境，另通过 AML_CHAT_BASE_URL、AML_CHAT_API_KEY、AML_CHAT_MODEL 配置聊天网关。模型凭据只从环境读取；新宿主不继承 AML_CHAT_*。不要在命令行或仓库填写真实 key。

~~~powershell
python tools/aml_experiment.py --binary target/debug/origence.exe --allow-chat-calls --report target/aml-experiment-new.json
~~~

可用 --question workshop-q2 先跑一题 pilot。报告目标必须不存在。每次运行至多 60 个聊天请求、单请求输入 40000 UTF-8 字节、累计 1200000 字节、输出最多 2048 token、60 秒请求超时、无自动重试；请求超时不保证远端停止推理。15 题完整实验正常需要 45 次聊天请求。模型输入和输出、实际 usage 与各阶段延迟分别记录；不预设网关价格或将 token 数当费用。温度为 0，不保证供应商完全确定性。

报告保留种子响应、补充查询及响应、合并候选和每个实验臂选择的原始证据。失败中止整轮，部分证据保留且 status=failed，不将失败视作拒答。Host 生命周期和临时目录清理复用已有演练工具。单轮、无随机化调用顺序，不是生产延迟比较；最后仍需独立新测试数据和下游 Answer/Eval。

## 试验迭代及 Answer 代理

[初始失败 pilot](pilot-v1/README.md)完整保留。第二次 pilot 在首个重排请求处 request_failed；工具将 URL/超时/连接异常统一脱敏，不能仅凭该错误断定具体供应商原因。后续独立复测不覆盖旧工件。

`tools/aml_answer.py --input EXPERIMENT.json --report NEW.json --allow-chat-calls` 对完整开发实验的 vector 与 expanded_rerank 分别生成带候选索引引用的答案；新测试集另加 `--dataset unseen`。不再上传或改动库。输入要求是完整成功实验、匹配语料 hash 和完整题目集合。

[固定参考](answer-references.json)在答题调用前编写；模型看不到参考和 gold 来源。代理指标要求所有参考短语组匹配，并引用全部必要来源；无答案题要求显式拒答。它不验证完整语义蕴含或所有附加陈述，不能替代人工审阅或官方 Eval。

## 冻结模型模式

两次独立的大候选请求在约 60 秒返回脱敏 request_failed，小请求仍成功；不能证明具体服务端根因。参考 [Model Studio 官方说明](https://www.alibabacloud.com/help/en/model-studio/deep-thinking)显式发送 `enable_thinking=false` 后，同一候选诊断请求 HTTP 200、1221 ms、usage 3435 token。该配置写入 [策略冻结记录](freeze.json)，完整开发/新测试均使用同一模式；模型调用仍可能失败，不声称硬取消或保证延迟。初始预检两次各 112 token，两个失败请求 usage 不可知，不视为免费。

一次完整开发尝试在第三次模型调用遇到非对象/无效 JSON，status=failed 且未汇总分数；保存于 dev-json-failure。依照 [官方结构化输出文档](https://docs.modelstudio.console.alibabacloud.com/en/model-studio/qwen-structured-output)，随后显式设置 `response_format={"type":"json_object"}`，仍对字段类型、合法索引和引用完整性做本地校验。

网关在 JSON 模式下仍可能返回候选数组而非对象。工具使用标准 JSON 解码后接受有界的查询数组/索引数组；若返回候选对象数组，每项正文必须与该合法索引的原始正文完全一致，最后仍从原候选中取值。重复、越界、改写正文和非法类型全部拒绝。模型 completion 在合成报告中保留供审计，不记录原始 HTTP 错误正文或凭据。
