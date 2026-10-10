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
| M0–M5 / P0 | 单宿主 SQLite+LanceDB+SQLite 图+Blob，HTTP/MCP，异步发布、幂等、账本、权限、溯源、撤回、版本 | 大规模/故障验收、依赖审计、生产运维可靠性 |
| I1 记忆身份 | 明确身份写入/唯一性/精确 lookup、expected_version、离线升级；候选前置限定已实现 | S0 真实 vector/hybrid 定向回归及主干完整 CI 已验收；语义匹配/冲突仍未实现 |
| I2 抽取 | 调用方明确 identity 的 capture、精确引用区间校验 | 未归一化状态、属性目录、单值/多值、语义匹配/冲突管理 |
| 检索 | keyword/vector/hybrid、摘要和图扩展、RRF、证据引用 | 目前仅 12 文档/24 查询无模型合成基线；真实模型/Agent 质量和组件收益未知 |
| I4 上下文 | 现有 search/resolve、身份标注、引文与字节预算 | 冲突标识、按类型/任务配额、跨 session/学习经验联检未完成 |
| P1 会话经验 | 目标规格与数据模型草案 | SessionTurn、Feedback、Guidance、Learning、Improve 全链路未实现 |
| P2 | 架构规划 | OIDC/ACL、配额/计费、服务化、GraphCompletion、有效时间等 |

既有评估：12 文档/24 查询；22 条有答案题中 14 条全部来源命中，8 条返回空，2 条无答案题没有误召回；keyword 文档 Recall@5 63.6%，S0 主干 Linux/macOS 日志亦保留该合成基线。**只代表合成关键词基线，不证明真正 RAG/Agent 效果**。Rust 1.98 的主干 CI 已单独核验，证据见 VALIDATION；没有复用历史 Rust 1.88/1.96 的绿灯。

2026-10-10 存储维护：图投影已按 [SQLite 图替换计划](2026-10-10-replace-kuzu-with-sqlite-graph.md) 替换；不改变 S1/P1 优先级。替换前 S0 CI 仍是历史证据，本轮验证单独归档：[PR #23](https://github.com/TheLudlows/Origence/pull/23)、run `38030946507` 的 fast-check/Linux/MSRV、默认 28+89 测试及 HTTP smoke 成功；PR #23 已合入，main `9c292a9` 的 run `38032343493` 五作业全部成功，三平台默认 28+89 测试、13 项图回归及 vector/hybrid HTTP 回归均通过。

## 3. 交付顺序与任务

### S0：完整身份候选过滤修复 — 已完成正确性验收

**原始问题**：全 scope 的 keyword/native-vector 候选先截断，再按身份过滤时，其他主体可能占满 top-100。PR #13 已将完整身份限定前置到候选生成；PR #15 已修复阻塞 Clippy 的嵌套 if；PR #20 的两个真实原生回归及包含它们的主干完整 CI 已核验。验收代码 `42db5c21a8ef4788d07209be7110d85e01d7a85f`，[run 37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081) attempt 1，5/5 作业成功。源码/合成 checkout/实际 merge、job 与实际测试计数已归档至 [VALIDATION](../../VALIDATION.md)；归档提交/PR 合并结果由 [Issue #21](https://github.com/TheLudlows/Origence/issues/21) 追溯。

- [x] **源码已实现（PR #13）**：授权 tenant/workspace + 完整 `MemoryIdentity` 定位已有资产；未匹配为空，不创建 slot，不隐式升级旧库。
- [x] **源码已实现（PR #13）**：keyword 与 vector artifact ID 集合在 top-k/rank 之前限定资产，最终继续复核权限、来源、当前版本、墓碑和身份。
- [x] **既有测试已提交**：keyword 的 >100 干扰、v1/v2、跨 workspace、缺失身份、撤回与 search/resolve；SQLite vector 候选 ID 与旧库不升级测试。
- [x] **Clippy 修复已合入（PR #15）**：保留 `-D warnings`，fast-check 新增无原生后端的 Clippy。
- [x] **新回归代码已合入（PR #20）**：`tests/identity_vector_regression.rs` 中 `s0_vector_identity_prefilter_real_lancedb` 与 `s0_hybrid_identity_prefilter_real_lancedb`，编入现有 `tests/local.rs`。真实 LanceDB、130 个更高排名干扰、目标关键词必不匹配、实际宿主 HTTP search/resolve、跨 workspace/tenant、当前版本、撤回/墓碑、显式模型降级。
- [x] **新增原生回归验收**：三平台原始日志均包含两个精确命名用例 `ok`；完整 local 套件各 `81 passed / 0 failed / 0 ignored / 0 measured / 0 filtered`，实际测试名称集合一致。
- [x] **主干跨平台验收**：fast-check `113779676713`、Linux Native/MSRV `113779676656`、Windows `113779676444`、macOS `113779676800`、Docker Smoke `113779676764` 在同一 main run attempt 1 全部 completed/success；checkout 均对应验收代码。
- [x] **证据归档**：VALIDATION 保存完整 provenance、原生 lib/local/bin/doc 分组与轻量/MSRV 实际计数，静态 JSON 保存日志摘录；STATUS 和本节已同步。原始历史证据保留，PR 的三个 skipped 平台不冒充主干通过。

**复现命令**：`cargo test --locked -j 2 --test local identity_vector_regression -- --nocapture`。测试仅使用 loopback 固定向量模型，不调用真实付费模型，不变更全局进程环境。

**验收**：不增大全局 top-k；旧无 identity 查询保持兼容；精确目标不被其他主体挤出候选；hybrid 不能靠 keyword 支路掩盖向量错误；来源撤回不回退旧版本；模型不可用时遵守 allow_partial。S0 是正确性门槛，不是 S1 语义效果评分。

### S1：建立检索有效性的实测证据（P0 质量收口）

- [x] 固定 24 题 keyword 基线及 retrieval-15 至 retrieval-22 八条失败；source/version/locator、数据 hash 和配置归档。
- [x] 冻结检索 v2：36 篇合成来源、300 题（开发 200 / 来源独立留出 100）。原 100 题及其既有人工标签不变；新增 200 题由助手编写并逐条核对原文证据。此项完成数据工程，不能据此宣称 300 题均经人工复核。
- [ ] 新增 200 条标签的独立人工复核；完整平台评估协议与生产代表数据不由本检索切片替代。
- [x] 同语料/同预算/同 embedding 的 keyword、vector、hybrid 六配置完成；报告文档/证据 Recall@5、Coverage@2000 UTF-8 字节、nDCG、无答案误召回、search/resolve p50/p95/p99、usage 与 provider 费用（CPU 未定价）。
- [x] 摘要和图谱独立开关：纯原文、+摘要、+图、+摘要+图四组独立消融，所有组复用同一真实模型投影。
- [x] 根据数据记录查询解析/FTS/BM25、拒答阈值、reranker 与 ANN 的优先级；本轮不提前引入新架构。

**工程交付**：代码 `77d43a8`，六组共 1800 次 search/1728 次 resolve，原始结果、模型 revision/file hash、命令、开发/留出分数与独立指标审计见 [S1 实测报告](../../../evals/s1/results/local-77d43a8-v2/README.md)。PR #24 源码 CI 的两个实际作业成功；图替换 main 五作业证据单独保存。严格标签复核尚未完成，不宣布完整平台质量验收通过。

**验收**：每次实验保留 manifest、commit、语料 hash、标签作者与复核状态、原始结果和复现命令，严禁使用 mock embedding 得到的得分宣称模型真实质量。

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
