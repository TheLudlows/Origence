# Origence · 当前状态

更新：2026-10-11。Origence 仍是本地原型。核心写入、权限、来源追溯、版本和检索能力已实现；S0 身份过滤正确性验收完成。S1 工程检索实验已完成，新增题目的独立人工复核仍待进行。

## 已交付与边界

| 范围 | 当前状态 | 主要限制 |
| --- | --- | --- |
| 本地宿主 | SQLite 关系库、LanceDB 向量、SQLite 图和本地 Blob；单宿主、单 Worker | 不自动迁移旧 Kuzu 图文件；切换说明见 [OPERATIONS](OPERATIONS.md#sqlite-图替换边界) |
| 写入与治理 | 记忆/知识发布、版本、scope、幂等、来源追溯、撤回和恢复 | 无企业身份、文档 ACL、配额或生产 SLO |
| API 与接入 | HTTP、CLI、MCP；文件及 PDF 入库 | PDF 线程解析没有硬取消或进程隔离保证，见 [OPERATIONS](OPERATIONS.md#pdf-解析) |
| 身份与上下文 | I1、受限 I2、I4 已实现；完整身份候选前置过滤已验收 | 语义冲突、属性目录、单值/多值和类型配额未实现 |
| 检索质量 | 36 篇/300 题开发与留出切分；真实本地模型六配置及摘要/图消融已完成 | 新增 200 题尚未独立人工复核；12 道无答案题的 vector/hybrid 均误召回 |
| 会话经验 | 目标规格已定义 | Session、反馈、显式 improve 和跨会话 Learning 未实现 |

SQLite 图替换已合入 main `9c292a9`；run [38032343493](https://github.com/TheLudlows/Origence/actions/runs/38032343493) 的五个作业全部成功，三平台默认 lib 28/local 89，13 项图回归通过。S0 身份过滤的独立跨平台验收见 run [37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081)。精确 checkout、job 和计数见 [VALIDATION](VALIDATION.md)。

## AML 当前实现切片

已实现管理员显式启用 namespace、完整 user_id 到独立 workspace 的原子映射，以及 AML Add/Search HTTP 最小闭环。Add 保存不可变消息来源和持久批次幂等记录，整批发布并完成索引后返回成功；超时重试观察原任务。Search 使用原文 vector，关闭摘要/图，返回含角色、时间和会话标注的证据。已有库不自动升级；使用专用评测新库。测试结果及当前限制见 [VALIDATION](VALIDATION.md)。已补齐固定模型的摘要/图等检索分支隔离回归，并完成真实本地 BGE-M3 小规模协议、发布后重启和停止宿主后的整库备份恢复演练；固定模型处理中强退也已验证原任务恢复；运行工具及清理步骤见 [AML_DRILL](AML_DRILL.md)。正式参赛模型、公网容量、生产数据清理与官方 Smoke 尚未完成。

## 当前阶段与下一步

S1 v2 在 36 篇合成文档、300 道问题（开发 200、留出 100）上完成六配置对照、1,800 次 search、1,728 次 resolve 和摘要/图消融，零请求错误。keyword/vector/hybrid-all 的文档 Recall@5 为 0.35%/97.57%/94.79%；vector 与 hybrid 的 12 道无答案题均全部误召回。摘要没有观察到增益，图配置 Recall 下降 2.78 个百分点。完整开发/留出分数、延迟、模型 hash 与限制见 [S1 实测报告](../evals/s1/results/local-77d43a8-v2/README.md)。

AML Add/Search 最小闭环与本地协议演练已实现；[PR #28](https://github.com/TheLudlows/Origence/pull/28) 已合入 main `85454bf`，PR 的 Linux/fast-check 已通过；本轮本地 16 用户/32 Add/80 Search 演练也已通过。按用户最新优先级，下一步直接使用已运行的本地 Ollama BGE-M3（1024 维）推进 AML 协议模拟、长对话/跨会话/时间更新/多跳/无答案检索质量和本地容量恢复验证；不以远端 embedding、服务器或域名作为前置。[本地 AML 质量 v1](../evals/aml/results/local-v1/README.md)已完成：144 条短消息、20 道合成开发题，16 道可答题 Recall@5/全部必要证据覆盖均为 100%，4 道无答案均有候选；仅 10 个共享题型模板，无独立留出集，不能推广为正式成绩。64 用户/8 路并发的 128 Add/320 Search 及重启、备份恢复也已通过。[本地 v2](../evals/aml/results/local-v2/README.md)已冻结并运行 512 条短消息/30 题的较长历史与近似干扰实验，开发/留出来源分离：Recall@5 为 79.17%/83.33%，全部必要证据覆盖为 66.67%/72.73%，7 道无答案均有候选。两道多跳/复合问题在 top-100 仍缺必要证据。[受限查询扩展/重排及 Answer 代理](../evals/aml/experiments/results/README.md)已完成开发集和新主题对照：新主题只重排/扩展重排 Recall@5 为 95.83%/97.92%，完整证据覆盖均为 93.75%，答题代理均为 15/16，4/4 无答案拒答。只重排检索 p95 1.55 秒、扩展重排 3.91 秒，后续优先验证较便宜的只重排。但开发集无答案拒答从原始 vector 的 3/3 降到 2/3，仍出现工单号替代序列号和相似活动温度混淆，暂不接入核心 API。经[业界调研与阶段决策](AML_RETRIEVAL_DECISION.md)，下一项先做失败归因与正确证据诊断，再冻结新反例和公开数据划分，比较专用多语言 cross-encoder 与现有聊天模型重排。证据充分性/逐主张核验是独立消融，不预设必然接入；Search 返回原文证据，AML Answer/Eval 仍归平台控制。检索深度、最终证据预算与阅读器分别验证，不按小样本直接改产品阈值。随后完成[离线失败归因、正确证据诊断与本地专用重排](../evals/aml/diagnosis/README.md)：只重排的 3 个可答失败分别为 2 个 top-40 裁剪和 1 个 top-100 缺失。正确证据下 28/28 通过旧代理，但逐题检查仍发现严格数值边界被弱化，不等于语义全对。RTX 5060 上 BGE-reranker-v2-m3 每题 100 对推理 p50 约 0.30 秒；相同 40 候选的开发/原新主题 Recall@5 为 75.00%/83.33%，低于聊天重排的 91.67%/95.83%；下游 Answer 代理为 8/12、12/16，也低于聊天重排的 10/12、15/16，暂不替换。新的 [v4 成对反例](../evals/aml/v4/README.md)32 题已冻结，尚未调用模型；下一步冻结执行与语义评分配置，再做新数据同预算比较，公开 LongMemEval 版本/许可/独立划分仍待完成。需要 Answer/Eval 时可使用已授权聊天网关，单独标记模型和本地代理指标。[依赖审计保留项](DEPENDENCY_AUDIT.md)继续跟踪。正式组别/模型、公网服务与域名、官方平台 Smoke/Full 放在本地验证之后。检索质量方面仍需独立人工复核新增 200 题，并在不损害可答题召回的条件下评估拒答；之后再决定中文关键词改进和图配置。该实验不代表生产语料、完整平台用例或 Agent 成功率。

随后推进 I2 单值/多值及歧义契约，再实现 Session → Feedback → Improve → Learning 最小闭环。依赖审计已运行但存在保留告警；生产物理擦除、断电和生产备份恢复仍需独立验收；P2 服务化能力根据实际需求推进。

当前文档分工：[平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md)保存完整架构，[身份规格](superpowers/specs/2026-10-08-memory-identity-and-context.md)保存契约；本文是唯一当前状态摘要，[路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)列未完成工作，[VALIDATION](VALIDATION.md)列当前验收。API 和运行边界见 [API](API.md) 与 [OPERATIONS](OPERATIONS.md)。
