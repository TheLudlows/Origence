# Origence · 当前验收摘要

本页记录当前验收结果。CI 通过、测试通过与产品效果分别判断。

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
