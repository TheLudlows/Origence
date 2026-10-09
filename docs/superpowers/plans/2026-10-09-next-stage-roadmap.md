# Origence 剩余任务与下一阶段实施计划

> 编写日期：2026-10-09。本文是 **P0 收口 → P1 产品闭环 → P2 服务化** 的执行路线图，不代表事项已验收。目标/架构以 [整体设计](../specs/2026-09-22-memory-knowledge-platform-design.md) 为准；身份、冲突、上下文契约以 [I1–I4 规格](../specs/2026-10-08-memory-identity-and-context.md) 为准；事实进度与 CI 证据分别以 [STATUS](../../STATUS.md)、[VALIDATION](../../VALIDATION.md) 为准。

## 1. 文档职责与规划缺口

- **整体设计（spec）**：定义目标、边界、架构、P0/P1/P2 和会话经验模型，不等于实际交付。
- **2026-10-08-memory-identity-and-context.md（spec）**：定义完整业务身份、属性/条件、capture 归一化、事实冲突、知识/记忆关联、会话上下文策略、I1–I4 验收反例；不是完整排期或任务列表。
- **2026-10-08-validation-and-evaluation.md（plan）**：P0 验收与评估基础设施的历史实施清单，有不少未勾项目，但缺乏 P1 Session/Feedback/Improve/Learning 的分批实施清单。
- **本文件（plan）**：衔接以上文档，记录尚未完成的切片、实施顺序、依赖、验收定义和产品取舍。实现情况不可从本文件未勾清单反推为已经通过测试。

## 2. 当前能力与真实缺口

| 模块 | 已实施 | 剩余关键问题 |
| --- | --- | --- |
| M0–M5 / P0 | 单宿主 SQLite+LanceDB+Kuzu+Blob，HTTP/MCP，异步发布、幂等、账本、权限、溯源、撤回、版本 | 大规模/故障验收、依赖审计、生产运维可靠性 |
| I1 记忆身份 | 明确身份写入/唯一性/精确 lookup、expected_version、离线升级 | 完整身份检索需候选前置限定，防止 top-100 漏召回 |
| I2 抽取 | 调用方明确 identity 的 capture、精确引用区间校验 | 未归一化状态、属性目录、单值/多值、语义匹配/冲突管理 |
| 检索 | keyword/vector/hybrid、摘要和图扩展、RRF、证据引用 | 目前仅 12 文档/24 查询无模型合成基线；真实模型/Agent 质量和组件收益未知 |
| I4 上下文 | 现有 search/resolve、身份标注、引文与字节预算 | 冲突标识、按类型/任务配额、跨 session/学习经验联检未完成 |
| P1 会话经验 | 目标规格与数据模型草案 | SessionTurn、Feedback、Guidance、Learning、Improve 全链路未实现 |
| P2 | 架构规划 | OIDC/ACL、配额/计费、服务化、GraphCompletion、有效时间等 |

既有评估：12 文档/24 查询；22 条有答案题中 14 条全部来源命中，8 条返回空，2 条无答案题没有误召回；历史 keyword 文档 Recall@5 63.6%。**只代表合成关键词基线，不证明真正 RAG/Agent 效果**。Rust 1.98 新 CI 需单独完成，历史 Rust 1.88/1.96 的绿灯不能替代。

## 3. 交付顺序与任务

### S0（最高优先级）：完整身份候选过滤修复 — 已合入，待 CI 验收

**问题**：SQLite `keyword_hits()` 全 scope 取候选后截断到 100，`search()` 最终才按 identity 过滤；目标在第 101 名之后可能漏召回，向量候选也须在 native top-k 前限定范围。

- [x] **源码已实现（PR #13）**：在授权 tenant/workspace 内根据完整 `MemoryIdentity` 定位已有资产；未匹配为空，不创建 slot，旧库不隐式升级。
- [x] **源码已实现（PR #13）**：keyword 候选和 vector artifact ID 集合在 top-k/rank 之前限制为目标资产，最后仍重新确认权限、来源有效性、版本、墓碑和身份。
- [x] **测试已提交，未证明通过**：>100 个强相关干扰候选、v1/v2 当前版本、跨 workspace、缺失 identity、撤回来源与 search/resolve 回归。
- [ ] **验收未完成**：PR #13 原生 Clippy 的 `collapsible_if` 和 main Windows 同一错误需修复；取得 Rust 1.98 fmt/Clippy、完整原生测试、Smoke 与 main 跨平台 CI 的新实测证据后才能标记完成。

**验收**：不增大全局 top-k 作为修复；旧无 identity 查询保持兼容；精确身份目标不再被其他主体挤出候选。保留向量模型不可用时原有 allow_partial 行为。

### S1：建立检索有效性的实测证据（P0 质量收口）

- [ ] 固定 24 题基线及 8 条失败问句；冻结原始 source/version/locator、标注、数据 hash 和配置。
- [ ] 先补不少于 100 条人工证据标注题，再按原设计扩展到 300 条；治理安全与质量分开统计。
- [ ] 在同语料/同预算/同 embedding 条件下测 keyword、vector、hybrid；分别报告文档/证据 Recall@k、Coverage@B、nDCG、无答案误召回、p50/p95/p99、模型调用费用。
- [ ] 新增摘要和图谱的**独立评测开关**，进行纯原文 → +摘要 → +图的消融，不将三种检索模式等同于独立消融。
- [ ] 用数据决定是否需要 FTS/BM25、中文查询解析、reranker 和 ANN，不预先引入更复杂架构。

**验收**：每次实验保留 manifest、commit、语料 hash、人工标签、原始结果和复现命令，严禁使用 mock embedding 得到的得分宣称模型真实质量。

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

先收口 S0（候选下推源码已合入，但 CI 尚未全绿），之后实施 S1 的评估基线，再按 S2/S3 拆分小 PR。每次 PR 关联本路线图任务、回归用例、代码 SHA 和 CI 结果，完成后同步 STATUS 与 VALIDATION。
