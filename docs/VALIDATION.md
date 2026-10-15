# Origence · 验收与历史证据

本页按原记录保留各阶段、精确提交与运行 ID 的验收证据。历史正文中的“当前”“进行中”“待验收”属于该阶段，不是最新项目状态；最新进度见 [STATUS](STATUS.md)。CI 通过、测试通过与产品效果分别判断，不将旧结果外推到新 head。

## 最新合入与验证索引（2026-10-15）

- [PR #63](https://github.com/TheLudlows/Origence/pull/63) 最终 head `22c646988d9fd997f0978b6920e8d41977b6d4f3` 的 [CI 38696372574](https://github.com/TheLudlows/Origence/actions/runs/38696372574) 四项作业全部通过：fast-check、Linux native、Windows、identity-layout。身份索引修复、账本两列精简及 Windows 文件快照修正已完成该精确 head 验证。
- PR #63 已合入 main `53de265bf6b018fa3c26502511c2fd8705d2bea2`，[主干 CI 38700118381](https://github.com/TheLudlows/Origence/actions/runs/38700118381) 适用门禁通过。[PR #62](https://github.com/TheLudlows/Origence/pull/62) 三批裁剪此前已合入 `4c0862e80ccfb8b403268170c06d391239790d2b`；此主干结果不改写各批原始验证记录。
- [Issue #52](https://github.com/TheLudlows/Origence/issues/52)、[#60](https://github.com/TheLudlows/Origence/issues/60)、[#61](https://github.com/TheLudlows/Origence/issues/61) 已关闭。[#64](https://github.com/TheLudlows/Origence/issues/64) 检索召回/延迟/资源与 [#65](https://github.com/TheLudlows/Origence/issues/65) 索引/压实维护中断恢复仅登记为后续验收，均为 **NOT_RUN**。真实 Agent 效果、官方 AML Smoke/Full、完整历史工件审计亦未由上述 CI 验收。

历史过程：[Issue #60 身份合表](#issue-60-第一批身份合表) · [身份修复与账本验证](#身份修复完整验收与第二批格式检查) · [Issue #61 第一批](#issue-61-第一批裁剪2026-10-14验证中) · [第二批精确头验收](#第二批精确头验收) · [第三批归档](#issue-61-第三批归档与文档收口2026-10-14验证中)。以下正文、失败结果、原始证据与链接保持不变；运行步骤见 [OPERATIONS](OPERATIONS.md)，接口变更见 [API](API.md)。

## 2026-10-15 合入后整理（待精确头验证）

身份测量 fixture 原样迁至非默认 `manual-diagnostics` 特性的独立测试目标；
移除专用测量 job 与 #60/#61 Windows 分支例外，保留 Windows main-only 策略。
本次整理的 Rust 格式、Clippy、Linux 原生测试与手工测量尚待精确 head 验证；
Windows 本地未运行且不在 PR 门禁中。上方已合入 head 的成功结果不代替本候选验收。

本地检查：SQL guard 识别 1142 个执行输入，零违规/零未解析；tools Python 共
113 项（2 项既有宿主依赖跳过），evals Python 42 项通过。Python 套件在随后 Rust
测试调整前运行，其 Python 源码未变。`git diff --check` 与五份文档 142 个相对链接/
锚点检查通过；本页历史验收正文与身份诊断历史段落逐字保持不变。未执行本候选 Rust/CI。

## 2026-10-12 第四阶段：显式 Improve → Learning

基于 main `be16bb1c2a273ec632e459abd277bd019fb36821`。新增确定性闭环、持久化阶段重放、principal-qualified 反馈序列、条件限制、子任务重试/取消和传递依赖撤回回归；不使用真实 provider，不据此声明语义质量或 Agent 收益。当前为实现检查记录，最终 PR head 的 CI 结果仍待补充。

云端编辑环境没有 cargo/rustc/rustfmt，未在本地执行 Rust 编译、格式、Clippy 或测试。Python SQLite 完整新库 DDL 执行和 foreign_key_check 通过；静态检查不替代实际 Rust 验证。保留现有 CI：PR fast-check/Linux native，Windows 仅 main；不恢复已取消的 macOS/container jobs。

[PR #43](https://github.com/TheLudlows/Origence/pull/43) 初始 head `60e7a0cb3d7c72d9c13f539fe11656004db71f9a` 的 [CI run 38202258799](https://github.com/TheLudlows/Origence/actions/runs/38202258799)：Linux 默认 all-target Clippy（warnings denied）通过，59 项库测试通过，集成测试 155 通过/3 失败。三个失败为夹具与契约不一致：reader 支持 quote 已脱敏、撤回后仍允许读取安全运行管理信息、已撤销 AuthContext 通过 Service 调用返回 Forbidden。夹具已按现有权限及文档修正；后续新增并发去重、循环/闭包边界、候选预算回归仍须最终 head 实际验证。fast-check 在 rustfmt 失败，按 CI 精确格式差异调整。该初始失败记录不代表最终通过。

## 2026-10-12 第三阶段：SessionTurn 与幂等 Feedback

基于 main `6a17c32b4407d530804ff63814d3858a474c4790`，按用户确认不等待该主干 CI 再开发。仅新库增加关系型会话表；无升级迁移、模型调用、索引任务、新 Worker 或反馈改权。公开请求类型覆盖严格字段、枚举、UUID、UTF-8 边界与显式实际证据；Service/HTTP 回归覆盖关系型事务、权限、历史 provenance、幂等及依赖撤回。具体执行结果以本 PR 最终 head CI 为准。

云端编辑环境没有 Cargo、rustc、rustfmt；本地静态检查不能替代编译、格式、Clippy 与测试。SQLite DDL 执行和兼容性规范化检查通过。独立审查未发现剩余生产阻塞，并已修正两个测试夹具问题。

[PR #41](https://github.com/TheLudlows/Origence/pull/41) head `933de3b1b2d4f61983988f9637a60e035f5b40b5` 的 [CI run 38199658817](https://github.com/TheLudlows/Origence/actions/runs/38199658817)：Linux native 成功，Rust 1.98.0、严格默认 Clippy、完整默认测试、debug build/HTTP smoke、受控进程恢复与 keyword 种子评估通过。新增 11 项契约单测与 15 项会话/反馈回归均成功。fast-check 在 rustfmt 检查失败（仅格式）；按工具输出调整格式，后续最终 head 必须重新通过格式、无默认特性检查与 Linux CI，不能用这次 Linux 成功替代完整门槛。Windows/macOS/container 在 PR 中按既有 main-only 策略跳过。历史记录保持原样。

## 2026-10-12 第二阶段：内部任务类型与证据可见性

基于已通过五项主干 CI 的 `7a20540cfadf94199d2787415c619de2aaba9721`。新增私有 JobKind/JobPayload 编解码及九种输入的 JSON v1 golden 回归，Service/Worker 使用类型化字段；WorkItem/ClaimedJob、SQLite 表和保存计划路径不变。损坏任务在 process_claim 的既有错误结算边界解码，新增未知操作、损坏 schema/字段/identity 与后续有效任务回归，以及过期 publication 在解码前按 generation 忽略的回归。保留第一阶段日志哨兵、目标漂移及原有恢复/权限/取消/来源撤回测试。

在现有语义测试中复用共享读取矩阵，对 get（当前/指定版本）、精确 lookup、keyword/vector/hybrid 及 resolve 比较资产、身份、来源、locator 与引用；覆盖根来源/集合成员撤回（清理前）、独立不确定证据、重复输入来源、资产删除与 scope 隔离。单块 resolve 覆盖精确字节预算和少一字节，包含语义标注及多语言引用。没有重写可见性 SQL，也不把工程夹具结果当作真实模型质量或 Agent 效果结论。

本地云端编辑环境没有 Cargo、rustc 或 rustfmt；静态差异检查通过，不能据此宣称 Rust 编译、格式、Clippy 或测试已执行。Rust 1.98.0 的默认/无默认特性、严格 Clippy、格式及完整 local 生命周期验证由该 PR 的精确最终 head CI 核验，结果以精确提交的 CI 为准。

[PR #40](https://github.com/TheLudlows/Origence/pull/40) 初始实现 head `349ac05c9085d4a82deafff42b155e1042f2916a` 的 [CI run 38196722600](https://github.com/TheLudlows/Origence/actions/runs/38196722600)：Linux native 成功，Rust 1.98.0 版本核验、默认严格 Clippy、lib 45/local 130、debug build/HTTP smoke、受控进程恢复和 keyword 种子评估均通过。六项 codec 单测、损坏任务/下一有效任务、publication generation、共享语义矩阵及第一阶段日志/目标漂移回归均在日志中明确成功。fast-check 在 rustfmt 处失败（仅格式）；随后按该工具输出修正换行、尾部空行与格式，不改变行为。无默认特性完整测试与格式门槛不能从这次 Linux 结果推断成功，仍由更新后 PR head 的 CI 核验；PR 中 Windows/macOS/container 按 main-only 规则跳过，合入后逐项核验主干。

## 2026-10-12 第一阶段收口：文档、日志与版本契约

基于 main `359365b366f20380847f7bec868caca60946de8d`，保持新库范围和既有发布语义。本次修正 API 的四种 normalization_status、重复写入响应和目标版本检查边界；更新 STATUS/S3 计划中的已合入与远端 CI 证据。以下历史验证记录保持原样，不将当时的“未提交/无新远端 CI”改写为后来结果。

原 fact_key 抽取的重复告警仅保留 job_id 和固定消息，不输出模型提供的键。新增 `worker::tests::duplicate_extracted_fact_log_omits_sensitive_key`，校验重复仍被丢弃、告警仍可按 job_id 关联且敏感哨兵字符串不出现在日志。新增 `semantic_capture_changed_unused_or_uncertain_target_does_not_block_publication`，在两个请求均受理后先发布一个目标，验证另一个请求的未使用目标、提议、否定、歧义及未经授权 update 的独立证据路径不被该目标版本变化阻断；原有确定性写入过期后 superseded 回归保留。不增加全目标 CAS、不改变事务、Worker、来源撤回、重试或持久化行为。

本次云端编辑环境未安装 Cargo/Rust。`git diff --check` 通过；以下命令已尝试但均因 `cargo: command not found`（退出 127）未执行，不能视为通过：

- `cargo fmt --all -- --check`
- `cargo test --locked --lib duplicate_extracted_fact_log_omits_sensitive_key`（worker 由 local-storage feature 启用；先前尝试的 no-default-features 形式即使有 Cargo 也不会选中该测试）
- `cargo test --locked --test local semantic_capture_`
- `cargo clippy --locked --all-targets -j 2 -- -D warnings`

本地完整测试、严格 Clippy、Rust 1.98.0 构建及原生存储验证均未执行。随后 [PR #39](https://github.com/TheLudlows/Origence/pull/39) 的实现提交 `238403bb2329fb959b0f5dd11e45d924aeba2ff6` 经 [CI run 38193448805](https://github.com/TheLudlows/Origence/actions/runs/38193448805) 验证成功：fast-check 与 Linux native 均通过。默认 lib 39/local 128、无默认特性 lib 36/local 89 全通过；新增日志哨兵与目标漂移回归、原有确定性版本竞争回归均在 Linux 日志中明确通过。格式、默认/无默认严格 Clippy、49 项 Python 工具测试及 9 项评估适配测试、debug build/HTTP smoke、受控进程恢复和 keyword 种子评估步骤均成功。Rust 版本确认步骤验证 1.98.0。

这项远端结果属于上述精确实现提交；本节补记只更新验证文档，不改变已测代码。PR 规则跳过 Windows/macOS/container，不将跳过计作验收；后续提交仍须检查自己的 CI。工程桩回归不证明真实模型质量或 Agent 收益。

## 2026-10-11 S3 首个切片：受控对话提取 → S2

按用户确认的三项串行完成纯库提取契约、S2 写入/持久 Worker 接入及端到端验证；只面向新库，不开发升级。基线 HEAD db939960b5ada880ca56c5a0968b7256af57f3cf，包含上一节所记 S2 的未提交工作区改动。[实施契约](superpowers/plans/2026-10-11-s3-semantic-capture.md)、[完整命令输出与 19 个最终源码/测试文件哈希](evidence/2026-10-11-s3-semantic-capture-validation.json)。新增公开库 API 与 POST /v1/captures/semantic；调用方限定完整身份/版本和单值覆盖授权，模型生成 operation/value/quote 后复用同一个 S2 路由函数。未增加依赖、表、CLI 隐藏协议、Worker 或升级路径。

实际工具链 rustc 1.98.0 (88d9e12ae 2026-08-18)，Windows MSVC。统一 exec 启动器不可用，使用本地 Node/PowerShell 调用已安装工具链；所有 Cargo 命令串行、offline/locked。复核发现共用模型响应的外层 JSON 解析仍在异步线程，已改为 await spawn_blocking；内层模型候选解析/校验与分词也在事务外等待 blocking 工作，不增加硬取消保证。修正后重新运行完整矩阵，最终全部退出 0：

| 命令 | 最终结果 |
| --- | --- |
| cargo fmt --all -- --check | 通过 |
| cargo clippy --offline --locked --all-targets -j 2 -- -D warnings | 通过 |
| cargo test --offline --locked --lib -j 2 | 38 passed，1.26 秒 |
| cargo test --offline --locked --test local -j 2 | 127 passed，129.70 秒 |
| cargo clippy --offline --locked --no-default-features --all-targets -j 2 -- -D warnings | 通过 |
| cargo test --offline --locked --no-default-features --lib -j 2 | 36 passed，1.17 秒 |
| cargo test --offline --locked --no-default-features --test local -j 2 | 89 passed，2.33 秒 |

阶段验证：新纯库提取测试 3 passed；共享路由重构后的既有 semantic_ 回归 10 passed；新捕获端到端最终 7 passed / 120 filtered，2.39 秒。新增夹具首次编译把 revoke_key 错当成需要幂等键的方法（E0061），并修正搜索过滤字段为 memory_identity。首次运行 4 passed / 2 failed：误期望未注册属性返回 400（现有契约为 422），误期望撤回当前作业来源产生 superseded（来源撤回主动取消，实际 cancelled）。修正测试断言，并补充处理中撤销 writer 权限后 6 passed；新增旧成员来源撤回/模型配置检查后 7 passed。错误及修正均保存在证据文件中；未为迎合测试更改现有错误码/撤回语义。

覆盖实际 loopback HTTP、公开 Service、SQLite/LanceDB、原串行 Worker 和 keyword/vector/hybrid/resolve：多语言原文字节范围及 extraction provenance；属性/条件/身份限制；未知字段、伪造引用、重复目标、超限和整批拒绝；空结果/未知身份；重复不增版本、受授权单值更新、冲突/提议/否定/歧义、集合并集；另一个同 workspace 调用方可检索，跨 tenant/跨 workspace 无泄漏；同名主体及不同环境独立；并发版本、幂等冲突、取消/重试、处理期间撤权及来源撤回；旧成员 provenance 经过并集/重建保持。embedding 失败前已保存计划，显式重试后 claim 并 Service 重开，提取请求总数仍为 1；受理后模型配置改变、待发布集合旧成员来源撤回均阻断发布。任何路由错误不发布已经验证成功的子集。

限制：本轮完成受控对话捕获，不是完整 SessionTurn/Feedback/Improve/Learning，也未新增会话结构化归档或实际使用证据追踪。目标稳定身份和条件来自调用方，非自动别名解析；模型桩仅证明工程契约，未验证真实模型分类准确率、独立语义质量或 Agent 收益。显式 allow_update 与精确 quote 不证明事实真假。重开验证不是新强退/断电验收；无新远端 CI、Linux/macOS、release/container 或生产 SLO 证据。保留原有逻辑撤回/物理擦除边界，历史记录及 S2 原始证据不改写。

## 2026-10-11 S2 显式记忆语义：三个任务串行交付

基于本地 HEAD db939960b5ada880ca56c5a0968b7256af57f3cf 的未提交工作区改动，按属性目录 → 写入规则 → 完整路径验收串行完成。[实施契约](superpowers/plans/2026-10-11-s2-memory-semantics.md)、[完整命令/输出/最终源码哈希](evidence/2026-10-11-s2-local-validation.json)。新增 workspace 管理员属性目录、单值/多值及必需条件；显式 semantic 入口复用命令幂等和单 Worker；重复不增版本、单值更新、多值补充、冲突/否定/提议/歧义的独立未归一化证据已覆盖。未修改依赖、Cargo.lock、身份 v1 编码、原有解析器或 CLI 分工。

实际工具链 rustc 1.98.0 (88d9e12ae 2026-08-18)，Windows MSVC；使用本地 Node/PowerShell 启动已安装工具链（统一 exec 启动器不可用）。下列命令串行执行，最终均退出 0：

| 命令 | 最终结果 |
| --- | --- |
| cargo fmt --all -- --check | 通过 |
| cargo clippy --offline --locked --all-targets -j 2 -- -D warnings | 通过 |
| cargo test --offline --locked --lib -j 2 | 35 passed，1.17 秒 |
| cargo test --offline --locked --test local -j 2 | 120 passed，140.95 秒 |
| cargo clippy --offline --locked --no-default-features --all-targets -j 2 -- -D warnings | 通过 |
| cargo test --offline --locked --no-default-features --lib -j 2 | 33 passed，1.17 秒 |
| cargo test --offline --locked --no-default-features --test local -j 2 | 89 passed，2.47 秒 |

阶段结果：目录测试 3 passed；语义规则单元 3 passed；第二阶段服务回归 2 passed；第三阶段 semantic_ 筛选 10 passed（含既有 normalization_status 回归）。完整集成初次为 119 passed / 1 failed：新增 HTTP 夹具只处理一次队列，误把待执行 cleanup 当成目标发布已完成。修正夹具为驱动同一串行 Worker 直至目标任务完成后，该 HTTP 用例单独 1 passed / 119 filtered（4.38 秒），完整重跑 120 passed。修正只涉及测试夹具；不隐藏首次失败。目录夹具初次编译使用不存在的 close 方法，改为 pool().close() 后通过。

新增回归通过公开 Service API 和实际 loopback HTTP → job → keyword/vector/hybrid/resolve 路径验证：管理员/reader 权限、跨 tenant 与同 tenant 跨 workspace、稳定主体和生产/测试条件隔离、幂等重放/冲突、并发 expected_version、32 成员上限、中文/emoji/组合字符原文定位、伪造范围拒绝、单值互斥输入、不确定状态渲染、旧入口/restore 绕过拒绝。固定向量模型失败后显式重试成功；取消后重试及已 claim 任务重开恢复通过。多值旧成员来源及 source_version 保持，支持来源撤回后整快照在向量候选及读取中退出；新显式补充可重建仍有支持的集合。原有 PDF、AML、身份、存储和维护测试包含于完整 local 回归。

限制：S2 仅新库，未提供旧库迁移；operation/value 由调用方分类，逐字 quote 不证明语义真假；没有自动别名合并、真实模型语义质量或新 Agent 效果结论。整快照撤回是保守可见性规则，当前发布来源撤回后需重新提供事实；不承诺全部派生副本立即物理擦除。重开恢复不是新 S2 强退/断电验收；没有本改动的新远端 CI、Linux/macOS、容器/release 或生产 SLO 证据。历史记录与实验工件保持原样。


## 2026-10-11 完整公开库、持续写入与再次压实

[计划与报告](../evals/aml/maintenance-scale-v1/README.md)、[原始记录](../evals/aml/maintenance-scale-v1/run.json)、[独立审计](../evals/aml/maintenance-scale-v1/audit.json)。真实本地 BGE-M3、32 用户/1524 批/15685 条消息，经实际 Add/Search 与停机 CLI 完成基线、压实、重启、外部四用户追加 256 批和再次压实。168 次 Search / 16800 个返回项无错误，完整响应 data/排序等值、原文字节/parser/范围/归属检查及维护行数/版本记录全部通过；完整压缩响应已保留，临时树已清理，无聊天调用。

HTTP p50 / p95：基线 5516.40 / 8026.00 ms；压实 714.51 / 846.79；重启 708.84 / 827.37；追加完成 1524.36 / 1904.78；再次压实 781.30 / 1041.05。每阶段 32 条、nearest-rank，另有追加期间 8 条；保留全部首请求，压实相关阶段最大值仍约 1.93–1.98 秒。17823 行、1524 → 8 碎片，第一次维护 2745.22 ms；追加后 18079 行、264 → 8 碎片，第二次 760.49 ms，两次行数均保持。

python -W error::ResourceWarning tools/aml_maintenance_verify.py --directory evals/aml/maintenance-scale-v1 --output evals/aml/maintenance-scale-v1/audit.json 退出码 0、passed，无模型调用。入库前段与本地回归重叠，不作吞吐量结论；本地测试在 Search 开始前已结束。公开历史有重叠，内容归属不能独立识别相同原文跨用户来源；跨 scope 的不同值和复用 ID 隔离由库与既有 HTTP 回归补充。无新语义质量、长期 SLO、多 profile 实际模型性能、维护中断、断电或磁盘不足验收。

## 2026-10-11 容器输入修复主干验收

PR #34 合入 main 5b4bd94008d833b04078975bef2d122fd09d5001；[run 38135583388](https://github.com/TheLudlows/Origence/actions/runs/38135583388) 五个作业全部成功。三平台默认 lib 32/local 110；Linux、macOS 与 container 的 HTTP smoke 通过，容器完成实际 release 编译。新的 Docker build-inputs 检查已在 PR #34 与主干 fast-check 执行成功。[精确记录](evidence/2026-10-11-container-input-main-ci.json)。这确认此前缺 examples 的构建故障已修复；新增 111 项回归计数属于下面的 PR #35，不混用两个提交的证据。

## 2026-10-11 PR #35 回归与工具提交 CI

[run 38136121328](https://github.com/TheLudlows/Origence/actions/runs/38136121328) 对 head 1c90de2a2676ded0df2baab095d9dadcfce23ad3 成功，合并预览 checkout 85cfafb。fast-check 轻量 lib 30/local 86、Docker build-inputs、Python 48 passed / 1 skipped（49 项）及 evals 9 passed；Linux 默认 lib 32/local 111、身份 8、Clippy/build/HTTP smoke/强退恢复通过。Windows/macOS/完整 container 按 main-only 策略跳过。[精确记录](evidence/2026-10-11-maintenance-regrowth-pr35-ci.json)。最终实测工件与文档追记的 CI 另核对。

## 2026-10-11 多 profile 重复维护回归

在 cc4ee55 生产实现上新增三维度 profile、四个 tenant/workspace scope、两代数据的三轮更新/删除/追加/压实回归；192 行，288 次库查询，维护前独立预期检查与维护后完整 NativeHit 等值检查通过。生产 Rust、依赖和 Cargo.lock 未改。完整公开库 HTTP 实验与复核结果见[本轮报告](../evals/aml/maintenance-scale-v1/README.md)，与确定性库测试分别判断。

Rust 1.98.0：cargo fmt -- --check 通过；cargo clippy --offline --locked --all-targets -j 2 -- -D warnings 通过（8.41 秒）；cargo test --offline --locked --lib -j 2 为 32 passed（1.19 秒）；cargo test --offline --locked --test local -j 2 为 111 passed（137.63 秒）。新增测试单独为 1 passed / 110 filtered（129.83 秒）。Python 3.13.11 设置 AML_TEST_BINARY：python -W error::ResourceWarning -m unittest discover -s tools -p test_aml_*.py 为 49 passed / 0 skipped（4.727 秒）。没有压实中断、断电或生产 SLO 验收。

## 2026-10-11 PR #33 主干 CI 与容器输入修复

main cc4ee554a9cf097e1ddea49a8330f9db39593b6d 的 [run 38122931813](https://github.com/TheLudlows/Origence/actions/runs/38122931813) 中 fast-check、Linux、Windows、macOS 成功，container 失败，不能称主干五项全绿。失败 [job 114421214131](https://github.com/TheLudlows/Origence/actions/runs/38122931813/job/114421214131) 在 Cargo 解析阶段报告缺少 examples/vector_layout_probe.rs：Dockerfile 未 COPY examples，且 .dockerignore 未放行它。

修复两处构建输入，并将既有构建准备拆为 build-inputs stage，以 cargo metadata --no-deps 检查真实 Docker 上下文中的所有声明目标。fast-check 在 PR 阶段执行该轻量 stage，提前捕获这类缺文件；最终镜像仍只复制 origence 二进制。没有改运行时或依赖。完整 release/container smoke 的结果后续单独核对，不以 metadata 检查代替编译验收。

## 2026-10-11 PR #33 实现提交 CI

[run 38122489432](https://github.com/TheLudlows/Origence/actions/runs/38122489432) 对实现 head 59854587de27530c48cf858a13cad261f1d58ec1 成功，实际合并预览 checkout 58bd52f。fast-check：轻量 lib 30/local 86，Python 工具 43 passed / 1 skipped（共 44），既有 evals 9 passed。Linux 默认 lib 32/local 110、身份回归 8、严格 Clippy、build、HTTP smoke、处理中强退恢复和关键词评测全部通过。Windows/macOS/container 按 main-only 策略跳过。[精确记录](evidence/2026-10-11-vector-compaction-pr33-ci.json)。本段追记仅改文档，不冒充后续文档提交或主干合并的 CI。

## 2026-10-11 向量碎片定位与显式停机维护

基于 main 6a609d3。[报告](../evals/aml/vector-perf-v1/README.md)、[工件清单](../evals/aml/vector-perf-v1/manifest.json)。只新增聚合后端计时、显式离线压实库 API / CLI，以及维护锁非重入保护；精确检索、权限、排序、原文、parser、Worker 和现有取消/隔离保证不变。新增命令逐表提交、保留旧版本，无跨表原子性、硬超时或硬内存保证；[运维边界](OPERATIONS.md#显式停机压实向量文件)。未执行用户业务库维护。

- Rust 1.98.0：cargo fmt -- --check 通过；cargo clippy --offline --locked --all-targets -j 2 -- -D warnings 通过（11.92 秒）；cargo test --offline --locked --lib -j 2 为 32 passed（1.17 秒）；cargo test --offline --locked --test local -j 2 为 110 passed（27.07 秒）。覆盖跨连接更新/删除可见性、同/跨进程维护互斥、dry-run、重复维护、旧版本可读、全表预检查拒绝未知表。未带 --offline 的 CLI 正确拒绝。
- Python 3.13.11，AML_TEST_BINARY 已设置：python -W error::ResourceWarning -m unittest discover -s tools -p test_aml_*.py 为 44 passed / 0 skipped（4.754 秒）。
- 列投影候选：同库基线/候选/基线复测共 24 次真实 Search，结果全等；HTTP p50 2203/2230/2215 ms，无收益，撤回候选并保存两份源码快照。
- 原生布局对照：1075 行、559 → 1 碎片，Compact 372.97 ms；压实前/后/重开共 24 次查询全部 ID/source/score/顺序一致，约 2.02–2.09 秒降到 0.41–0.44 秒。这是合成向量存储测量。
- 最终真实 CLI / HTTP 对照：本地 BGE-M3，公开 46 批/502 消息加 512 外部批次，基线/dry-run/压实/重开各 8 次，共 32 次。CLI 活跃宿主拒绝通过；停机压实 2272.10 ms，558 → 1 碎片、1087 行不变；HTTP p50 2247.79 / 2215.76 / 644.13 / 623.94 ms，p95 2348.88 / 3985.38 / 1848.29 / 1779.17 ms。nearest-rank，包含首请求；完整响应 data、顺序、字节和 scope 校验通过。三个实验临时库均清理，无聊天调用。
- 限制：单个已观察历史、重复查询、debug 构建，未重跑完整 32 用户库；无维护期间强退/断电/磁盘不足、多 profile 压力或生产 SLO 验收。不能把局部改善当全库成绩或质量提升。
- 上一 main 6a609d3 的 [run 38117103321](https://github.com/TheLudlows/Origence/actions/runs/38117103321) 已成功；不是本次实现提交的 CI，新 PR 验证另记。

## 2026-10-11 时间与裁判消融、Search 分段定位

基于 main 737e209；[报告](../evals/aml/temporal-v1/README.md)、[冻结](../evals/aml/temporal-v1/freeze.json)、[输入/引用审计](../evals/aml/temporal-v1/audit.json)。10 道已观察时间题三臂与 12 个支持校准反例双臂，共 54 次调用 / 53,719 个供应商报告 token，无执行错误、无重试。新日期旁注不改变证据字节；新支持分类仍存在错误放行。旧 supported 字段与新支持分类器分别为 8/12、11/12，与旧 correct AND supported 联合判断不可混为同一指标；旧联合判断这批负例为 0 错误放行。所有新结论只作诊断，不改历史分数或宣称质量验收。

Rust 仅新增不含内容/ID 的聚合 debug 计时。真实 BGE-M3、最大已观察历史 46 批/502 消息先后在两个隔离宿主测量：单历史 HTTP 611–760 ms；追加其他四个用户 512 批单消息后 2143–2312 ms，额外成本集中在向量阶段。16 次 Search 原文/范围和宿主内重复 ID 均通过，两份临时库均清理。尚未复现完整 32 用户库或区分向量后端内部开销；不是优化或 SLO 验收。精确二进制/脚本哈希及首版执行器快照在报告目录。

- Rust 1.98.0：cargo fmt -- --check 通过；cargo clippy --offline --locked --all-targets -j 2 -- -D warnings 通过（9.20 秒）；cargo test --offline --locked --lib -j 2 为 31 passed（1.25 秒）；cargo test --offline --locked --test local aml_ -j 2 为 10 passed / 98 filtered（17.54 秒）。构建完成 35.05 秒。
- Python 3.13.11，AML_TEST_BINARY 已设置：python -W error::ResourceWarning -m unittest discover -s tools -p test_aml_*.py 为 44 passed / 0 skipped（4.607 秒），新增毫秒精度、负 epoch、闰日、缺失/非法时间、负日期差、UTF-8 原文不变与无效裁判输出检查。
- python tools/aml_temporal_verify.py 离线重建 30 份输入与引用、核验 24 份校准字段及冻结哈希，passed；54 次调用输入合计 191,501 UTF-8 字节。
- 前一轮 main 737e209 的 [run 38114714602](https://github.com/TheLudlows/Origence/actions/runs/38114714602) 五个作业全部成功，[记录](../evals/aml/temporal-v1/previous-main-ci.json)；它不是本轮改动的 CI。新 PR CI 在提交后核对。

## 2026-10-11 同预算实验完成与方案决定

[完整报告](../evals/aml/budget-v1/README.md)已完成既定四步。v4 32 题和公开 32 题的 16 份阶段工件原文、范围、排序、预算、引用及关联哈希均复验通过；共 320 次聊天调用 / 477,289 个供应商报告 token，无执行错误，无自动重试，两个临时宿主目录均清理。公开 1,524 批/15,685 条消息的 Add/Search 阶段耗时 30.45 分钟，Search p50/p95 5143.35/5289.27 ms；不是生产容量或 SLO 验收。

公开自动受支持成功 vector/chat/BGE 22/32、22/32、21/32；可答成功 19/29、19/29、18/29，3/3 无答案均拒答。BGE−chat 的 10 分组区间为 −16 到 +25 个百分点。v4 自动原始计数保留，但题意/标签及裁判歧义使安全门槛无法验收。审计记录同模型裁判错误拦截、错误放行及参考日期冲突，不事后改题、剔除失败或重算分数。因此三种实验组件均不新增接入生产流程。

新增聚类统计回归后，Python 3.13.11 `python -W error::ResourceWarning -m unittest discover -s tools -p test_aml_*.py`：41 passed / 0 skipped，4.752 秒（AML_TEST_BINARY 已设置）。`tools/aml_budget_verify.py` 在项目 GPU venv 下对两组各 32 题/8 工件检查全部通过；验证本身没有模型调用。Rust 实现自上节已通过的 head 2044d5a 后未改动，不重复测试来替代实际证据。最终 PR CI 另核对，不把较早 CI 冒充最终提交验收。

## 2026-10-11 PR #31 实现提交 CI

[run 38112578905](https://github.com/TheLudlows/Origence/actions/runs/38112578905) 对 head `2044d5a6751f095595b1bd01fc5c930a268e364d` 成功，实际合并预览 checkout `63111bd`。fast-check 轻量 lib 30/local 86，Python 工具 39 passed/1 skipped（共 40），既有 evals 9 passed；Linux 默认 lib 31/local 108、身份回归 8、Clippy/build/HTTP smoke/强退恢复成功。Windows/macOS/container 按 main-only 策略跳过。[精确记录](evidence/2026-10-11-aml-pr31-ci.json)。此前 main `072ad2c` 的 [run 38110941638](https://github.com/TheLudlows/Origence/actions/runs/38110941638) 五个作业均已成功；不把它当作本轮新改动的三平台验证。

## 2026-10-11 AML 长消息来源与同预算评测

基于 main `072ad2c`。Search 只增加已有 locator 的 parser/source_path/byte_basis/byte_start/end/message_count 标注；原文、分块算法、稳定 ID、解析器版本、Worker、超时/取消/隔离保证不变。新严格评测解码器检查原消息 UTF-8 半开区间、用户 scope 和元数据；片段命中不当作完整消息覆盖。

- Rust 1.98.0：`cargo fmt --all -- --check` 通过；`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过，8.27 秒；`cargo test --offline --locked --lib -j 2` 为 31 passed，1.18 秒；`cargo test --offline --locked --test local aml_ -j 2` 为 10 passed / 98 filtered，17.71 秒。真实 HTTP Add/Search 回归覆盖跨块中文、emoji、组合字符及范围连续性。
- Python 3.13.11 设置 AML_TEST_BINARY：`python -W error::ResourceWarning -m unittest discover -s tools -p test_aml_*.py` 为 40 passed / 0 skipped，7.583 秒，含实际强退恢复。新增范围篡改、UTF-8 内部偏移、缺中间块、用户隔离、前缀预算、公开标签隔离和共享历史分组回归。初次测试夹具仅有两块，无法构造“缺中间块”；扩大夹具到三块以上后通过，不影响实现/数据。
- 受测宿主已 build，二进制与执行器/输入哈希写入[执行冻结](../evals/aml/budget-v1/freeze.json)。固定 32 道 v4 及 32 道公开题，不根据结果改变题目、预算或提示词；运行结果见[本轮报告](../evals/aml/budget-v1/README.md)。本节写入时评测仍在执行，暂无最终质量或新 CI 结论。

## 2026-10-11 PR #30 远端 CI

[run 38110406726](https://github.com/TheLudlows/Origence/actions/runs/38110406726) 对实现 head `c7bc20192bd7d4b8839ba6954f57688376f80aa9` 成功，实际 checkout 为合并预览 `6007c93660b599973b440042f91ac3df4ad7e7a5`。fast-check 轻量 lib 30/local 86、fmt/Clippy、Python 工具 33 passed/1 skipped、既有 evals 9 passed；Linux 默认 lib 31/local 108、身份回归 8、Clippy/build/HTTP smoke/处理中强退恢复通过。Windows/macOS/container 按 main-only 策略跳过，不能称三平台主干已验收。[精确记录](evidence/2026-10-11-aml-pr30-ci.json)。

本次追记仅增加 CI 文档和[公开数据准备清单](../evals/aml/public/README.md)：固定发布版文件已校验，32 道 test-only ID 预选，未运行模型或修改受测实现。

## 2026-10-11 AML 失败归因、正确证据与本地专用重排

基于 main `de17218125cccd83e6cce3b5c39340fc5ae3bd52`。新增离线诊断、oracle、cross-encoder 和其 Answer 对照工具；共享 SHA-256 改为等价的 1 MiB 分块读取以兼容现有 GPU Python 3.10。正式 Rust Search/Add、依赖、Worker、超时/取消/隔离保证未改变。[完整报告](../evals/aml/diagnosis/README.md)、[命令及工件哈希](../evals/aml/diagnosis/manifest.json)。

- 离线核对 35 题、三个检索/回答策略；原文、用户、引用、输入关联哈希与重算指标一致。只重排 3 个可答失败含 2 个 top-40 裁剪、1 个 top-100 缺失。首次压缩归档关联检查失败保留，修复验证压缩前字节，不改历史报告。
- 正确证据诊断 28 次聊天调用 / 8735 token，旧代理为 12/12 和 16/16；7 道无答案排除。逐题复核发现严格小于被弱化等边界问题，不能宣称语义全对或可靠拒答。
- 固定官方 BGE-reranker-v2-m3 文件、大小与 SHA-256，RTX 5060 / float16 / batch 8 完成 35 题、3500 对。相同 40 候选 Recall@5 为 75.00%/83.33%，低于历史聊天重排的 91.67%/95.83%。100 对推理 p50 298.18/299.05 ms，p95 887.32/303.75 ms；开发含首次推理，模型加载另计 3319.31 ms。GPU allocated 峰值 1164659200 bytes，不是总进程或端到端指标。
- 追加固定 Answer 比较 35 次 / 21420 token：专用重排可答代理 8/12、12/16，无答案拒答 3/3、4/4；仍低于聊天重排可答成绩，暂不替换。本轮共 63 次聊天调用、30155 个供应商报告 token；这是分开的两个受限运行，均未超过单运行 60 次上限，无自动重试。
- 保留 Python 3.10 哈希 API 启动失败、继承 SciPy 二进制不兼容导入失败及修复说明。依赖修复仅在 target 下的项目 venv；模型/缓存不进入 Git。原 helper 快照保留 Oracle 受测代码，模型/提示词/标签不因环境修复调整。
- [v4 成对数据](../evals/aml/v4/README.md)32 题 / 16 对 / 188 短消息冻结，尚未运行模型，执行配置仍待冻结；预运行时间戳修正前快照保留。无独立人工复核、真实长历史或公开 LongMemEval 新成绩。
- Rust 1.98.0 `cargo fmt --all -- --check` 通过；`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过，1.27 秒。无 Rust 实现修改，未重复全量 Rust 测试。
- Python 3.13.11 设置 AML_TEST_BINARY 后，`python -W error::ResourceWarning -m unittest discover -s tools -p test_*.py -v`：34 passed / 0 skipped / 0 failed，4.680 秒，含实际宿主处理中强退恢复。Python 3.10 的初版 6 项诊断回归也通过；GPU 实验实际完成。
- `git diff --check` 通过；当时 34 个变更文件的凭据/编码扫描无问题，82 个本地文档链接存在。模型权重已验证被 gitignore 排除；后续新增的本清单/链接再核对。远端 CI 未在此记录中宣称通过。

## 2026-10-11 AML 检索方案调研与决策

基于 main `de17218125cccd83e6cce3b5c39340fc5ae3bd52`，复核既有 Cognee/演进取舍/效果评估文档、当前 AML Search 和 Python 实验实现，并核对官方重排模型、Graphiti、Cognee BEAM 报告、上下文充分性研究及 AML 公开合同。结果见 [阶段决策](AML_RETRIEVAL_DECISION.md)。采用两阶段检索主线，专用/聊天重排待同预算比较；充分性和答案核验单独消融，不改变平台 Answer 边界。

本轮仅新增决策文档并更新 STATUS、索引及本记录；没有改 Rust/Python 实现、依赖、数据或历史实验工件，没有下载新模型或新增模型调用。`git diff --check` 通过；决策/STATUS/索引的 34 个本地链接全部存在，三份文件 UTF-8 解码无替代字符且使用 LF，本节新增决策链接也已核对。未运行 fmt、Clippy 或测试：本轮无实现变更，既有验收记录不作为新策略通过的证据。文档中的待执行实验、模型表现、独立复核和生产接入均未标为已完成。

## 2026-10-11 PR #29 远端 CI

[run 38101081592](https://github.com/TheLudlows/Origence/actions/runs/38101081592) 对实现 head `30ec9364b830472b4b70c635facd2477e6bfae64` 通过，实际 checkout 为合并预览 `7b472a3cf56a7c837dd8a397b6571748ac2fd967`。Linux/MSRV 默认 lib 31/local 108、Clippy、build、HTTP smoke、处理中强退恢复和关键词种子评估通过；fast-check 轻量 lib 30/local 86、fmt/Clippy、工具测试 19 passed/1 skipped（恢复另在 Linux 实跑）、既有评估 9 passed。

Windows/macOS/container 按 main-only 策略跳过，不能据此称三平台主干验收完成。精确 job/step/checkout 见 [CI 工件](evidence/2026-10-11-aml-pr29-ci.json)。本记录仅新增文档证据，不改变以上受测实现。

## 2026-10-11 受限重排/查询扩展与引用式 Answer 对照

基于 02edd6c，新增纯 Python 实验工具、单元测试、合成 v3 数据及工件。业务 Rust、CLI/API 行为、依赖和 Cargo.lock 均未改变；独立宿主复用现有演练 Host，退出回收自身进程和临时数据，不新增业务 worker 或重试循环。受测宿主仍为 main 85454bf，SHA-256 见报告。

- [开发和新主题完整结果](../evals/aml/experiments/results/README.md)保留五个成功运行的逐题原始响应、用量、模型配置、失败分析；[运行清单](../evals/aml/experiments/results/manifest.json)记录精确命令、硬件和模型 digest。检索开发集 15 题/45 次聊天调用（120770 token）；新主题 20 题/60 次（157467 token）。真实本地 BGE-M3 1024 维，聊天 qwen3.8-flash，显式关闭额外推理、JSON 模式、温度 0。
- 开发集 vector/只重排/扩展重排 Recall@5 为 79.17%/91.67%/95.83%，完整证据覆盖 66.67%/83.33%/91.67%。新主题为 63.54%/95.83%/97.92%，完整覆盖 56.25%/93.75%/93.75%。扩展加 RRF 单独使用反而退化，未推广。新主题只重排和扩展重排检索 p95 为 1554.85/3913.75 ms；均校验原文、metadata、用户范围，独立临时树清理通过。
- Answer 代理的可答题“参考短语匹配且引用全部必要来源”：开发集 vector/只重排/扩展重排 8/12、10/12、10/12，无答案拒答 3/3、2/3、2/3；新主题为 9/16、15/16、15/16，无答案均 4/4。开发双臂 Answer 30 次调用/13851 token，追加只重排 15 次/4863 token，新主题三臂 60 次/25711 token。只重排答题比较是在看到检索结果后追加的探索性分析。
- 初始单题策略失败、两次 60 秒请求失败、两次对象格式失败、新主题两次包装格式中断全部保留在 experiments 下各独立目录。未改测试题、标签或检索提示词来修复中断，仅接受有界索引列表/索引对象/单层已知对象包装；正文若存在必须逐字匹配，始终从原候选返回证据。新主题结果属于冻结策略的修复后重跑，不能宣称一次无故障的未见验收。
- 每运行最多 60 次聊天调用、单请求 40000 UTF-8 字节输入、总输入 1200000 字节、单次输出上限 2048 token、60 秒请求超时，无自动重试；超时不保证供应商硬停止。使用已授权网关且只发送合成内容；模型/host 凭据未写入仓库。额外诊断用量见 [diagnostics](../evals/aml/experiments/diagnostics.json)，无 usage 的失败请求不视为免费，未推算货币费用。
- Rust 1.98.0：`cargo fmt --all -- --check` 通过；`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过（1.30 秒）。无 Rust 实现修改，不重复全量 Rust 测试。
- 设置 `AML_TEST_BINARY=target/debug/origence.exe`，`python -W error::ResourceWarning -m unittest discover -s tools -p test_*.py -v` 最终 20 passed/0 failed，4.420 秒；覆盖查询/模型调用预算边界、索引/原文/对象包装、拒答与引用、冻结 hash，包含既有真实进程恢复。`git diff --check`、变更文档本地链接/凭据扫描和压缩工件核对通过。远端 CI 另记。

决策与限制：仅交付实验工具，暂不接入核心 Search；较便宜的只重排作为下一轮候选，但开发集仍把工单号当作序列号、混淆相似活动温度并降低拒答率。数据均由同一 agent 编写，未独立人工复核；短语代理不验证所有语义、数值边界或附加陈述，不等于官方 Eval。没有生产延迟/SLO、完整长上下文、真实企业语料或注入安全保证。后续优先实体/事实类型区分与保守拒答，服务和域名后置。

## 2026-10-11 AML v2 冻结长历史与近似干扰基线

基于 08b606c，新建合成 v2 数据与冻结配置，评测器新增显式 `--dataset v2` 和按 split 汇总；默认 v1 不变，旧原始工件不改写。Rust 实现与依赖未改变。

- `python tools/aml_quality.py --dataset v2 --binary target/debug/origence.exe --report target/aml-quality-v2-1791679172382.json`：退出码 0，32 批 Add/30 次 Search 完成，512 条短消息、原文 UTF-8 69049 字节。所有命中均通过用户、来源、原文和 metadata 检查，临时树已清理。
- 开发 12 可答/3 无答案，Recall@5 79.17%、全部必要证据覆盖 66.67%；留出 11 可答/4 无答案，对应 83.33%/72.73%。合计 7/7 无答案有候选，两个可答题在 top-100 仍缺必要来源；[结果及失败表](../evals/aml/results/local-v2/README.md)保存精确指标、来源排名、原始响应压缩包和机器/模型清单。没有运行后调参或改标签。
- Rust 1.98.0 `cargo fmt --all -- --check` 通过；`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过（1.11 秒）。Rust 未改，不重复全量代码测试。
- 设置 `AML_TEST_BINARY=target/debug/origence.exe`，`python -W error::ResourceWarning -m unittest discover -s tools -p test_*.py -v`：10 passed/0 failed，4.391 秒。新增冻结语料 hash、开发/留出用户与来源不交叉、标签完整性检查；既有真实宿主恢复与评测失败工件测试通过。
- `git diff --check`、变更文档本地链接/凭据扫描、工件与冻结语料/runner hash 核对通过；未运行本轮远端 CI。

边界：数据由同一 agent 编写，留出仅保证来源/用户分离，没有独立人工复核，存在共同风格与题型偏差；无 streaming、长消息分块、Answer/Eval、正式 AML 成绩或生产容量保证。无答案有候选不等于答错。两版语料不同，不拿分数变化当算法改进实验；后续若根据已观察留出题调整实现，需要新的未见测试集。

## 2026-10-11 AML 本地质量 v1 与 64 用户演练

基于分支 codex/aml-local-first 的 33f4937 新增 Python 评测器、合成语料、测试及文档；Rust 实现、Cargo.lock 未改变，实际宿主来自 main 85454bf，二进制 SHA-256 记录在每份工件。

- `python tools/aml_quality.py --binary target/debug/origence.exe --report target/aml-quality-1791678668955.json`：退出码 0，20 次检索完成。2 用户/16 会话/144 条短消息，16 可答题 Recall@5 和全部必要证据覆盖率为 100%，4/4 无答案返回候选；每个响应的用户范围、来源序号、role/timestamp 和完整原文核对通过。临时树已清理。原始响应、哈希、命令、模型 digest 和硬件见 [v1 结果](../evals/aml/results/local-v1/README.md)。
- `python tools/aml_drill.py --binary target/debug/origence.exe --users 64 --concurrency 8 --rounds 5 --report target/aml-capacity-1791678724785.json`：退出码 0；128 Add/320 Search，全部 7 项协议/隔离/幂等/恢复检查通过。Add p50/p95 1278.21/1714.81 ms、Search 1386.41/1613.23 ms；重启 1950.81 ms、停机整库复制恢复 1936.86 ms。临时树含备份清理成功。[原始演练工件](evidence/2026-10-11-aml-bge-m3-64-users.json)。
- Rust 1.98.0：`cargo fmt --all -- --check` 通过；`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过（3.95 秒）。没有 Rust 实现修改，不重复全量 Rust 测试。
- 设置 `AML_TEST_BINARY=target/debug/origence.exe`，`python -W error::ResourceWarning -m unittest discover -s tools -p test_*.py -v`：9 passed、0 failed，4.325 秒。覆盖多跳部分命中/重复候选、无答案分母隔离、跨用户/原文破坏拒绝、失败报告脱敏/保留部分结果/禁止覆盖，以及既有真实进程恢复。既有 CI 通配发现新测试，无工作流变更。
- `python -m unittest discover -s evals -p test_*.py -v`：7 passed/2 skipped/0 failed，0.016 秒；两项 POSIX shebang fixture 在 Windows 跳过。文档本地链接、凭据扫描、工件哈希核对和 `git diff --check` 通过。未运行本轮远端 CI。

局限：语料为自行编写且未独立人工复核的开发集，两个用户共享 10 个题型模板，只有短消息和重复日常干扰；无留出集、完整长上下文、Answer/Eval 或官方 AML 得分。无答案非空只说明召回候选，不能推断回答错误。并发演练不是长期容量/SLO/内存或断电验收；不同运行的并发、规模与缓存状态不同，不把延迟差异归因为单一瓶颈。已有历史工件未改写。

## 2026-10-11 本地优先 AML 演练

基于已合入 main 的 PR #28（`85454bf69e673af4411b2bdd7874942aabf92682`），本轮仅更新文档与新增证据，未修改实现或依赖。

- 本地 Ollama `bge-m3`、1024 维；执行 `python tools/aml_drill.py --binary target/debug/origence.exe --users 16 --concurrency 4 --rounds 5 --report target/aml-local-first-1791677443798.json`，退出码 0。模型环境设置见 [本地运行步骤](AML_DRILL.md)。
- 16 用户、32 Add、80 Search；用户隔离、跨 session、未知用户、稳定证据 ID、幂等、发布后重启和停止宿主后整库复制恢复均通过，临时树已清理。Add p50/p95 为 592.85/1922.98 ms，Search p50/p95 为 357.38/491.22 ms；重启/恢复验证为 1711.34/1709.71 ms。计数和延迟不包含额外预检/重放验证调用。
- [原始报告](evidence/2026-10-11-aml-bge-m3-local-first.json)记录二进制 SHA-256、模型配置及边界。该合成协议演练不产生语义排序分数，不等于官方 Smoke、长期负载或生产 SLO。
- 核对[官方公开仓库 README](https://github.com/AML-memory/agent-memory-leaderboard/blob/1b8142bfe0f20f1c5218d6b554aa0012de34e504/README.md)及该提交文件树：正式 Smoke 由 AML 平台编排，需平台 AML Key 与公网可访问的 Add/Search；公开代码未包含完整离线 Smoke runner、评测数据与标签。官网/API guide 本次请求返回 HTTP 522，未宣称已复核其最新全文。本地模拟继续进行，服务器/域名后置。
- 文档本地链接、凭据扫描和 `git diff --check` 通过。本轮无代码变更，不重跑 Rust fmt/Clippy/测试，沿用下列历史验收；未运行新远端 CI。

## 2026-10-11 PR #28 CI 与依赖审计

[run 38096774752](https://github.com/TheLudlows/Origence/actions/runs/38096774752) 的两个实际作业通过：
head `035aa12f83fd46665636d436caf0a7e0b9a98aa3`，实际 checkout 为合并预览
`f7d72fc673df9e604eb7f7db510e2ee497964f38`。Linux/MSRV 默认 lib 31/local 108、
Clippy、build、HTTP smoke、处理中进程强退恢复和关键词种子评估通过；fast-check
轻量 lib 30/local 86、fmt/Clippy、Python 客户端 4 passed/1 skipped（恢复用例在 Linux
另跑）、既有 Python 评估 9 passed。Windows/macOS/container 按 main-only 策略跳过，
不能据此宣称新版本三平台 main 验收。精确 job/checkout 证据见 [CI 工件](evidence/2026-10-11-aml-pr28-ci.json)。

RustSec cargo-audit 0.22.2 审计 719 个锁文件依赖，数据库提交
`7eebec69c352c7191b1f13eb95dd510eeca5d1de`；退出码 1，2 vulnerability、
3 unmaintained、2 unsound，不能标为通过。没有 ignore，默认撤回版本检查未返回
告警。原始报告、实际 feature/依赖路径与后续处置见 [DEPENDENCY_AUDIT](DEPENDENCY_AUDIT.md)。
本轮没有升级依赖；当前 stdio 不使用 rmcp 公告涉及的 HTTP 传输，rsa 未在当前
激活依赖树中，lru 上游缓存的具体调用已检查但不作为安全证明。

本段及附带证据仅文档更新，不改变以上受测代码；历史记录不改写。

## 2026-10-11 AML 处理中恢复与整库备份（后续切片）

继续在 `7b8d7719117f0b8c6ada4acdb5493ac64141278f` 上仅修改演练工具、Python 测试、CI 和文档。

- 设置 `AML_TEST_BINARY=target/debug/origence.exe` 后执行 `python -W error::ResourceWarning -m unittest discover -s tools -p 'test_*.py' -v`：5 passed，0 failed，4.384 秒，无 ResourceWarning。
- 固定模型请求阻塞时强退真实宿主：确认原 job 为 processing、版本数为 0；重启并重试同一 Add 后，receipt/job ID 不变、仅 1 个 completed job、1 个版本和 2 个完整消息 chunk，重复请求及稳定证据 ID 验证通过。
- [真实 BGE-M3 备份恢复演练](evidence/2026-10-11-aml-bge-m3-backup-drill.json)：8 用户、16 Add、24 Search，停止宿主后复制完整数据树，恢复到独立副本后原 receipt 与证据 ID 保持不变。重启验证 1727.83 ms、副本恢复验证 1741.41 ms；这两个计时都包括启动、重放和检索。数据树及测试备份均清理，target/aml-drills 为空。
- 初版测试的 Python sqlite3 context manager 只退出事务而未关闭连接，造成 Windows 临时目录清理失败；使用 contextlib.closing 显式关闭后通过。另关闭 urllib HTTPError 响应句柄，消除资源警告。失败残留仅为该测试自建合成目录，核对绝对路径后已清理。
- 没有变更 Rust 实现或依赖；沿用下面完整 Rust 验收，不重复运行无关编译。`git diff --check` 与改动文档本地链接/凭据扫描通过。

Linux CI 新增这一真实进程恢复用例；无 binary 的 fast-check 跳过它。仍未测试原生存储写入中间的进程死亡、断电、长期满负载、生产快照或供应商保留。备份采用宿主进程已退出后的完整文件树，不能推广为在线跨库快照保证。历史演练工件保持不变。

## 2026-10-11 AML 检索分支与运行演练

基于 main `9bfc51b0406c0b83ddf0be61e167e980f0e68f79` 的 AML 本地变更，整理到
`codex/aml-readiness`；本段为本地验收，远端 CI 另记。Windows x86_64 MSVC，
声明 Rust/Cargo 1.98.0，离线锁定依赖；Cargo.lock 未变。

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings`：通过，2.46 秒。
- `cargo test --offline --locked -j 2 --no-fail-fast`：lib 31/local 108 通过，0 failed；main/doc-tests 0。编译 35.48 秒，执行 1.16/23.21 秒。
- `cargo test --offline --locked --no-default-features -j 2 --no-fail-fast`：lib 30/local 86 通过，0 failed；doc-tests 0。编译 7.38 秒，执行 1.15/2.25 秒。
- `python -m unittest discover -s tools -p 'test_*.py'`：4 passed，0 failed，覆盖向量形状/数值、HTTPS 与嵌入凭据拒绝、错误脱敏/重定向拒绝和分位数。
- `python -m unittest discover -s evals -p 'test_*.py'`：9 项中 7 passed、2 skipped（未设置可执行文件的既有集成用例），0 failed。
- `git diff --check`、变更文件凭据扫描和文档本地链接检查：通过。

新增公开库 API 回归通过真实 SQLite/LanceDB/SQLite 图验证 AML 派生 scope 的关键词、
向量、摘要、图及 resolve 组合、跨用户重复请求和 key 撤销。模型夹具固定向量；摘要
独有词项的得分变化证明分支参与，图实体/边及来源证据保持用户范围。首轮新增测试
失败于按原大小写匹配规范化图名称，修正为大小写归一后的检查后完整重跑通过。

[固定向量演练](evidence/2026-10-11-aml-fixture-drill.json)覆盖 4 用户、8 Add、8 Search；
[真实本地 BGE-M3 演练](evidence/2026-10-11-aml-bge-m3-drill.json)覆盖 8 用户、16 Add、24 Search，
1024 维，模型 digest 和实际二进制 SHA-256 记录在工件中。后者 Add p50/p95 为
278.23/1708.15 ms，Search p50/p95 为 276.71/444.52 ms；两次演练均通过跨 session、
未知用户、用户隔离、稳定 ID、幂等及发布后强退重启，自己创建的临时树清理后不存在。
报告计数是计时主体，额外预检/验证/重放调用不计入分位数。

用户提供的远端模型网关 `/v1/models` 返回 200，公布 16 个聊天模型，没有公布
embedding 模型；`qwen3.8-flash` 最小聊天请求返回 200、有效 content，usage 为
prompt 65/completion 27/total 92。只使用合成提示，不在仓库保存网关凭据、完整地址或
回复正文。这不证明网关完全不支持 embeddings；仍需可用模型名与维度。
本地 BGE-M3 是既有开发环境，不能替代正式参赛模型或组别确认。

限制：本轮没有语义排序分数、长时间 Full 负载、峰值内存/费用、公网 HTTPS、网关
长连接、发布中强杀、断电、备份恢复或供应商保留验证；临时目录删除不等于物理擦除。
演练工具及专用部署/清理步骤见 [AML_DRILL](AML_DRILL.md)。原生测试需要进程，但库实现
没有新增 subprocess；演练脚本只启动独立待测应用，退出时终止并回收自己创建的宿主。
PowerShell 开发环境初始化脚本受本机策略限制，自动审批拒绝 Bypass；最终直接使用
已安装工具链和缓存完成全部检查，没有修改策略或运行该脚本。

## 2026-10-10 AML Add/Search 最小闭环

继续基于 `9bfc51b0406c0b83ddf0be61e167e980f0e68f79` 的本地工作区，包含上一切片的 scope 变更；未提交、未运行远端 CI。Windows x86_64 MSVC，Rust/Cargo 1.98.0，使用安装在 D:\Rust 的声明工具链、本地依赖缓存与 vendored protoc；Cargo.lock 和依赖未变。

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings`：通过，11.62 秒。
- `cargo test --offline --locked -j 2 --no-fail-fast`：通过；lib 31 / local 107，0 failed；main/doc-tests 0。编译 51.36 秒，lib 执行 1.14 秒、local 25.78 秒。
- `cargo test --offline --locked --no-default-features -j 2 --no-fail-fast`：通过；lib 30 / local 86，0 failed；doc-tests 0。编译 18.86 秒，lib 执行 1.14 秒、local 2.23 秒。
- `git diff --check`：通过；13 个新增/相关文件的 UTF-8、行尾空白及文档本地链接检查通过。
- 首轮 `cargo test --offline --locked -j 2 aml_ -- --nocapture` 为 lib 2 通过、local 6 通过/2 失败：测试错误地预期非法输入为 400，并预期模型失败自动 retry；按既有 422 与显式 job retry 契约修正测试，以上完整回归已覆盖并通过。实现额外收紧 Add 成功检查，要求向量索引对应 committed 账本。

新增 6 项测试（parser 2、AML HTTP/Service 3、scope-only 旧 schema 1）。公开库 API 验证消息边界、多语言/emoji/组合字符、原文重建与消息局部 UTF-8 区间；空/超限/多模态/非法 JSON 整批拒绝。真实 loopback HTTP + 单 Worker + SQLite/LanceDB 验证管理员启用、同步 Add 响应、立即 vector Search、相反事实用户隔离、跨 session 召回、未知用户只读空结果、长 ID、top_k、稳定结果 ID、重复请求及内容冲突、reader/未认证拒绝、脱敏错误与模型不可用不降级。

库层回归验证并发相同 Add 仅一任务、等待超时和丢弃观察 future 不取消持久任务、排队任务重开宿主后复用原 receipt/key 轮换、原文来源/locator、profile 改变及来源撤回不重放虚假成功。模型 fixture 在第二条消息 embedding 失败，整批没有版本或 chunks；显式重试原 job 后完整发布一次。上一 scope-only 数据库仍可映射但不自动安装 Add 表，缺失 Add 功能明确不可用，不兼容表定义拒绝检查。

边界：模型为固定向量的测试服务，不能据此判断语义排序/召回质量、真实参赛配置或吞吐。重启用例覆盖已落库排队任务；未新增 AML 专用的发布中强杀/断电测试，既有保存发布计划恢复回归仍通过。断线用例通过丢弃库观察 future 模拟，未注入真实 TCP 半关闭。未实际等待 25 分钟或测试网关长连接。A2 摘要/图等全部分支专项验收、真实模型验证、部署容量、物理清除和官方 Smoke 未完成。旧库不自动迁移，需专用评测新库。历史证据保持不变。

## 2026-10-10 AML 用户 scope 库接口

基于 `9bfc51b0406c0b83ddf0be61e167e980f0e68f79` 的本地工作区变更；未提交、未运行远端 CI。Windows x86_64 MSVC，声明工具链 Rust/Cargo 1.98.0；使用已安装工具链与本地依赖缓存（Cargo `--offline --locked`），未更改 Cargo.lock 或依赖。

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings`：通过。首轮发现测试中未使用的 Lifecycle 导入，移除后重跑通过。
- `cargo test --offline --locked --no-default-features --test local aml_scope -j 2`：4 passed / 0 failed。
- 上一命令生成的 `target/debug/deps/local-81113c4df62aa8b5.exe` 无过滤运行：85 passed / 0 failed，2.29 秒，覆盖轻量存储、权限、身份、图、PDF 等回归。
- `cargo test --offline --locked -j 2 --no-fail-fast`：通过，lib 29 / local 103（含新增 AML 5 项），0 failed；main/doc-tests 均为 0 项。首次默认功能编译 13 分 08 秒，lib 执行 1.15 秒，local 执行 25.83 秒。
- `git diff --check`：通过；新增/修改文档 UTF-8 与行尾空白检查通过。

新增存储用例验证：显式管理员启用、并发首次创建唯一、Unicode/空格/大小写/运行前缀原样区分、同 tenant 不同 namespace 及跨 tenant 隔离、未知用户不分配、不接受伪造 scope/role、reader 不创建、派生 scope 不扩权、凭据撤销、事务中途失败无孤立 workspace、重开映射不变、旧库不自动升级及异常 schema 拒绝。Service 集成回归另覆盖原文上传 → durable job → keyword search、相反事实及跨会话来源、跨用户文件/资产/job 不可见、密钥轮换和撤销后待发布任务失败，已在本轮默认功能测试通过。

边界：这是 A2 的库接口切片。AML Add/Search HTTP 适配、批次幂等及同步完成、真实模型 vector/hybrid/摘要/图的 AML 全分支端到端验收、部署容量、物理清理和官方 Smoke 尚未完成；不将本地测试视为完整 A2 或参榜验收。历史 S0/S1 证据保持不变。

## 2026-10-10 参榜准备文档

新增 [AML 专项准备计划](AGENT_MEMORY_LEADERBOARD.md)，并补充文档索引及路线图入口。核对当期官方规则与 S1 v2 工件；专项任务仍为待实施。本轮涉及的四份文档经本地文件链接存在性、UTF-8 解码与替换字符、行尾空白检查通过，`git diff --check` 通过。仅修改文档，未重跑 Rust fmt、Clippy、代码测试或远端 CI，不代表 AML 接口、部署或正式评测已验收。

## S1 v2 检索实验

代码 `77d43a859d9c83c93d3a88d4fb0c7928a10d4f16`，36 篇合成来源、300 题（开发 200/留出 100），288 可答/12 无答案。BGE-small-zh-v1.5，六配置共 1,800 次 search、1,728 次 resolve，零请求错误。

| 配置 | Recall@5 | Coverage@2000B | 无答案误召回 |
| --- | ---: | ---: | ---: |
| keyword | 0.35% | 0.35% | 0/12 |
| vector | 97.57% | 98.61% | 12/12 |
| hybrid-all | 94.79% | 95.66% | 12/12 |

摘要相对纯原文 Recall 变化为 0；加图下降 2.78 个百分点。详细开发/留出分数、排名、延迟、usage、费用、逐题审计和模型 hash 见[完整 S1 报告](../evals/s1/results/local-77d43a8-v2/README.md)。新增 200 题尚未独立人工复核；单轮 CPU 延迟不是负载测试，留出集结果不能代表生产质量。CPU 成本未定价。

## 当前 CI 与实现验收

- **SQLite 图替换**：PR #23 已合入 main `9c292a99aec658ba924d5b82c1a86f229ad4d115`。main run [38032343493](https://github.com/TheLudlows/Origence/actions/runs/38032343493) 五个作业全部成功；Linux、Windows、macOS 均为 lib 28/local 89，包含 13 项图回归；Docker Smoke 成功。逐 job 证据见 [主干证据](evidence/2026-10-10-sqlite-graph-main.json)。
- **S1 实验代码 PR #24**：run [38034197676](https://github.com/TheLudlows/Origence/actions/runs/38034197676) 的 Linux/MSRV 与 fast-check 两个实际作业成功，默认 lib 29/local 89、轻量 lib 28/local 72、Python 9、Clippy/fmt/build/HTTP smoke 通过；Windows/macOS/container 按 main-only 策略跳过。该 PR 运行不能单独证明这些平台已验收。精确计数见 [CI 静态证据](evidence/2026-10-10-s1-pr24-ci.json)。
- **S0 身份过滤**：跨平台 main run [37918105081](https://github.com/TheLudlows/Origence/actions/runs/37918105081) 五作业成功，属于 PR #23 合入前代码；只作为 S0 正确性证据。
- **PDF 解析**：`pdf_oxide 0.3.78` 保留页码、UTF-8 来源范围和整份失败策略；取消/隔离边界见 [OPERATIONS](OPERATIONS.md#pdf-解析)。

整理后的本地文档链接检查与 `git diff --check` 通过。Rust 1.98.0 `cargo fmt --all -- --check` 和 `cargo clippy --locked --all-targets -j 2 -- -D warnings` 已在前一轮通过；本轮只改文档，未重跑代码测试或远端 CI。

## 2026-10-11 当前文档精简

仅整理文档入口、STATUS、产品路线图及 AML 计划：区分共享产品底座、AML 专用适配与实验；明确跨 session 检索已实现、反馈学习闭环未实现。移除重复指标和过时进度描述，保留验收条件、任务编号及证据入口；正式规则的旧核对记录显式标注待复核。四份文档按 Unicode 码点计数从 16637 减至 9297（减少 44.1%）。

本地检查：四份文档的 48 个本地链接（含 3 个锚点）通过；仓库 Markdown 指向这四份文档的 23 处引用检查通过；UTF-8 解码、替换字符及行尾空白检查通过，git diff --check 退出码 0。4 个外部链接本轮未联网重验，不宣称更新官方规则。历史验收正文、设计/API/运维契约与 evals/evidence 工件保持不变。仅文档变更，未运行 Rust fmt、Clippy、代码测试或新 CI；本次检查不增加运行时能力或产品验收结论。

## 2026-10-11 设计、操作与研究文档整理

在上一轮基础上整理六份活动文档：平台设计、身份规格、API、OPERATIONS、AML_DRILL、效果评估标准。接口和运维细节集中到 API/OPERATIONS，保留领域不变量、输入限制、取消/关闭、schema 兼容、清理和验收边界；修正旧 Rust 1.88 描述、已删除存储计划链接、身份候选过滤及备份演练旧状态。六份文档按 Unicode 码点从 60301 减至 54774（减少 9.2%）。

文档索引新增研究快读；AML 阶段决策和 Serverless 旧方案仅更新导读，正文逐字保持不变。两份 Cognee 研究、演进取舍 v2、依赖审计及配图说明原文未改；历史验收、evals/evidence 和代码未改。本轮未联网重验研究来源，不将历史结论改称当前外部事实。

检查：本轮九份文档 108 个本地链接（含 39 个锚点）、全仓库 Markdown 的 85 处相关引用通过；九份文件严格 UTF-8 解码、替换字符和行尾空白检查通过；git diff --check 退出码 0。原 VALIDATION 字节前缀完整保留，仅追加本条。仅文档变更，未运行 Rust fmt、Clippy、代码测试或新 CI，不新增运行时/产品效果验收。

## 2026-10-11 本轮实验收尾与最终实验主干 CI

用户确认结束本轮实验；正式 Search 保持原文 vector，重排/时间辅助/核验留实验层，离线压实已交付。PR #33–#35 的最终实验主干为 2b7b6a67300d9e96eca8be0de282d308b8ded576；[run 38138182208](https://github.com/TheLudlows/Origence/actions/runs/38138182208) completed/success，fast-check、Linux、Windows、macOS、container 五项全部成功。只记录 API 返回的作业结果，不新增未读取日志的测试计数；本记录不代表后续文档提交的 CI。

[收尾证据](evidence/2026-10-11-experiment-closeout.json)：同预算 28、时间诊断 12、早期向量性能 13 份报告目录工件及完整库 manifest 全部 25 项，合计 78 项 SHA-256 核对通过。前三份旧 manifest 只核对报告目录内工件，不把旧源码 hash 与当前代码混用。本次无模型调用、无重新评分，不改历史报告、分数或实验工件。

维护故障、磁盘空间、持续容量、冷启动、独立标签复核及生产数据清理移至产品路线图的 R1–R4/Q1，明确启动条件；收尾不豁免正式参榜与生产验收。文档整理一并提交，当前开发入口切回 S2。

收尾文档本地检查：13 份 Markdown 严格 UTF-8、替换字符与行尾空白通过，190 个本地链接（含 47 个锚点）有效，git diff --check 退出码 0；src/tests/tools/evals/Cargo.toml/Cargo.lock 无差异。仅文档与收尾证据变更，不重复本地 Rust fmt/Clippy/代码测试；本次 PR CI 单独核对。


## 2026-10-12 — S4 EvidenceBundle 工程实现与独立 Agent 框架

基线：Improve/Learning [PR #43](https://github.com/TheLudlows/Origence/pull/43)
已合入 main `55b5d8792445058fa0bb6e948a853b73f5c632da`。新增只读上下文、独立路由、
完整可见性复核、条件/冲突/UTF-8 预算回归，以及独立冻结合成数据和有界模型/工具
Agent 框架。具体范围与非目标见 [S4 契约](superpowers/plans/2026-10-12-s4-evidence-bundle.md)。

当前云执行器未安装 Cargo/rustfmt，故本地 Rust fmt/Clippy/测试 **NOT_RUN**；
不得将静态检查或 fixture 当作 Rust 验收。新增关系型查询已用五份权威 fresh-schema
在 Python SQLite 中逐条 EXPLAIN 编译，并通过 integrity_check。最终 Rust 验证以
本 PR 最终 head 的适用 fast-check/Linux CI 和独立审查为准，main CI 不作为等待门槛。

真实 Agent 评测 **NOT_RUN**：当前执行器无可用 torch/transformers 与已验证模型
缓存；未调用付费远程模型、未下载模型、未发送私有数据。模型、数据绑定和所需
授权确定后另行执行。fixture/模拟器只验证框架，不证明真实 Agent 收益、引用
蕴含性或成本优势。S4 工程交付不等于 S4 产品效果完整验收。

S4 Python 框架：`python3 -m unittest discover -s evals -p 'test_*.py' -q` 的
33 项 S4 单测通过（无模型/网络调用）；冻结 10 个新合成任务的四组 fixture 共
40/40 有界交互循环完成。原始轨迹、全部任务分母、未知成本、输入/代码配置哈希
和限制见 [S4 框架记录](../evals/s4/README.md)。原始结果已本地保留，公开结果工件
等待明确发布授权，暂未随本 PR 上传；这些数字仅为框架执行记录，不是产品检索/真实 Agent 质量分数。

工程 PR：[PR #44](https://github.com/TheLudlows/Origence/pull/44)。初始 head
`785c629c` 的最低 Rust 身份测试、完整目标 Clippy 通过；格式检查发现格式差异，
已按 CI rustfmt 精确输出修正。最终新增 race 测试、Python 框架和格式修正仍须
同一最终 head 的完整适用 PR CI 才能合入；不得把初始检查当作最终通过证据。


## 2026-10-12 — SQLite 既有职责拆分（验证范围与执行记录）

基线为 main `22fa1221ea1a1aa67a5d19bed5d6dbb418891168`。仅整理既有实现：
生命周期/schema/进程锁、身份绑定、版本/分块、来源/所有权、账本及作业队列
移至 SQLite 私有模块，DomainTx 保留权限/幂等/提交包络并直接委派同一个事务。
不修改 SQL/schema、数据库格式、来源与可见性条件、重试/取消/错误语义或依赖。
原有显式身份工具仅移动，不扩充升级或迁移范围。历史验证记录保持不变。

本地无 Cargo/Rust 工具链，Rust fmt/Clippy/测试在本地为 NOT_RUN；不能用静态
比对替代编译和运行。静态比对检查原有方法体、SQL 字符串和 bind 顺序保持原样；
新增薄委派仅传递同名参数并 await。PR CI 加入聚焦 SQLite 契约测试，并运行
完整默认/无默认特性测试、两组 Clippy（warnings denied）及 fmt。结果以 PR
最终 head 的 Actions 记录为准；[PR #45](https://github.com/TheLudlows/Origence/pull/45)
保留最终结果与精确运行链接，通过后再评估合并。

执行中发现并修正：`e5445bd` 的 fmt 排版不符；`69b79eb` 的共享 ROLES
常量移动后 app 模块不可见（真实编译错误，恢复原父模块位置）；`2876b1f`
的 fmt、两组 Clippy 及完整无默认特性测试通过，但新增聚焦 CI 命令误把
测试模块当作独立 target，尚未运行聚焦或完整默认测试。聚焦命令已改用
现有唯一 `local` 集成 target 内的 sqlite_/initialization/storage_recovery 过滤，
不更改测试本体。最终接受必须以修正后 head 的整套 CI 通过为准。

本次不进行真实 Agent 质量验收（NOT_RUN）、不上传此前待上传合成诊断工件，
不变更 DEPENDENCY_AUDIT 的保留风险；通用可见性规则收口是后续独立步骤。


## 2026-10-12 — Shared SQLite visibility layers

General SQLite visibility consolidation follows the core split at main
`1a76e293d63cbc27a9d0b714f031b6d35580b846`. Shared fixed-alias SQL now owns
current chunk scope/current-version/deletion/root predicates, local S2 snapshot
support, and active exact-version asset roots. Search and context share the base
current-row predicate; context retains route filtering before locator validation.
Asset reads and archives share the version-root predicate, with explicit history
remaining valid after a newer version is published.

Specialized rules remain separate: exact graph artifact type and source/version
ownership; original S2 member provenance; writer-only raw session reads; iterative
Learning closure/cycle/64-node bounds and the accepted exact publishing-root
exception; cleanup and low-level event reads. No schema, dependency, API,
migration, ranking, or authorization-envelope change is intended.

The editing environment has no Rust toolchain. Local Rust formatting, Clippy and
tests are NOT_RUN here. Independent differential SQLite 3.53.1 review compared 225 locator cases,
50,625 ordered two-chunk combinations and four out-of-scope malformed cases.
The old and shared predicates agreed for malformed JSON, missing/null/scalar/
object supports, inactive/missing/cross-workspace sources and EXISTS early
termination. This is supplementary SQL evidence, not Rust acceptance. Exact-head PR CI must supply
`cargo fmt --all -- --check`, strict default/no-default Clippy, focused contracts
through the existing single `local` test target, and both full suites before merge.
Existing HTTP/AML recovery/evaluation steps remain unchanged.


## 2026-10-12：MCP 依赖风险独立处置（PR #47）

基线 main `7c8531afbfa2612a35d6869668df29f761c1d4a6`；实现验证提交
`8e1c6cb5e06cb7a211b07b9223af69a46308328f`。
[CI 38219833473](https://github.com/TheLudlows/Origence/actions/runs/38219833473)
的 fast-check 与 linux-native 全部成功；Windows 按现有 main-only 策略跳过，macOS/容器
不在当前自动 CI 范围。最终 PR head 的状态须另看 [PR #47](https://github.com/TheLudlows/Origence/pull/47)，
不把此实现提交的结果冒充后续提交或主干结果。

- `cargo fmt --all -- --check`、默认与 `--no-default-features` 的 all-target Clippy
  (`-D warnings`)、两种配置测试通过；默认配置为 68 个 library、179 个 integration tests。
- 原有 SQLite/可见性聚焦回归、HTTP smoke、受控模型桩进程恢复及关键词评测通过。
- 新增 `python3 tools/mcp_smoke.py --binary target/debug/origence` 已在 Linux 真正启动二进制
  stdio 验证，不是仅调用 Rust 方法或模拟 SDK 客户端。使用临时新库、临时 key，关闭模型，
  清除继承的 OC/proxy 配置；不接触用户数据或真实 provider。
- 测试覆盖本地 offline 与 HostClient 转发：initialize/initialized/ping，六种协议协商，
  tools/list 恰好四个工具及完整输入字段/required/default/enum/nullable 结构，无 outputSchema；
  每个工具的实际成功调用、UTF-8 预算/引用、当前和历史版本、无命中与错误后继续调用。
- 错误回归覆盖 JSON-RPC `-32602` 缺参/类型/未知字段/未知工具及应用领域安全消息；
  SDK/serde 参数错误可能反射调用方提交值，保持其既有协议不等于全面诊断脱敏。
- 权限回归包括两种模式的无效/已撤销 token 启动拒绝、reader 的 session route 拒绝，
  host 模式已连接 key 撤销后四个工具全部拒绝；offline 遵守排他数据库锁，不用原始 SQL
  绕锁模拟撤销。EOF 正常退出和重开证明 stdio 关闭后释放锁；强制 kill 不算成功。
- 子进程仅用于黑盒真实 stdio/CLI 边界验证，不能由库内调用替代；生产实现未引入子进程。
  等待/退出有界且失败路径清理子进程；测试日志临时文件不是运行时硬输出配额。
- 最终修改仅 rmcp 精确版本和必要宏依赖、显式 features、ServerInfo builder、stored-router
  与旧参数错误兼容适配。四个工具业务实现、权限、HTTP host 转发、schema/存储职责不变；
  输入 JSON Schema 方言及已知版本协商差异见 [依赖审计](DEPENDENCY_AUDIT.md)。
- 独立源码审查复核了直接路由适配、启用状态守卫、macro 不覆盖显式方法与测试边界，
  未发现阻断项。编辑环境没有 Cargo；所有 Rust 执行结果来自上述 CI，不声称本地执行。
- fresh `cargo audit --json` 实际退出 **1**：rmcp 公告已移除，保留 rsa vulnerability、
  两条 lru unsound、三条 unmaintained 和 yoke-derive yanked；无 ignore。
  [完整锁定证据](evidence/2026-10-12-mcp-dependency-audit.json) 与提交 Cargo.lock 的
  SHA-256 相同。临时只读 CI 生成锁/采集证据，完成后从最终变更删除；永久 CI 只新增 stdio 回归。

该处置不代表 clean audit、生产安全验收、真实 Agent 质量提升或旧库升级/迁移验收。


## 2026-10-12：AML 通用基础能力 P0–P2（实施中）

基线 `edd058e646d2fd66ab60f0e0262559e5b3fcba05`；[PR #50](https://github.com/TheLudlows/Origence/pull/50) 当前为 draft。新库 DDL、owned workspace/key/command 契约和通用消息批次均须在同一最终 head 的 CI 与独立审查后才可认定完成。当前旧 AML 路径仅用于阶段过渡，最终 P3 移除。

- 云端编辑环境无 Rust/Cargo；不声称本地 Rust tests、Clippy 或 rustfmt 通过。
- [工具 run 38229211031](https://github.com/TheLudlows/Origence/actions/runs/38229211031) 在 `8617a2a57f627245f089cdc32deca1b43abff721` 使用 Cargo 1.98 工具链解析 `uuid/v5`。Cargo.lock SHA-256 为 `f83306ef2cd9f4f0f3068615e9dcdb520450441d57b528528f92a8eeacf003cf`；变化仅增加 sha1_smol 1.0.1 与 uuid 对它的依赖，保留 rmcp 1.8.0。下载产物 ZIP SHA-256 `e30eef51ffc5186e30ace7b2f6f2665657515fdf030be323064543abaae4a318` 与 Actions 元数据一致。
- 锁文件生成/格式工具不是功能通过证据；临时 workflow 在最终验收前移除。初始普通 CI 因尚未提交更新的锁文件而失败属于过渡结果，不能用于验收。
- Python 独立 UUIDv5 固定字节/ID 向量测试 1 passed；本地工具单测 50 tests，49 passed、1 skipped（缺少本地二进制的受控恢复测试）；评估适配单测 42 passed。最初因工作副本未包含冻结数据而报文件缺失，补齐同基线的只读仓库 fixture 后重跑通过；没有导入评测服务、运行真实模型或改写历史报告。
- 首个实现 head `1c5fc23279d5213e0edb5d8db7e6d599b189d81d` 的 [CI 38230253089](https://github.com/TheLudlows/Origence/actions/runs/38230253089) 为 **失败的中间验证**：Linux Rust 1.98.0 确认、all-target Clippy（warnings denied）与全部聚焦回归通过；默认 library 74 passed，integration 199 passed / 1 failed。fast-check 因 rustfmt 失败而未执行其余步骤。HTTP/MCP/进程恢复/关键词评估未运行，Windows 按 PR 策略跳过。
- 失败为新增 prepared-publication 重启测试把 retry_wait 强制设为 processing，却保留了未来 next_retry_at，导致重开后没有可 claim 的任务。测试现改为明确已到期的中断任务（next_retry_at=0），不修改生产重试/恢复策略。旧 head 的其他通过项不替代这项修正后的精确 head 复验。
- 独立审查发现状态轮询复制/反序列化大 publication 的问题。改为 SQLite 元数据投影和一次 materialized chunk-ID CTE；200×4096 维合成 SQL 复现中旧相关扫描约 5.4 秒，修正完整查询约 0.074 秒。SQL-only 复核了全部可见、单索引 generation 失配及来源撤回的计数，不冒充 Rust/native 验收。队列 JSON 和消息恢复解析移到事务外 spawn_blocking，避免复制不用的 publication；仍为原单 Worker。
- 修正后实现 head `857b4fcfd61ac758eeb23693a37b8c1998972538` 的 [CI 38232658239](https://github.com/TheLudlows/Origence/actions/runs/38232658239) fast-check/Linux 全部通过：无默认 features library 68、integration 101；默认 library 74、integration 202。format、两种严格 Clippy、聚焦 suites、HTTP、真实 stdio MCP、受控模型桩进程恢复、关键词合成评估全通过；Windows 仍按 PR 策略跳过。临时工具 workflow 已移除。
- 最终审查另发现新增 creator retry 检查不应阻止现有特权 cleanup 回收：原 creator 撤销后，当前 admin 必须仍可重试 failed cleanup，调用方 Delete 权限、不可取消和 original created_by 保持。该保留既有行为的修正与真实删除/撤销回归须在后续 head 再验；上述通过结果不能代替它。
- 最终精确 head CI 与合入 main 检查结果待补；真实模型/官方评测 NOT_RUN。

## 2026-10-12：P1/P2 最终验收与 AML API 切换 P3/P4

P1/P2 最终 head `95917b89811a259ccfa944cb21422344c8472992` 的 [CI 38233936996](https://github.com/TheLudlows/Origence/actions/runs/38233936996) completed/success；fast-check、linux-native 全通过，Windows 按 PR 策略跳过。无默认 features library 68、integration 101；默认 library 74、integration 203。Rust 1.98.0、fmt、两种严格 all-target Clippy、聚焦可见性、HTTP、真实 stdio MCP、受控模型桩进程恢复、合成关键词评估均通过。Python tools 50 run / 49 passed / 1 skipped（该 job 无二进制；Linux 独立恢复步骤通过），evals 42 passed。cleanup 原 creator 撤销后当前 admin 重试回归已实际执行。独立审查核对最终代码无阻断项。

[PR #50](https://github.com/TheLudlows/Origence/pull/50) 已 squash 合入 main `8e39c873ba5bbdefc3df1789d53c94e10ead7891`；Git tree 与验收头相同。主干 fast-check/Linux/Windows 结果另行跟踪，不能把 PR 的 Windows skipped 当作通过。

P3 一次切换 API 并移除旧专用路径，当前代码仍待其最终 head 的完整 CI；P1/P2 的通过结果不替代 P3 验收。当前云端仍无 Rust/Cargo，Rust、native、真实宿主/CLI 测试的本地执行均 NOT_RUN。

- 本地 Python tools：56 run，54 passed、2 skipped（受控进程强退恢复与新增停服整库备份恢复，缺少 AML_TEST_BINARY）；evals 42 passed。包含下层边界、当前 parser 严格解码、多批次 session 不歧义及显式历史 decoder 回归；冻结 eval/report 数据未改写。
- 独立源码审查核对 default-off 路由、认证 root 派生/能力、只读未知 Search、删除下层 AML 表/任务/DTO、旧 schema 无写入拒绝、历史工具显式解码；无剩余阻断项。它不是 Rust/native 执行证据或 GitHub maintainer approval。
- 补强 T13：真实 LanceDB 临时表路径故障，要求持久 retry_wait、无部分可见版本/分块，修复测试自己的阻塞文件后复用原同 generation 的计划/证据 ID，不重复 embedding。生产代码没有新增故障开关或 Worker。
- 补强 T14：同一个受控 loopback 模型、新建临时库发布多语种多块批次，确认进程已退出后复制完整数据树，恢复到另一个新目录；先检索再重放，核对原回执/job/creator/source/asset/version/chunk/locator，不修改原目录/备份。Host.stop 使用已记录的进程 kill，不宣称优雅退出、在线快照或断电保证。
- 补强 T15：撤回后真实 HTTP Add 重放返回 409 且原回执及领域 ID 不变。独立审查另发现 ready 索引的 dimension 失配应与 vector Search 一样不可见；现从 materialized saved-chunk CTE 一次投影维度，匹配同 chunk/index 的有效维度，并保持 profile/generation/ledger/数量检查，无 native 每次轮询探测。SQLite-only 2×2 和 200×4096 fixture 通过正常/错维度/恢复/撤回/非法维度，约 0.13–0.23 秒；此项不替代 Rust/native 回归。
- T01–T02 固定跨语言字节/ID；T03 并发 workspace/batch；T04–T07 所属关系、reader/普通 key、跨 root/tenant、递归、撤销/降权/能力关闭与 I/O 后复核；T08–T10 creator 不转移、全部内容/规格冲突、普通命令 principal 隔离；T11–T12 精确批次消息 UTF-8 locator 和非 AML system/tool；T16 既有 history/S2/S3/Session/Learning/Bundle；T17 新库无 AML 表和旧库原字节保留拒绝；T19 同 generation ID 恢复、显式 retry 规格/回执稳定、已发布任务不重写；T20 下层源码/DDL边界均有已注册测试。T18 需本次最终 default/no-default/HTTP/MCP/CLI CI 证明。

真实 provider、真实评测导入、官方 Smoke/Full 与产品质量结论均 **NOT_RUN**。本次验收只覆盖固定合成/受控模型工程行为；不更改历史实验结果、不扩展迁移或生产级验收。最终精确 head、适用 CI 与主干结果在完成后补记。


### P3 实现 head 已执行的验收

实现 head `9c49231704430ea685e6e30661088442e1e62833` 的 [CI 38236518092](https://github.com/TheLudlows/Origence/actions/runs/38236518092) completed/success：fast-check/Linux 全通过，Windows 按 PR 策略 skipped。无默认 features library **69**、integration **99**；默认 library **75**、integration **203**，全部通过，无 Rust 测试跳过。format、双配置 all-target Clippy（warnings denied）、全部既有聚焦 suites、HTTP、真实 stdio MCP、关键词合成评估通过。

`native_vector_write_failure_keeps_batch_invisible_and_recovers_original_plan` 在真实 LanceDB 上执行通过；维度不符 live status、非 AML system/tool、所属权限及新 schema 拒绝测试均包含在同头 integration suite。Linux 的受控真实宿主步骤实际执行 **2/2 passed**：处理中进程强退恢复、停服复制整个库到独立目录后的恢复和多次重放。fast-check tools 的 56 run / 54 passed / 2 skipped 仅表示该 job 未配置 binary，两项没有被省略验收；evals **42 passed**。

独立审查逐个核对远端 head 的变更文件 blob hash 与已审查快照，确认临时 format workflow 删除、Cargo.lock 与历史 eval/evidence 无变化。格式产物来自 Rust1.98 [run 38236327157](https://github.com/TheLudlows/Origence/actions/runs/38236327157)，artifact11741110762 ZIP SHA-256 `39d18931fe28f3a78eb1bc22d82dd44947be2d7cdfaaf52d25fdbb6eedacddc2` 已验证；它只提供格式，不充当功能证据。最初未格式化 head09061e8 的普通 CI 被后续提交取代，不用于验收。

后续最小部署修正：Compose 的显式环境列表现在转发 `OC_ENABLE_AML: ${OC_ENABLE_AML:-false}`，使 `.env` 的显式启用设置能到达宿主；静态边界回归新增该断言，本地 3/3 passed。它不改变默认关闭、安全能力或容器命令，不恢复 Docker/macOS CI，也不声称已运行 Docker。此配置/文档提交与任何后续提交在 ready/merge 前仍必须通过自己精确 head 的适用 CI；最终 SHA、CI 链接和合入结果以 [PR #51](https://github.com/TheLudlows/Origence/pull/51) 的核对记录为准，不借用旧头绿灯。

P1/P2 合入主干 `8e39c873ba5bbdefc3df1789d53c94e10ead7891` 的 [main CI 38235896871](https://github.com/TheLudlows/Origence/actions/runs/38235896871) 已到终态：fast-check、linux-native、windows 全部 success。这只证明该主干提交；P3 合入后的主干（包括 Windows）仍单独跟踪。


### P4 配置与文档复验

含 Compose 透传/静态断言的 head `efcb1ddd92deb7185b6b497f527f5db735014587` 经独立六文件 hash/delta 复核无阻断项，[CI 38238050316](https://github.com/TheLudlows/Origence/actions/runs/38238050316) completed/success，fast-check/Linux 全通过、Windows 按 PR 策略 skipped。Rust 仍为无默认 **69/99**、默认 **75/203**；tools 新增配置断言后 **57 run / 55 passed / 2 skipped**，evals **42 passed**；两个被 fast-check 跳过的真实宿主恢复用例在 Linux 独立步骤 **2/2 passed**，HTTP/MCP/原生故障/聚焦/关键词评估同样全过。Compose 本身只验证配置文本，Docker 运行仍 NOT_RUN。

至此最终实现及部署配置验收完成。最后归档仅修改本验证记录与执行计划勾选，不改变源代码、配置、测试或历史结果；归档提交同样必须通过其精确 head 的适用 CI 后才可 ready/merge。最终归档 head/CI 与合入 main 后的 Windows 等结果，在 [PR #51](https://github.com/TheLudlows/Origence/pull/51) 的核对记录继续保留，避免自引用提交 SHA 或把前一头绿灯用于后一头。


## 2026-10-13 — Turso / LanceDB 正式接入（进行中）

草稿 [PR #58](https://github.com/TheLudlows/Origence/pull/58)，独立于历史 T0
草稿 #53。新后端尚未完成运行验收。

- 准备提交 898e3ef 的全量重新解析锁暴露 office_oxide 0.1.13 与既有
  pdf_oxide 0.3.78 的编译不兼容；同时 Turso 内部组件漂移到 0.8.3。
  此失败保留，不通过更新 PDF 实现或伪造锁绕过。
- 实现提交 99beb45c 的
  [准备 CI](https://github.com/TheLudlows/Origence/actions/runs/38388541517)
  已使用 Cargo 保留既有锁并精确解析：Turso 全套 0.8.2、LanceDB 0.40.0、
  Lance 13.0.0、Arrow 58.3.0、DataFusion 54.1.0、office_oxide 0.1.9。
  生成的真实锁与 rustfmt 补丁作为 artifact 11808495075 归档。
- 同一提交编译 FAIL：新查询适配的泛型异步 Executor 生命周期/Send 证明
  引起 HTTP/MCP 级联错误，正在修复；未跳过 Send、多线程 host 或相关测试。
- 本地 Python tools：57 tests，55 PASS、2 按缺少二进制条件 SKIP；evals：
  42/42 PASS。云编辑环境无 Rust 工具链，这些结果不替代 Rust 编译或集成运行。
- 新增原生驱动、Lance 原生检索及生产发布/回滚/撤回/Context/Summary 回归；
  本段记录时仍为 NOT_RUN。最终验收必须依据修复后的精确 head 重跑。

真实模型/官方 AML 质量评测、生产性能 SLO、在线跨库备份、旧库迁移均未宣称通过。

- 修复提交 d95935da 的
  [准备 CI](https://github.com/TheLudlows/Origence/actions/runs/38390159122)
  cargo check --locked --all-targets PASS（15.32 秒），无 Rust warning；格式补丁
  由 artifact 11809150175 生成并回灌。准备步骤再次解析仅改变若干 Windows
  间接依赖引用，未采纳该二次锁变化；后续准备检查改为只验证已提交锁，不再更新。
  最终精确锁与测试门禁仍看后续提交的正式 CI，不把准备检查等同整体验收。

- d95935da 的正式 [CI 38390159324](https://github.com/TheLudlows/Origence/actions/runs/38390159324)：
  Rust 1.98 最小特性身份测试、全目标 warnings-denied Clippy PASS；首组原生
  核心/图/权限/可见性回归 59 项中 58 PASS、1 FAIL。失败是畸形 JSON 查询后
  Turso 已终止事务，旧用例随后 commit 得到 no transaction is active；不吞掉
  commit 错误，也不将已回滚写入报告为成功。其余运行门禁当次未执行。
  同次 fmt FAIL 的正式补丁已应用。后续 CI 将优先运行新增生产发布和 Lance
  合同，保留全部原回归，并允许 PR 自身缓存失败作业的依赖以避免重复冷编译。

- 804701d9 的 [准备 CI 38393321292](https://github.com/TheLudlows/Origence/actions/runs/38393321292)
  全目标编译再次 PASS。artifact 11811247090 包含 Cargo 真实生成的锁与格式补丁；
  锁 diff 仅为 origence 根依赖新增已锁存的 `lance`，所有依赖 package 记录完全
  不变（准备步骤有逐记录断言，本地亦复核）。新增原生事务中止护栏、最终发布
  proof、物理 IVF 分区校验及其测试均已编译；运行测试仍待回灌锁/格式后的 CI。

- 7413db37 的 [正式 CI 38394036490](https://github.com/TheLudlows/Origence/actions/runs/38394036490)：
  fmt、无默认特性 Clippy 与全功能 warnings-denied Clippy PASS；无默认特性
  78 个 lib 测试全过（含 9 个原生 Turso 驱动测试与真实 Pending/错误读后的
  fail-closed 回归）。无默认特性集成 98 PASS、1 FAIL，为旧 catalog 缺表后
  仍期待重开的用例；已改为拒绝不兼容库且 DDL/原文不变的合同回归，待复跑。
  全功能运行此记录时仍在执行，不能据此宣称生产发布、Context 或 ANN 全过。
  Cargo 生成的 feature tree 确认未启用 Turso FTS；摘要仍走应用层 token 聚合。

- 同一 7413db37 全功能聚焦运行已结束：最终 publication proof 3/3、Turso driver
  9/9、production_storage 7/7、Lance 原生检索 10/10、权威/图 59/59、初始化
  4/4、恢复 2/2、Session feedback 15/15、EvidenceBundle 12/12 PASS。
  semantic_memory、semantic_capture、learning_loop 三组分别在既有大型 HTTP/
  学习回归发生 stack overflow SIGABRT；没有 Rust panic backtrace。完整测试与
  HTTP/MCP/进程恢复步骤因此 NOT_RUN，不把通过子集当作整体通过。
  后续修复缩小公开 search/resolve/evidence_bundle future 的内联状态，并打印
  未构造/未 poll 的 native 与 public future 类型尺寸；原三测试不改、线程栈不
  增大，必须真实重跑判断是否解决，当前不能确定唯一根因。

- 6e0bb01a 的[准备 CI 38397478901](https://github.com/TheLudlows/Origence/actions/runs/38397478901)
  全目标编译 PASS；[正式 CI 38397478898](https://github.com/TheLudlows/Origence/actions/runs/38397478898)
  的 Linux Rust 1.98 最小特性身份测试与全目标 warnings-denied Clippy PASS。
  公开 search/resolve/evidence_bundle future 类型尺寸实测分别为 176/176/256
  字节；Turso acquire/begin/begin_with 为 448/512/496 字节，commit/rollback
  为 424/440 字节。诊断只取类型尺寸，不构造或 poll 被测 future；尺寸结果不是
  完整调用链的运行栈上限，也不单独证明 stack overflow 根因。
- 同一 6e0bb01a 的全功能聚焦运行：最终 publication proof 3/3、Turso driver
  9/9、production_storage 8/8、Lance 原生检索 10/10、权威/图 59/59、初始化
  4/4、恢复 2/2、semantic_memory 6/6、Session feedback 15/15、learning_loop
  20/20、EvidenceBundle 12/12 PASS。新增 vector 表丢失时 readiness/已完成
  receipt 拒绝，以及来源撤回后允许 cleanup 的回归通过；原生事务错误后禁止
  自动提交、真实 Pending 后显式 rollback/Drop 清理与畸形 JSON 可见性回归
  再次通过。既有 semantic_memory 与 learning_loop 爆栈用例在未增大线程栈、
  未修改原测试断言的条件下通过。
- semantic_capture 组已有 7 项明确 PASS，但
  `semantic_capture_http_multilingual_search_isolation_and_updates` 仍发生
  stack overflow / SIGABRT，整个聚焦步骤 FAIL。未把进程中止记为普通断言失败，
  也未将其余七项通过当作该组整体通过。完整默认特性测试、debug build/HTTP
  smoke、MCP smoke、AML 进程恢复与 keyword 种子评估当次均为 NOT_RUN。
- 该头 fast-check 在 fmt 检查失败，其后无默认特性 Clippy/完整测试及 Python
  门禁 NOT_RUN；正式 rustfmt 补丁已从准备工件回灌本地，仍须后续精确提交验证。
  catalog 缺表用例的“不兼容拒绝且 DDL/原文保持”修正已在该头编译，但未被
  本次聚焦筛选选中，完整测试又未执行，因此该修正的运行结果仍为 NOT_RUN。
  本段仅追记已测提交的结果，不将准备检查、格式回灌或通过子集视为最终验收。

- 8a3815f1 的[正式 CI 38400833421](https://github.com/TheLudlows/Origence/actions/runs/38400833421)
  与准备检查已完成：fmt、无默认特性/全目标 warnings-denied Clippy PASS；
  无默认特性 78 lib + 99 integration 全过，包含 catalog 拒绝且不修复数据。
  Python tools 55 PASS / 2 SKIP，evals 42 PASS。原三个 stack overflow 用例在
  原线程栈与原断言下全部通过；semantic_capture / process_next future 分别
  为 96 / 24 字节（inner 2040 / 7936 字节），只调整异步状态的分配边界。
- 同头全部 focused 组 PASS；完整默认特性 lib 93/93 PASS，integration
  217 PASS / 4 FAIL。失败为两个压实 fixture 仍预期所有 fragment 合为一个、
  身份过滤 fixture 仅写权威 chunk 而没有原生 FTS/发布账本、graph 重放仍预期
  只有 graph 一条账本。后续修正必须保持检索来源、版本/隔离、完整身份候选与
  精确账本幂等断言；不能仅放宽失败计数或加入隐式重建。Lance 13 压实保留
  indexed / unindexed fragment 覆盖边界，不将两组误报为未压实。
  该头 HTTP/MCP smoke、受控模型进程恢复与 keyword 评估仍 NOT_RUN。

### 本轮实现结果：b50e8748

[正式 CI 38405514854](https://github.com/TheLudlows/Origence/actions/runs/38405514854)
completed/success；fast-check 与 linux-native 全通过，Windows 按既有 PR 策略
SKIP。fmt、无默认特性及全目标 warnings-denied Clippy、Rust 1.98 身份测试通过。
无默认特性 78 lib + 99 integration；默认特性 93 lib + 221 integration 全通过。
全部 focused native/publication/Context/Summary/semantic/learning 回归再次通过；
前述 4 项 fixture 合同修复与 3 个原 stack overflow 用例均已闭合。

真实二进制 HTTP readiness/auth/publication/identity/keyword smoke PASS；MCP
host-forwarding 与 offline 协商/schema/安全错误/撤回/EOF/reopen PASS；受控模型
夹具下的真实宿主处理中强退/恢复 2/2 PASS。Python tools 55 PASS / 2 SKIP、
eval adapters 42/42 PASS；两个 tools 缺二进制跳过项由 Linux 专用步骤实跑。

Keyword seed 为 24 题（22 可答、2 无答案），请求错误与无答案误命中均 0，
document Recall@K / MRR / nDCG 均 0.63636。该值是本次 keyword-only 合成诊断，
不是“所有问题正确”、ANN 通用召回保证或真实模型产品效果验收。证据工件
[retrieval-linux / 11819827413](https://github.com/TheLudlows/Origence/actions/runs/38405514854/artifacts/11819827413)，
SHA-256 `4c2a3b9e6777ca193ad2413a83a2d56ace12ae591194e59d6df6e9918d00e304`。

本轮保留 fresh-only 范围；同版本重开/幂等/撤回、跨库阶段失败重放、候选前完整
身份预检与返回前权威复核已通过工程回归。真实模型/官方 AML、通用 ANN 性能
SLO、维护中断/磁盘不足、任意断电与在线跨库备份仍不在本次已验范围。

最后归档只更新当前状态、实施/验证记录并移除临时 preparation workflow，不改
运行源代码、依赖锁或测试。归档精确 head 的适用 CI 仍须通过，结果在草稿
[PR #58](https://github.com/TheLudlows/Origence/pull/58) 保留；不自动合入。


## PR #59：无 JOIN 分阶段验证

2026-10-14，基于 main `1d5e0554b38e12df094a894f11a23fb26a117568` 分阶段提交，
保持 PR 待验收，不在中间阶段合入。历史失败不删除、不改记为通过。

### 原故障与首批闭环

Windows 原 `local_app::local_host_api_cli_mcp_models_and_recovery` 曾在宿主终止后
离线 search 失败；调试日志记录 `0xc00000fd` / `-1073741571`，栈含 Turso parser。
该进程中止不是一般断言失败。修复保持 Windows 主线程原 1 MiB 栈和原用例断言，
按应用查询路径拆分关联；没有通过扩大线程栈、跳过用例或替换断言消除故障。
这不是 Turso parser 任意输入/复杂 SQL 的通用修复，也不是完整调用链栈上限证明。

首批 `3969f811c09b854fa5fd322572991cbe8afabf2d` 的
[正式 CI 38504115746](https://github.com/TheLudlows/Origence/actions/runs/38504115746) 与
[Windows 验证 38504115832](https://github.com/TheLudlows/Origence/actions/runs/38504115832)
均成功：Linux/Windows 默认各 107 lib + 222 integration，原离线单例 1 passed；
无默认特性 83 lib + 99 integration。该时点仍有 37 处冻结关联债务，不能称全仓无 JOIN。
后续发现的异常 event.kind 严格解码差异由后续阶段修复，首批通过不掩盖此限制。

### 分领域记录

以下计数只归属所列已测 head。Windows“原例”指上述未改的离线恢复用例。
fmt/Clippy 阻断后的步骤均不视为运行或通过。

| 阶段 / head | 已确认结果 | 失败或未运行边界 |
| --- | --- | --- |
| B `fc2e454c` | Windows 125 lib + 原例 1；剩 27 债 | Windows fmt 阻断；Linux Clippy items_after_test_module 阻断完整测试；后续修正 `a4cde5c7` |
| C `7e364843` → `97b7b9e` | 前头 Windows 134 lib + 原例 1；修正头适用 CI 成功，Linux 134 + 225；剩 24 债 | 前头 Windows fmt / Linux dead_code 阻断；删除无生产调用 wrapper，未压制 lint |
| D `25d382d4` | Linux 147 + 225；Windows 147 + 原例 1；剩 22 债 | Windows 后续完整验证 fmt 阻断，Windows 无默认特性 check 未运行 |
| F `ec1281d0` | Linux 154 + 225；Windows 重试 154 + 原例 1；剩 16 债 | Windows 首次官方工具链下载 TCP 10060 超时，未编译/测试；重试后续 fmt 阻断 |
| G `e456b92c` | Linux 161 + 225；Windows 161 + 原例 1；剩 9 债 | Windows 后续完整验证 fmt 阻断，Windows 无默认特性 check 未运行 |
| H `e9b28d82` | Linux 168 + 225；Windows 168 + 原例 1；剩 7 债 | Windows 后续完整验证 fmt 阻断，Windows 无默认特性 check 未运行 |
| I `b538a3e1` | Linux 177 + 225；Windows 177 + 原例 1；剩 4 债 | Windows 后续完整验证 fmt 阻断，Windows 无默认特性 check 未运行 |

I 阶段原始运行：[38519881075](https://github.com/TheLudlows/Origence/actions/runs/38519881075)、
[38519881083](https://github.com/TheLudlows/Origence/actions/runs/38519881083)。
格式修正头 `60222e3660ca5a7b2de375bc4fd54d452d5e4a6c` 的
[普通 CI 38520951077](https://github.com/TheLudlows/Origence/actions/runs/38520951077) 与
[Windows 38520951080](https://github.com/TheLudlows/Origence/actions/runs/38520951080)
全部适用检查终态成功。Linux/Windows 默认均 177 lib + 225 integration，Windows
原离线单例 1 passed；Linux 无默认特性 132 lib + 102 integration。Windows 无默认
特性仅运行 `cargo check --lib`，不是测试通过。Python 两组 100 + 42 通过，
另行 guard 测试 43 通过。

E 实现头 `f4f715fa` 完成最后 4 处关联改写（含测试 fixture），全仓 927 SQL 输入零债、零 unknown，
43 项 guard 测试通过。新增 E 回归为 3 lib + 3 integration；[运行 38523749953](https://github.com/TheLudlows/Origence/actions/runs/38523749953)
中的 `linux-native` job 全部步骤成功，默认 180 lib + 228 integration；该运行的
`fast-check` 因 fmt 失败，不能称整个运行成功。Windows 运行
[38523749817](https://github.com/TheLudlows/Origence/actions/runs/38523749817) 为 180 lib + 原离线单例 1 passed；后续 Validate 因 fmt 阻断，
Windows 无默认特性 check 未运行，不能称该头 Windows 全部验证通过。

最终格式修正与文档归档仍需其精确 head 的适用 Windows/Linux 检查通过。
最新归档 head、运行链接及终态以 [PR #59 Checks](https://github.com/TheLudlows/Origence/pull/59/checks)
和 PR 正文精确记录为准，不将已知阶段计数移记到未来提交。
交审保留 branch-gated 临时 workflow，后续另行审查，不在本轮按 Issue #61 删除。

### 保留的合同与验证边界

查询改为参数化单表读取，使用固定槽位批量和原生 SQLite/Turso 比较，在原事务
快照内以 Rust 关联；不替换成跨表子查询或 CTE。
保留正常数据下的权限与 raw session writer 差别、current/historical 差别、缺文件处理、
历史多 owner、原生 NULL/类型比较及 source/version/generation/publication 校验。
读取/解码按调用方实际消费字段安排：隐藏行或未消费异常字段不应使合法结果失败；
选中必要非法字段仍须安全报 Backend 等既定错误。多个坏字段的首错顺序、无关字段
的旧错误或旧 panic 不作为兼容合同。逐 version JSON/闭包成本并未全部消除。

固定槽位（多数 8，向量就绪匹配 4）限制每批请求数，不是 payload 字节/总内存预算。
状态检查不把完整向量计划读入 Rust；准备和重试仍承担序列化/原生校验成本，
ID 元数据为 O(total ID bytes)，没有新增输入大小上限。
E 阶段允许对 publication plan 的异常 chunks 根类型明确返回
`Unavailable("invalid message publication plan")`；正常数组、缺失及 null 的 pending
行为保留，异常根类型即使处于 pending 也明确报错。此项不能扩张为其他异常形状的
通用改约，也不涉及 Issue #60/#61、schema 精简或暂停的命名清理。

`tools/check_no_join_sql.py` 检查展开后的应用 SQL，包含测试和真实执行 sink；
冻结 baseline 只能缩减，最终接受需零债且零 unresolved。它是有界静态回归检查，
不是 Rust 编译器、全程序证明或原生 parser 安全证明；固定槽位和静态展开上限
不提供通用内存/性能保证。Python 检查、独立代码审查与实际 Windows/Linux 测试
分别提供证据，不能互相替代。真实模型、生产 SLO、任意断电与在线跨库备份验收
不因本轮查询改写而自动完成。

### PR #59 后续审计与轻量回执摘要

Context/startup 收窄读取范围，guard 补括号隐式多表与 `write!` 修改 SQL 的反例；
按调用方投影过滤后再解码，不用通用损坏值适配框架。此后另经确认，MessageBatch
计划同事务保存 `receipt_metadata`（ID 原生 JSON 单值与显式维度/null）。非法计划
可能保存时提前失败；正分块已准备计划缺失/损坏摘要报
`Unavailable("invalid message publication plan")`，不回填旧计划。同代重试校验并
复用原摘要，正常缺失/null/空计划保持既有行为。摘要不等于发布完成，真实 chunk、
索引、source/version/generation 及账本仍核对。绕过保存接口只改重型计划、不改
摘要的损坏，不由状态查询再完整扫描发现。

原生诊断保留失败结果：八槽反复提取重型 JSON 的固定 `765d6719` 在
[38531694088](https://github.com/TheLudlows/Origence/actions/runs/38531694088)
的 200×4096 metadata 中位数为 main 416.58 ms / 765d 9063.42 ms。
独立 JSONB 候选在[38534537552](https://github.com/TheLudlows/Origence/actions/runs/38534537552)
同轮 main 223.20 ms / 765d 4868.48 ms / JSONB 7374.51 ms；虽正确性通过，性能更差，
没有装入产品。不比较不同 runner 的绝对耗时。

`1e64ae50b8a3153a83368fac553a8bf5949a21e0` 的摘要
[诊断 38541994079](https://github.com/TheLudlows/Origence/actions/runs/38541994079)
终态成功，metadata artifact `11882214092`、完整 artifact `11883362035` 保留源码/SQL
哈希、真实锁、features、逐次样本与格式 patch。Turso 相关包锁为 0.8.2、SDK features
为空；同 Linux runner/release 构建，2 次预热、5 次计时，以下均为中位数。
历史 main 固定 `1d5e0554`，八槽对照固定 `765d6719`，历史 SQL 仅在临时只读诊断
副本执行，没有回到产品查询或 guard 豁免中。

| chunks×4096 | main metadata ms | 765d metadata ms | 摘要 metadata ms | 实际 API 读取 ms | API 首次保存 ms | API 重试 ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 48.586 | 293.016 | 23.846 | 31.911 | 100.348 | 130.536 |
| 100 | 97.882 | 1132.134 | 50.436 | 61.061 | 237.164 | 290.886 |
| 200 | 226.735 | 5591.052 | 127.758 | 148.592 | 559.325 | 636.291 |

三方 metadata 输出每次一致，查询数 main 2、765d 16/28/52、摘要 4；对应摘要后
payload 4,438,195 / 8,875,945 / 17,751,545 bytes。SDK 适配首次保存/重试包含
序列化、原生派生/校验、原子 UPDATE，200 项为 319.801 / 432.945 ms，不含事务边界。
表中实际 API 包含真实 begin/commit、权限及权威见证检查，不含 Service 物理可用性
门禁、模型或 LanceDB；没有历史完整 API 对照，不能据此声称其端到端加速比。
各尺寸的保存/重试一致、变更成员拒绝、缺全文见证/恢复和来源撤回均 PASS；8 项
原生诊断测试通过（包括保留的 4 项旧 JSONB 正确性测试）。

格式化摘要源码后的 `a50f4572` 在
[38543901622](https://github.com/TheLudlows/Origence/actions/runs/38543901622)
独立复验成功，artifact `11883508259` / `11884776018`。三尺寸同轮结果仍一致；
200 项 main/765d/摘要 metadata 为 424.678 / 9253.002 / 218.058 ms，实际 API
读取/首次保存/重试为 241.151 / 633.309 / 806.398 ms。有限夹具不证明生产 SLO、
无界输入复杂度或并发整体性能。

工程结果与性能分开：`1e64ae50` 普通 CI 因 fmt/未使用字段 Clippy 失败；Windows
208 lib 通过、1 个旧 OR/新 CASE 全等断言失败，原离线单例通过。实测证明 plain
parser 的未消费超大支持整数在旧 OR 中仍报 native 错误；新投影故意忽略 plain/null
无关支持异常，semantic 实际消费同一非法值仍报错，后续已锁定这一准确合同。

`a50f4572` Linux 230 integration 通过、两平台 lib 212 通过/1 个旧 Context 候选
消费正文预期失败；Windows 原 1 MiB 离线链路重新出现 `0xc00000fd`。
first-chance 指向 `ready_vector_keys` 的 8 槽重复匹配表达式/Turso parser，说明无
JOIN 本身不保证栈安全。后续将该单表查询改为 4 槽、364 bytes/21 binds：公共 gate
只出现一次，ID/generation 各自 IN4 后仍用原生布尔精确配对。100 请求实际 25 次
fetch（原 8 槽为 13 次）；未请求的历史多代不增加读回行，双 IN 交叉组合不会误放行。

修正头 `f0fe2cc1752a4c151c8d9e150408827ddacc88ca` 的
[Windows 38546403861](https://github.com/TheLudlows/Origence/actions/runs/38546403861)
为 214 lib 全通过、未改原离线单例 1 passed、first-chance 检查成功；后续完整
Windows Validate 被一处 bind 链格式阻断，无默认特性检查未完成。
[普通 38546403979](https://github.com/TheLudlows/Origence/actions/runs/38546403979)
的 Linux 作业通过，默认 214 lib + 230 integration，fast-check 仍因同一格式失败。
原主线程栈、调度与原离线回归断言未降低。最终图窄投影/格式/文档头仍需独立全 CI，
结果以 PR 正文精确 head/run 为准，不沿用阶段通过结论。


## Issue #61 第一批裁剪（2026-10-14，验证中）

基线 main `75c238ffa830d50840d4b84c2dce6d7d69a4793c`。空 record_source、
keyword_hits(false) 和六处 identity catalog 探测收口；保留真实 event/version
来源及事务回滚、摘要多行 AND、完整 schema 启动拒绝。探针完整冻结归档，
两个 #59 临时 workflow 删除。原 Windows 1 MiB 生命周期回归未改。

本地 SQL guard 及其 47 项回归通过；无 Cargo/rustfmt，因此本地 Rust 构建、
格式、Clippy、原生测试均 NOT_RUN。Linux/Windows 的最终验收以本 PR 精确
提交的实际 CI 结果为准，尚未完成时不得称通过。


## Issue #61 第二批接口收口（2026-10-14，验证中）

删除六个旧 trait 方法及专属实现/mock，保留 register_owner、
register_pending、confirm_committed、retry_delay、owners/ledger；有效共享
owner、撤回、scope、同版本重开、FK 顺序回归保留在当前 maintenance 路径。
新增当前 fail_job 的 claim fence、固定延迟、预算终止及 ledger 不被改写测试；
finish_cleanup 新增失败原子回滚和 orphan 结算不改写重试元数据的覆盖。
不声称旧 ledger failure 与 fail_job 完全同义，不保留旧坏值首错排序。

移除后端 Capabilities 声明，KeyCapabilities 与 max_hops 实际限制未改。
SQL guard 的 lancedb.rs 整文件源码绑定因只删除 Capabilities 导入/方法而刷新：
旧 fd13d098719af04ed33ff12dcd0cf75a11c5f132b7c9bcabe066ced855f66343，
新 db68bf43a036b04ca8cac6dfb925068687c76634159e37fb0f63d7f1be9605fd。
独审确认 scoped_filter 定义、调用及类型逐字未改，ann.rs、full_text.rs、
scope.rs 字节和绑定值保持原样；无新增允许路径/站点，47 个 guard 测试保留。
这是当前源码安全契约更新，未改任何历史实验 freeze。

identity-check / check_memory_identity(path) 保持现有库只读、独占锁、格式
与完整 schema 检查；删除无效 dry_run 和假升级状态，保留身份/权限/来源/版本
回归并验证成功和失败检查不改写数据库字节。

本机无 Cargo/rustfmt，Rust 格式、Clippy、构建及行为测试依赖精确提交 CI；
第二批尚未发布时不得将第一批通过结果归属为第二批验收。


### 第一批精确头验收

第一批 `4139e4a55916ae9fdf1977f25d6a9b0fcc94ae69` 的
[CI 38587438901](https://github.com/TheLudlows/Origence/actions/runs/38587438901)
三个作业全部通过：Linux/Windows 各 216 lib + 230 integration；无默认特性
139 lib + 102 integration；格式、Clippy、SQL guard、HTTP/MCP、受控进程
恢复与 keyword 评估通过。实际 PR merge tree 与该 head tree
`37ea1cb9f359d01c4156e6a6ea7b197c5f355635` 完全一致。
该结果只属于第一批；不外推至第二批或之后提交。


### 第二批精确头验收

第二批 `44b74835928321482ca62fac0d16c716e0bb732b` 的
[CI 38591327253](https://github.com/TheLudlows/Origence/actions/runs/38591327253)
三个作业全部通过：Linux/Windows 默认特性各 213 lib + 231 integration；
Linux 无默认特性 136 lib + 102 integration；Windows 无默认特性为
`cargo check --locked --no-default-features --lib -j 1`，不记作测试运行。
结果只属于该精确 head，不替代后续 AML 归档、文档改动或最终 PR head 的验收。
本节是追加证据，前文当时的“验证中”及第一批记录保留，不重标历史结果。

## Issue #61 第三批归档与文档收口（2026-10-14，验证中）

实施基线为第二批 head `44b74835928321482ca62fac0d16c716e0bb732b`。
AML 研究执行器及其完整本地依赖移至 [archives/aml-research](../archives/aml-research/README.md)，
归档来源与逐文件校验见该目录 manifest；当前工具保留协议/恢复回归、必要数据 helper
与离线工件核验。README/docs 入口、STATUS、API、OPERATIONS 与本页分别负责导航、
当前进度、接口契约/破坏性变更、运行步骤/边界、精确版本证据；既有历史记录不删除或改写。

`evals/` 的 freeze、哈希、分数、报告与原始工件保持原样。离线核验的源码输入须由
`--source-root` 显式指定与 freeze 匹配的历史源码树；可使用冻结基线
`2b7b6a67300d9e96eca8be0de282d308b8ded576`，具体命令见[归档说明](../archives/aml-research/README.md)。
当前 checkout 或移动后的归档目录不能冒充历史源码根，也不将重新计算的当前结果写回旧 freeze。

本环境缺少完整外部历史工件和对应 tokenizer，未执行完整历史工件审计或真实模型实验。
第三批格式、Clippy、构建与原生 Linux/Windows 行为验收仍待其精确 head CI；
第二批绿色结果不是本批通过证据。已执行的本地检查如下：

- tools unittest：113 项（104 项原有 + 9 项新增），保持原有 2 项依赖实际宿主的跳过，其余通过；未删除原有用例。
- evals unittest：42 项通过。SQL guard 检查 1062 处执行点，零违规/零未解析；其 47 项回归通过。
- 归档中的 18 份 Python 原件及 1 份 research requirements，共 19 份文件，Git blob ID 与 SHA-256 均与归档来源 `44b74835928321482ca62fac0d16c716e0bb732b` 精确一致。
- 指定历史源码 `2b7b6a67300d9e96eca8be0de282d308b8ded576` 后，budget 的 9 项、temporal 的 9 项源码哈希全部匹配；maintenance runner 的历史哈希也匹配对应 plan/run。此项仅核验源码，不是完整工件审计。
- 离线 diagnosis 通过。完整 budget/temporal/maintenance 工件审计仍为 NOT_RUN：本地缺少所需公开语料，budget 另缺 tokenizer 依赖/模型文件，maintenance 大型响应归档本地不可用。缺少输入不是已验证的工件损坏，也不以 helper/unit 测试冒充完整审计。

本地文档检查：六份当前文档的 161 个相对链接/锚点全部解析成功；未发现尾随空白。
本页原有各阶段验收正文与整理前逐字一致，仅调整页头导航并追加本批证据。
该检查不执行归档研究入口，不代替完整历史工件审计或原生行为测试。


## Issue #60 第一批：身份合表

基线 main `4c0862e80ccfb8b403268170c06d391239790d2b`。实现范围与
微基准边界见[身份存储合同](contracts/identity-asset-layout.md)。

本地环境没有 cargo/rustc/rustfmt，Rust 编译、格式、Clippy、原生测试与
性能测试尚未运行；Linux/Windows 结果等待本批精确提交 CI。静态检查不能
替代原生执行，也不沿用旧提交的通过结果。

基线本地执行：no-JOIN 有界检查 1062 inputs、47 项 checker 单元测试；
tools Python 113 项（2 跳过），evals Python 42 项通过。它们是改动前基线，
不是最终实现验收。最终 head、原始测量与跨平台结果在确认后追加。

### 首轮执行与测量边界

实现 `da59fa55` 的 CI 38668157170 在 fast-check/Windows 的 rustfmt
检查失败，不能称为 Rust 测试通过；随后仅按 Rust 1.98 输出应用47处
token等价格式修改。

格式后实现 `fca098e7ec89ab0d9748832279bfcac72d376e50` 的
[CI 38668569849](https://github.com/TheLudlows/Origence/actions/runs/38668569849)：
Linux native 全套成功，包括 Clippy、216项库测试、238项集成测试（基准
单独忽略）、HTTP/MCP、受控进程恢复及关键词评估。fast-check 的无默认
特性136项库测试、109项集成测试通过；其后的原生布局基准因既定30分钟
job上限取消，故 fast-check 整体不是通过，后续Python步骤未执行。
Windows job 116206187165 亦完成成功，包括原生验证及无 Lance 的 authority 检查。

[debug测量原始行](evidence/issue60-identity-layout-debug-fca098e7.txt)保留
首轮事实。1k行：创建旧5.588166s／新7.841706s（更慢），复用
28.621334s／11.686636s，绑定lookup 17.404110s／11.710922s；
停库DB+WAL为2,838,688／2,480,248字节，不代表压实后的数据库尺寸。
10k旧创建338.824476s，之后旧布局reuse从02:08:01至取消未完成；
新布局10k未运行。不能从局部读取收益声称整体性能提升。

后续测量保留1k/10k数据规模和完整创建，reuse/lookup各限定最多1,000个
均匀样本；二者在同一release可执行文件中运行，记录每阶段及每10批提交
进度，放入独立20分钟job（构建最多10分钟，测量命令7分钟并留工件上传
时间），不加长原fast-check上限。debug与release数据不得混作直接对照。
新测量与精确最终头结果待执行，初轮超时不改写为通过。


### 独立 release 构建预算

`888f9acd5814bca6ae8bdd203e19ee5c04e575a7` 的
[CI 38671905476](https://github.com/TheLudlows/Origence/actions/runs/38671905476)
独立基准 job 116216741185 在冷 release 构建阶段超时，测量没有开始。
依赖编译约7分54秒，02:44:55开始编译origence；02:47:14达到原10分钟
构建上限。此失败与先前debug测量运行超时不同，不能作为运行性能证据。
缓存保存也失败（编译子进程仍修改target导致tar退出1）。

据此仅调整编译预算：独立job35分钟，编译命令25分钟/步骤26分钟，
GNU timeout保留30秒强杀宽限以清理进程组；测量仍固定7分钟/步骤8分钟。
构建输出也保留工件。不改变两布局的编译配置、数据规模或测量算法；
此预算调整本身不代表性能通过。

该头的 fast-check、Linux native、Windows 三项功能验证均成功；独立布局
基准仍为构建超时失败，整轮CI不能称全绿。


### 首次完整 release 对照（ad8ab727）

[CI 38673296988](https://github.com/TheLudlows/Origence/actions/runs/38673296988)
在 head `ad8ab727cf3778729be46b3f865ffe4f06052eef` 四项 job 全部成功：
fast-check、Linux native、Windows、identity-layout。GitHub merge ref
`1859dc899657f75048af85aa597be6e98fb8bcfd` 与 head 的 tree 都为
`fbfd29930f76dc77e71e0dca800fc9008f1d6232`，main 仍是 `4c0862e8`。
冷 release 构建11分03秒，测量73.96秒，未超过7分钟运行上限。
[原始测量行](evidence/issue60-identity-layout-release-ad8ab727.txt)与
[原始工件](https://github.com/TheLudlows/Origence/actions/runs/38673296988/artifacts/11945820917)
保留环境、构建和测量结果。

| 数据集/阶段 | 旧布局秒 | 新布局秒 |
|---|---:|---:|
| 1k create（1k操作） | 0.357005 | 0.451673 |
| 1k reuse（1k操作） | 1.155132 | 0.460553 |
| 1k binding lookup（1k操作） | 0.699401 | 0.460980 |
| 10k create（10k操作） | 14.353148 | 26.539850 |
| 10k reuse（1k均匀样本） | 11.959848 | 4.998808 |
| 10k binding lookup（1k均匀样本） | 7.473771 | 4.994607 |

1k创建慢26.5%，10k慢84.9%；10k停库DB+WAL为11,147,192→11,051,488字节，
不是压实后稳态尺寸。减少SQL次数不等于整体加速。功能通过，性能取舍仍未接受。

旧/新DDL并非约束完全等价：新布局增加paired-null/memory-only CHECK，
资产行更宽，唯一索引/FK结构也改变。计时包括SQL约束检查、JSON读取解析、
提交及进度输出，但不含输入key/JSON构造和canonical重算。原fixture CHECK
非空分支的条件顺序与生产不同；历史数字不重写。无执行计划/profile/消融时，
不能将回退归因于单一约束、索引或合表。固定次序单样本也有噪声限制。

经确认继续定位：仅增加ignored诊断fixture，新after使用生产精确CHECK顺序，
其无新增CHECK变体只用于隔离成本、不作为生产候选；交替重复测量创建各阶段，
并尝试记录执行计划。不改生产约束，不据静态推测给出性能原因。新诊断结果待CI。


### 创建阶段/CHECK 消融（183e9398）

[CI 38677085702](https://github.com/TheLudlows/Origence/actions/runs/38677085702)
的 fast-check、Linux、Windows、identity-layout 四个 job 全部成功；
identity-layout job 116233033555 release增量构建3分24秒，诊断140.46秒。
[原始阶段、计划与进度行](evidence/issue60-identity-creation-183e9398.txt)保留全部
1k三轮/10k两轮结果。以下为10k正序/逆序两轮（秒）：

| fixture | 总创建 | identity miss | assets insert |
|---|---|---|---|
| 旧布局 | 14.320567 / 14.312924 | 12.060185 / 12.076598 | 0.827270 / 0.817714 |
| 精确生产CHECK合表 | 26.891802 / 26.993154 | 24.762397 / 24.872053 | 1.181712 / 1.172173 |
| 仅去新增CHECK合表 | 27.095197 / 26.935301 | 25.105781 / 24.939272 | 1.037942 / 1.041211 |

回退主要集中在identity miss查询；去CHECK没有改善整体创建时间，故新增约束
不是这轮主要回退来源。CHECK影响写入阶段的约0.14秒不能解释约12.6秒总差值。
旧identity insert另耗约0.48秒；fact collision约0.73–0.75秒，commit约0.20秒。

原生EXPLAIN QUERY PLAN：旧identity查询选择身份表asset唯一索引、仅用scope前缀；
新两变体选择oc_memory_fact_slot、仅用scope前缀，未用identity_key等值约束索引。
fact collision则使用完整scope/fact_key覆盖索引。计划在1k建库完成后采集，
使用已有key，展示相同SQL的规划而非实际miss逐条轨迹；INSERT计划零行不代表零成本。
此证据支持查找路径是主要瓶颈，不足以宣称任何普适引擎缺陷或布局必然变慢。

下一测试仍只改fixture SELECT：原查询、显式identity_key IS NOT NULL、该谓词
加INDEXED BY身份索引；生产DDL/约束及返回语义保持，先核计划与边界结果，再比较
相同分阶段时间。强制索引仅作诊断，不能未经验证直接成为生产方案。


### 等价查询路径诊断（85a21005）与最小修复

[CI 38678537293](https://github.com/TheLudlows/Origence/actions/runs/38678537293)
的 native identity-layout job 116237566198 成功，测量114.35秒。
fast-check/Windows 因三处rustfmt格式失败，整轮不是通过；随后按真实Rust1.98
输出仅调整等价排版。原始[查询、阶段与计划行](evidence/issue60-identity-query-85a21005.txt)
保留，不能用原生基准通过替代功能门禁。

完整生产DDL、六字段投影与全部约束固定，仅改变SELECT，10k两轮（秒）：

| 查询 | 总创建 | identity miss |
|---|---|---|
| 原生产查询 | 26.261058 / 26.128000 | 24.208089 / 24.099817 |
| 加显式IS NOT NULL | 26.439306 / 26.290508 | 24.378090 / 24.240430 |
| 上述谓词 + INDEXED BY身份索引 | 2.727091 / 2.733739 | 0.847135 / 0.848831 |

1k三轮原查询0.459–0.465秒，强制身份索引0.248–0.257秒。空库及10k填充后
计划均确认前两者选fact索引scope前缀，第三者使用完整scope/identity_key。
该受控对照支持访问路径是本fixture的主要瓶颈；不是约束本身的成本，亦不是
合表必然导致变慢。原、新布局未都作同等查询优化，不能据此断言优化合表
在任何等优化历史布局上更快。精确生产方法仍以功能回归验证，非端到端性能承诺。

最小生产改动仅identity_slot_inner查询增加已验证的INDEXED BY及显式非空谓词。
完整schema admission保证精确partial unique索引存在；缺失或错误定义仍拒绝，
运行中被外部删除则报错，不回退扫描。已验证key非空，保持scope/kind/投影/绑定、
坏body先于tombstone错误及全部事务边界。显式非空重申partial索引适用条件；
没有单独测试“只有hint没有非空”，不声称该谓词是引擎支持hint的必要条件。
仅此查询选定访问路径，不扩至lookup/candidate等未测方法。

宽松坏数据unit fixture增加同名非唯一partial索引，以保留故意重复/非memory
损坏数据；生产schema不变，legacy collision Conflict断言不放宽。既有创建、
复用、版本/隔离、tombstone、错误顺序、索引缺失与恢复回归继续运行。
修复后的精确head完整CI与原生复验待执行；不以旧头通过替代。

### 验证文档完整性修复

本轮恢复上传时发现，先前通过终端读取大型VALIDATION文档的输出被截断，
造成85a21005远程文档含输出截断占位内容。现从本地完整版本恢复历史全文，
保留各次失败与成功记录；代码与CI原始日志未受该文档传输缺陷影响。
逐文件比较本地455个已跟踪文件与该头远程blob SHA，仅VALIDATION不一致；
本轮每个修改文件均以本地完整字节Git blob SHA核对上传结果，不再依赖
单次终端输出或仅搜索截断标记判断完整性。


### 第二批：账本未维护字段（待精确头执行）

全链路核查确认oc_artifact_ledger.attempt/last_error不参与当前生产DTO、发布、
可见性、重发或恢复决策；旧设计及测试确实保留人为写入值，经确认取消该约定并
删除两列。不是将ledger重试责任迁到job；现行job attempt/错误码、next_retry_at、
generation/run_token、完整ledger主键与状态判断全部保留。公开pool()直接SQL访问
被删列不兼容；LedgerEntry.idempotency_key/Serde/helper明确暂不变。

调整确认/清理/恢复fixtures时保留错generation、无来源、no-op、重复注册、回滚、
到期retry_wait及next_retry_at保留断言；增加已删列旧布局只读检查和打开拒绝回归，
验证原schema/数据不被修改。不修改冻结归档DDL。字段盘点仍是指定旧main基线，
不将其改写为新schema。待独审及新精确提交Linux/Windows门禁，不沿用身份批通过。

### 身份修复完整验收与第二批格式检查

身份修复 head `aa1ebb501892205b195004fff0c357f7176458ea` 的
[CI 38691175438](https://github.com/TheLudlows/Origence/actions/runs/38691175438)
四个job全部成功（fast-check、Linux native、Windows、identity-layout）。真实
identity_slot方法的创建/复用、坏数据顺序、tombstone、权限与恢复回归随全套执行。
同头原生重复fixture的10k创建：原查询26.636623/26.176131秒，选身份索引
2.724988/2.744707秒；完整测量115秒，原始工件见
[11952301181](https://github.com/TheLudlows/Origence/actions/runs/38691175438/artifacts/11952301181)。
性能范围仍是隔离的查询创建fixture，不是全部生产端点时延。

第二批head `5300eba89d6ae7508b2f86de0f94a1c6db5f2dad` 的
[CI 38693470838](https://github.com/TheLudlows/Origence/actions/runs/38693470838)
fast-check/Windows被Rust1.98格式检查拦下，不能称通过。随后对三个测试文件
应用真实formatter输出的5处token等价排版；SQL与行为不改，需新head完整CI。


第二批格式修正head `948d3ec75af8038c547b9ba9f85dc73b6f2ce3ad` 的
[CI 38694688166](https://github.com/TheLudlows/Origence/actions/runs/38694688166)
Linux、fast-check及原生基准成功；Windows新旧ledger布局测试在目录文件快照时
读到仍被宿主OS排他锁持有的`.lock`文件，报Windows error33，其余238项集成通过，
整体不能称成功。修正测试仅快照DB/WAL/SHM文件，缺失sidecar允许不存在，其他读取
错误仍失败；保留在线check与停库open/offline拒绝、原数据及持久文件逐字节比较。
不改生产锁或schema拒绝行为，需新精确head跨平台验证。
