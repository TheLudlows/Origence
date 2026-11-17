# Origence · 当前验证

核验日期：2026-11-17。代码基线：main
[`0a1a56c1da74e78be0118782b04a3f5a37251060`](https://github.com/TheLudlows/Origence/commit/0a1a56c1da74e78be0118782b04a3f5a37251060)。
除单列的 #74 结果外，本文记录的是该基线的 Linux / Rust 1.98.0 检查。
当前能力和后续工作见 [STATUS](STATUS.md)，本文只登记证据与限制。

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
