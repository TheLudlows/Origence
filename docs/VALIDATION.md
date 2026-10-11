# Origence · 当前验收摘要

本页记录当前验收结果。CI 通过、测试通过与产品效果分别判断。

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
