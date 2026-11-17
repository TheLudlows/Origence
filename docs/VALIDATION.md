# Origence · 当前验证

核验日期：2026-11-17。代码基线：main
[`0a1a56c1da74e78be0118782b04a3f5a37251060`](https://github.com/TheLudlows/Origence/commit/0a1a56c1da74e78be0118782b04a3f5a37251060)。
除单列的 #74 结果外，本文记录的是该基线的 Linux / Rust 1.98.0 检查。
当前能力和后续工作见 [STATUS](STATUS.md)，本文只登记证据与限制。

## 2026-11-17：#79 CI 故障修复与检查审计

失败运行 [44036124032](https://github.com/TheLudlows/Origence/actions/runs/44036124032)
对应 #79 `5ce7f4c`。读取实际 job 日志后，本地精确复现 MCP 的失败调用链：
`protocols → check_tools → schema_object` 在第 299 行访问空参数 schema 缺省的
`properties`，抛出 `KeyError`。前一节通过的 `anyseek_smoke.py --binary` 是较小的
合同入口，不等同于 CI 的 `--run python3 tools/mcp_smoke.py` 完整检查。
fast-check 的实际失败为 `graph_evidence_for_publication` 在 no-default 下 dead_code。

修复提交 `b6b0bf0af39f67d8cf5ed3fe49058fa90e0471c8`：图方法仅在其唯一调用方所在的
`local-storage` 下编译；同一 feature 控制 `tests/local.rs` 的 Service 夹具；空 schema
返回缺省空属性集合，字段集合、required、类型和 additionalProperties 检查不变。
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

## main CI

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

## 本次文档整理检查

仅修改 Markdown；实现源码、配置、依赖、原始 evidence 与冻结评估结果保持不变。
历史文章通过上述固定 Git 版本保留，不改写其原始验证事实。

| 检查 | 结果 |
| --- | --- |
| 全仓 Markdown 的本地目标、锚点、UTF-8 与围栏 | PASS：24 篇，138 个本地链接，30 个片段锚点 |
| `git diff --check`、文档变更范围检查 | PASS |
| 旧 implementation/reviews 目录及失效活动链接 | 已移除，现行领域契约可从文档入口访问 |
| Rust / Python 运行检查 | 本次文档整理未重跑；前文为同一代码基线已执行的结果 |
