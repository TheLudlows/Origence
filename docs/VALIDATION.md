# Origence · 当前验证

核验日期：2026-11-17。本次已同步 main
[`3d474d9`](https://github.com/TheLudlows/Origence/commit/3d474d9b4efe2edb0cf0c6a4aca9c0d1c3cba3fe)，
最新本地检查见首节；后续历史结果仅适用于各自注明的提交，不代表本次 CI 状态。
当前能力和后续工作见 [STATUS](STATUS.md)，本文只登记证据与限制。

## 2026-11-17：#79 合并最新 main

将 main `3d474d9b4efe2edb0cf0c6a4aca9c0d1c3cba3fe` 普通合并到 #79 的
`04eb4b6774e5c9f6a8346b05b95d10754d58557a`，未重写分支。实际文本冲突仅在
STATUS/VALIDATION：现状采用主干 UI/诊断/Run 内容，双方历史验证按原提交保留。
代码自动合并后另核对 Search/Context/publication_tests：主干诊断 capture、route 标签、
bundle 观察和新 fixture 元数据全部保留，最终所有 native 排名参与者仍由 Turso 批量复核。
Run/API、AnySeek、UI、CI 与主干相同；主干已有 fixture feature gate，移除本分支重复 gate。
没有改变 SQL、错误优先级、Run 重放/失败/撤回合同，也没有恢复旧 CI/UT。

本轮真实后端固定为当前 CI 的 `d813b915b7f12e7139ca316adc15699688493196`，
在独立干净 worktree 构建；不再以早期 `6614bbf` 的结果代表当前协议。
本地 Origence Rust 1.98.0、AnySeek Rust 1.99.1、Node 24.22.1；临时数据与凭据。

| 合并后检查 | 结果 |
| --- | --- |
| `cargo test --locked --lib` | PASS：216 项；含离线 rank、publication 最终复核/栈约束、诊断隔离及 Run 存储合同 |
| `cargo test --locked --test local` | PASS：202 项；含 EvidenceBundle、生产存储、诊断配置、Run 成功/失败/重放/来源撤回与重开 |
| `npm ci --ignore-scripts --cache /tmp/pr79-npm-cache`、`npm test` | PASS：UI 构建与 15 项；初次默认缓存目录不可写，改用临时缓存后通过 |
| Python tools / evals | PASS：11 / 42 项 |
| `cargo test --locked --no-default-features --lib` | PASS：136 项 |
| 默认/no-default `cargo clippy --locked --all-targets -- -D warnings` | PASS；无默认命令另带 `--no-default-features` |
| `cargo fmt --package origence -- --check`、`git diff --check` | PASS |
| `cargo build --locked` | PASS |
| 最新固定 AnySeek 下 `anyseek_smoke.py --cargo-tests --binary … --run python3 tools/mcp_smoke.py --binary …` | PASS：8 项真实引擎合同、完整 MCP 转发/离线、HTTP/stdio、Run 诊断关联/重试/重开 |
| 最新固定 AnySeek 下 `anyseek_smoke.py --run python3 tools/ui_smoke.py --binary … --ui-entry ui/dist/server.js` | PASS：UI 到后端入库/目录/历史/检索/上下文、Run 隔离及来源撤回 |

本轮未重跑 no-default 完整集成套件、Windows、真实浏览器布局、生产性能或真实模型/客户端任务。
不把合并前的 CI 或本地通过写成新 HEAD 的远端 CI 通过。

## 2026-11-17：#79 CI 故障修复与检查审计

失败运行 [44036124032](https://github.com/TheLudlows/Origence/actions/runs/44036124032)
对应 #79 `5ce7f4c`。读取实际 job 日志后，本地精确复现 MCP 的失败调用链：
`protocols → check_tools → schema_object` 在第 299 行访问空参数 schema 缺省的
`properties`，抛出 `KeyError`。前一节通过的 `anyseek_smoke.py --binary` 是较小的
合同入口，不等同于 CI 的 `--run python3 tools/mcp_smoke.py` 完整检查。
fast-check 的实际失败为 `graph_evidence_for_publication` 在 no-default 下 dead_code。

修复提交 `b6b0bf0af39f67d8cf5ed3fe49058fa90e0471c8`：图方法仅在其唯一调用方所在的
`local-storage` 下编译；同一 feature 控制 Service 测试夹具；空 schema
返回缺省空属性集合，字段集合、required、类型和 additionalProperties 检查不变。
后续将夹具 gate 从 `tests/local.rs` 的声明移到 `tests/support/mod.rs` 文件级，
保持同一 feature 行为，避免与新 main 在模块入口新增 catalog 测试产生文本冲突。
移动后重跑两种 feature 全目标 Clippy 和 no-default 完整套件。
没有全局 lint allow，也没有删掉失败检查。开始本轮时 main 已前进到 `44e06df`
（#77 合并），这三处最小修复与该主干一致；本分支未合入 UI 或 #78 诊断功能。

Linux / Rust 1.98.0 本地结果（后续仅补文档）：

| 检查 | 结果 |
| --- | --- |
| `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings` | PASS |
| `cargo clippy --locked --all-targets -j 2 -- -D warnings` | PASS |
| `cargo test --locked --no-default-features -j 2 --no-fail-fast` | PASS：135 lib + 2 协议 + 92 local；5 个真实后端用例保持 ignored |
| `cargo test --locked --lib retrieval` | PASS：14 项，含 rank/publication/身份/返回合同 |
| `cargo test --locked --test local graph` | PASS：4 项；图发布恢复、一跳/撤回、证据精确身份、workspace 隔离 |
| `cargo build --locked -j 2` | PASS；新产物用于下述 MCP 验证 |
| 固定真实 AnySeek 下 `tools/anyseek_smoke.py --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server --run python3 tools/mcp_smoke.py --binary /workspace/Origence/target/debug/origence` | PASS：与失败 CI 同一完整入口；host-forwarding 和 offline 两路径均通过 |
| `python3 -m unittest discover -s tools -p 'test_*.py' -v` | PASS：8 项 |
| main `44e06df` 已有的 `tools/test_mcp_schema.py` | 原样取到临时目录运行，3 PASS；空对象、缺失非空字段、未知字段拒绝，无新增重复 UT |
| `cargo fmt --package origence -- --check`、`git diff --check` | PASS |

CI/UT 审计：本次删除 **0** 项，工作流保持不变。

- 旧 linux-native 的 no-default `memory_identity` 子集与 fast-check 完整 no-default 套件
  使用同一 Rust/Ubuntu、feature、push/PR 触发范围；新 main 已移除该重复步骤，无需在 #79 再做清理。
- 保留默认/no-default 完整套件、MCP 完整离线/转发检查、HTTP smoke 和固定真实 AnySeek
  ignored 合同：它们的 feature、入口及 fixture 不等价，不能相互替代。
- 保留 publication/EvidenceBundle/来源撤回/隔离用例；旧库拒绝、旧 key 冲突和遗留 graph.db
  不被覆盖的检查仍对应当前只读拒绝与数据保留合同，名称中的 legacy 不表示测试失效。
- 保留 syscall fault UT：`.lance` 只是合成文件名，测试仍检查 C 注入器的 sentinel/path
  限制、计数、writev、暂停/终止行为，不能据扩展名判断废弃。
- `manual-diagnostics` 布局诊断为显式 feature + ignored，普通 CI 本来不执行；其进一步去留
  不影响这次门禁，不扩大为历史工具清理。

本地未重跑全量默认集成、Windows 或真实模型/生产性能；远端新 CI 需独立看结果。

## 2026-11-17：独立检索排序与权威复核拆分

基线 main `dcde761ba11b34d88ce5da5f9025a22076571dfa`；实现及本地 checkpoint
`552989e3dc251a97dd75174348342d36084e80e8`。以下为该代码树的 Linux / Rust 1.98.0
结果（后续提交只补本节和 STATUS）。独立分支未包含 #77、#78。

| 命令 / 检查 | 结果 |
| --- | --- |
| `cargo test --locked --offline --lib retrieval::rank` | PASS：2 项；无 AnySeek、模型或数据库 IO，覆盖分路去重/上限、分数/UUID 并列顺序、原始分数与 RRF60 |
| `cargo test --locked --lib retrieval::publication_tests` | PASS：现有 6 项；迟到撤回、pending/orphan/精确发布身份、正文变更、返回停止点/隐藏候选、语义错误和公开 future 栈大小 |
| `cargo test --locked --test local evidence_bundle` | PASS：13 项（含 Learning bundle 回归）；独立路由 top-k、历史/撤回/权限/预算/冲突与外部 IO 后复核 |
| `cargo test --locked --test local production_storage` | PASS：8 项；重放单次投票、冲突 generation、错误来源候选、投影失败/丢失和撤回 |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `cargo test --locked --no-default-features --lib` | PASS：135 项；有既有图方法 dead_code 警告 |
| `cargo fmt --package origence -- --check`、`git diff --check` | PASS |

无默认特性严格 Clippy **FAIL**：
`cargo clippy --locked --no-default-features --all-targets -- -D warnings` 与
`cargo clippy --locked --no-default-features --lib -- -D warnings` 均止于
`src/storage/sqlite/app.rs:288` 的 `graph_evidence_for_publication` dead_code。
在未修改的 main `dcde761` 工作树重跑后一条命令，复现同一错误；本分支没有修改该方法。
未运行无默认特性完整集成套件，不把库测试通过写成完整门禁通过。

真实 AnySeek checkout 干净且固定为 `6614bbf29bad0041c65738831bc1a16da4c69660`，
沿用既有 `tools/anyseek_smoke.py` 独立临时目录、collection 和测试凭据：

```sh
python3 tools/anyseek_smoke.py \
  --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server \
  --cargo-tests --binary /workspace/Origence/target/debug/origence
```

PASS：8 项真实引擎合同，以及真实 AnySeek → Origence HTTP/stdio MCP 的检索、读取、
引用、更新、历史、撤权及重开。使用本次代码构建的 Origence；没有修改或放宽 fixture。
EvidenceBundle/production_storage 定向集成测试仍使用仓库的协议 fixture，
不将其计为真实 AnySeek 或真实模型质量验收。

手工检查：迁移方法仍调用相同批量 authority 读方法，未新增或修改 SQL、动态 SQL 或执行入口；
排序与复核函数体除 receiver/格式外保持等价。最终所有 native 排名参与者仍先完整复核，
再按原响应停止点核验返回命中；快照开闭、外部 IO 顺序、图种子过滤和错误顺序不变。
新增 Turso 模块沿用 `local-storage` cfg，不引入 Service 或排序模块依赖。
未运行 Windows、本次全量默认集成套件、生产性能或真实客户端/模型任务。

## 2026-11-17：PR #78 同步 main 与冲突处理

将 main `44e06df`（含 PR #77 管理界面）合入检索诊断分支 `0bf9748`。
解决 CI、STATUS 和测试入口三处冲突；保留 Node UI 构建、真实 UI 门禁及固定夹具缓存，
缓存 key 与检出共用 AnySeek `d813b915b7f12e7139ca316adc15699688493196`。
同时保留目录测试与检索诊断测试、Run 列表接口与私有准备重放逻辑。

| 检查 | 结果 |
| --- | --- |
| Rust 默认全套测试 | PASS：lib 214、协议 4、集成 202 |
| Rust 无默认特性全套测试 | PASS：lib 136、协议 2、集成 92 |
| 两种特性集严格 Clippy、格式及默认构建 | PASS：Rust 1.98.0，all-targets / -D warnings |
| Node UI `npm ci --ignore-scripts`、`npm test` | PASS：Node 24.19.0，构建及 15 项测试 |
| Python tools / evals | PASS：11 / 42 项 |
| 真实 Node UI → Origence → AnySeek | PASS：入库、目录、历史、检索、上下文、私有 Run 隔离和来源撤回 |
| 真实 AnySeek 合同及 HTTP/stdio MCP | PASS：8 项合同，Run 诊断、原结果重试、语料更新及重开 |

全部使用隔离临时数据，Node 仅通过 HTTP 访问服务。未修改主干 UI 源码、原始 evidence、
依赖锁文件或冻结评估结果。本地 Linux 检查不代表真实浏览器布局、Windows、生产容量或模型效果验收。

## 2026-11-17：重试与 embedding 空间复核修复

基于 Origence `3d74879`，后端为 AnySeek
[`d813b91`](https://github.com/TheLudlows/anyseek/commit/d813b915b7f12e7139ca316adc15699688493196)。
Linux；Origence Rust 1.98.0，AnySeek Rust 1.99.1。

- 成功准备保存私有 bundle 快照，在检索前核对事件和请求摘要；重放及并发提交复核当前
  key/归属、精确历史引用和传递依赖。Run 读取/前缀同步补齐普通资产的完整来源核验，
  避免只核验 Learning 依赖。分页 trace 不复制内部快照。
- 查询诊断 v2 逐次核对当前 embedding 空间 SHA256；相同维度和分析摘要不能替代该校验。
- 真实隔离复现：首次 200，后端 query 改为 503 后，同 ID 重放仍为 200、响应完全相同，
  Run 只有一条事件且重放未访问后端；替换为不同空间集合后，向量查询从原来的 200 变为 503。

| 检查 | 结果 |
| --- | --- |
| 默认全套 `cargo test --locked -j 2 --no-fail-fast` | PASS：lib 214、协议 4、集成 201 |
| 无默认特性全套测试 | PASS：lib 136、协议 2、集成 92；命令另带 --no-default-features |
| 两种特性集严格 Clippy、默认构建与格式 | PASS：all-targets / -D warnings，Rust 1.98.0 |
| 定向 HTTP/存储回归 | PASS：后端故障时重放、重开、不同请求冲突、并发先成功后失败、跨 key/撤销隔离、资产撤回阻断重放/trace/前缀、空间变化拒绝 |
| 真实 AnySeek 合同与 HTTP/stdio MCP | PASS：8 项合同及更新语料/重开后原上下文和回执不变 |
| Python tools / evals | PASS：8 / 42 项 |

新增 SQL 均为带 scope/run/event 条件的单表查询；引用关联复用现有存储适配器，
来源检查与重放处于同一事务，无模型或 AnySeek IO。历史 evidence、依赖及冻结评估工件保持不变。
本轮不声明 Windows、生产性能、真实模型效果、真实 S3 或断电验收；此前 CI 结果仅属于各自 SHA。

## 2026-11-17：检索配置、诊断与私有 Run 初次验证

基于 main `dcde761`，真实引擎为 AnySeek
[`88ab7c1`](https://github.com/TheLudlows/anyseek/commit/88ab7c154d1345c646b02a56b4dcc82e73995d0d)。
本地 Linux，Origence 使用 Rust 1.98.0，AnySeek 使用 Rust 1.99.1。

| 检查 | 结果 |
| --- | --- |
| 默认测试 `cargo test --locked --no-fail-fast` | PASS：lib 214、协议 4、集成 201；8 项真实后端测试单独运行 |
| 无默认特性全套测试 | PASS：lib 136、协议 2、集成 92；5 项真实后端测试按该特性集忽略 |
| 默认 / 无默认 `cargo clippy --locked --all-targets -- -D warnings` | PASS；无默认命令另带 --no-default-features |
| 真实 AnySeek 合同 | PASS：8 项，包含全文/向量/图、重放、未知写入、清理及恢复 |
| 真实 HTTP/stdio MCP、Run | PASS：配置核对、请求关联、准备记录、幂等回执、重开后诊断不变 |
| MCP 完整冒烟 | PASS：宿主转发/离线协议、schema、安全错误、撤权、EOF 和重开 |
| Python tools / evals | PASS：8 / 42 项 |

新增定向回归覆盖：相同维度不同 embedding 空间拒绝、分析摘要重开核对且不覆盖绑定、
并发请求隔离与 128 条上限、成功/失败 Run 诊断、同 ID 重放保留原观察、畸形诊断丢弃、
私有读取隔离及同版本重开。旧上下文确定性断言只排除本次新增的请求标识/耗时，
其余内容、引用、冲突和预算仍完整比较。

本次修正测试辅助模块的特性门控、仅本地使用的图核验方法门控，以及 MCP 空 object
schema 的 properties 缺省读取；未为绕过测试放宽权限、来源、索引或发布校验。
绑定格式 v4 仅面向新目录，旧绑定明确拒绝，不自动转换已有数据。

服务端进度在查询后另采样；诊断不证明跨系统事务、已交付或模型已使用。
真实引擎合同使用隔离测试数据，不代表真实 embedding 质量、Claude Code/Codex 任务收益、
Windows、生产容量、真实 S3 或断电验收。历史段落按各自提交保留。

## 2026-11-17：移除独立 Turso 图存储

基于一跳适配的本地 `42c6e8f`（与 [PR #74](https://github.com/TheLudlows/Origence/pull/74) 的 `e281142` 同树）。
本地 Linux / Rust 1.98.0，代码头 `f0caae4c75df01c9e8e98f2c515caccbab0d032f`。全量构建中发现的无用测试 re-export 已移除，最终严格 Clippy 通过。

- `cargo test --locked --no-fail-fast`：213 库测试、199 集成测试和 4 协议测试通过；旧图存储独有测试移除。
- 应用回归：新库不创建 `graph.db`；旧文件未读取或修改；远端已写、权威未提交的冻结图发布可在重启后清理和重放。共享边撤回一份来源后保留另一份，未发布与撤回证据均不泄露。
- 真实本地 AnySeek `6614bbf`：一跳方向/来源/替换、BM25 排序和分数对照、HTTP/stdio MCP 烟测通过。
- 格式、`git diff --check`、`cargo clippy --locked --all-targets -- -D warnings` 通过。
- 不修改 `context.db` schema、AnySeek 代码或协议；未运行 GitHub Actions、Windows 和生产验收，不提供旧库迁移。

## 2026-11-17：AnySeek 一跳候选与原文排序

基线为 [PR #73](https://github.com/TheLudlows/Origence/pull/73) 的 `0a1a56c`，
验证版本 `1094887725fe903fe1eaf0019e9e4ebf8b153842`。本地 Linux，Rust 1.98.0，真实 AnySeek `6614bbf` 使用独立临时目录。

| 检查 | 本轮结果 |
| --- | --- |
| `cargo test --locked --no-fail-fast` | 222 库测试、212 本地集成和 4 协议测试通过；默认忽略的真实后端用例在下行单独记录 |
| 真实后端图用例 | 通过：同一来源 13 条一跳邻边、入边/出边、Unicode 子串、scope/version/generation、清理重放；相同 chunk 加图前、加图后、清除图后，相关性排序和分数位一致 |
| 真实 AnySeek + Origence HTTP/stdio MCP 烟测 | 通过：检索、读取、引用、更新、历史、密钥撤销与重开；该通用烟测不等同于原生客户端验收 |
| 格式、Clippy（`--all-targets -- -D warnings`）与差异检查 | 通过 |
| Python 工具及评估辅助 UT | 8 + 42 通过 |

独立引擎对照：固定 24 个 chunk 和同一查询，加入 40 个无全文字段的独立图 Record 会改变 BM25 排序；
改为只更新已有 Record 的图元数据，24 个命中的排序及分数完全相同。这是有限样本的回归证据，不是容量或生产性能验收。
公共检索的图候选仍需 Turso 的精确发布证明，来源更新后人工残留的旧索引和撤回事件在清理前后均不可见。
未运行 GitHub Actions、Windows、生产规模或真实客户端任务；部署仅覆盖新建库及 v3 collection。

## #73 基线的本地运行结果

| 检查 | 结果 | 范围 |
| --- | --- | --- |
| `cargo test --locked --no-fail-fast` | PASS | lib 218、AnySeek 协议 4、集成 211；共 433 项，默认忽略的 7 项另行运行 |
| 真实 AnySeek 合同测试 | PASS | 7 项，覆盖向量、作用域、响应丢失、重放、清理及公开 Service 重开 |
| 真实 AnySeek → HTTP / stdio MCP 合同 | PASS | 搜索、读取、引用、版本更新/历史、凭据撤销与重开 |
| `cargo fmt --package origence -- --check` | PASS | 当前 Rust 包 |
| `python -m unittest discover -s tools -p 'test_*.py'` | PASS | 8 项 |
| `python -m unittest discover -s evals -p 'test_*.py'` | PASS | 42 项 |
| `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings` | FAIL | 测试辅助模块无条件导入受 local-storage 控制的 Service |
| `tools/mcp_smoke.py` 对真实 AnySeek | FAIL | 空参数 schema 缺少 properties，检查脚本触发 KeyError |

真实 AnySeek 固定为
[`6614bbf29bad0041c65738831bc1a16da4c69660`](https://github.com/TheLudlows/anyseek/tree/6614bbf29bad0041c65738831bc1a16da4c69660)，
使用隔离的新建 collection。向量为合同夹具，不是实际 embedding 模型的语义质量结果。
可按当前工具入口复核（binary 路径按环境替换）：

```sh
cargo build --locked
python3 tools/anyseek_smoke.py --anyseek-binary PATH_TO_ANYSEEK --cargo-tests --binary target/debug/origence
python3 tools/anyseek_smoke.py --anyseek-binary PATH_TO_ANYSEEK --run python3 tools/mcp_smoke.py --binary target/debug/origence
```

MCP 失败位置为 [schema_object](../tools/mcp_smoke.py#L299)。诊断运行只在内存中
将空 object schema 缺失的 properties 视为空对象，随后宿主转发与离线 MCP 检查通过。
**该诊断未修改仓库脚本，不计为原始冒烟通过，也不证明真实 Claude Code/Codex 任务已验收。**
无默认特性失败位置为 [Service 导入](../tests/support/mod.rs#L12)；本地与 CI 一致。

## 历史 main CI（0a1a56c）

[Origence CI · run 43998026090](https://github.com/TheLudlows/Origence/actions/runs/43998026090)
对应上述精确 main 提交，总体 **FAIL**。

| Job | 结果 | 失败位置或边界 |
| --- | --- | --- |
| fast-check | FAIL | Authority-only Clippy，未进入后续无默认特性全套检查 |
| linux-native | FAIL | 默认 Clippy/测试、构建及 HTTP smoke 通过；MCP smoke 报 KeyError，后续真实 AnySeek 全套和检索评估未运行 |
| windows | PASS | 仅该提交、该 workflow 已执行的 Windows 检查 |

## 尚未形成的验收结论

- Claude Code / Codex 真实客户端同 key 归档查询、Skill 选择/加载、工具采集与完整任务追溯。
- 从记忆/执行经验生成候选的产品流程、用户实际审阅与研发/业务收益。
- 实际模型的检索/事实质量、生产容量与延迟、生产备份恢复及断电行为。
- 当前 Cargo.lock 的新一轮安全审计；旧依赖审计不能推定现在的结果。

## 历史证据

逐阶段验收、旧实现状态与依赖处置记录从活动页面移出；原文、失败、提交号与工件可从
[清理前 VALIDATION](https://github.com/TheLudlows/Origence/blob/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/VALIDATION.md)、
[历史依赖审计](https://github.com/TheLudlows/Origence/blob/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/DEPENDENCY_AUDIT.md)
和[原始工件](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/evidence)查阅。
仓库 evidence 目录内的既有原始工件保持不变。历史结果只属于其标明的提交/后端，
旧文中的“当前”“进行中”“待验收”不作为现在的状态。

## 历史文档整理检查（PR #75）

仅修改 Markdown；实现源码、配置、依赖、原始 evidence 与冻结评估结果保持不变。
历史文章通过上述固定 Git 版本保留，不改写其原始验证事实。

| 检查 | 结果 |
| --- | --- |
| 全仓 Markdown 的本地目标、锚点、UTF-8 与围栏 | PASS：24 篇，138 个本地链接，30 个片段锚点 |
| `git diff --check`、文档变更范围检查 | PASS |
| 旧 implementation/reviews 目录及失效活动链接 | 已移除，现行领域契约可从文档入口访问 |
| Rust / Python 运行检查 | 本次文档整理未重跑；前文为同一代码基线已执行的结果 |

## 2026-11-17：Node.js 管理界面合并前验证（Issue #76）

本节对应 `3fc320ef9983aa844ae517e5a661c5045a758cac`，是合并新 main 前的证据。
合并后复验另列，不把不同代码头的数量混用。

实现基线 `0a1a56c1da74e78be0118782b04a3f5a37251060`，测试在本变更工作树运行；
精确发布提交随 PR 记录。环境为 Linux、Rust 1.98.0、Node.js 24.22.1、npm 11.9.0；
真实 AnySeek 测试依赖固定为 `6614bbf29bad0041c65738831bc1a16da4c69660`。
所有行为测试使用新建临时库和测试凭据，不调用外部模型，不接触已有用户数据。

已通过：

- `cd ui && npm test`：12 项（含 6 个页面流程子测试）通过。覆盖同源 HTTP 转发、
  并发用户凭据隔离、原始二进制上传、409 保留、路径/Origin/重定向拒绝、超时、
  中文/emoji/组合字符的 UTF-8 定位、固定版本差异、表单幂等重试、五页导航和 401 清理。
  TypeScript strict 编译随该命令执行；happy-dom 只验证 DOM/请求合同，不是浏览器布局验收。
- `cargo test --locked -j 2 --no-fail-fast`：218 库、4 AnySeek 本地协议、212 本地集成通过；
  7 项单独配置的真实 AnySeek 合同测试在此命令中保持 ignored，没有改写为通过。
- `cargo test --locked --no-default-features -j 2 --no-fail-fast`：141 库、2 AnySeek 本地协议、
  105 本地集成通过，4 项真实后端测试保持 ignored。
- `cargo test --locked --test local catalog::catalog_pages -j 2`：目录专项 1 项通过。
  包含分页、失效来源被过滤后的游标推进、元数据不返回正文、workspace 隔离、
  其他管理员不能读取私有 Run、获授权轮换延续逻辑归属及旧凭据拒绝。
- 默认与 `--no-default-features` 的 `cargo clippy --locked --all-targets -j 2 -- -D warnings`、
  `cargo fmt --all -- --check` 通过。
- `python3 tools/anyseek_smoke.py --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server --run python3 tools/ui_smoke.py --binary target/debug/origence --ui-entry ui/dist/server.js`
  通过真实 Node UI → Origence → AnySeek HTTP 链路：认证、文件上传、异步发布、两类目录、
  固定版本、关键词检索、上下文字节预算、准备回执、跨 key Run 拒绝和来源撤回。
  这项真实引擎验收没有调用真实模型，不证明检索质量或 Agent 任务收益。
- `python3 -m py_compile tools/ui_smoke.py`、`git diff --check` 通过。

核验中修正：新建知识请求的 expected_version 必须为空而非 0；幂等删除测试需传请求键。
无默认 feature 全目标检查暴露原有 `tests/local.rs` 无条件加载 Service 夹具的问题；
已仅对该夹具模块增加与其调用者一致的 feature gate，没有删除任何无默认 feature 测试。

SQL 手工审视：新增仅两个参数化单表 SELECT，分别读取 scoped 资产候选与当前逻辑 key 的 Run。
关联/可见性复用原存储适配器，在同一授权只读事务完成；没有 JOIN、跨表子查询、
额外提交、模型/文件/AnySeek I/O、schema 变更或迁移。资产目录每页最多扫描 100 个候选，
Run 不返回事件或扩大私有读取范围。Windows 行为尚未在本环境验证。

未完成：真实浏览器的布局、键盘/触屏与无障碍验收。管理预览在识别标准 host/port 参数后
启动到 relay 阶段，但 `sites-preview` 报 `relay could not open browser port 4173`；
共享 Chrome 对对应地址返回连接拒绝，没有用 DOM 测试冒称浏览器验证通过。
线上部署、生产计量/告警、真实模型与 Claude Code/Codex 任务也未验收；缺失计量仍显示未知。

## 2026-11-17：Node UI 同步 main 后复验（PR #77）

集成主干 `dcde761ba11b34d88ce5da5f9025a22076571dfa`，保留 #74 图投影实现与 #75
文档收敛，仅在当前文档上合并 UI 记录，未恢复旧图存储或删除的历史长文。
精确最终提交由 PR #77 head 标识；以下数量仅属于此次集成后的代码树。

- `cd ui && npm test`：12 项通过；知识页额外覆盖文件选择、原始 Blob 上传、格式选择和
  file_id 入库请求，仍在同一个页面流程子测试内，不虚增测试计数。
- `cargo test --locked -j 2 --no-fail-fast`：213 库 + 4 协议 + 200 集成，共 417 项通过；
  8 项真实后端合同保持 ignored。与前节数量不同来自上游图实现及测试收敛，未删除 UI 回归。
- `cargo test --locked --no-default-features -j 2 --no-fail-fast`：135 库 + 2 协议 + 92 集成，
  共 229 项通过；5 项保持 ignored。
- 默认及无默认特性严格 Clippy、Rust 格式检查与 `git diff --check` 通过。
- 固定真实 AnySeek `6614bbf` 上重跑 Node UI → Origence HTTP 全链路 smoke，通过。
- 同一真实引擎上运行原始 `tools/mcp_smoke.py`，host-forwarding 和 offline 两条完整路径通过，
  覆盖版本协商、schema、工具、受控错误、活跃撤销、EOF/重开；不是先前的内存诊断替代。
- `python3 -m unittest discover -s tools -p 'test_*.py' -v`：11 项通过。

本次对空参数 schema 仅将返回的可选 properties 缺省为空对象；仍拒绝字段缺失、
字段变化和 additionalProperties 放宽。新增 3 项单测保留这些断言。
图的内部读取辅助方法只在其调用者所属 local-storage 特性下编译，不使用 allow(dead_code)
压制检查，也不改变默认图查询行为。此前历史 FAIL 不被改写。

Windows、真实浏览器和生产/客户端验收边界与前节相同，仍未验证；新的 GitHub CI 结果
另见 PR，不将本地通过冒称远端 CI 已全部通过。

## 2026-11-17：UI 代码复核与 CI 去重（PR #77）

复核基线为 `bb0e6d9876ac1d5e5c303e55368e016ee1e0bf27`。
该基线的 [GitHub Actions #360](https://github.com/TheLudlows/Origence/actions/runs/44013075805)
已完成：`node-ui`、`fast-check`、`linux-native` 成功；`windows` 按既有 main-only 策略跳过。
以下修改及本地验证属于其后的工作树，精确提交和修改后的远端结果见 PR #77。

代码复核修正三处异步界面行为：凭据/路由取消后，仍在解码的响应不能再返回；
旧检索不能覆盖新查询；取消上下文预览后退出等待状态，并允许再次预览。
先用两个 API 回归复现解码期取消失效（均因未拒绝响应而失败），修正后运行：

- `cd ui && npm test`：15 项全部通过，包含 strict TypeScript 编译、两个解码期取消回归、
  乱序检索和取消后重新预览，以及原有 HTTP/页面合同。
- 将 `ui/dist/` 单独复制到临时目录，以该目录的 `server.js` 启动
  `tools/ui_smoke.py`，通过真实 Node → Origence → 固定 AnySeek `6614bbf` 链路。
  这验证集成 job 只需 UI 构建产物，无需安装 npm 依赖或再次编译 UI。
- 官方 `actionlint 1.7.7 .github/workflows/ci.yml`、`git diff --check` 通过。
  actionlint 二进制按官方发布的 SHA-256 校验；未增加项目依赖。

CI 调整基于上述 #360 日志，而非减少独立验收：

- 移除 Ubuntu 原生工具安装（约 7.5 秒，日志为 0 newly installed）、工具链版本重复断言，
  以及已被 `fast-check` 完整无默认特性测试覆盖的 `memory_identity` 子集（约 12.8 秒）。
- 共享 Rust debug/incremental 配置；四个 job 名称不变。保留默认/无默认特性的完整测试、
  严格 Clippy、依赖树检查、原始完整 MCP、真实后端合同、评估证据及 Windows 策略。
- 保留 `cargo build --locked -j 2`：测试启用 dev-dependency 的 Turso 特性，
  不能用测试产物替代正常服务构建。
- AnySeek 二进制使用精确源码、Rust 版本、Ubuntu/架构和构建配置的独立缓存键，
  不使用宽松回退键。#360 每次重编译夹具约 58.6 秒，另有约 6 秒工具链准备；
  缓存仅包含可执行文件，所有行为验收仍使用新建临时数据。缓存命中后的实测另见 PR。
- `node-ui` 上传一次编译的产物，`linux-native` 复用并新增真实 UI HTTP 合同门禁；
  原先该真实链路只有本地证据。未把 DOM 测试当作浏览器布局验收。

本次未更改 Rust 源码、schema 或 SQL；未为本次 TypeScript/工作流变更重复本地 Rust 全套。
远端工作流仍执行上述完整 Rust 检查。Windows、真实浏览器及生产验收边界保持前述记录。
## 2026-11-17：SQLite 全面替换与元数据精简

基线 main `01d4f82b20c986278b077e8660f9bbc0538e8d6c`，验证在 SQLite 替换工作树执行；
精确提交与远端结果由本次 PR head 标识。Linux、Rust 1.98.0、rusqlite 0.43.0，
bundled SQLite 3.54.1；真实 AnySeek 固定 `d813b915b7f12e7139ca316adc15699688493196`
（夹具使用其要求的 Rust 1.99.1）。所有行为验证使用新建临时库及测试凭据，无外部模型调用。

替换范围包括全部关系/队列适配器、Service/CLI/错误边界、测试和手工诊断、Cargo 锁文件、
CI、容器与当前文档。删除旧引擎模块和别名；历史证据与此前真实失败记录保持原样。
Cargo 生成的新锁文件从 482 个包变为 370 个：删除 116、加入 4，净减少 112；
原有保留包的版本未改变。rusqlite 关闭默认特性，仅启用 bundled；不增加 ORM、
异步数据库或第三方连接池。全特性的 production dependency tree 排除所有退休存储引擎。
另移除未使用的直接 `futures` 声明；它仍由 rmcp 传递引入。移除前后全部已解析包和
features 完全一致，最后的 `cargo check --all-features --all-targets -j 2` 通过。

元数据删除 `oc_versions.content_hash` 和 `oc_artifact_ledger.idempotency_key`，
同时移除调用方的冗余字段/参数及字符串辅助函数。内容摘要由所读不可变正文计算，
账本使用已有完整主键的 ON CONFLICT 幂等；外部冻结计划和 operation epoch 保持原义。
命令幂等、Run/Skill 快照摘要、授权、来源、版本、owner/index/ledger 的独立状态仍被验证。

已通过：

- `cargo clippy --locked --all-features --all-targets -j 2 -- -D warnings` 与
  `cargo clippy --locked --no-default-features --all-targets -j 2 -- -D warnings`。
  前者同时编译手工诊断目标；不将其诊断执行当作生产验收。
- `cargo test --locked -j 2 --no-fail-fast`：218 lib + 4 协议 + 202 local，
  **424 项通过**；8 项单独运行的真实后端合同仍在常规命令中 ignored。
- `cargo test --locked --no-default-features -j 2 --no-fail-fast`：138 lib + 2 协议 +
  92 local，**232 项通过**；5 项真实后端测试保持 ignored。
- `cargo build --locked -j 2`、fmt、`git diff --check`、actionlint 1.7.7、
  Cargo metadata 与全特性退休引擎依赖检查通过。正常服务二进制只包含新 SQLite 格式标记，
  Linux `ldd` 无动态 SQLite 或 libgomp 依赖。
- `npm --prefix ui test`：15 项通过；Python tools 11 项、eval adapter 42 项通过。
- 在上述正常服务二进制及固定真实 AnySeek 上分别运行 `tools/smoke.py`、
  `tools/ui_smoke.py`、原始完整 `tools/mcp_smoke.py`，HTTP、UI 与
  host-forwarding/offline MCP 两路径通过。
- `tools/anyseek_smoke.py --cargo-tests`：**8 项真实引擎合同通过**，并通过其后
  HTTP/stdio MCP、引用、版本更新、撤销、重开及 Run 诊断关联/重试/持久化检查。
- `evals/run.py`：24 个 keyword 用例，errors=0、resolve_errors=0；document recall/MRR/NDCG
  均约 0.63636。此合成工程检查不证明模型或真实 Agent 任务收益。

新回归验证每条连接的 WAL、FULL、外键与 busy timeout、五租约上限、真实文件上的
取消/FIFO 回滚、RETURNING 排空、错误后拒绝继续提交、读快照、跨池释放、宿主锁生命周期，
以及直接 SQLite integrity/foreign-key 检查、元数据列消失、中文原始字节摘要与同版本重开。
只读测试分别覆盖非空已提交 WAL（必须可读）和 checkpoint 后带中文/空格/URI 特殊字符
的路径（不创建 sidecar）；没有用 immutable 忽略待恢复的 WAL。

早期回归暴露旧引擎假设：SQLite 将大 JSON 整数转换为 REAL、每连接有独立 DDL 缓存，
SHM read mark 也会被只读访问更新。测试改以 SQLite 原生结果为 oracle，保留无效身份、
解码顺序、授权和证据预算断言；没有为退休引擎添加兼容分支。损坏 schema 夹具重开其
有意改变的 DDL；活跃库比较数据库和 WAL，不将 SHM 的瞬时协调位当作持久业务字节。

手工 SQL 审视：两个元数据列及相应绑定/读取被移除，版本摘要在同一授权读事务中计算；
没有 JOIN、跨表子查询、额外业务提交或事务内模型/文件/AnySeek IO。数据库 IO 在专属
blocking 连接 FIFO 中；取消不硬停 SQL，已发出 COMMIT 的未知结果仍需按原幂等键核对。
只面向新库，不迁移、重写或自动转换既有文件；旧格式在可写打开之前只读拒绝。

本次 CI 将 Windows 检查扩展至 PR，远端结果另见 PR，不将 Linux 结果外推到 Windows。
标准 Docker 构建的首次尝试在下载 registry/config.json 时因本环境自签名 CA 链失败；
同一 Dockerfile 仅用临时构建期 CA secret 补充受信链后继续构建，未关闭 TLS 校验。
容器最终结果另见 PR。浏览器布局、真实模型/双客户端任务及生产性能不属于本次验收。
