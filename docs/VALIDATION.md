# Origence · 当前验证

最近一次本地核验为 2026-11-21，代码基线按各节所列提交。2026-11-17 的验证记录已同步至 main
[`92258fc`](https://github.com/TheLudlows/Origence/commit/92258fc578bbdb1c21ca949ddbbcfb1729a5813a)。
最新本地检查见首节；后续历史结果只适用于各自注明的提交，不代表本次 CI 状态。
当前能力和后续工作见 [STATUS](STATUS.md)，本文只登记证据与限制。

## 2026-11-21 · #91 冻结题与 P6 回执离线关联

基线 `1f25c9912bdc7341892aca77b906f66a23b9c14b`（#121）。纯合成用例验证：由独立提供的冻结摘要追到题集原始字节、留出观察行、实际交付记录和精确 Run 前缀；再核对保存的签认原件及 owner 回执。详见 [P6 证据说明](../evals/pilot/P6-EVIDENCE.md)。
Linux Python 3.12.14：54 项试点离线测试通过（新增 18 项），42 项评估适配测试通过；工具套件 11 项通过、7 项跳过。公开索引 CLI 和 `git diff --check` 通过。
结构匹配不是签名验证、线上撤回核对或业务通过；未跑真实客户端、模型、正式试点或独立签认。

## 2026-11-21 · #91 离线评分与 P6 索引预检

基线 `ddf7fc99ed3c64011592d14406dc6dcdd889865b`。旧评分器可将 `reviewer: true`、
`evidence: [null]` 的合成填表行计作成功。修复后拒绝不合规的评审人和证据引用，保留预登记分母，
并将组指标标记为 `OBSERVATION_SHEET_ONLY`。P6 检查器核对 Run 前缀字段与索引、文件原始字节，
见 [P6 证据说明](../evals/pilot/P6-EVIDENCE.md)。公开样例仍为 `NOT_RUN`。

Linux Python 3.12.14：试点 36 项、评估适配 42 项通过；工具套件 11 项通过、7 项跳过。
公开样例 CLI 与 `git diff --check` 通过。未执行真实客户端、模型、正式试点或独立签认。
索引预检不核验冻结题、证据真实性、签认原件或 owner 回执。

## 2026-11-19 · #92 与主干 #108 整合

整合主干 `2094f81c990aaf7d69d4ff09295c1a2a837c4ba1`。24 项离线试点测试与公开合成评分 CLI 冒烟通过。重复评分结果逐字节一致，评分拒绝同 split 错批、准备批、空冻结模板和公开演练记录。两种评分模式均为四组，每组 8 个任务、32 个核验项。

#108 的四项 CI 在 `96fb05052ae70eca63ae5b8ad1fba72abef84b7f` 通过。整合后的 #92 以其自身提交的 CI 为准。真实客户端、模型、正式试点及独立签认仍待验证。[#108 CI](https://github.com/TheLudlows/Origence/actions/runs/44439456768)

## 2026-11-19 · #92 与主干 #106 整合

整合主干 `533658b4f378228f888c1c9b27b5d9b119e19c6b`。24 项离线试点测试与公开合成 CLI smoke 通过。
复跑核对独立冻结摘要、相同输入的逐字节一致输出、错批和公开演练记录拒绝，以及原分母。
#106 的 Hook 夹具证据、#87 的 Windows 接入证据和 #91 的公开演练各自保留原覆盖范围。
本轮未启动产品客户端或模型，真实 Hook 采集、正式试点和独立签认仍待验证。

## 2026-11-18 · #105 Codex 与 Claude Code Hook 离线验证

Linux x86_64（6.18.57），Rust 1.98.0，主干基线 `e024b78eb2ecbf1aa1851783c89be8238ca819a1`。
`cargo fmt --all -- --check`、`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 和
`cargo test --offline --locked -j 2` 通过。Rust 共 455 项通过、8 项需要真实 AnySeek 的现有用例忽略。
MCP schema 的 3 项 Python 回归通过，三份接入示例和两份脱敏夹具通过 JSON 解析检查。

- 五项 Hook 夹具覆盖默认关闭时零队列、零 HTTP，项目和客户端边界，身份缺失、乱序、去重、
  队列并发与容量、连接拒绝后的持久恢复、回执缺失时逐字节重放以及配置与凭据变化时的出队隔离。
- Codex 夹具核对停止检查、主线程中断、MCP 工具结构化错误和 Bash 的未知状态。
  Claude Code 夹具核对同提示 ID 的开始与停止、工具成功和失败、可选耗时、工具 abort、API 错误和明确关联的子代理。
  两端都检查敏感占位内容不会进入队列和 HTTP 负载。
- 合成 stdio MCP 客户端通过真实本地 Origence HTTP 和 SQLite 验证两端显式复用，覆盖 MCP 先建与 Hook 先建、
  其他 key 读取拒绝、reader 写入拒绝、轮换撤销后旧记录滞留和新事件仍使用原逻辑 key 的 Run。
  现有 MCP、Run、PDF 来源定位和回归用例一并通过。

草稿 PR #106 的首轮 CI 在 Windows Clippy 上因 `unused_mut` 失败。创建队列目录的可变
构建器只在 Unix 的权限分支中使用。修复后 Unix 显式创建 `0700` 目录，Windows 使用
继承私有父目录 ACL 的常规目录创建调用。Linux 重新运行格式、全目标 Clippy、五项 Hook、
两项 MCP/Run 定向测试及上述聚合回归，结果均通过。
[首轮 Windows CI 日志](https://github.com/TheLudlows/Origence/actions/runs/44326867155/job/134880390115)

2026-11-19，基于主干 `e663c3a723262450b639680752a1e10ed69a43f8`，修正并发夹具对队列锁的断言。
#92 的 Windows CI 中，八个线程同时提交同一事件，四个线程在 75 毫秒的等待预算内未拿到锁，
触发 `hook queue busy`。夹具现在要求恰好一条入队，锁忙的提交在竞争结束后必须去重，
且主动持锁时另一条事件不会修改队列。原有发送器锁隔离与队列容量断言继续执行。
Linux Rust 1.98.0 的格式检查、全目标 Clippy、5 项 Hook 和 2 项 MCP/Run 定向回归通过。
`cargo test --offline --locked -j 2` 共 455 项通过，8 项需要真实 AnySeek 的既有用例忽略。
[原 Windows CI 日志](https://github.com/TheLudlows/Origence/actions/runs/44436948640/job/135268079885)

当前证据来自本地夹具。真实 Codex 和 Claude Code 的 Hook 加载、实际事件及标识覆盖、Windows 文件行为、
真实模型和正式试点仍待验收。Claude Code 缺少可靠的整轮用户中断信号，无关联 ID 的事件会跳过。
显式 MCP 关联需要调用方已持有准确标识，自动会话正文归档留在后续规划。
[客户端接入与覆盖说明](CLIENTS.md#按项目启用私有-run-hook)

## 2026-11-18 · #91 离线题集与评分表

Windows，基于 #92 的 `2f29c8c`。用两组公开种子复验跨集排重、整表错配、逐行混入及题目文件被替换。试点测试 22 项通过；原评估测试发现 42 项，40 项通过、2 项跳过。Rust 1.98.0 的 `cargo fmt --all -- --check` 和 `cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过。[复跑步骤](../evals/pilot/RUNBOOK.md#题集与评分表)

## 2026-11-18 · #91 公开双场景跨客户端复做

Windows 公开演练完成数据库排障与报销复核的双向交接。[逐次结果、失败尝试和权限检查](../evals/pilot/VALIDATION.md#公开双场景跨客户端复做2026-11-18)

## 2026-11-18 · #91 双客户端加载同一版 Skill

两端原生加载成功。来源撤回后新导出返回资源不存在，已有本地文件仍匹配发布摘要。[逐端证据与限制](../evals/pilot/VALIDATION.md#双客户端原生-skill-加载2026-11-18)

## 2026-11-18 · #87 双客户端归档互读与独立 Run

Windows，main [`4160fd5`](https://github.com/TheLudlows/Origence/commit/4160fd5a7cbd7e21af54903a9471fcda15f5f08c)，AnySeek `6a46fc7`。新建 workspace，仅向两端提供同一个临时 writer key。Claude Code 2.1.327 报告模型 `glm-5.3`，Codex CLI 0.170.0 指定 `gpt-6.2-sol`，其事件未报告精确模型修订。整轮耗时 62.26 秒。

- Claude Code 显式归档含随机答案的问答，并创建和准备第一个 Run。Codex 凭 turn ID 读回答案，反向归档另一份问答，创建和准备第二个 Run。全新 Claude Code 会话按第二个 turn ID 读回。两个读取提示均未提供目标答案，两端最终输出都包含对方的随机答案；服务端按 ID 核对了问答正文和两份准备回执。
- 用相同凭据通过 HTTP 重放准备请求，回执和事件列表保持一致。重用事件 ID 改变查询返回 409。缺少 embedding 的 vector 准备返回 503，Run 新增一条服务端观察到的准备失败事件。
- 同 workspace 的另一管理员读取归档和 Run 均返回 404。轮换后旧凭据返回 401，新凭据保留原会话、归档和准备回执。撤销后，读取及重放均返回 401。

本轮覆盖 [#87](https://github.com/TheLudlows/Origence/issues/87) 的接入验收。业务答案、原生 Skill 加载和独立业务核验属于 [#91](https://github.com/TheLudlows/Origence/issues/91) 的双场景试点。

仓库内的[复跑脚本](../tools/live_archive_smoke.py)在 Windows 上使用上述主干构建和 AnySeek 完成同一组检查，耗时 67.68 秒。两次跨客户端读取均返回正确的随机答案，准备回执、冲突、失败事件与凭据隔离检查全部通过。整理脚本时，一次旧二进制在启动客户端前因缺少会话接口返回 404，另一次未能从 Codex 的最终答复核对对端答案。最终复跑显式读取 Codex 的最终消息文件。此脚本的 Linux 实机运行待验证。

Rust 1.98.0 的 `cargo fmt --all -- --check`、`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过。Python 脚本编译检查通过，`test_mcp_schema.py` 的 3 项测试通过。运行方式见[双客户端归档验收](CLIENTS.md#复跑双客户端归档验收)。

## 2026-11-18 · Codex 非交互检索与版本读取

在 Windows 上用 main [`4160fd5`](https://github.com/TheLudlows/Origence/commit/4160fd5a7cbd7e21af54903a9471fcda15f5f08c)、
本地 AnySeek 和 Codex CLI 0.170.0 运行合成报销题。两轮都指定 `gpt-6.2-sol`，
使用相同任务提示、只读沙箱和 `approval_policy = "never"`。每轮使用新 workspace、
独立 reader key 和相同的 v1、v2 规则。各轮的直接 MCP 对照均能读取 v2。

- 第一轮耗时 31.09 秒。Codex 发现了两个工具，两次调用 `context_search` 均因客户端审批失败，Origence 未收到工具调用。
- 第二轮耗时 34.62 秒。为 `context_search` 和 `context_get` 分别配置 `approval_mode = "approve"` 后，
  搜索返回 v2，随后按命中的资产 ID 和版本读到 v2 正文。客户端返回的资产 ID、版本和金额 100 TEST 单位均与预期相符。

该结果覆盖一题合成预检。P0 真实任务与独立签认仍待执行。配置方法见[客户端接入](CLIENTS.md#用-codex-非交互验证只读检索)。

## 2026-11-17：#88 独立 Skill 候选入口（本地切片）

基于 `293e2bf6af3b180d43c8050731047b552ac07519` 的独立 CLI、选定依据绑定修订、
合成权限/撤回回归与未验收边界见 [#88 验证记录](https://github.com/TheLudlows/Origence/blob/a56246406a0688f646a45a85cc2954fb14a8d28a/docs/validation/issue-88-skill.md)。
未发布，不改变下方历史证据，也不代表五路 P0 联合验收。

## 2026-11-17：#90 控制台事实与知识治理（本地）

基线 main `293e2bf6af3b180d43c8050731047b552ac07519`，独立分支
`feat/console-governance-p0`。选取旧本地 `be3474b` 的目录过滤和 `e9dc434` 的任务/
409 反馈，重新适配当前 SQLite/AnySeek 正文存储，不搬旧 schema、STATUS 或其它功能。
不改 Run UI、来源核验接口、共享导航/DTO、CI、存储格式或权限。

记忆目录通过 `memory_filter` 与 `Service::list_memories` 精确匹配持久身份：主体、
属性和完整条件集合；未知身份不推断。每次按 scope 主键 seek 最多 100 个候选，另取
1 个 lookahead；在同一读事务筛选、复核正常资产来源/语义可见性，再返回最多 limit 条。
候选游标允许空页续翻，不返回总数，不无限扫描。目录无正文，身份定义及写入不变。
人工审查新增 SQL：单表 `oc_assets`，绑定 tenant/workspace/after；无 JOIN、子查询、
新表/索引、迁移、事务内原生 IO 或中间提交。EXPLAIN 回归确认 SEARCH/INDEX，无 SCAN
或 TEMP B-TREE。此证据不等于生产规模性能验收。

知识编辑的 409 显式保留草稿及原 expected_version，用户可重读当前版本比较；失败、
取消或 superseded 的已知资产任务可重新读取当前已发布元数据，不把任务失败等同资产丢失。
重试/取消仍调用原接口与权限，完成或撤回状态不能由 UI 绕过。

### 自动化结果

Linux，Node 24.22.1 / Rust 1.98.0：

- `npm --prefix ui run check`、`npm --prefix ui test`：构建、19 项通过。
  复用同一 pages fixture，保留 Run/PDF/关联等既有断言；新增筛选续翻与任务/409 子用例。
- `cargo test --locked --test local catalog::`：5 项通过，包含目录 HTTP/Service、
  精确过滤、生产/staging、未知身份、撤回、轮换、跨 workspace、limit 1 翻页、
  101 个未发布候选的空页续翻、非法过滤/上限及既有精确 Run 反查。
- `cargo test --locked --lib memory_identity::` 与加 `--no-default-features`：各 9 项通过。
- `cargo test --locked --test local racing_expected_versions_supersede_the_loser`：1 项通过。
- `cargo test --locked --no-default-features --features sqlite --test local catalog::`：
  3 项 semantic_catalog 通过；HTTP/原生目录测试由 local-storage cfg 排除，不能算复跑。
- `cargo build --locked`、`cargo fmt --all -- --check`、`git diff --check` 通过。
  default / `--no-default-features` / `--no-default-features --features sqlite` 三组
  `cargo clippy --locked --all-targets -- -D warnings` 通过。

### 本地真实浏览器范围

使用当前 main 钉住的 AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`
（Rust 1.99.1 重建），本分支 Origence/Node UI，Chromium 156.0.8078.79 headless。
当前 `tools/anyseek_smoke.py --anyseek-binary <pin binary> --run python3 <fixture>`
包裹三组复用的 Playwright 脚本；各自 fresh SQLite v2/AnySeek 目录、一次性测试 key、
合成文档，无 API 拦截，无付费模型/生产数据。原脚本因记忆正文标签、响应等待条件以及不适用的普通 chunk 高亮假设失败；
修正选择器/精确请求等待，并改用真实 source_span 语义事实后重跑。属性由后台 API
预置（UI 代理不开放管理属性路径），不修改代理白名单，不将普通 chunk 偏移冒充
来源区间；以上失败不作为产品通过证据。

记忆流程通过：reader 目录以真实过滤请求得到 production 25+1 条，已撤回条目不返回；
主体/属性/完整条件、staging、未知身份、空条件与筛选重置均核验；撤回 staging 后重筛为空。
浏览器编辑 production 时身份字段锁定，提交 v2 后同主体 staging 仍为原正文/v1。
另用后台登记属性与真实语义写入产生带非零 source_span 的中文、emoji、组合字符引用：
浏览器点击核验后的 mark 等于权威来源 UTF-8 字节切片，reader 直接读原始来源为 403。
正常显式事实不因本次验证获得额外高亮能力；PDF 页内文本仍独立未完成。

知识流程通过：真实损坏 PDF 更新/重试均失败，v1 仍可读与关键词检索；浏览器旧表单
遭遇真实 expected_version 409，保留草稿并重读 v2；reader 无更新按钮且写入/任务访问拒绝，
writer 不能撤回来源，已完成任务 retry/cancel 为 409，撤回失败来源后不能重试。
Run 只复验不改动：320px 旧 Run 引用在当前 v2 时仍打开固定 v1；撤回清正文、旧引用
再点击/固定 URL 刷新/重开 Run 拒绝旧正文；另一 key 被拒绝及真实事件来源标签，共 7 项通过。

未推送、无新 CI；真实 Windows、实体设备/辅助技术、全部语义状态、PDF 页内高亮、
完整 #76 与外部双客户端/企业任务均未在本路完成。截图、临时数据和凭据不提交。

## 2026-11-17：#82 同步 #85 后解决文档冲突（当前）

从 PR #82 HEAD `74de4655d5c251970be716ad873f8cfdb1ee0a3a` 普通 merge main
`92258fc578bbdb1c21ca949ddbbcfb1729a5813a`（已合入 #85）。冲突只有 STATUS 与本文；
合并保留窄屏/PDF 验证、#85 workspace 自动路由和各批次历史证据。相对该 main，
Rust、API、SQL、Cargo/Node 依赖与 CI 无改动；UI 改动范围仍是 CSS、来源提示和 DOM 回归。

本次 Linux / Node 24.19.0、Rust 1.98.0：

- `npm --prefix ui run check`、`npm --prefix ui test` 通过，**17 项**；使用与锁文件一致的既有依赖。
- `cargo fmt --all -- --check`、默认特性 `cargo clippy --locked --all-targets -- -D warnings`
  与 `git diff --check` 通过。没有重跑完整 Rust 测试集；main 的 435 项结果按其原提交保留。
- 真实 localhost Node UI → Origence → AnySeek **通过**：身份、上传/发布、目录、历史、
  检索、上下文、私有 Run 隔离和来源撤回。后端使用与 main 相同 Rust 源码的生产二进制、
  固定 AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`、独立 fresh 数据目录、
  自动 workspace collection 和一次性服务凭据，不调用真实模型、不拦截或模拟 API。
- Chromium **154.0.8037.57** headless：五页 × 1440/1024/736/375/320 × 浅/深色，
  **50 个布局组合**无整页横向溢出且退出可见；另 **10 个 Run 详情布局**通过。
  320px 触屏模拟中主题切换、调用链路导航、可见按钮高度 ≥44px 和退出后回到登录通过。
- 375px 页面调用真实 PDF source 的来源预览：页码 2 提示、不可核验页内文本说明、
  不生成高亮与授权下载通过；下载 SHA-256 与上传的两页 PDF 原字节一致。
  本轮没有增加真正的 PDF 页内文本展示或高亮，也没有重复历史的全部键盘/撤回复验。

浏览器脚本与一次性数据仅留在临时目录，不提交 token/截图或测试产物。真实 Windows、
实体触屏与屏幕阅读器未执行；本次不宣称 #76 完整验收。远端 CI 以此合并提交为准。

## 2026-11-17：#82 同步最新 main 后的限定复验

原 PR HEAD `6978745ff33fb16dce472f403818ca1db3fa29d3`，普通 merge main
`739be8cc8cf62ba80d8ea598504327e1fd63e24e`。解决 STATUS、本文和 pages fixture
冲突：保留主干 SQLite、固定版本关联 Run、Windows 启动器与 AnySeek 正文存储；
合并 PDF 来源与资产关联两个 fixture 分支及断言。相对 main 无 Rust/API/SQL/CI 改动。
下方历史证据中的缺口分类只适用于各节基线，关联 Run 已由 #83 补齐。

- Node 24.22.1：`npm --prefix ui run check`、`npm --prefix ui test` 通过，17 项。
  npm 首次安装因默认 home cache 不可写失败，改用临时 cache 后 `npm ci` 成功。
- Rust 1.98.0：`cargo build --locked`、`cargo fmt --all -- --check`、default /
  `--no-default-features` / `--no-default-features --features sqlite` 三组
  `cargo clippy --locked --all-targets -- -D warnings` 通过；`git diff --check` 通过。
- `cargo test --locked --test local catalog::` 5 项通过（包含 semantic_catalog），
  `cargo test --locked --test local content_storage::` 4 项通过；覆盖精确版本关联、
  key 隔离/撤回、重开、正文损坏及读取期间权限变化。没有重跑完整 Rust suite。
- 使用合并后重建的 Origence/Node UI、从精确 pin
  `d813b915b7f12e7139ca316adc15699688493196` 重建的真实 AnySeek，独立 fresh
  SQLite v2 目录/collection、合成数据和一次性 key，未调用外部模型。
  复用原 Playwright 脚本，使用当前 `tools/anyseek_smoke.py --anyseek-binary … --run
  python3 …` 分别执行布局、撤回、PDF 三组；无 API 拦截或响应模拟。
- Chromium 156.0.8078.79 headless：五页 × 1440/1024/736/375/320 × 浅/深色，
  50 个布局组合无整页横向溢出、退出可见；小屏触屏模拟按钮 ≥44px，导航 tap 通过。
  桌面键盘导航及 10 个 Run 失败事件键盘选择/焦点组合通过。
- 320px 固定 v1（当前已 v2）、来源撤回、旧 Run 再点击、固定 URL 刷新登录、
  重开 Run 拒绝旧正文、另一 key 拒绝和事件来源标签共 7 项通过。
  375px 真实两页 PDF：第二页定位、不可核验页内文本提示、无假高亮、授权下载
  SHA-256 与原始字节一致，全部通过。

脚本/截图/临时 key 留在本地且不提交；未运行真实 Windows、实体触屏或屏幕阅读器。
本节不宣称 #76 完整验收。远端 CI 以此次 merge 提交的实际 run 为准，不能沿用旧 HEAD 结果。

## 2026-11-17：#85 同步 main 与冲突整合（当前）

将 main `739be8c`（含 #84 Windows 联启与 #86 AnySeek 正文存储）合入 #85。保留
`origence-content-sha256-v1` payload schema、不可变历史内容/来源核验和 workspace 自动路由。
新增正文读取接口向每次查询传递已鉴权 Scope；路由回归用相同 chunk/source ID 在三个
租户/workspace 组合保存不同正文，并核对各自返回值。后端绑定改为
`origence-content-backend-v2`，同时固定正文格式和自动路由；旧绑定拒绝、不转换或覆盖。

Windows 启动器不再要求或转发 `OC_ANYSEEK_COLLECTION`，改用携带服务凭据的
`/api/v1/live` 验证版本与服务凭据；Origence 继续检查已发布集合。初始化文档移除旧
租户注册/Key 签发/手工建集合请求。测试断言正确探测路径、服务凭据、进程环境隔离、
退出清理和失败传播；未放宽端口、目录、输出抑制和进程所有权约束。

Linux / Rust 1.98.0；AnySeek 正常生产构建固定 `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`
（Rust 1.99.1）；不使用真实用户数据或模型服务。

| 检查 | 结果 |
| --- | --- |
| 默认完整回归 | PASS：222 lib + 5 协议 + 14 core + 194 local = 435 项；8 项真实契约另执行 |
| 核心库 / 独立 SQLite authority | PASS：87 / 233 项；5 项真实契约 ignored 不计入通过 |
| Clippy | PASS：default、no-default、no-default + sqlite 三组 all-targets，均 `-D warnings` |
| 格式/补丁 | PASS：`cargo fmt --check`、`git diff --check` |
| 真实 AnySeek | PASS：8 项契约，HTTP/stdio MCP、历史版本/Run 重放与 AnySeek 进程重启后正文/索引一致 |
| 真实 HTTP / Node UI | PASS：两套 smoke，包含上传、发布、检索、上下文、历史、私有 Run 和来源撤回 |
| Linux PowerShell 7.4.13 | PASS：7 项模拟子服务/进程合同回归；官方 release SHA-256 核验后运行 |

本次冲突处理没有新增 SQL 或依赖；保留 main 的单表事务、异步 IO 边界与权限/来源复核。
新增当前记录，以下各批次证据保留原范围。Linux PowerShell 不等于真实 Windows 控制台
信号/原生 MSVC 验收；Windows/Linux 远端 CI 以本次合并提交记录为准。

## 2026-11-17：workspace 自动映射（同步 main 前）

`AnySeekConfig::new(base, service_key)` 只接收连接与服务凭据。移除租户和集合配置；
已鉴权 Scope 的 tenant UUID 直接作为命名空间，workspace 对应 `ws_<无连字符 UUID>`。
服务启动无占位集合；首次使用创建固定 schema，已发布集合在启动时严格核验，丢失不补建。
绑定 v6 保存 endpoint/路由规则/模型及首次使用时持久化的分析摘要，不保存凭据。

Linux / Rust 1.98.0；真实引擎为 AnySeek
[`6a46fc7`](https://github.com/TheLudlows/anyseek/commit/6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a)
正常生产构建（Rust 1.99.1，无 fault-injection）。CI 夹具固定同一提交。

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked --no-fail-fast` | PASS：431 项，另 8 项真实引擎契约 ignored 后单独执行 |
| `cargo test --locked --no-default-features` | PASS：87 项，另 5 项真实引擎契约 ignored，不计通过 |
| 默认与无默认特性 Clippy | PASS：`cargo clippy --locked [--no-default-features] --all-targets -- -D warnings` |
| 格式与补丁 | PASS：`cargo fmt --check`、`git diff --check` |
| 真实 AnySeek 契约 | PASS：8 项；全文/向量/图、发布重试、响应丢失恢复与 public Service 重开 |
| HTTP / stdio MCP | PASS：搜索、读取、引用、更新、历史、用户 key 撤销、Run 诊断与进程重开 |
| `tools/smoke.py` 经真实 AnySeek | PASS：公开 HTTP、身份范围、发布、检索与重开 |
| Node UI → Origence → 真实 AnySeek | PASS：上传、发布、目录、历史、检索、上下文、私有 Run 隔离和来源撤回 |

协议回归覆盖数字开头 tenant UUID、同租户不同 workspace、不同租户复用 workspace UUID、
并发初始化、创建响应丢失后严格回读、已发布集合丢失后不重建、分析摘要漂移与凭据不落盘。
夹具按命名空间/集合限制数据，真实验收不再预创建集合。AnySeek 未知 scoped ID 返回 403；
客户端仅在带服务凭据的命名空间列表确认缺失后创建，不把任意权限错误当作缺失。

新增的已发布 workspace 枚举复用原有有界单表读取与同事务来源核验，没有新增 SQL 语句、
JOIN 或表；事务结束后再访问远端集合。此前一次离线 CLI 夹具直接注入投影、绕过首次绑定，
现先通过公开发布路径初始化；完整回归及真实验收均在修正后重跑。
本轮只验证独立新库及同版本重启/恢复；未执行 Windows 本地构建、真实云 S3 或真实模型质量验收。
以下保留此前提交的验证记录，其显式 namespace/collection 配置已由本节自动路由替代。

## 2026-11-17：AnySeek 服务凭据与显式租户命名空间

基线 main `c70a1aa`。`AnySeekConfig::new` 显式接收服务凭据、命名空间和 collection；
环境新增必填 `OC_ANYSEEK_TENANT`。请求统一发送 `apikey` 与 `X-AnySeek-Tenant`，
命名空间与集合名均验证为 AnySeek 的小写路径组成部分，服务凭据仍标记为敏感且不持久化。
后端绑定 v5 固定命名空间，改命名空间在远端 IO 前拒绝且保留原绑定；同一后端的服务
凭据轮换不改变绑定。Origence 的业务 UUID scope、用户逻辑 key、私有会话和权限复核不变。

Linux / Rust 1.98.0，匹配 AnySeek Catalog 7 正常生产构建（Rust 1.99.1，无 fault-injection），
最终 pin [`800e3f1`](https://github.com/TheLudlows/anyseek/commit/800e3f1c2862c72d7de6afecde1ab921cef24671)。
全部 real-engine 夹具使用 localhost、独立临时数据根/命名空间/collection 和一次性服务凭据，
不调用真实模型，不访问外部 S3 或用户服务。

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked --no-fail-fast` | PASS：431 项；另有 8 项真实引擎契约显式 ignored，随后单独执行 |
| `cargo test --locked --no-default-features` | PASS：88 项；另有 5 项真实引擎契约 ignored，不计通过 |
| 真实 AnySeek 契约 | PASS：8 项，含全文/向量/图、丢失响应、发布核验、恢复及 public Service |
| 真实 HTTP / stdio MCP | PASS：搜索、读取、引用、更新、历史、Origence 用户 key 撤销、重开及 Run 诊断重放 |
| `tools/smoke.py` 经真实 AnySeek | PASS：公开 HTTP 路径与进程重开 |
| Node UI → Origence → 真实 AnySeek | PASS：身份、上传、发布、目录、历史、搜索、上下文、私有 Run 隔离和来源撤回 |
| 默认及无默认特性 Clippy | PASS：`cargo clippy --locked [--no-default-features] --all-targets -- -D warnings` |
| `cargo fmt --check`、all-targets check、正常服务构建、`git diff --check` | PASS |
| Node 24 UI | PASS：`npm --prefix ui run check`、`npm --prefix ui test`（16 项） |

协议夹具也强制服务凭据和命名空间，防止 mock 忽略新增请求头。新增回归验证命名空间
格式、后端绑定隔离、绑定文件保持原样，以及服务凭据不进入绑定。没有新增 SQL 或查询
执行入口；变更仅在 HTTP 适配、配置、已有绑定和验收夹具内，原 JOIN-free 约束保持。

首次后续库测试构建因编译缓存耗尽 workspace 磁盘失败；保留失败记录后仅清理可重建的
Cargo incremental 缓存，设置 `CARGO_INCREMENTAL=0`，以上各检查均在重跑后通过。
未改写数据目录。未执行 Windows、真实外部 Agent/模型、生产容量、跨主机或断电验收。
Node UI HTTP smoke 不等同于完整真实浏览器/无障碍验收；已有历史浏览器证据仍按各自提交解释。

## 2026-11-17：#86 检视后删除无用元数据与分词回退

从 PR #86 的 `70c7281` 继续，已合并 main `8fe01a5`（#84 Windows 开发启动器）。
人工核对全部调用链后删除 `oc_chunks.search_terms`：候选读取只消费 ID，旧实现仅传递词项
再丢弃；删除 `oc_summaries.model_revision`：没有 SQL 读取方，模型来源已保存在 AnySeek payload。
同步删除对应写入参数、SELECT/解码、候选 tuple 字段，以及目录对不存在 content 字段的删除。

所有发布类型改为一次性在事务外 blocking 任务中冻结正文/摘要词项；移除可空分块词项、
按类型分流和恢复时重新分词的回退，摘要分词也不再发生在发布事务中。摘要召回词项、
原始来源、locator、owner/index/ledger 和完整恢复计划仍有实际消费者，保持原有校验。
既有历史验证记录不改写；schema 仍按该 PR 的 fresh-only 范围开发，不自动转换旧布局。

本地 Linux / Rust 1.98.0：default **433 passed**（221 lib + 4 协议 + 14 core + 194 local）；
pure-core **87 passed**；独立 sqlite authority **233 passed**。三组 Clippy 均 warnings denied
通过，fmt/diff 检查通过。新增损坏冻结词项后的重试拒绝回归，证明不会重新分词或发布；
列缺失断言覆盖新增删除项，既有来源、历史、跨 scope、取消、重试与 private Run/Skill 测试通过。

真实 AnySeek d813b91 的 **8 项契约通过**；真实 HTTP/stdio MCP、历史读取、Run 诊断、
重开以及 AnySeek 进程重启后当前/历史正文和索引检查通过。最新 Windows/Linux CI 以 PR #86
的当前提交运行记录为准；生产性能、模型质量、浏览器布局等范围未由本轮增加验收。

## 2026-11-17：正文与索引共用 AnySeek collection

基线 main `c70a1aa`，分支 `refactor/anyseek-content`，Rust 1.98.0，Linux；AnySeek
固定为 `d813b915b7f12e7139ca316adc15699688493196`（已有 Rust 1.99.1 夹具二进制）。
本节记录该分支的本地工作树，后续 Windows/Linux 精确提交 CI 状态以 PR 为准。
只使用合成数据、临时目录/集合与一次性凭据；没有迁移或改写既有用户数据。

- `cargo fmt --check`、`git diff --check` 与 default / pure-core / sqlite 三组
  `cargo clippy --locked --all-targets -- -D warnings`（后两组分别加对应 feature 参数）通过。
- `cargo test --locked --no-fail-fast`：221 lib + 4 AnySeek 协议 + 14 core + 193 local
  = **432 passed**；8 项真实引擎契约默认忽略、在下一项单独执行。无 doctest。
- `cargo test --locked --no-default-features`：71 lib + 2 协议 + 14 core = **87 passed**。
  `cargo test --locked --no-default-features --features sqlite`：138 lib + 2 协议 + 14 core
  + 79 local = **233 passed**；纯核心不引入 SQLite，Cargo.toml/Cargo.lock 未新增依赖。
- `tools/anyseek_smoke.py --cargo-tests --binary target/debug/origence`：**8 项真实 AnySeek
  契约通过**。新增原始 UTF-8、CRLF、摘要/模型版本、locator 随 payload 保存与读取断言。
  正常 HTTP/stdio MCP 的查询、版本更新、历史读取、撤销 key、Run 诊断与重开检查通过。
- 同脚本 `--binary` 新增真实 AnySeek 进程正常停机/重启，再启动 Origence 读取当前/历史正文
  并检索，**通过**。不宣称任意断电或跨存储在线一致快照。
- 真实 AnySeek 包裹 `tools/smoke.py` 与 `tools/ui_smoke.py`：HTTP 与 Node UI → Origence
  → AnySeek 全链路通过，包含上传、搜索、Context、版本历史、私有 Run 和来源撤回。
  `ui/npm run build` 与 `ui/npm test`：**16 passed**；未新增浏览器布局/无障碍验收。

新增 `content_storage` 回归证明：正文与索引属于同一 Record，三处旧正文/摘要列已移除，
固定历史版本与恢复在 Origence 重开后保留原字节；缺失/篡改 payload 明确失败；GET、Search、
Context 在读取期间撤销 key 或撤回来源均拒绝返回。既有 PDF、语义集合、Learning、Skill、
私有 Run、幂等、丢响应恢复、跨 scope 与 SQL 事务/取消回归继续通过。

人工逐条审查新增/改动执行 SQL（含 fixture、DDL 与 format 展开）：只读单表或原有关联适配器，
无 JOIN/关联子查询，无事务内远端 IO、无中间提交。SQL 仍保存原始来源、locator、冻结召回词项、
完整恢复计划及私有快照；只移出已发布正文/摘要文本，不宣称清除所有文本副本。
格式 `origence-sqlite-v2` / `origence-content-sha256-v1` 面向 fresh 数据；旧格式拒绝，不自动转换。
生产相关性、容量、延迟、模型质量与在线备份恢复不由上述回归证明。

## 2026-11-17：#76 精确版本关联 Run（独立本地切片）

基线 main `593e8fa71d7c354587cffdd1aae7c4b2e2d2a554`；独立分支，不包含第一批
`fix/issue76-browser-coverage` 的改动。新增只读 `GET /v1/assets/{id}/runs?version=N`，
限制当前逻辑 key、精确版本及 1–100 候选，UUID 游标。新表只是首条已记录引用的定位投影，
引用事件仍为权威：客户端上报和服务端准备在原事务内更新，同快照读取资产、来源、owner
并重新核验事件中的精确引用；不扫描所有 Run，不返回私有正文。不可读 proof 保守省略，
不宣称完整关联/采集，具体边界见 Run 合同。所有新增 SQL 已逐条审查，均为单表操作，无 JOIN。

真实 localhost Node UI → Origence → AnySeek，fresh v4 合成数据/一次性 key、无模型。
AnySeek pin `d813b915b7f12e7139ca316adc15699688493196`；Chromium 156.0.8078.79，
1440×1000 headless，Playwright 实点无响应拦截：Run → 固定 v1 → 已记录关联 → 原 Run；
当前 v2 关联为空；同 workspace 另一管理员关联为空且直接 Run 被拒绝；来源撤回后重新读取
关联会清除正文和旧链接；旧引用重点击、刷新登录和重新打开 Run 均拒绝旧证据。
共 10 项真实浏览器断言通过。此切片没有重复五页响应式验收，也未运行真实外部客户端。

本地 Node 24.22.1 `npm --prefix ui run check`、`npm --prefix ui test` PASS（构建/16 项），
现有 DOM fixture 仅增加精确版本关联往返子用例。Rust 1.98.0：
- `cargo test --test local catalog::` PASS（5 项，含既有 semantic_catalog 3 项）；
  新用例验证 v1/v2、同 workspace 另一管理员隔离、轮换、幂等去重、游标/上限、跨 workspace、
  重开及来源撤回。增加权威引用缺失的空页游标断言后，精确重跑该用例 PASS。
- `cargo test --test local workspace_schema_rejection::` PASS（5 项）；
  `cargo test --no-default-features --test local workspace_schema_rejection::` PASS（5 项）。
  缺失新表时打开被拒绝且数据库字节不变。
- `cargo test --test local retrieval_diagnostics::run_diagnostics_are_private_durable_and_replay_original_evidence` PASS（1 项）。
- `cargo fmt --check`、默认/no-default `cargo clippy --all-targets -- -D warnings`、
  `git diff --check` PASS。Windows 未跑；没有新 CI（未推送）。
不推送、不创建 PR、不关闭 #76；生产计量、告警、PDF 页内文本及完整外部验收仍缺。

## 2026-11-17：#76 第一批响应式与 PDF 来源验收（本地 checkpoint）

基线 main `593e8fa71d7c354587cffdd1aae7c4b2e2d2a554`（#80 已合入）。
修复真实浏览器复现的 panel grid 最小内容宽度：320px Run 目录原为 350px、
320/375px 知识目录原为 430px；现在表格在容器内滚动，不撑开整页。
小屏保留账户退出入口；粗指针（包括混合输入设备）链接与主题按钮达到 44px 高度。
PDF 来源展示明确区分页码/页内字节范围、缺失的可核验页内文本、原始文件下载，
不展示来源事件的空正文或将其伪装成页内高亮。没有修改 API、权限或存储。

本地 Node 24.22.1 `npm --prefix ui run check`、`npm --prefix ui test` PASS（构建与 16 项）。
唯一新增 DOM 子用例验证 PDF 页码、缺失文本说明、下载入口及不生成错误高亮。
Rust 1.98.0 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、
`cargo clippy --all-targets --no-default-features -- -D warnings`、`git diff --check` PASS。

真实 localhost Node UI → Origence → AnySeek，fresh v4 合成数据、一次性 key、模型禁用；
引擎 pin `d813b915b7f12e7139ca316adc15699688493196`。Playwright 驱动系统
Chromium 156.0.8078.79（headless），不拦截 API。50 个五页布局组合：
1440/1024/736/375/320 × 1000、浅/深色；各页 document.scrollWidth 等于 viewport，
退出入口可见。736/375/320 使用触屏上下文与真实 tap；每页通过 CDP 重申触屏模拟，
断言 any-pointer:coarse 为真后测量所有可见按钮高度 ≥44px（初版脚本中浏览器会重置
输入模拟，不能把该次鼠标状态算作触屏结果）。桌面导航可用键盘 Enter；10 个 Run
详情组合可用键盘选择真实准备失败事件且焦点 outline=solid，显示 server-observed/not-prepared。
这不是物理触屏、屏幕阅读器或所有控件键盘顺序的完整验收。

另一次独立 fresh v4 流程上传带真实 xref 的两页合成 PDF，等待真实入库完成；375px
浏览器关键词检索第二页并点击核验：locator page=2、parser=pdf-oxide-0.3.78-v1，
显示页内文本不可用，无 mark；点击授权下载，SHA-256 与上传 PDF 原始字节一致。
截图留在本地验证产物，没有纳入仓库。实际来源事件 API 不返回提取后的页内文本，
故页内文本高亮仍未完成；不能对拼接后的整份资产正文套用页内偏移。

Checkpoint `78d39f57017860c0ceccb53b38b4f72cfe958def` 后另跑真实 320×1000 浏览器：
Run 引用在当前资产 v2 时仍打开固定 v1；撤回来源立即清除正文；预先打开的旧 Run
再次点击、固定 v1 URL 刷新后重新登录、重新打开原 Run 均拒绝旧内容；另一 logical key
读取 Run 被拒绝。真实失败标签与客户端上报标签同时通过。所有 7 个断言通过。

### #76 未勾选条目的证据分类

| 条目 | 当前状态与边界 |
| --- | --- |
| 对象/列表/聚合/告警合同 | 部分：资产目录和当前 logical key Run 目录存在；聚合、告警与反查合同缺失 |
| Memory 身份及状态查询 | 部分：现有目录/检索/显式身份入口和语义状态守卫；完整筛选与真实状态矩阵未验收 |
| Knowledge 上传/旧版保留/来源/恢复 | 部分：当前真实 PDF 上传、任务发布、来源下载已验；全部格式与更新失败/恢复矩阵未重跑 |
| expected_version/409/重复/任务/角色 | 部分：既有 DOM 覆盖重试身份，服务端合同存在；全角色真实浏览器并发/任务矩阵未验收 |
| 固定版本/PDF/UTF-8 | 部分：上节已有真实固定 v1/撤回证据，本批 PDF 第二页与原始下载通过；UTF-8 中日韩/emoji 边界 UT 通过，PDF 可核验页内文本 API 缺失 |
| 真实预算/Token/成本 | 部分：实际 UTF-8 字节预算已显示；没有可信 Token/价格合同，不推算费用 |
| key/角色/撤回/过期引用 | 部分：上节真实另一 key 拒绝、固定引用撤回已验；全部角色/过期组合未完成 |
| 键盘/触屏/主题/尺寸 | 部分：本批 50 布局、触屏按钮/导航和 10 Run 键盘组合通过；完整焦点顺序、辅助技术、实体设备未验收 |
| 冻结真实客户端/企业任务 | 外部验收：未运行真实 Claude Code/Codex 客户端或企业任务 |

下一批后端合同的源码核对：单次准备诊断最多记录 128 次查询观察并带省略数，不能
当完整 Run 成功率/P95；没有可信生命周期、Token、价格、聚合窗口及告警状态合同。
资产→Run 没有反向接口/索引，不能用跨 key 枚举或前端缓存补齐；需按 owner key 有界查询、
精确版本与同快照来源复核，返回覆盖/游标。PDF 可核验页内文本需按不可变版本读取并
验证页内 chunks 的连续 UTF-8 范围、parser 和来源权限，不能用当前解析器重解释旧偏移。
这些合同未在本批臆造实现，#76 仍未完成。

## 2026-11-19 — #76 窄屏触控靶区与交互（Chromium 模拟）

基于 main `2ad69260522676ecf70f9cd86fc3e3e47d453c8a`。在 Chromium 的粗指针触屏模拟中，
320 / 375 / 736 CSS px 下的「退出」原为 38×44 px，上下文来源展开控件只有约 24 px 高。
修复仅在粗指针条件下生效：按钮最小 44×44 px，原生来源展开控件最小高度 44 px，
保留原有展开标记。私有会话选框虽小，包裹它的 47 px 整行标签可点按，未修改。

Linux、Node 24.22.3、Playwright 1.57.0 / Chromium 153.0.8010.0。构建后的 UI 经真实
Node relay 对接 loopback 合成 HTTP。三种宽度（浅 / 深 / 浅色）原尺寸断言均失败、修复后
通过。使用浏览器 tap 验证新增知识对话框 Close 后焦点归还、重新打开不保留草稿；
私有会话整行边缘可反复切换，提交时如实带上 session；来源可从目标边缘展开/收起，
固定 v1 链接和路由焦点正确；最后点按退出清空私有界面。检查页面无整页横向溢出。
既有导航键盘/焦点与上下文知识原文 Chromium 回归、UI 类型检查及 21 项 Node 测试通过。

```sh
npm --prefix ui run build
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-narrow-touch-browser.cjs
```

这不是实体触屏、真实屏幕阅读器或完整 #76 验收。未运行真实客户端、模型、试点或部署。

## 2026-11-17：Run 事件来源标签与真实浏览器定向验收（本地）

基线 main `01d4f82b20c986278b077e8660f9bbc0538e8d6c`。仅修改 UI 事件标签及详情：
`server_prepared` 显示服务端准备成功，明确不证明收到/使用；`server_observed` 的
`context_preparation_failed` 显示服务端观察到准备失败/未准备上下文；`reported/client`
显示客户端上报，其他或未知来源不推定为客户端。API、权限与后端语义不变。
现有 pages fixture 扩充失败/客户端/未知来源，校验时间线和详情，没有新增重复测试套件。

本地 Node 24.22.1：`cd ui && npm run check && npm test` PASS（构建及 15 项）。
Origence Rust 1.98.0 `cargo build --locked`、`cargo fmt --package origence -- --check`、
`cargo clippy --locked --all-targets -- -D warnings` 和 `git diff --check` PASS。

真实浏览器：Playwright 驱动系统 Chromium **156.0.8078.79**，headless、1440×1000，
直接访问 localhost 上的真实 Node UI → Origence → AnySeek。AnySeek 固定当前 main pin
`d813b915b7f12e7139ca316adc15699688493196`；fresh v4 临时目录、一次性 key、无外部模型。
API 只预置资产/Run/客户端事件：先发布 v1 并准备 Run，再发布同一资产 v2；用禁用模型的
vector 准备请求产生真实 503 和服务端失败事件。未拦截或伪造浏览器响应。

| 浏览器操作 | 观察结果 |
| --- | --- |
| 打开 Run，逐条点击成功、失败及客户端事件 | 来源 badge 正确；失败详情含 server / not_prepared，成功明确未确认收到或使用 |
| 另一个 key 打开该 Run | 拒绝读取，未出现原正文 |
| 当前资产已为 v2，点击 Run 中 v1 引用 | URL 带 `?version=1`；标题/正文为 v1，不含 v2 的独立标记 |
| 在 v1 详情点击“撤回来源”并确认 | 显示“内容已阻断”，旧正文清除 |
| 撤回后在预先打开的 Run 标签页再次点击旧 v1 引用 | 显示“暂时无法读取”及 resource not found，不返回旧正文 |
| 刷新固定 v1 地址并重新登录；再次打开原 Run | 拒绝读取，不再显示旧正文 |

复核可在新建 workspace 中按上表重复：先准备 v1 的 Run、更新至 v2，再在浏览器
点击引用并撤回来源，分别检查已打开标签页、刷新后页面与另一 key 的访问。
截图及本地驱动脚本不纳入 PR；测试凭据不保存，结束时服务关闭、临时数据删除。

这些是单次指定路径的真实浏览器结果，不是纯 HTTP 或 happy-dom 替代验收；
未覆盖移动端、全站可访问性、所有浏览器、真实客户端采集、计量/告警或反向关联，
不表示 issue #76 完整完成；本节仅记录本地验证，不代表 CI、合并或部署。

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
历史结果以链接中的原始工件及其标明的提交和后端为准。当前进度见 [STATUS](STATUS.md)。

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

## 2026-11-17：核心库无元数据库与按需访问（PR #81）

在 SQLite 替换基线 `01c0d4c` 上同步 main `593e8fa`，保留 #80 的 Run 来源界面修正。
以下为本轮工作树结果，精确提交和新的 Windows/Linux CI 由 PR head/运行链接标识；
不把上一轮 #372 的成功或容器镜像外推为本轮结果。

本轮减少实际依赖和访问：

- `rusqlite` / `fs2` 只由可选 `sqlite` feature 启用。关闭默认特性的核心库不包含 SQLite
  实现或错误类型；完整宿主 `local-storage` 自动启用 SQLite，单独 authority 显式启用它。
- 分离始终可运行的 Blob/PDF 核心集成测试，PDF 字节夹具由核心与宿主共用；
  原有测试没有删除或同时重复执行。手工诊断按其实际调用者依赖 `sqlite`。
- 服务入口复用同一事务中的身份读取完成权限判断；提交仍重新检查权限。关键词候选与
  入口授权共用快照，外部 IO 前关闭它；向量/混合模式保留模型前后的独立授权快照。
- 空 read set、关联、空摘要查询不准备无关表；Learning 缺失或前置条件已否定、父任务
  已取消时停止无关查询。必需数据与非空证据的失败仍拒绝操作，完整 schema 在开库和
  显式健康检查执行。更新的是空操作的消费边界，没有压制必需查询的错误。
- 纯向量请求不调用词法流程；其他查询的分词在 blocking 池、事务外执行。

Linux Rust 1.98.0 已通过：

| 配置 / 命令 | 结果 |
| --- | --- |
| `cargo test --locked --no-default-features -j 2 --no-fail-fast` | 71 lib + 2 协议 + 14 core = **87 项**；5 项真实引擎检查按原设置 ignored |
| `cargo test --locked --no-default-features --features sqlite -j 2 --no-fail-fast` | 138 lib + 2 协议 + 14 core + 78 local = **232 项**；5 项 ignored |
| `cargo test --locked -j 2 --no-fail-fast` | 221 lib + 4 协议 + 14 core + 188 local = **427 项**；8 项 ignored |
| 三种配置 all-targets Clippy（完整配置含 all-features） | warnings denied，全部通过 |
| 正常服务 build、fmt、diff、actionlint 1.7.7、Cargo metadata | 通过 |
| `cargo tree --locked --no-default-features --edges normal,build --prefix none` | 无 rusqlite、libsqlite3-sys、fs2；CI 增加相同门禁 |
| Node UI | 15 项通过，包含合入 #80 后的来源区分断言 |

新增资源回归使用各测试库独立、仅编入单测的计数，不记录生产元数据，不使用耗时阈值：
空读取集合在没有业务表的库上产生 **0 次客户端 driver 调用**；真正请求证据仍报存储错误。
关键词空语料请求只需 **3 个授权读事务**（此前 4 个），client driver 调用上限 20；
此路径无需索引/Summary 表或可访问的 AnySeek，但显式 `check` 仍拒绝不完整 schema。
事务入口只需 BEGIN 和 key/workspace 两次读取（3 次 driver 调用），没有重复身份查询；
同事务撤销自身 key 后提交被拒绝并回滚，外部撤销后旧授权对象也不能开启新事务。

固定真实 AnySeek `d813b915` 上，正常服务二进制通过 HTTP、Node UI、完整 MCP
host-forwarding/offline smoke；另跑 8 项真实后端合同及其 HTTP/stdio、撤销、重开与
Run 诊断持久化验收，通过。全部新建临时数据，不调用外部模型。

手工 SQL 复核：非空查询的参数化单表 SQL 保持不变；关联仍在同一适配器事务中完成。
仅减少空批次、已确定结果的分支和入口重复授权读取，不增加 JOIN、额外提交、
跨请求权限缓存、事务内模型/文件/AnySeek IO 或业务元数据。DDL/格式未改变。
本轮未重复容器 release 构建；既有 SQLite 容器互操作证据按原提交保留。远端结果见 PR。

## 2026-11-17 · PR #83 synchronization with merged SQLite #81

Merged main `f62168b4cbedfbe6a3a97dbb3161f9e6375fb2df` into the existing #83
head `537fdbd982740c1283f3ef0e01dc47ef6a651347` without rewriting history.
Coordinated Run storage with SqliteTx/sqlite_db/sqlite_err and updated the tests'
old driver paths. Client events and server-prepared events both retain the reverse
projection insert in their existing write transaction. Fresh-only schema admission,
first-reference coverage and current-logical-key/source guards are unchanged.

Promoted the narrowly scoped combination diagnostic into the existing catalog
regression: an injected projection-insert failure rolls back the client event and
server-prepared event, then retrying the same server event ID succeeds and yields
the exact v2 association. Withdrawing independent v1 leaves that v2 association
visible. This proves ordinary transaction failure handling, not power-loss durability.

Executed on the actual synchronized worktree, Linux / Rust 1.98.0 / Node 24.22.1:

- `cargo test --locked --test local asset_run_references_are_exact_bounded_private_and_withdrawal_gated`:
  1 passed, 189 filtered (fixed version, other-admin/key/workspace isolation,
  rotation, pagination/limits, authoritative proof recheck, reopen, withdrawal,
  client/prepared projection rollback and successful retry).
- `cargo test --locked --test local workspace_schema_rejection`:
  5 passed, 185 filtered.
- `cargo test --locked --no-default-features --features sqlite --test local workspace_schema_rejection`:
  5 passed, 74 filtered; includes missing reverse table and old-layout rejection
  without repair. These integration tests use the existing protocol peer for
  AnySeek and are not real-engine ranking acceptance.
- `cargo build --locked`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo clippy --locked --no-default-features --features sqlite --all-targets -- -D warnings`: passed.
- `cargo clippy --locked --no-default-features --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`, `git diff --check`: passed.
- `npm --prefix ui run check`; `npm --prefix ui test`: passed, 16 UI tests.

Real AnySeek pin `d813b915b7f12e7139ca316adc15699688493196`, models disabled,
fresh temporary v4 data, disposable keys, rebuilt synchronized Origence binary:

- `python3 tools/anyseek_smoke.py --anyseek-binary "$ANYSEEK_BINARY" --run python3 tools/mcp_smoke.py --binary "$ORIGENCE_BINARY"`:
  passed both host-forwarding and offline MCP (protocol/schema/tools, safe errors,
  live/invalid/revoked credentials, EOF and reopen).
- Reused the real Chromium fixture via the same AnySeek launcher with the
  synchronized UI/server. Chromium 156.0.8078.79 passed Run -> fixed v1 after v2
  publication -> recorded association -> same Run; v2 and another same-workspace
  admin cannot enumerate v1-owned associations. Browser source withdrawal,
  association refresh, stale citation re-click, URL reload and Run reopen all
  deny old content; server preparation failure remains server-observed/not-prepared.
  No route interception, production data, paid models, screenshots or credentials
  were added to this commit.

Windows and full combined CI were not run locally. GitHub results must be checked
on the new pushed HEAD; the old separate PR green runs and the earlier temporary
combination tree are not evidence that this new HEAD passed remote CI. No complete
#76/native Claude Code/Codex task or production acceptance is claimed.

## 2026-11-17 · Windows-native development launcher (local)

Independent branch based on main `c70a1aaa0b3cb75872109406dc17b1b461d1f771`
(includes SQLite #81 and reverse references #83); no older feature branches included.
`tools/start-dev.ps1` targets foreground PowerShell 7.4+ and prebuilt native executables.
It requires already provisioned separate data directories, ordinary Tenant key and
collection; no startup provisioning, migrations, directory creation, overwrite,
remote writes, secret arguments or persistent logs. Loopback only, models disabled.
AnySeek readiness reads the authorized collection metadata; Origence's own full
backend admission and /health/ready gate the optional UI. Startup/HTTP/grace waits
are bounded. Only owned Process handles are stopped; no process-name/port killing.

Commands and results:

- PowerShell 7.4.6 Linux parser check of start-dev.ps1: passed. Initial pwsh startup
  could not create its default cache on the read-only home; XDG_CACHE_HOME,
  XDG_CONFIG_HOME and XDG_DATA_HOME were set to disposable paths for validation.
- `ORIGENCE_TEST_PWSH=/path/to/pwsh python3 -m unittest discover -s tools -p test_start_dev.py -v`:
  7 passed on Linux pwsh. Fake HTTP subprocesses verify ready/API/UI ordering,
  early exit, not-ready timeout, auth denial, redirect refusal, missing-data refusal,
  occupied-port noninterference, owned-process cleanup, spaced argument paths,
  credential separation and suppression of deliberately secret-bearing child output.
  These are process/HTTP contract fixtures, not real backend or Windows results.
- `python3 -m unittest discover -s tools -p 'test_*.py'`: 11 passed, 7 opt-in launcher
  cases skipped without ORIGENCE_TEST_PWSH. No CI scope/configuration changed.
- `cargo build --locked`, `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `git diff --check`: passed under Rust 1.98.0.
- `npm ci --ignore-scripts --prefix ui` (disposable cache), `npm --prefix ui run build`:
  passed with Node 24.22.1.

Real local validation used pwsh 7.4.6 on Linux, actual AnySeek built at
`d813b915b7f12e7139ca316adc15699688493196`, Origence rebuilt from this worktree,
and the built Node UI. A separate fixture explicitly created a temporary Tenant,
ordinary key/collection and offline workspace, stopped bootstrap AnySeek, then
invoked the unmodified launcher with paths containing spaces. Twice it observed
all readiness gates, authenticated /v1/whoami retaining the same workspace, UI
HTTP 200, group SIGINT shutdown without force warnings, closed ports and retained
context.db; the second start reused the first data. Captured parent output was
checked not to contain master secret, master key, ordinary key or workspace token.
No production data, paid model/provider calls, Windows emulation or WSL was used.
The local real-service harness is external to the repository; the reproducible
manual setup/commands and Windows checklist are in WINDOWS-DEV.md.

Validation found and corrected a PowerShell case-insensitive variable collision
that replaced the component display name with an environment variable name.
The real bootstrap fixture also initially precreated an empty AnySeek directory:
the pinned engine interpreted that as recovery and exited. Documentation now uses
an absent path for explicit first initialization, while the launcher requires an
existing nonempty initialized directory. No automatic repair was added.

Not executed: native Windows/MSVC/Windows Terminal Ctrl+C, ready-stage interruption
on Windows, Windows process-reuse races, production recovery or power-loss tests.
Shared-console Ctrl+C is best effort until verified there. If signal delivery fails
or grace expires, owned-process termination is explicitly a forced stop; closing
or killing the parent terminal may bypass finally and leave children alive. The
launcher is not a production service supervisor. No remote PR/push/deployment.

## 2026-11-17 — #87 minimal MCP Run host adapter (local only)

Base and read-only latest-main check: `293e2bf6af3b180d43c8050731047b552ac07519`.
Independent branch `p0/mcp-host-adapter`, worktree `/workspace/origence-mcp`.
Read repository AGENTS.md, current status/design, Run contract and client/API
implementation; no repository `.agents/skills` exists and workspace `.agents`
is empty. Interface and two-host observation boundaries are frozen in
[the host contract example](../examples/run-host-contract.md).

Four MCP tools delegate through typed HostClient methods to existing Run HTTP
routes. Shared DTOs, Run storage, authentication, SQL, navigation, CI and STATUS
are unchanged. Manual execution-input review found no added SQL or storage calls;
all authority/source/version/replay decisions stay on the existing HTTP path.
Embedded local MCP explicitly rejects these new Run operations with a server-URL
requirement; the documented remote CLI/MCP path is covered.

Linux, Rust `1.98.0 (88d9e12ae 2026-08-18)`, rmcp `1.8.0`, models disabled,
fresh temporary SQLite, synthetic content and loopback AnySeek protocol fixture:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test --test local mcp_runs -- --nocapture`: 1 passed.
- `cargo test --test local foundation_http -- --nocapture`: 1 passed.
- `cargo test --test local retrieval_diagnostics -- --nocapture`: 2 passed.
- `git diff --check`: passed.

The new test launches two actual Origence stdio processes and verifies tool
listing, same-logical-key archive and turn readback, independent Run IDs, other
writer/admin denial, unchanged retries and changed-payload conflicts, all seven
existing report variants, explicit unknown, rejection of forged server events,
HTTP preparation success/replay/failure, source withdrawal, credential rotation
and revocation of the replacement credential on an already-open MCP connection.
Initial test failure was a fixture mistake: it revoked the old credential ID
after rotation. The corrected test revokes the returned new credential ID;
server key semantics were not changed. Final rerun passed.

These synthetic peers are not Claude Code/Codex task evidence. Claude Code is
not installed in this environment; no real host/model task was invoked. Actual
two-host/version-frozen task observations, native Windows and enterprise task
outcomes remain unaccepted. No production credentials/data, paid model calls,
migration, push, PR, Issue update, merge or deployment was performed.

### #87 publication follow-up — explicit HTTP feedback/Improve and MCP gates

Continues local implementation `3877322753ff8bd2b921f5fe1a43e6a33352b108`.
Read #87 and #89: the four Run tools do not replace explicit turn archival,
session/evidence feedback or Improve, and do not add independent verification.
[Runnable synthetic HTTP commands](../examples/run-host-contract.md#explicit-archive--feedback--improve-over-existing-http)
use the same logical-key session and preserve explicit opt-in for Improve.
Run feedback remains a report; helpful feedback is not verified task success.
No service API, persistence, generic DTO, CI, navigation or STATUS change.

New protocol coverage: MCP archive → HostClient HTTP feedback → readback,
idempotent feedback and changed-payload conflict, other writer/admin denial,
explicit Improve with disabled models returning unavailable and leaving no accepted
improvement. The existing 20 local synthetic-model Learning regressions cover
positive Improve HTTP acceptance, processing, exact citations, recovery and source
withdrawal. No real provider or paid model is used.

The old exact seven-tool gates were updated to the exact eleven-tool contract,
including required fields and nested context schema. Report is now explicitly an
object in the adapter schema and still decoded into the existing strict Run DTO.
The first full smoke found a second stale assumption: revoked calls defaulted to
retrieval `{query}` arguments. Valid per-Run-tool arguments now exercise the same
strict unauthenticated error assertion for every tool; no error gate was relaxed.

Final local results (Linux, declared Origence Rust 1.98.0):

- `cargo test --test local mcp_runs -- --nocapture`: 1 passed.
- `cargo test --test local local_host_api_cli_mcp_models_and_recovery -- --nocapture`: 1 passed.
- `cargo test --test local learning_loop -- --nocapture`: 20 passed (local synthetic model fixture).
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo build --locked`, `cargo fmt --all -- --check`, `git diff --check`: passed.
- `PYTHONPATH=tools python3 -m unittest discover -s tools -p test_mcp_schema.py`: 3 passed.
- Actual stdio eleven-tool schema check with a temporary synthetic auth stub: passed.
- `python3 tools/anyseek_smoke.py --anyseek-binary /workspace/anyseek-fixture/target/debug/anyseek_server --run python3 tools/mcp_smoke.py --binary target/debug/origence`: passed host-forwarding and offline modes, all protocol negotiations, exact schemas/tool lists, sanitized errors, live revocation, EOF and reopen.

The smoke used a clean checkout of the CI-pinned real AnySeek source
`6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`, rebuilt with Rust 1.99.1,
`--locked --no-default-features --bin anyseek_server -j 2`. Binary SHA256:
`8e9eb15edd86fb38b4a837476ed219bc6c7bc85a89d9f7460bb4c18316a57beb`.
Upstream build warnings were not changed. All databases, collections, content and
credentials were disposable synthetic fixtures. This is real service/protocol
coverage, not real Claude Code/Codex task or independent business-result evidence.

Immediately before publication, remote main remained
`293e2bf6af3b180d43c8050731047b552ac07519`; no branch-matching PR existed.
No rebase or force push is needed. User authorization now permits a normal push
and independent draft PR with Refs #87; the earlier local-only record above is
historical. Exact published HEAD and CI state are recorded in the PR/handoff.
Real two-host/version-frozen tasks and Windows remain unaccepted. No Issue update,
merge or deployment is authorized by this follow-up.

## 2026-11-17 — #89 Run 外部核验记录（本地 P0 切片）

- 已 fetch 核实 main/base：`293e2bf6af3b180d43c8050731047b552ac07519`。
  实现与下列测试对应提交：`5965f50848a17a6206c91110ffee8d2636efd32c`；
  独立分支 `p0/run-verification`，工作树 `/workspace/Origence-run-verification`。
- 合同见 [run-verification](contracts/run-verification.md)。仅 owner 转录外部结论，
  不将 `reporter=user/test`、外部核验者标签、自报成功或 prepared 状态解释为认证角色/平台验真。
  前缀摘要、记录凭据、服务端时间、声称核验者/时间、结论、依据序号/摘要和可撤销状态分别保存。
  版本未知不填 latest；核验者角色不授予 Run 正文权。MCP/接入工具接口未改变。
- 环境：Linux，Rust `1.98.0 (88d9e12ae 2026-08-18)`，Node `v24.22.1`。
  `CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup CARGO_TARGET_DIR=/workspace/target-run-verification`。
  只使用 tempdir 新库、合成文本/身份、Models::disabled 和 loopback AnySeek 协议夹具；无生产凭据、真实任务或付费模型调用。
- `cargo test --lib run --features sqlite`：8 passed。含严格 DTO 拒绝角色伪造、
  准确前缀冲突、失败结论与自报成功共存、未知/确切声明版本、重试/冲突、撤销、分页、
  同 schema 重开、跨管理员 Key 拒绝和凭据撤销后读写/重放拒绝。
- `cargo test --test local run_verification --features sqlite`：2 passed。
  公共 Service + HTTP 验证 prepared 资产 v2、自报成功、owner 转录失败、实际 Skill 使用版本未知；
  后续 Run append 不改变核验前缀；来源撤回阻断核验读取、重放和撤销调用。
  精确 schema 缺表拒绝且原数据库文件字节未变，不迁移、不回填。
- `npm --prefix ui test`：17 passed（含 tsc/build）。验证独立核验显示、撤销、未知版本、
  不可信文字不生成 HTML、刷新失败清除旧核验内容及恢复、精确 GET relay 凭据转发。
  relay 仅新增 `/v1/runs/{UUID}/verifications` GET 白名单；未改共享导航、通用 DTO、CI 或 STATUS。
- `cargo fmt --check` 与 `git diff --check`：通过。
- `cargo clippy --all-targets --all-features -- -D warnings`：通过；首轮发现新 DTO 中一处
  `collapsible_if`，已按建议合并等价条件并在最终实现提交重新验证。
- SQL 人工检查：核验模块所有 SELECT/COUNT/INSERT/UPDATE、测试建库与撤权语句均为单表；
  无 JOIN、逗号 FROM、跨表子查询或 CTE。关联前缀/来源/逻辑 owner 在原授权事务内复核，
  提交前沿用实时权限复核；无外部 IO 或中间提交。
- 未验收：Windows 行为、真实浏览器/无障碍、双客户端与企业任务、真实模型效果、独立核验者签认。
  当前 UI 只读展示/刷新；写入与撤销通过独立 HTTP/Service API。此切片不代表 #89 全部通过，
  也不提供 tracing/成本/生产告警。需接入与试点方按独立合同对接；联合合同确认后再安排发布。
- 本路仅本地提交，未推送、未开 PR、未更新 Issue、未合入、未部署。

## 2026-11-17 — #89 草稿发布前定向复验

- 续接本地 `87b284c8fa3b9c324db2aa039af69a026b2c1dd5`；重新 fetch 后 main 仍为
  `293e2bf6af3b180d43c8050731047b552ac07519`。本轮测试/合同提交：
  `ddca4c52b3d71b2650f0ad13ce1e613089a42858`。
- 对照 #87 已发布 Issue 的采集合同：显式归档/Run prepare 与核验分离，`src/run.rs`、
  MCP 接入接口没有本路变更；未发现 #87 可供联合运行的公开实现 PR，故不声明联合验收。
  本路只对应 PRD 的“Run 所有者记录外部签认依据”，不生成独立签认、平台核验通过状态或
  业务通过率。用户不能用 owner 转录将自己提升为认证核验者；#89 不关闭。
- 再跑 `cargo test --lib run --features sqlite`：8 passed；
  `cargo test --test local run_verification --features sqlite`：2 passed。
  加强真实 Service/HTTP readback 断言：prepared、自报 success、转录 failed、
  `platform_verification:not_performed`、`verifier_authentication:unverified_claim` 分离；
  非 owner reviewer 无权撤销，来源撤回后核验与 Run 正文均返回 NotFound/Forbidden。
- `npm --prefix ui test`：17 passed；`cargo fmt --check`、`git diff --check`、
  `cargo clippy --all-targets --all-features -- -D warnings` 均通过。
- Chromium `156.0.8078.79`：`npm --prefix ui run build` 后运行
  `node ui/scripts/check-run-verification-browser.mjs` 通过。真实 Chromium DOM、已构建 UI
  和 relay，合成 HTTP 响应；覆盖准备/上报/外部三种结论分别显示、未知使用版本、撤销、
  不可信文字转义、来源不可用后刷新清除旧核验内容，以及另一 Key 拒绝后的正文/核验不显示。
  该浏览器夹具不替代上述真实 SQLite/Service 授权验证，也不代表真实双客户端/生产任务验收。
- schema 差异仅新增 `oc_run_verifications`，沿用精确 fresh-only admission；本轮再次通过
  缺表拒绝且数据库字节不变测试。应用 SQL 仍单表，跨表关联与授权/来源复核留在原事务。
- 允许正常推送本分支并创建针对 main 的草稿 PR（Refs #89）。不改 Issue、不合入、不部署。
  Windows、完整无障碍、真实双客户端/业务任务和外部独立签认仍未验收。

## 2026-11-17：#88 Learning 来源专项补验（草稿 #94）

`a709dfe2702efcb223d7765fc9ae0b7a71a838c9` 只增加确定性测试：同空间另一 writer
经真实 CLI/HTTP 选择已发布 Learning 精确 v1，私有支持读取拒绝；撤回原归档支持后，
待发布候选与已发布包均返回 NOT_FOUND，清理仍 pending。21 项 Learning 回归、
原 Run CLI 回归及格式/Clippy 通过。详见 [#88 追加记录](https://github.com/TheLudlows/Origence/blob/a56246406a0688f646a45a85cc2954fb14a8d28a/docs/validation/issue-88-skill.md#learning-source-follow-up-2026-11-17-draft-pr-94)。
未变更产品代码，不代表真实客户端或业务验收。

## 2026-11-18 — #95 coordinated integration after #93/#94

Merged main `a56246406a0688f646a45a85cc2954fb14a8d28a` into original #95
`dfbaade8eb8848d6085cf5b9f61f986f2c8b9428` without rewriting history. Only this
validation history conflicted; both entries remain. Automatic module/UI-test
merges preserve the existing source, exact-prefix, logical-key and fresh-only
schema contracts; no implementation change was needed.
Linux Rust 1.98.0: fmt and diff checks, `cargo test --locked --lib run_verification`
(2), `cargo test --locked --test local run_verification` (2),
`cargo test --locked --test local workspace_schema_rejection` (6), and
`cargo clippy --locked --all-targets --all-features -- -D warnings` passed.
Node UI build/tests: 19 passed. All database fixtures were newly created temporary
stores, not old stores converted to the new schema. No external model or production
material was used. New exact-HEAD CI is required before merging; full pilot remains NOT_RUN.

## 2026-11-18 — #96 coordinated integration after #93–#95

Merged main `e78b6ba5f3d6fcb4730789981a092c85cdc4c12a` into original #96
`07ddb1640c652bdd82c8ba168fe86bac9563cfa9` without history rewriting. Only this
validation history conflicted; all records remain. No MCP/HTTP, storage, DTO or
CI behavior changed during resolution.
Linux Rust 1.98.0: fmt/diff checks, `cargo test --locked --test local mcp_runs::`
(1), `cargo test --locked --test local foundation_http::` (2), full-feature
all-target strict Clippy, and `cargo build --locked --bins` passed.
`PYTHONPATH=tools python3 -m unittest discover -s tools -p test_mcp_schema.py`:
3 passed. Databases and protocol/model fixtures were fresh and synthetic. These
checks do not certify real native clients or business outcomes. The new exact-HEAD
CI must pass before merge; actual final main will be independently retested.

## 2026-11-18 — #76 路由焦点与编辑对话框名称（本地）

基于 main `48cfa7c643b8c077ad33c55981def51979afde05`。切页时将焦点放在新主区域，
无效地址或版本则聚焦错误内容，目标不进入 Tab 顺序。焦点在页面替换时移动一次，后续
请求完成后保留用户已经选中的控件。共享编辑与提取对话框由现有标题提供可访问名称，
关闭行为沿用原生 dialog。界面布局、路由结构和 API 保持原行为。

- Linux、Node 24.22.1。 `npm --prefix ui run check` 与 `npm --prefix ui test`
  通过，19 项。复用页面 DOM 流程断言主区域焦点与对话框的标题关联。
- Chromium 156.0.8078.79。使用已构建的 UI、真实 Node 转发及 loopback 合成 HTTP
  响应。1024px 和 320px 均通过键盘切页、重复导航、浏览器前进后退、无效路径、
  延迟目录响应保留输入焦点和文本、编辑及提取对话框在可访问性树中的名称、
  Escape 和关闭按钮返回原控件，以及对话框打开时无整页横向溢出。
  浏览器回归脚本 `ui/scripts/check-navigation-focus-browser.cjs` 单独运行，使用
  Node 24、Playwright 1.62.1 和 Chromium。从仓库根目录执行以下命令，按本机调整
  Chromium 路径。Playwright 装在临时目录。

  ```sh
  browser_deps=$(mktemp -d)
  npm install --prefix "$browser_deps" --ignore-scripts playwright@1.62.1
  npm --prefix ui ci --ignore-scripts
  npm --prefix ui run build
  NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
    node ui/scripts/check-navigation-focus-browser.cjs
  ```
- `node ui/scripts/check-run-verification-browser.mjs` 通过。
  `cargo fmt --all -- --check`、
  `cargo clippy --locked --all-targets --all-features -- -D warnings` 和
  `git diff --check` 通过。

本轮范围限于上述 UI 回归。屏幕阅读器、实体触屏、其他浏览器、完整 #76 及真实产品
客户端任务仍待验收。记录和实现均为本地提交。

## 2026-11-18 — #76 私有 Run 的固定版本 PDF 页内定位

- 最新 main 核对为 `48cfa7c643b8c077ad33c55981def51979afde05`。独立本地
  分支 `fix/issue76-pdf-page`。本地对象库未找到候选 `56989af`，未将其当作交付依据。
  查阅仓库 AGENTS.md 和来源/Run 合同，仓库及工作区无适用的 `.agents/skills`。
- Linux x86_64，Rust/Cargo 1.98.0、Node 24.22.1、Chromium 156.0.8078.79。
  测试只使用合成 PDF、新临时 SQLite、动态 loopback 端口和 AnySeek 协议夹具，
  模型禁用。Codex/Claude Code 产品客户端测试保持暂停。
- `cargo test --locked --offline --test local run_pdf_source`：1 passed。
  通过已授权原件与发布片段哈希验证 v1 第 2 页，在 v2 发布后仍返回 v1 原件字节；
  相同文本重复时返回页码和无 span，原件仍可授权打开。跨 Key、reader、无关证据 ID、
  请求指定版本、来源撤回后的重复获取均按合同拒绝。覆盖公共 Service、上传/发布/搜索
  管道和真实 HTTP 请求。
- `cargo test --locked --offline --lib pdf_page_span`：1 passed。覆盖 UTF-8 多字节边界、
  片段哈希、未知解析器、重复匹配和纯栅格页无高亮。首轮非 ASCII 夹具未声明字库编码，
  修正夹具 WinAnsi 编码后复跑通过。现有
  `cargo test --locked --offline --no-default-features --test core pdf_`：9 passed，
  含多语言、分页、损坏、操作符截断、文件/页数/总文本限制和拒绝部分发布。
- `npm --prefix ui test`：19 passed。随后从仓库根目录执行
  `node ui/scripts/check-run-pdf-browser.mjs`：通过。真实 Chromium 和构建后的 UI/relay
  使用合成 HTTP 响应，验证 v1 页内文本 mark、原件字节、第 2 页和第 3 页的实际新标签
  Blob URL、回退不高亮以及撤回后拒绝新开；没有回退到 asset 最新版或裸文件 URL。
  首轮浏览器行为断言通过后有配置目录清理竞争，改用正常关闭并重试清理后整条通过。
  `node ui/scripts/check-run-verification-browser.mjs`：既有 Run 核验浏览器回归通过。
- `cargo fmt --all -- --check`、`cargo clippy --locked --offline --all-targets --all-features -- -D warnings`
  与 `git diff --check` 通过。复用已有单表存储查询，未新增 SQL、依赖、权限、schema 或迁移。
  撤回后的新响应要求重新获得权限和来源检查，已经打开的原件遵循现有下载语义。
- 局限：高亮位于核对后的页内提取文本，浏览器原件在对应页打开。未加坐标覆盖或 OCR。
  浏览器与真实 HTTP 后端分别验证，Windows 和完整 #76 验收未跑。本切片仅在独立分支
  本地提交，未推送、未发 PR。

### #76 PDF 真实服务与 Chromium 联验

同步 main `8cdf88f709d708b75689768e6caada45fde56155` 后，在
`47b73234de9d57bb3b996b1a244421a06276e4f2` 复验。原 PDF 提交
`a95b931642372211575f7a11c57668208ffe43c1` 至该合并提交的 Rust、Cargo、
测试工具和 CI 无差异，UI 则重新构建以覆盖主干 #98 的页面变化。以下补记仅改变验证文档。

真实 AnySeek 使用 CI 固定源码提交 `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`，
Rust 1.99.1 从源构建。AnySeek 二进制 SHA256
`ef581925d9afa44ce7be3694144ef010354925304e63ca737fc7b33d1687cc3a`，
Origence 1.98.0 工具链编译的测试二进制 SHA256
`e05e5f930d94bd003fa337d0ba8fe01c24375f504d8e61810496ce6eaf678f52`。
两者在联验前后分别校验一致。使用 Node 24.22.1、Chromium 156.0.8078.79、
动态 loopback 端口及临时合成数据，模型关闭。

- 同一 Run `a1d2cca4-f58b-4bac-a378-cf036fa218db` 在真实 AnySeek 上固定 v1
  引用，再发布 v2。正常工作空间登录后，构建的 UI 经 relay 从真实纳入授权的 HTTP
  接口读取 v1 第 2 页，页面提取文本高亮 `NeedleUnique historical fixed PDF page`。
  浏览器收到的原件在原生查看器打开第 2/2 页，SHA256 为
  `0604486708bf7f3959dcdd53b5af8d4817b32cd1b738beb06d9fb8461bc69fc7`，
  与 v1 字节相符。原生 PDF 页面无像素覆盖层。
- 重复片段回退到页 1 与授权原件，文本无高亮。纯栅格 PDF 按现有规则拒绝发布，
  其原件沿用已有授权下载。缺证据 ID 为 404，reader 读原件为 403，另一 admin
  读取私有 Run/来源为 404。
- 撤回 v1 时浏览器保留旧按钮。再次点击收到真实 404 后移除按钮和高亮，
  v2 仍为当前版本。联验保留了浏览器原件哈希、原生查看器截图和 HTTP 状态记录。
- 本地格式、Clippy、PDF 单元 1 项、Service/HTTP 1 项、无默认特性的 PDF 核心
  9 项及 UI 19 项均通过。合成 Chromium 回归和上述真实服务联验均通过。

## 2026-11-18 — #76 Run 创建量与趋势（本地）

基于 main `3c66b5b06fbf40cc22641b373f892da1b617aff0`。总览以一个服务端
截止时间和一个授权读快照计算当前逻辑 key 的创建记录。UTC 半开滚动窗口返回
24 个一小时桶或 7 个 24 小时桶，空桶为 0。Run 目录仍按 UUID 分页。重复创建
请求及证据撤回不改变创建计数，业务结果无法从创建记录推断。

SQLite 新增单个 tenant/workspace/owner/created_at 覆盖索引。使用现有精确
schema admission，缺索引的旧目录在写入前拒绝，不建索引、不迁移。测试中的
实际 EXPLAIN 为 `SEARCH oc_runs USING COVERING INDEX oc_runs_owner_created_at
(tenant_id=? AND workspace_id=? AND owner_key_id=? AND created_at>? AND created_at<?)`。
每个桶执行一个单表标量计数，最多 24 个互不重叠的范围查询，不提取 Run 或事件正文。

- Linux / Rust 1.98.0。 `cargo test --locked --no-fail-fast` 通过，450 项通过、
  8 项默认忽略的引擎专项未计入。 `cargo test --locked --test local run_creation_ -- --nocapture`
  复跑覆盖查询计划、固定时刻的桶边界与总和、幂等、跨 scope/逻辑 key、读权限、轮换、
  无效窗口与范围参数、超过首屏和空计数。现有 Run 核验/证据撤回测试另确认详情拒绝而
  该 Run 仍保留在目录和创建量中。
- `cargo fmt --all -- --check` 及默认全特性、authority-only、core-only 的
  `cargo clippy --locked --all-targets -- -D warnings` 均通过。authority-only 使用
  `cargo test --locked --no-default-features --features sqlite --test local
  missing_run_creation_index_is_rejected_without_repair` 验证旧文件原样保留。
- Node 24.22.1。 `npm --prefix ui run check`、`npm --prefix ui test` 通过，19 项，
  覆盖两时间窗口、列表独立、错误与重试。既有 Chromium 路由焦点回归通过。
- Chromium 156.0.8078.79、实际 Node relay、实际 Origence/SQLite 与固定 AnySeek
  `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`。合成临时数据中两窗口均正确
  显示 30 个 Run，而目录首屏为 25。另一管理员及 reader 均为 0，窗内空桶、
  桶合计、UTC 边界与 320px 布局均通过。浏览器使用
  `tools/anyseek_smoke.py` 启动隔离的引擎。

业务成功率、端到端 P95、Token 和成本仍未采集。本切片不提供告警、生产容量验收、
Windows 运行证据或真实客户端验证。仅本地提交。

草稿发布前补验了两个方向的 schema 拒绝。用基线 `3c66b5b` 和含索引的
`a43c270` 分别构建 Origence，通过 `--offline workspace-create` 各建一个临时库，
再用另一版本运行同一打开操作。新版拒绝缺索引的库，旧版拒绝含新索引的库。
两次打开均返回 incompatible schema，SQLite 文件与打开前逐字节一致。
实现与测试代码自 `a43c270` 保持一致，草稿准备提交仅补全兼容边界说明。

## 2026-11-18 — #76 资产详情页签键盘操作（本地）

基于 `origin/main` 的 `3c66b5b`。方向键及 Home/End 移动页签焦点，Enter、空格或点击
激活页签。Tab 从页签组进入当前面板，继续 Tab 可达版本比较。reader 直接查看正文，仍可
比较版本。原始来源仅在激活时请求；离开来源页签或当前资产路由时取消待完成的原文请求。
版本比较在独立区域显示，重新比较时只保留最新请求。沿用现有 API 和角色边界。

- Linux、Node 24.22.1：`npm --prefix ui run check` 及 `npm --prefix ui test`（19 项）通过。
- Chromium 156.0.8078.79（headless）、Playwright 1.62.1、已构建的 UI 和真实 Node 转发，
  上游为 loopback 合成 HTTP 夹具。1024/736/375/320 px × 浅/深色 × writer/reader
  共 16 组通过。验证 Tab/Shift+Tab、左右方向键环绕、焦点轮廓、激活键和点击、可访问性树
  的页签与面板关系，以及页面无横向溢出。reader 发出 0 个原文请求。故意延迟原文与历史
  版本响应，验证切回正文、快速重新比较和切到另一固定版本时旧请求已取消；原文与比较
  交错返回时各自显示，比较仍保留固定版本和增删内容。
- #98 的路由焦点和对话框浏览器回归（1024/320 px）、现有 Run 独立核验与 PDF 来源脚本均通过。
- Rust 1.98.0 的 `cargo fmt --all -- --check`、
  `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` 及
  `git diff --check` 通过。

从仓库根目录复跑。Playwright 装在临时目录，按本机调整 Chromium 路径。

```sh
browser_deps=$(mktemp -d)
npm install --prefix "$browser_deps" --cache "$browser_deps/npm-cache" --ignore-scripts playwright@1.62.1
npm --prefix ui ci --cache "$browser_deps/npm-cache" --ignore-scripts
npm --prefix ui run check
npm --prefix ui test
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-asset-tabs-browser.cjs
```

浏览器证据覆盖键盘、鼠标和 Chromium 可访问性树。屏幕阅读器、实体触屏、真实业务账号
与 Codex/Claude Code 产品客户端任务未执行。#76 保持开放。

### 合入前复验

同步已包含 #100 的主干 `63bdcdf4691c9ba761e4581b4aa62e94c89e94f1`，保留上方两份
验证记录。原有资产页与脚本改动保持一致。复跑 UI 类型检查、19 项测试、16 组资产页签
Chromium 检查、#98 的两个宽度及现有 Run 核验和 PDF 来源浏览器脚本，全部通过。
Rust 1.98.0 格式检查、离线全目标全特性 Clippy（warnings denied）及差异检查通过。

## 2026-11-18 — #76 资产版本比较状态与文字对比度（本地）

基于 `origin/main` 的 `6dcd03336c1d0605032a9083e9a4a2a02a972f1c`。版本比较区预置
`role=status`、`aria-atomic=true` 的常驻节点。加载和完成提示放在同一个 polite 状态区，
差异正文独立显示。当前请求失败时清空加载文字，在结果区显示错误警报。比较请求被
替换或离开详情时取消，过期成功和错误均不会更新状态或差异区。

Chromium 156.0.8078.79 用元素计算样式合成前景、背景，再按 sRGB 相对亮度计算对比度。
阈值按未舍入值核对。浅色仅将下列三个资产详情元素的文本前景从 `#137f6d` 调为
`#106f60`。暗色对应值与基线一致，主题色和背景定义保持原样。

| 元素 | 浅色背景 | 浅色修复前 | 浅色修复后 | 深色 |
| --- | --- | ---: | ---: | ---: |
| 选中的正文 / 原始来源页签 | `#f1f5f6` | 4.4656:1 | 5.5219:1 | 7.0595:1 |
| 新增差异文字 | `#eef6f5` | 4.4666:1 | 5.5232:1 | 7.2363:1 |
| 版本区肯定状态徽标 | `#eaf3f2` | 4.3417:1 | 5.3687:1 | 6.9543:1 |

浅色加载、完成和差异摘要为 4.7429:1，错误和删除文字为 5.0615:1。Node 24.22.1 和
Playwright 1.62.1，按上方命令复跑同一 loopback 合成 HTTP 夹具，16 组全部通过。验证
状态节点在请求前已存在、加载和完成内容在同一 Chromium AX 节点、完成提示与差异分开、
延迟 503 和重试、重复同版本、切到另一版本后过期响应及比较按钮焦点。writer 页签键盘
和原文请求行为通过，reader 原文请求仍为 0。

UI 类型检查、19 项测试、#98 的两个宽度、Run 核验和 PDF 来源浏览器回归、Rust 1.98.0
格式检查及离线全目标全特性 Clippy 均通过。Chromium 结果覆盖无障碍语义与焦点。
本轮缺少可观察实际播报的屏幕阅读器和获准测试的实体触屏，两项仍待验收。#76 保持开放。

## 2026-11-18 · 统一数据底座文档校准

以 main `4160fd5` 为代码基线，按记忆、知识、Skill 和 Trace 交叉核对产品文档、公开接口与当前实现。改动范围为文档和 Cargo 包描述。

- Windows / Rust 1.98.0 的 `cargo +1.98.0 fmt --all -- --check` 和
  `cargo +1.98.0 clippy --locked --offline --all-targets --all-features -- -D warnings` 均通过。
- `cargo +1.98.0 test --locked --offline --no-fail-fast --target-dir target/doc-positioning-check --quiet`
  通过，449 项通过、0 项失败，8 项需要隔离真实 AnySeek 的测试默认忽略。
  首次使用默认构建目录时，正在运行的 `origence.exe` 锁住目标文件，构建以 Windows 错误 5 退出。改用独立构建目录后完成复跑。
- 检查了 12 份活动 Markdown 的 125 个本地链接及目标锚点，未发现指向已移除锚点的仓库内链接。`git diff --check` 通过。

本轮覆盖文档一致性和 Rust 默认测试集。真实客户端双场景试点及独立签认仍按 [#91](https://github.com/TheLudlows/Origence/issues/91) 追踪。

## 2026-11-18 · 移除 AML 协议适配

基于 `4160fd5`，移除协议模块、HTTP 路由、启动开关及仅供协议派生 ID 使用的 UUIDv5 和 `sha1_smol`。消息批次、所属 workspace 和原始来源定位继续使用通用接口。SQLite 使用既有完整 schema 核对拒绝额外对象。核查了实际执行的单表目录查询与测试 DDL/DML，未新增关联查询。现有数据库拒绝测试覆盖检查和重开，并比对拒绝前后的文件字节。

- Windows / Rust 1.98.0 的 `cargo +1.98.0 fmt --all -- --check` 和 `cargo +1.98.0 clippy --locked --offline --all-targets --all-features -- -D warnings` 通过。
- `cargo +1.98.0 test --locked --offline --no-fail-fast --target-dir target/doc-positioning-check --quiet` 通过，448 项通过，8 项真实 AnySeek 测试默认忽略。
- `cargo +1.98.0 test --locked --offline --no-default-features --target-dir target/doc-positioning-check --quiet` 通过，90 项通过，默认忽略 5 项。
- `cargo +1.98.0 test --locked --offline --no-default-features --features sqlite --test local --target-dir target/doc-positioning-check --quiet workspace_schema_rejection` 通过 6 项。
- 在新的临时目录用固定 AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a` 运行 `tools/anyseek_smoke.py --test-executable`，8 项真实引擎测试全部通过。真实 HTTP 宿主验证旧路由均返回 404，CLI 拒绝旧开关，正常发布与全文检索通过。
- `docker compose --env-file .env.example config --quiet` 通过。当前机器只有 Docker Desktop 所用的 WSL 发行版，Linux Docker 引擎不可用，本轮未运行 Linux 测试。现有 CI 工作流仍可覆盖 Linux 与 Windows。

## 2026-11-18 · CI 去重

四个作业及原有测试、Clippy 和真实引擎联验命令均保留。`fast-check` 负责统一格式检查，Windows CI 跳过重复格式检查，Windows 本地 `Validate` 默认仍检查格式。`linux-native` 复用默认特性集成测试生成的宿主二进制，进入冒烟测试前检查它可执行。

- Windows / Rust 1.98.0 的 `cargo +1.98.0 fmt --all -- --check` 通过。
- 在全新的 `target/ci-binary-check` 目录设置 `CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_INCREMENTAL=0`，执行 `cargo +1.98.0 test --locked --offline -j 2 --no-fail-fast --quiet`。448 项通过，8 项真实 AnySeek 测试默认忽略。未额外执行 `cargo build`，集成测试生成的 `debug/origence.exe` 已通过 `serve --help` 验证。
- 本机 Windows PowerShell 5.1 运行 `powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File tools/build.ps1 -Action Validate -Jobs 1 -SkipFormatCheck` 通过。Clippy 通过，默认测试 448 项通过，8 项默认忽略。工作流的 Linux 和 GitHub runner 实测仍待 CI。

## 2026-11-18 — #76 运行创建趋势重试焦点（本地）

基线 `origin/main` 为 `e024b78eb2ecbf1aa1851783c89be8238ca819a1`，已包含 #98–102。
修复前，在 1024 px 的真实 Chromium 中，首次 503 后可用 Tab 从「统计范围」进入重试按钮。
按 Enter 后按钮立即离开 DOM，焦点落到 `body`。再次 503 后显示另一个按钮，焦点仍在
`body`。重新聚焦按钮并按空格成功后，焦点也落到 `body`。

修复后，首次失败仍显示错误和重试按钮。请求等待期间同一按钮保留焦点与可见轮廓，
`aria-disabled` 表示暂不可再次触发。失败时同一按钮恢复可用。成功后隐藏按钮，只有该按钮
仍持有焦点时，才把焦点移到同一区域的「统计范围」。已用 Shift+Tab 移开的焦点保持原处。

- Linux、Node 24.22.1、Playwright 1.62.1、Chromium 156.0.8078.79。1024/320 px 的
  `ui/scripts/check-navigation-focus-browser.cjs` 通过。已构建 UI 与 Node 转发使用合成
  loopback HTTP 夹具，控制响应顺序并检查首次失败、键盘重试、等待期重复按键、再次失败、
  成功时两种焦点路径，以及 #98 既有路由和对话框焦点。16 组资产页签浏览器回归均通过。
- `npm --prefix ui run check`、`npm --prefix ui test`（19 项）、`git diff --check`、
  Rust 1.98.0 的 `cargo fmt --all -- --check` 和
  `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` 通过。

使用上方相同的 UI 构建和 Playwright 安装方式，从仓库根目录复跑。

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-navigation-focus-browser.cjs
```

### 实际读屏和实体触屏待验收

从仓库根目录启动同一夹具的手动模式，用命令输出的本地地址和合成 key `synthetic-key`。
服务仅监听 loopback。每次验收前重新启动夹具。手动模式的五次响应已通过 HTTP 验证，
依次为 503、503、200、503、200，每次延迟约 1.2 秒。

```sh
npm --prefix ui run build
ORIGENCE_FIXTURE_PORT=43176 node ui/scripts/check-navigation-focus-browser.cjs --serve
```

1. 在能记录实际播报的读屏会话中连接夹具。记录首次失败播报，Tab 到「重试读取创建量」并按
   Enter，记录加载、重复失败及按钮焦点。再次用空格重试，记录成功播报，确认焦点移到
   「统计范围」。然后切到「最近 7 天」，在下次重试未完成时移开焦点，确认成功后焦点仍在
   原处。记录设备、读屏、浏览器版本和实际播报。
2. 用一台获准且通过既有本地连接可访问夹具的实体触屏重跑失败、重试、再次失败和恢复。
   实际点按，检查响应等待期间的状态可读、连续点按不会重复触发、恢复后结果可达且滚动
   可用。记录设备、浏览器、视口和手势结果。

本轮未运行实际屏幕阅读器和实体触屏。正式试点、真实模型及产品端 Codex/Claude Code 测试
保持暂停，#76 保持开放。

## 2026-11-18 — #76 Run 详情采集声明（本地）

基于 `origin/main` 的 `e024b78eb2ecbf1aa1851783c89be8238ca819a1`。基线仅在所选事件的
原始 JSON 中展示 `coverage`。Run 详情现用已有 Trace 分页里的结构化事件，按证据顺序显示
事件序号、组件、声明状态和上报来源。同一组件的后续声明保留前序缺口。声明未经独立
核验，整轮采集范围仍未知。有后续事件时明确限定在已加载范围。无声明的成功读回显示
「已加载事件中未见采集声明，覆盖未知」。

沿用现有路由、授权和原始事件详情。403 等读取失败清除旧声明并显示原错误，重试从首批
事件开始。切换 Run 和 401 登出复用既有路由及凭据生命周期清除私有视图。

- Linux、Node 24.22.1，`npm --prefix ui run check` 与 `npm --prefix ui test`（20 项）通过。
  合成 HTTP 回归覆盖重复组件的顺序、四种声明状态、不可信文本转义、分页范围、403 清除
  与重试、等待另一 Run 期间清旧数据，以及 401 清除私有视图。
- Chromium 156.0.8078.79、Playwright 1.62.1。通过已构建的 UI 和真实 Node 转发，用
  隔离的 loopback 夹具，在 1024 和 320 px 实际走登录、分页、403、重试、切换 Run 和
  401 登出。#98 路由焦点回归和 16 组资产页签回归通过。旧 Run 核验 CDP 脚本偶发
  `ERR_ABORTED`，用主干原脚本也可复现。本轮用下方独立 Playwright 检查确认声明行为。
- Rust 1.98.0 的 `cargo fmt --all -- --check`、
  `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` 及
  `git diff --check` 均通过。

用上文相同的 UI 构建和 Playwright 安装方式，从仓库根目录复跑。

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-run-coverage-browser.cjs
```

本轮使用合成数据进行 Chromium 和 DOM 验证。实际读屏和实体触屏仍待验收，真实客户端、
模型和正式试点保持暂停，#76 保持开放。

### #76 所选事件与固定 Run 前缀摘要（Chromium）

基于 main `56682df8613f8cd19c91ef2f7751eee931600f14`。主干在读取事件 1 的固定
前缀后切到事件 2，迟到的事件 1 摘要会显示在事件 2 的详情。修复后摘要请求绑定当前
所选事件，切换事件时取消；响应须匹配 Run ID、事件序号与 SHA-256 摘要格式。重新读
取时清除上次结果，权限拒绝或不匹配不会继续显示旧摘要。此改动不改变服务端归属、
权限检查、固定前缀摘要算法或对客户端上报的解释。

- 扩展既有 Run Chromium 脚本，以真实 UI/代理访问 loopback 合成 HTTP。基线复现迟到
  的错误归属；Chromium 153.0.8010.0 下 1024/320 px 均通过。覆盖旧选择成功/403、
  重新读取中的旧值清除、活动 403、返回错误序号、保留按钮焦点、去重与正确结果重试。
- Node 24.22.3 类型检查与 21 项 UI 测试、既有固定版本 PDF Chromium 和
  `git diff --check` 通过。未运行真实 Agent 客户端、读屏语音、实体触摸、模型或试点。

### #76 Run 事件分页与外部核验恢复

基于 main `2fcde29eabdf5a5ed67c56ad5fd7a5d0baf31501`。基线加载第三页会把
第二页选择重置为事件 1，403 后有权重试也不会重新读取外部核验。修复后追加事件
而不重建已选详情，保留固定前缀和现有焦点；末页仅在焦点仍位于「加载后续事件」
时移到新事件。分页失败仍清除旧证据并取消在途核验；有权重试从第一页重取。
服务端权限、摘要规则和外部核验的证据边界不变。

- Node 24.22.3 的 `npm test`（21 项）和 Chromium 153.0.8010.0 的
  `ui/scripts/check-run-coverage-browser.cjs`（1024/320 px）通过。用相同新增回归
  在基线上分别复现重试未恢复核验和第三页导致选中事件跳回。
- 夹具覆盖 403 清除、旧核验请求取消（晚到 401 不会注销重试后的会话）、当前 401
  退出、焦点及固定前缀保留。均为合成验证，未运行真实客户端、读屏、触屏或试点。

## 2026-11-19 — #76 中间宽度主导航键盘回归

基于 `origin/main` 的 `5c3a423f013fec0141fb8278612fa305e3805612`。此前 736/375 px 的
历史布局验证和资产详情页签已有证据。可复跑的主导航及焦点脚本此前使用 1024/320 px。
在原脚本中增加 736/375 px × 浅/深色的四个自动化场景。

- 从「运行总览」经 Shift+Tab 与 Tab 双向穿越五个主导航入口及相邻的退出、主题控件。
  核对焦点顺序、`focus-visible`、轮廓和控件位于视口内。用 Enter 激活「上下文工作台」，
  验证主区域接收焦点、唯一 `aria-current` 更新，下一次 Tab 进入「任务与查询」。
- 在总览和工作台读取真实元素位置。736 px 主导航在工作区侧面，375 px 主导航在工作区
  上方，两页均无整页横向溢出。
- Linux，Node 24.22.1，Playwright 1.62.1 驱动真实 Chromium 156.0.8078.79 headless。
  使用已构建 UI 与 Node relay，数据来自 loopback 合成 HTTP 夹具。四个新增场景连续
  两轮通过，原 1024/320 px 路由、对话框及总览重试焦点回归同轮通过。
- UI 类型检查、20 项 UI 测试、Rust 1.98.0 格式检查、严格全目标全特性 Clippy 和
  `git diff --check` 通过。该基线的[主干 CI](https://github.com/TheLudlows/Origence/actions/runs/44441569348)
  四项通过。Chromium 脚本按下方命令单独运行。

用上文相同的 UI 构建和 Playwright 安装方式，从仓库根目录复跑。

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-navigation-focus-browser.cjs
```

本轮验证键盘输入和浏览器布局。实际读屏与实体触屏仍待验收。真实客户端、模型及正式试点
未运行，#76 保持开放。

复审 #110 时加强了两个可渲染断言：在每个宽度比较两种方案下工作台的计算背景色，
并核对每次键盘聚焦时轮廓宽度大于零且轮廓位于视口内。延迟知识目录夹具沿用 4 秒请求
时限，未捕获请求时明确失败。旧脚本在临时强制浅色并移除焦点轮廓时仍通过；新版脚本
分别识别这两种退化与缺失的知识目录请求。无注入的真实 Chromium 四组场景和原
1024/320 px 回归连续两轮通过，UI 类型检查及 20 项 UI 测试通过。

GitHub CI 的 `node-ui` 运行 `npm test`，包括构建及 Node 测试。Chromium 焦点脚本按
上述命令在本地单独执行；`node-ui` 的结果只涵盖其实际运行的构建及 Node 测试。

## 2026-11-19 — #76 上下文知识的固定版本与原文字节核验

基于 main `1c5e00297b5074a420acfcf70a84db103ab38a86`。基线 Chromium 可以从上下文工作台
打开携带版本号的资产链接，但不能高亮普通 Text/Markdown 知识证据的原文。核验时沿用
来源事件与原始文件的既有 HTTP 权限，只在 `source-ranges-v1` 的 UTF-8 字节切片与证据
文字一致时高亮。区间切断字符或引文不一致时显示提示。资产链接继续读取引用中的版本号。

- Linux，Node 24.22.1，Playwright 1.62.1 和 Chromium 156.0.8078.79 headless。
  构建后的 UI 经 Node relay 访问 loopback 合成 HTTP 夹具。主干在等待内联文档高亮时失败，
  修复后两轮通过。测试在当前版已有 v2 时打开 v1，并检查内联与上传来源的非零字节范围、
  中文、emoji、组合字符、UTF-8 BOM、两种拒绝高亮情形及 reader 不请求原文。
- UI 类型检查、20 项 UI 测试、严格全目标全特性 Clippy、Rust 格式检查通过。
  既有资产页签与私有 Run PDF 的合成 Chromium 回归通过。此次知识来源核验只覆盖
  Text/Markdown，PDF 页内定位仍以原 Run 验证记录为准。
- 另外用上述版本 Chromium、构建后的 Node UI、Rust 1.98.0 构建的同一代码和 CI
  钉住的 AnySeek `6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`（Rust 1.99.1）联验通过。
  内联 Text 与上传 Markdown 分别使用 fresh SQLite/AnySeek、临时 workspace 和合成文档。
  浏览器预览 v1 后通过真实 API 发布 v2；v1 证据来自实际 `/v1/context`，其非零范围从
  字节 2400 开始。核验高亮精确匹配中文、emoji 和组合字符；点击证据后读取 v1，正文
  无 v2 内容。未拦截 HTTP。角色和异常区间的断言来自上面的合成 HTTP 回归。

使用上文相同的 UI 构建和 Playwright 安装方式，从仓库根目录复跑浏览器验证。

```sh
NODE_PATH="$browser_deps/node_modules" CHROMIUM=/usr/bin/chromium \
  node ui/scripts/check-context-knowledge-browser.cjs
```

实际读屏、实体触屏未执行。真实产品客户端、模型及正式试点保持暂停。

### #76 原始引用核验的状态和焦点（Chromium）

基于 main `2ad69260522676ecf70f9cd86fc3e3e47d453c8a`。文字来源核验的读取、定位、
无法高亮和失败结果进入已挂载的简短状态区域；原始文档和定位细节仍是普通内容。
重取时清除旧结果；过时的响应不替换此次结果，焦点留在核验按钮。

- Chromium 153.0.8010.0，真实构建 UI 和同源转发，loopback 合成来源。既有上下文知识
  脚本扩展后，在基线无法找到激活前的状态区域；修复后通过。通过 Chromium AX tree 核对
  `status` 节点、`polite`、`atomic`、激活后同一节点的状态更新、403 后重试和焦点。
  下载时的新授权拒绝清除旧的原始预览，两种不可核验的 UTF-8 情况仍不高亮，
  reader 不发起原文请求。
- Node 24.22.3 类型检查和 21 项测试通过。资产详情页签/对比度和私有 Run PDF 构建浏览器
  回归通过。此次不改固定版本、权限或 PDF 页内核验合同。
- 这是浏览器无障碍树证据，不是读屏软件或实际语音播报证据。云端没有 Orca/语音调度
  程序，D-Bus 的本地套接字也无法在测试环境中开启。实体触屏和实际读屏仍待验收。

## 2026-11-19 — #76 入库任务状态刷新与重试（本地）

基于 main `df330cf`。手动刷新会取消尚未完成的旧状态查询；过时响应不更新页面，
也不决定轮询何时停止。失败后的显式重试会继续轮询同一任务，重复触发不会在同一次
操作进行中再发出重试。当前版本仍由服务端决定，关闭对话框不取消后台任务。

- Node 24.22.3：`npm --prefix ui run check`、`npm --prefix ui test`（21 项）、
  `git diff --check` 通过。
- Chromium 153.0.8010.0：构建后运行
  `CHROMIUM=<浏览器路径> node ui/scripts/check-knowledge-job-browser.cjs`（需提供 Playwright）。
  真实 UI relay 对接 loopback HTTP 夹具：失败时仍显示 v1；一次重试后由自动轮询
  显示发布完成并打开 v2；人为延迟的旧 processing 请求被取消，不覆盖已完成状态；
  Escape 返回触发器焦点，前进/后退不恢复旧对话框。主干未修版本两项基线回归均失败。

这些是本地合成 HTTP/浏览器结果；不替代前文真实服务检查，也不代表生产、
真实客户端、实体触屏或读屏验收。未改动服务端接口、存储、权限或部署。

## 2026-11-19 — #76 上下文查询向量的可追溯输入 Token

基于 main `df330cfdf389eaefec463d745e0aeca779eaf4fe`。在已有的上下文准备检索诊断中记录
模型端上报的查询向量输入 Token 和配置摘要；没有新增模型调用、存储表或计价逻辑。
只显示单个准备事件，成功重放保留原回执和固定前缀。独立预览不写入 Run。

- Rust 1.98.0：检索诊断集成 3 项、诊断单测 2 项、格式和严格全目标全特性 Clippy 通过。
  loopback 模型协议夹具及真实本地 SQLite 验证归属和伪造防护、单次共享调用、重放不发请求、
  拒绝或失败的正确归因、未产生 Run 的预览、明确返回 0 / 缺报 / 无效计数及旧证据序列化。
- Node 24.22.3：类型检查和 20 项 UI 测试通过。DOM 验证服务端事件中的 0、缺失/空值保留
  未知，以及客户端自报不显示为模型服务回执。既有 Run 采集和主导航回归在 Chromium
  153.0.8010.0 使用构建后的 UI 和 loopback 夹具通过，覆盖 1024/320px 及导航的 736/375px。
- 未运行真实模型或产品客户端，也未验收整轮/Agent Token、成本、业务成功率、端到端 P95、
  告警或正式试点。不使用这里的计数计算费用或填充总览。

## 2026-11-22 — #76 客户端已归档引用的固定版本入口

基于 main `66a1720624af64ff939a958a7e893cbcaee09a94`。客户端上报
`context_reported` 的非 PDF 引用先前只展示 JSON；现在从通过 Run 读取返回的归档引用
打开固定版本。未归档的客户端声明不生成链接；按原有资产读取接口重新鉴权，不回退到
当前版本，也不增加原始来源读取入口。「客户端上报；未经独立验证」及 PDF 核验行为不变。

Node 24.22.3。新增的 DOM 回归在未修代码中因缺少固定版本链接失败，修复后
`npm --prefix ui run check`、`npm --prefix ui test`（22 项）及 `git diff --check` 通过。
夹具覆盖当前已为 v2 仍打开 v1、未归档无链接、来源撤回后拒绝 v1 且不显示新旧正文。
这是合成 HTTP/DOM 校验，未运行真实客户端、模型、正式试点或生产部署。
