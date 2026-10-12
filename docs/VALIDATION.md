# Origence · 当前验收摘要

本页记录当前验收结果。CI 通过、测试通过与产品效果分别判断。

## 2026-10-12 第四阶段：显式 Improve → Learning

基于 main `be16bb1c2a273ec632e459abd277bd019fb36821`。新增确定性闭环、持久化阶段重放、principal-qualified 反馈序列、条件限制、子任务重试/取消和传递依赖撤回回归；不使用真实 provider，不据此声明语义质量或 Agent 收益。当前为实现检查记录，最终 PR head 的 CI 结果仍待补充。

云端编辑环境没有 cargo/rustc/rustfmt，未在本地执行 Rust 编译、格式、Clippy 或测试。Python SQLite 完整新库 DDL 执行和 foreign_key_check 通过；静态检查不替代实际 Rust 验证。保留现有 CI：PR fast-check/Linux native，Windows 仅 main；不恢复已取消的 macOS/container jobs。

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

