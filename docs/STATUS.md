# Origence · 实现状态与后续工作

产品名称由 openContext 统一为 **Origence**，Rust 包与 CLI 改为 `origence`；历史 PR、CI 和设计文档保留原名以便追溯。

更新：2026-10-10。**S0 正确性验收已完成。** 验收代码为主干 `42db5c21a8ef4788d07209be7110d85e01d7a85f`（[PR #20](https://github.com/TheLudlows/Origence/pull/20) 已合入）。[main run 37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081) attempt 1 的五个作业全部 completed/success；三平台原始日志均确认两个真实 LanceDB vector/hybrid HTTP 用例为 `ok`，各 29 项 lib、81 项集成全部通过。PR #13 身份下推及 PR #15 Clippy 修复均已合入。完整源码/checkout/run/job/计数见 [VALIDATION](VALIDATION.md) 和 [Issue #21](https://github.com/TheLudlows/Origence/issues/21)。本次随后归档提交仅修改文档与证据，其 CI 不与上述代码提交混用。

图后端于 2026-10-10 按 [替换计划](superpowers/plans/2026-10-10-replace-kuzu-with-sqlite-graph.md) 切换为 `SqliteGraphStore`，已删除 Kuzu crate、适配器、旧测试、探针和 `local-graph` feature；CI/Docker/Windows 脚本不再配置 CMake/Ninja。上方 S0 run 属于替换前代码，不能作为本轮替换验收；本轮 [PR #23](https://github.com/TheLudlows/Origence/pull/23) 的 [run 38030946507](https://github.com/TheLudlows/Origence/actions/runs/38030946507) 已完成：fast-check 与 Linux/MSRV 成功，默认 lib 28/local 89、13 项图回归、Clippy/HTTP smoke 全通过；PR #23 已合入 main `9c292a9`；主干 run `38032343493` 五作业全部成功，三平台默认 lib 28/local 89、13 项图回归与 vector/hybrid 定向回归均已核验。精确证据见 VALIDATION 最新记录。旧图文件需按 OPERATIONS 在新目录重新发布来源，不自动迁移。

最低 Rust 版本为 **1.98**：Linux 原生 CI 以 **1.98.0** 执行完整测试并核验 MSRV，开发/Windows/macOS/Docker 使用 **1.98.1**。PR 执行 `fast-check` 与 `linux-native`；main 追加 Windows、macOS 和 Docker。Linux Native 统一 Debug，macOS 保留 Release，Docker 保留 Release 镜像/Smoke。以下旧平台测试数字属于历史证据，不代替最新代码验收。

## 当前能力与验收快照

以下区分代码已实现、运行验收通过与效果已验证；历史切片的“待 CI”描述只表示记录当时状态。

| 范围 | 当前实现 | 已核验证据与剩余边界 |
| --- | --- | --- |
| M5 / A1 | SQLite/LanceDB/SQLite 图/本地 Blob，单宿主与单 Worker，直接发布 | SQLite 图替换 main `9c292a9` 的五作业全部成功；三平台默认 28+89 测试、Linux/macOS build/smoke 与 Docker 已核验，旧 S0 跨平台结果保留为历史证据 |
| I1 记忆身份 | v1 编码、唯一绑定、显式写入、离线安装、精确 lookup、调用方版本前置条件；keyword/vector 候选前置过滤 | PR #20 真实 LanceDB、130 个高排名干扰及 vector/hybrid 服务级回归在三平台均已通过；S0 正确性闭环 |
| I2 抽取匹配 | 调用方完整身份的单身份 capture、原文区间、来源读取与绑定状态 | 旧版无原生/原生用例有实测记录；自动推断、属性目录、未归一化状态和语义冲突未实现 |
| I4 上下文 | 类型/身份标注、完整身份过滤、引用与 UTF-8 字节预算 | PR #20 的 vector/hybrid HTTP search/resolve 一致性已在三平台通过；冲突/类型预算未实现 |
| 检索质量 | 六配置真实模型对照、摘要/图独立开关；36 篇/300 题及来源独立留出集 | S1 工程检索实验已完成，keyword/vector/hybrid-all Recall@5 为 0.35%/97.57%/94.79%；向量/混合无答案误召回 12/12，新增 200 题独立人工复核未完成；见 [实测报告](../evals/s1/results/local-77d43a8-v2/README.md) |
| P1 会话到经验 | 目标设计已明确 | session/feedback/guidance/learning/improve 主闭环未实现 |
| P2 增强与服务化 | 目标设计已明确 | 时间有效期、GraphCompletion、企业身份、配额和生产后端未实现 |

历史验收 CI（PR #11/Rust 1.88 阶段，不代表当前主干全绿）：[PR run 37871058933](https://github.com/TheLudlows/openContext/actions/runs/37871058933)，2026-10-09 12:03（UTC+8）完成，7/7 作业 success。三平台各 29 项原生 lib、77 项集成均通过，Windows 接口检查、Linux/macOS release/HTTP smoke 与关键词宿主评估、容器、Rust 1.88 默认后端 check 和轻量作业通过；全部日志和两平台评估工件已有核验记录。这些运行证据归属于 `8d8d224`，不将后续提交自动标记通过。完整 job/commit 证据见 VALIDATION；CI 通过与召回质量分开验收。

## 已交付

- 单宿主共享引擎：先初始化、检查、恢复任务与对账清理，再运行 HTTP 和单 Worker。OS 文件锁拒绝另一进程打开同库；SQLite 图通过 SQLx 异步连接池访问，关系库独占锁仍约束单宿主。
- `OC_DATA_DIR` 统一数据目录；CLI 的 `serve`、workspace/key 管理、search/get/resolve 和 MCP。客户端默认转发 HTTP；离线显式 `--offline`，宿主不可达不回退。
- SQLite 显式 scope 过滤，复合外键、WAL、IMMEDIATE 写事务、原子幂等/审计/入队；撤销与短写事务串行化，外部 IO 后重新认证和核验提交条件。
- writer 结构化记忆与 capture 抽取直接发布（A1 自动发布）：无候选/审核表，expected_version 乐观并发，账本化多事实发布；知识入库、不可变版本、历史标题、追加恢复、文件上传和文本 PDF 解析。
- 持久化发布计划与跨库账本；确定性产物 ID、generation/run_token 隔离、取消、有限自动重试、显式重试和启动恢复。外部产物只有在 SQLite 最终提交后可作为证据。
- 中文预分词关键词检索、精确向量检索、摘要/图谱证据与 RRF、引用和保守预算。最终结果核验当前版本、来源、资产和授权。
- 删除立即阻断读取，后台按 SQLite owner 清理派生数据；共享实体和关系有其他有效来源时保留，最后一个 owner 消失才删除。
- 普通测试用临时数据目录运行，涵盖 API/CLI/MCP/PDF 子进程、本地模型模拟和恢复；CI、容器配置及文档统一到单宿主。

旧 PostgreSQL 运行代码、Apalis SQL 和专用测试从活动树裁剪，历史参考为提交 `72fb5aa`。未提供 PG 到本地库的数据迁移或可选 PG 运行模式；未来可实现存储接口，不能据此宣称已支持。

## 当前推进批次（2026-10-09）

S0 已完成“真实向量定向回归 → 包含这些测试的 main CI → 验收证据归档”。新用例位于 `tests/identity_vector_regression.rs`，通过 `tests/local.rs` 编入既有原生集成可执行程序，无额外 native Job，也没有跳过测试来取得绿灯。三平台完整 local 套件的 81 个实际测试名称一致，0 failed/ignored/measured/filtered；轻量套件不替代原生证据。

`vector` 与 `hybrid` 各有一个命名用例：真实 SQLite/LanceDB 批量建立 130 个更接近查询的干扰身份，验证全局 top-100 缺失目标，再通过实际 Origence 子进程的 HTTP search/resolve 验证身份限定能返回目标。目标正文故意不匹配关键词，避免 hybrid 用关键词支路掩盖向量错误。还覆盖当前 v2、物理存在的 v1 向量、同名身份跨 workspace/tenant、缺失身份/条件、模型不可用与显式降级、来源撤回和资产墓碑。模型仅为 loopback 固定向量服务，不使用真实付费模型。

最终证据为 [main run 37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081) attempt 1：fast-check `113779676713`、Linux Native/MSRV `113779676656`、Windows `113779676444`、macOS `113779676800`、Docker `113779676764` 全部成功。五个 checkout 均为完整验收代码 SHA。PR head、PR 合成 checkout 与实际 main merge 的 SHA 不同、tree 相同，分别记录在 VALIDATION。原 `e8ee1ae` 不含新用例，历史通过记录仍不能代替此轮证据。

P1 仍为会话问答、指导、反馈、经验蒸馏、水位与阶段化 improve；时间有效期/as_of 仍属 P2。真实质量评估已补 S1 的 300 题合成检索切片，包含真实本地模型、独立消融与留出集；完整平台用例协议、Agent 与生产验收仍待后续。12 文档/24 查询和固定向量正确性回归保留为独立历史基线。

[记忆身份与上下文前置设计](superpowers/specs/2026-10-08-memory-identity-and-context.md) 的 I1、受限 I2 与 I4 切片已落入代码。原文区间与 explicit_identity 只表示可追溯性和绑定方式，不证明语义正确。图实体仍按名称生成 ID，记忆身份不能替代图谱同名消歧。Service 仍依赖 LocalEngine/SqliteTx，应用层完整后端解耦尚未完成。

## 接下来

1. **S0 已完成**：完整身份候选下推、真实定向回归、主干完整 CI 和证据归档已闭环；归档提交及正常合并关系由 Issue #21 的完成记录追溯。见 [路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)。
2. **S1 工程实验与归档完成（2026-10-10）**：固定旧 24 题及八条失败问句，冻结 36 篇/300 题（开发 200、来源独立留出 100），真实 BGE + Qwen 同发布产物六配置对照与摘要/图独立消融完成；原始响应、模型 hash、引用/预算独立审计、延迟和费用已归档。[报告](../evals/s1/results/local-77d43a8-v2/README.md) 与 VALIDATION 记录全部失败。新增 200 题为助手标注，独立人工复核尚待完成；本结果不认证完整平台 300 用例、Agent 任务或生产质量。
3. **S2/S3**：补单值/多值和歧义契约，推进“会话与实际证据归档 → 幂等反馈 → 显式 improve → 有来源的经验重新入库”。
4. 依赖安全审计、retention、物理擦除、孤儿文件、断电和备份恢复仍需独立验收；ANN/重排/profile/tokenizer 由实际质量和规模决定。
5. P2 企业身份、文档 ACL、配额、公平调度、审计查询、指标及生产后端按后续设计推进；不扩展本地多 Worker。

当前仍是本地原型交付，不承诺生产 SLO、完整 V3.1 场景或竞品效果排名。分支保护/Required Checks 是独立仓库治理配置，不由一次功能测试通过自动满足。

## 历史切片

PR #3–#11 的身份编码/升级、单身份 capture、来源读取、lookup、标注、版本前置条件与 MCP 测试修复，及后续 PR #13/#15 的候选过滤和 Clippy 修复，均在 [VALIDATION](VALIDATION.md) 保留原始 commit/run 与验收边界。当前状态以上方快照及最新证据为准。
