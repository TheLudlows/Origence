# 新测试主题 v3

在策略测试前固定 2 个新用户（营地、乐团）、16 会话、256 条短消息、20 道题（16 可答、4 无答案）。与 v2 的设备、展览来源和主题分离，仍由同一 agent 编写，未独立人工复核。[freeze](freeze.json)记录语料和答案参考 hash；观察测试输出后不调策略或改标签。

此测试包含三条来源的关系链、时间更新、相似标识、多语言、偏好和无答案；不是官方数据或真实长上下文。冻结数据用于比较 vector、只重排、查询扩展和扩展重排。

运行：`python tools/aml_experiment.py --dataset unseen --binary target/debug/origence.exe --allow-chat-calls --report target/aml-unseen-new.json`。沿用本地 embedding 和环境变量中的聊天网关；最多 60 次聊天请求，不支持挑选单个测试题。

完整检索结果见[新主题对照](../experiments/results/README.md)。首次及第二次运行因网关输出包装中断；只修复有界格式适配后，以同一数据、提示词和检索参数重跑，全部中断工件保留。
