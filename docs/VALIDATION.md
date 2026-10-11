# Origence · 当前验收摘要

本页记录当前验收结果。CI 通过、测试通过与产品效果分别判断。

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
