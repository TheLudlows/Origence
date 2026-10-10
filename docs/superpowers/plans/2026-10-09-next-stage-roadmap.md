# Origence · 后续实施路线图

> 本文只保留当前未完成工作和验收条件。完整架构见[平台设计规格](../specs/2026-09-22-memory-knowledge-platform-design.md)，身份与上下文契约见[I1–I4 规格](../specs/2026-10-08-memory-identity-and-context.md)。当前进度见 [STATUS](../../STATUS.md)，实际证据见 [VALIDATION](../../VALIDATION.md)。

## 已完成阶段

- **S0 身份候选过滤**：完整身份前置限定、真实 LanceDB 回归和跨平台 main CI 已完成，证据见 [VALIDATION](../../VALIDATION.md)。
- **S1 工程检索实验**：36 篇/300 题、六配置真实模型对照及摘要/图消融已完成；新增 200 题尚待独立人工复核。vector Recall@5 为 97.57%，hybrid-all 为 94.79%，12 道无答案题均被召回。完整结果见 [S1 报告](../../../evals/s1/results/local-77d43a8-v2/README.md)。

S1 后续质量门槛：独立复核新增标签，评估拒答策略并扩展完整平台及 Agent 任务验证。开发/留出分数不等于生产质量。

## 尚未完成工作

参榜专项见 [Agent Memory Leaderboard 准备计划](../../AGENT_MEMORY_LEADERBOARD.md)：优先完成文本赛道 Add/Search 适配、用户隔离、部署与质量基线。该计划列出待实施任务及验收条件，不以完整 S2–S4 为参榜前置，也不替代下列产品验收。

### S2：I2 记忆语义最小契约（P1 前置）

- [ ] workspace 范围的受控属性目录，区分单值/多值属性。
- [ ] 同一稳定身份的重复、更新、补充、否定、提议、歧义分别路由到幂等/追加/拒绝/未归一化；**语义相似不授予覆盖权**。
- [ ] 未归一化事实保留来源及不确定状态，不能默认为当前确定事实；别名匹配仅产生建议而不直接合并。
- [ ] 加入同名主体、跨 workspace、不同环境、相反事实、并发 expected_version 的反例；不偷偷更改旧 fact_key 语义。

**验收**：没有跨主体覆写；条件化声明不会被扩大为全局规则；冲突保留有效原文，当前事实不被不确定输入覆盖。

### S3：P1 Session → Feedback → Improve → Learning 最小闭环

- [ ] `SessionTurn`：保存 session/turn、问答、**实际使用**的 evidence ID/source_version（不是仅检索候选），安全范围和幂等键。
- [ ] `Feedback`：针对 turn/证据的幂等反馈；不重复计入权重。
- [ ] `Improve`：显式触发并记录 stage_run/watermark/重试结果；首版只做经验提案、核验、发布，不实现设计中的全部八阶段。
- [ ] `Learning`：保存 statement、why、条件和来源 turn/guidance，接受的经验走既有 add/cognify，避免递归 improve。
- [ ] 第二个会话能准确检索学习经验，撤销支持来源后退出召回；所有新表/接口遵守 tenant/workspace 权限和旧库显式升级边界。

**验收**：实际 Agent 交互从归档 → 反馈 → 显式提升 → 带证据经验发布 → 跨会话复用形成完整可重复流程；失败/重试不制造重复经验或失效事实。

### S4：统一上下文与真实 Agent 效果（P1 完成）

- [ ] 基于既有 SearchHit/resolve 引入统一 EvidenceBundle：scope、当前有效性、来源版本、类型、评分、预算。
- [ ] 分路召回当前记忆、会话、知识原文、条件化 Learning；相互矛盾的有效证据保留并明确标记，不用文本相似去重不同结论。
- [ ] 根据任务裁剪上下文并保留完整引用；Guidance 是外部数据，不能被提升为系统指令。
- [ ] 对比无记忆 Agent、简单 Hybrid、金标准证据输入，测任务完成、引用支持率、成本与延迟。

**验收**：安全治理不回归，跨会话任务的可复现改进有实证而非功能数量。

### P2 暂不启动的大型事项

新增独立数据库后端/分布式 Serverless、完整 SaaS 控制面、OIDC/ACL、配额计费、复杂 GraphCompletion、有效时间（valid_from/to/as_of）、ANN 与重排的规模化优化，均根据后续指标和客户需求立项，不作为 S0–S4 的前置条件。

## 4. 并行保障

Rust 1.98 的完整 CI、cargo audit/RustSec、fs2 文件锁生命周期、长期容量及断电/备份/恢复演练，使用 [VALIDATION](../../VALIDATION.md) 实际证据独立记录。安全与溯源是硬门槛，不能用平均召回率冲抵。文档原则：**设计 ≠ 实现 ≠ CI 成功 ≠ 产品效果已验证**。

## 5. 执行入口

S0 原生与主干验收已完成并归档。S1 工程检索实验现已在 PR #24 交付，新增标签独立人工复核仍待完成；本次没有启动 S2/S3。Issue #21 是历史 S0 记录，不与本轮 S1 证据混用。每次 PR 关联本路线图任务、回归用例、代码 SHA 和 CI 结果，完成后同步 STATUS 与 VALIDATION。
