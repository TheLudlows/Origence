# 实现状态与后续工作

更新：2026-10-09。当前推进以 main `99f8926` 为起点：PR #13 的完整身份候选下推与 PR #14 的 CI 优化均已合入；S0 源码已实现，但 PR #13 的原生 Clippy 和随后 main 的 Windows Clippy 因 `collapsible_if` 失败，**不能宣称 S0 已经跨平台验收通过**。本轮 P0 修复此告警并把轻量 Clippy 前移到 PR 快速检查，新 CI 结果须独立核验。M5/A1 的此前验收证据见 [VALIDATION](VALIDATION.md)，P1 闭环与 P2 仍未交付。

最低 Rust 版本现调整为 **1.98**：Linux 原生 CI 以 **1.98.0** 执行完整测试并核验 MSRV，开发/Windows/macOS/Docker 使用 **1.98.1**。CI 只在 `main` push 和目标为 `main` 的 PR 触发；PR 运行 `fast-check`（增加无原生后端的轻量 Clippy）与 `linux-native`，`main` 追加 Windows、macOS 与 Docker Release 验证；Linux Native 只运行 Debug。原有 memory-identity、Python evaluation、MSRV 检查分别并入快速检查和 Linux 原生 Job。以下 1.88/1.96 验收仅为历史证据，升级后需重新验证。

## 当前能力与验收快照

以下区分代码已实现、运行验收通过与效果已验证；历史切片的“待 CI”描述只表示记录当时状态。

| 范围 | 当前实现 | 已核验证据与剩余边界 |
| --- | --- | --- |
| M5 / A1 | SQLite/LanceDB/Kuzu/本地 Blob，单宿主与单 Worker，直接发布 | 三平台 fmt/clippy 与全量原生测试、Linux/macOS release/HTTP smoke、容器构建/HTTP smoke 及 Rust 1.88 默认后端 check 通过 |
| I1 记忆身份 | v1 编码、唯一绑定、显式写入、离线安装、精确 lookup、调用方版本前置条件；PR #13 已实现身份候选前置过滤 | 旧版功能有实测证据；PR #13 的新候选过滤测试已提交，但完整 Clippy/原生验收因告警未收口，待 P0 CI 修复后复测 |
| I2 抽取匹配 | 调用方完整身份的单身份 capture、原文区间、来源读取与绑定状态 | 无原生解析/SQLite 和三平台原生 HTTP 用例通过；自动推断、属性目录、未归一化状态和语义冲突未实现 |
| I4 上下文 | 类型/身份标注、完整身份过滤、引用与 UTF-8 字节预算 | 默认请求/身份匹配及三平台原生 render/HTTP 通过；冲突/类型预算未实现 |
| 检索质量 | keyword/vector/hybrid、摘要与一跳图扩展；12 文档/24 查询合成种子 | Python adapter 5 项通过；Linux/macOS 无模型 keyword Recall@5 均为 63.6%，相同 8 个可答问句漏召回，工件已核验；无真实模型质量/成本及组件消融结果 |
| P1 会话到经验 | 目标设计已明确 | session/feedback/guidance/learning/improve 主闭环未实现 |
| P2 增强与服务化 | 目标设计已明确 | 时间有效期、GraphCompletion、企业身份、配额和生产后端未实现 |

历史验收 CI（PR #11/Rust 1.88 阶段，不代表当前主干全绿）：[PR run 37871058933](https://github.com/TheLudlows/openContext/actions/runs/37871058933)，2026-10-09 12:03（UTC+8）完成，7/7 作业 success。三平台各 29 项原生 lib、77 项集成均通过，Windows 接口检查、Linux/macOS release/HTTP smoke 与关键词宿主评估、容器、Rust 1.88 默认后端 check 和轻量作业通过；已读取全部日志并核验两平台评估工件。此前 main `73fc51a` 的三个平台均因测试中 `published` 变量遮蔽使 MCP get 的 asset_id 为 null 而失败，PR #11 已修复。最终文档补记只修改 Markdown；这些运行证据归属于 `8d8d224`，不把后续文档提交的 CI 直接标为通过。完整 job/commit 证据见 VALIDATION；CI 通过与召回质量分开验收。

## 已交付

- 单宿主共享引擎：先初始化、检查、恢复任务与对账清理，再运行 HTTP 和单 Worker。OS 文件锁拒绝另一进程打开同库；Kuzu 同步调用通过有界阻塞执行器串行执行。
- `OC_DATA_DIR` 统一数据目录；CLI 的 `serve`、workspace/key 管理、search/get/resolve 和 MCP。客户端默认转发 HTTP；离线显式 `--offline`，宿主不可达不回退。
- SQLite 显式 scope 过滤，复合外键、WAL、IMMEDIATE 写事务、原子幂等/审计/入队；撤销与短写事务串行化，外部 IO 后重新认证和核验提交条件。
- writer 结构化记忆与 capture 抽取直接发布（A1 自动发布）：无候选/审核表，expected_version 乐观并发，账本化多事实发布；知识入库、不可变版本、历史标题、追加恢复、文件上传和文本 PDF 解析。
- 持久化发布计划与跨库账本；确定性产物 ID、generation/run_token 隔离、取消、有限自动重试、显式重试和启动恢复。外部产物只有在 SQLite 最终提交后可作为证据。
- 中文预分词关键词检索、精确向量检索、摘要/图谱证据与 RRF、引用和保守预算。最终结果核验当前版本、来源、资产和授权。
- 删除立即阻断读取，后台按 SQLite owner 清理派生数据；共享实体和关系有其他有效来源时保留，最后一个 owner 消失才删除。
- 普通测试用临时数据目录运行，涵盖 API/CLI/MCP/PDF 子进程、本地模型模拟和恢复；CI、容器配置及文档统一到单宿主。

旧 PostgreSQL 运行代码、Apalis SQL 和专用测试从活动树裁剪，历史参考为提交 `72fb5aa`。未提供 PG 到本地库的数据迁移或可选 PG 运行模式；未来可实现存储接口，不能据此宣称已支持。

## 当前推进批次（2026-10-09）

执行与验收清单见 [P0 验收补齐与评估基线](superpowers/plans/2026-10-08-validation-and-evaluation.md)。本轮已修复 PR #10 合入后的 MCP 测试失败，收口现有 CI 的完整证据并统一状态、目标设计与计划。Rust 1.98 完整运行测试（尚待新 CI 验证）、依赖审计和质量/运维验收仍保留未勾项。

P1 仍为会话问答、指导、反馈、经验蒸馏、水位与阶段化 improve；时间有效期/as_of 仍属 P2。真实质量评估沿用 300 用例目标。12 文档/24 查询的无模型关键词宿主基线已在 Linux/macOS 运行并核验，两平台结果一致：22 个可答用例中 14 个完整命中、8 个返回空，2 个不可答无误召回。不能把样本、fixture 或执行成功当作完整质量结果。

[记忆身份与上下文前置设计](superpowers/specs/2026-10-08-memory-identity-and-context.md) 的 I1、受限 I2 与 I4 标注/过滤切片已落入代码，验收范围以上表为准。原文区间与 explicit_identity 只表示可追溯性和绑定方式，不证明语义正确。PR #13 已将完整身份过滤前置到 keyword/vector 候选生成以避免不同主体挤占 top-100；新逻辑仍须通过 Rust 1.98 原生测试验收；图实体仍按名称生成 ID，记忆身份不能替代图谱同名消歧。Service 仍依赖 LocalEngine/SqliteTx，存储接口基础已交付，应用层完整后端解耦尚未完成。

下一阶段实施顺序、负责人可拆分的交付切片与验收门槛见 [2026-10-09 剩余任务路线图](superpowers/plans/2026-10-09-next-stage-roadmap.md)。S0 代码已通过 PR #13 合入 main，待修复 Clippy 和完整主干 CI 后，才把 S0 验收项标记完成。

## 接下来

1. **P0/S0 验收收口**：修复 PR #13 遗留的 `collapsible_if`、在 fast-check 运行轻量 Clippy，核验 >100 干扰候选及 scope/来源/版本测试与 main 跨平台 CI 结果。
2. 保留已核验的关键词失败用例，评估全词匹配下的问句与多来源漏召回；冻结人工证据标注、数据 hash 与配置，取得 keyword/vector/hybrid 对照基线，再用独立控制验证摘要/图收益。逐步扩至 300 用例，记录误合并、延迟及成本。
3. 补 I2 必要语义契约：属性单值/多值、更新/补充与歧义状态；推进 P1 最小闭环“会话与实际证据归档 → 幂等反馈 → 显式 improve → 有来源的经验重新入库”。不要求第一版一次实现全部 improve 阶段。
4. 按质量/规模结果选择 ANN、重排、profile 重建、tokenizer 和分页；Rust 1.98 完整运行测试、依赖审计，以及 retention、物理擦除、孤儿文件和备份恢复演练仍需单列验收。
5. P2 企业身份、文档 ACL、配额、公平调度、审计查询、指标及生产后端按后续设计推进；不扩展本地多 Worker。

当前仍是本地原型交付，不承诺生产 SLO、完整 V3.1 场景或竞品效果排名。依赖安全审计（`cargo audit` 核验 RustSec 告警；核验单 Worker OS 文件锁依赖 fs2 的维护状态，评估 fs4 的锁生命周期和平台兼容）、PDF OS 资源沙箱、断电恢复和长期压测尚未完成。

## 历史切片

PR #3–#11 的逐轮实现与验收记录（身份编码/升级、单身份 capture、来源读取、lookup、标注、身份过滤、版本前置条件、MCP 测试修复）见 [VALIDATION](VALIDATION.md) 各时间段；每条的 commit、CI run 与覆盖范围均留存于此处，不在此重复。当前状态以上方快照及 VALIDATION 最新核验为准。

