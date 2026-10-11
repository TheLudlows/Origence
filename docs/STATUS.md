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

AML Add/Search 最小闭环、本地 BGE-M3 协议模拟、64 用户并发及重启/备份恢复已完成；正式 Search 仍为原文 vector，不生成 Answer。早期短消息与查询扩展结果见 [本地实验](../evals/aml/experiments/results/README.md)，失败归因和正确证据/专用重排诊断见 [诊断报告](../evals/aml/diagnosis/README.md)。这些数据均已观察，只用于回归，不作未见验收。

按“长消息校验 → 同预算冻结 → 新数据评测 → 方案决定”的 [本轮执行](../evals/aml/budget-v1/README.md)已完成：Search 暴露原有消息内字节定位，严格校验原文/范围/scope；v4 32 道成对题和固定公开 LongMemEval 32 题完成三臂比较，公开 1,524 批/15,685 条消息经真实 Add/Search。16 份阶段工件全部一致性通过，320 次聊天调用、477,289 个供应商报告 token，无执行错误。公开自动裁判的受支持成功为 vector/chat/BGE 22/32、22/32、21/32；三臂 3/3 无答案正确拒答。BGE 会话 Recall 稍高但未转成更高回答成功，聚类区间宽；这些不是官方 AML 成绩。

复核发现 v4 题意/标签歧义、同模型裁判误扣分与错误放行、公开时间题参考/日期疑似冲突。原始数据、分数和历史工件保持不变，争议单独记录，不能把自动分数当严格语义准确率。**生产 Search 保持 vector；聊天重排、本地 BGE、同模型核验继续留在实验层。** BGE 的延迟优势仅支持继续研究，质量门槛尚未满足。正式组别/模型、独立人工复核与官方 Smoke/Full 仍未完成。

[时间与裁判诊断](../evals/aml/temporal-v1/README.md)已完成：已观察公开题中 10 道时间题三臂、12 个支持分类反例双臂，共 54 次调用 / 53,719 token，无执行错误。日历日期差修复一个明确相对日期案例，但事件对应、取整与参考冲突仍存在。新支持分类器 11/12 与自建标签一致，仍会结构化错误放行；未经独立人工复核，继续留在实验层。

分段性能定位已发现规模相关成本：最大单历史查询约 0.61–0.76 秒；增加其他用户的 512 批数据后约 2.14–2.31 秒，候选数和向量批次数不变，主要增加在向量阶段。权限/原文字节及稳定性检查全部通过，没有修改排序或削弱权限。尚未重跑完整库约 5.29 秒场景，也没有性能优化收益声明。**下一项优先：细分向量后端打开表/元数据/过滤扫描耗时，验证共享表规模和碎片因素，再选择最小优化；并明确日期单位/取整契约及独立校准标签。** 当前 64 题均为已观察回归集。按用户优先级，服务器/域名后置；已授权聊天网关可供受限 Answer/Eval 实验，凭据不入库。[依赖审计保留项](DEPENDENCY_AUDIT.md)继续跟踪；S1 新增 200 题仍待独立人工复核，再决定中文关键词及图配置。

随后推进 I2 单值/多值及歧义契约，再实现 Session → Feedback → Improve → Learning 最小闭环。依赖审计已运行但存在保留告警；生产物理擦除、断电和生产备份恢复仍需独立验收；P2 服务化能力根据实际需求推进。

当前文档分工：[平台设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md)保存完整架构，[身份规格](superpowers/specs/2026-10-08-memory-identity-and-context.md)保存契约；本文是唯一当前状态摘要，[路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)列未完成工作，[VALIDATION](VALIDATION.md)列当前验收。API 和运行边界见 [API](API.md) 与 [OPERATIONS](OPERATIONS.md)。
