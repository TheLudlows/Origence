# Claude Code 与 Codex 接入

**简体中文** · [English](CLIENTS.en.md)

Claude Code 和 Codex 通过 Origence 服务端的 Streamable HTTP MCP 检索。可直接注册端点，或安装带自动 Run Hook 的插件。

用 [`origence-setup`](INSTALLER.md#接入-agent) 配置 Claude Code 或 Codex 的插件和 Run Hook 环境。部署本地 API 和控制台使用 `origence-deploy`。

## 直连 HTTP MCP

HTTP 宿主提供 Streamable HTTP MCP 端点 `/mcp`。在客户端的启动环境设置工作区 API Key
`OC_API_KEY`。只检索已发布资料用 `reader` 权限。以下使用本机默认端口，远程部署时换成实际 HTTPS 地址。

```powershell
claude mcp add --transport http origence-http http://127.0.0.1:8080/mcp --header 'Authorization: Bearer ${OC_API_KEY}'
```

```powershell
codex mcp add origence-http --url http://127.0.0.1:8080/mcp --bearer-token-env-var OC_API_KEY
```

在同一终端启动客户端，用 `/mcp` 检查连接，并调用 `context_session` 核对 `workspace_id`。
默认使用凭据的默认工作区。要选择其他已授权工作区，在 HTTP MCP 请求中配置 `X-Origence-Workspace`
请求头。下文的插件使用启动环境中的 `OC_WORKSPACE_ID` 填充此请求头。

检索和显式工具调用无需本机 `origence`。插件中的自动 Run Hook 需要本机二进制。[远程部署配置](OPERATIONS.md#远程-mcp)

## 安装 `origence` 插件

准备一台客户端可访问的 Origence HTTP 宿主和支持插件 Marketplace 的客户端。插件安装 HTTP MCP，并默认启用私有
Run Hook。请工作区管理员签发个人 Key。只需检索时使用 `reader` 并可[直接注册端点](#直连-http-mcp)。
使用自动 Run Hook 时准备 `writer` Key 和支持 `origence plugin-hook` 的本机可执行文件。管理员可执行
`origence key-create <WORKSPACE_ID> --role reader` 或 `--role writer`，通过受保护的渠道交付 Key。

从设置了 `OC_API_KEY` 的终端启动客户端。默认宿主是 `http://127.0.0.1:8080`。
远程宿主使用 HTTPS。PowerShell 示例：

```powershell
$env:OC_SERVER_URL = 'http://127.0.0.1:8080'
$secret = Read-Host 'OC_API_KEY' -AsSecureString
$env:OC_API_KEY = [Net.NetworkCredential]::new('', $secret).Password
```

启用自动 Run 上报时，在启动前将 `origence` 所在目录加入 `PATH`。Windows 示例支持目录中的空格：

```powershell
$origenceExe = (Get-Item -LiteralPath 'C:\Program Files\Origence\origence.exe').FullName
$env:Path = (Split-Path -Parent $origenceExe) + [IO.Path]::PathSeparator + $env:Path
```

### Claude Code

```sh
claude plugin install origence --marketplace TheLudlows/Origence
```

插件用 `OC_SERVER_URL` 拼接 `/mcp`，设置地址时省略末尾的斜杠和 `/mcp`。
测试本地检出时，把 `TheLudlows/Origence` 换成仓库根目录的绝对路径。重新启动 `claude`，
用 `/mcp` 或 `claude mcp list` 检查来自插件的 Origence 连接，用 `/hooks` 核对插件 Hook。
客户端或组织要求确认时，审查插件并按提示授权。卸载使用 `claude plugin uninstall origence@origence-tools`，
可选执行 `claude plugin marketplace remove origence-tools`。
[Claude Code 插件命令](https://code.claude.com/docs/en/plugins/cli-reference)

### Codex

```sh
codex plugin marketplace add TheLudlows/Origence
codex plugin add origence@origence-tools
```

Codex 插件内的 URL 固定为本机默认地址。使用其他端口或域名时，用同名服务器覆盖插件提供的地址：

```powershell
codex mcp add origence --url "$env:OC_SERVER_URL/mcp" --bearer-token-env-var OC_API_KEY
```

同名覆盖后，若要用 `OC_WORKSPACE_ID` 选择工作区，把 [示例](../examples/codex.mcp.toml) 中的
`env_http_headers` 节加入该服务器配置。使用自动 Run Hook 时，`OC_SERVER_URL` 应指向同一 API 宿主。
移除覆盖配置使用 `codex mcp remove origence`。

测试本地检出时，Marketplace 来源可换成仓库根目录的绝对路径。在新的 `codex` 会话中用
`/mcp` 查看是否已连接。`codex plugin list` 核对安装。在 `/hooks` 审查并信任插件 Hook，
由 Codex 的信任流程和管理策略决定何时允许运行。
卸载使用 `codex plugin remove origence@origence-tools`，可选执行
`codex plugin marketplace remove origence-tools`。
[Codex 插件命令](https://learn.chatgpt.com/docs/cli/reference#codex-plugin)

### 验证连接和排查

在新会话调用 `context_session`，将返回的 `key_id` 和 `session_id` 与该 workspace 的
预期归档身份核对。工作区 UUID 可用同一 key 调用 `GET /v1/whoami` 核对。用
`context_search` 的 `keyword` 模式搜索管理员已发布的已知资料，再用命中结果的
`asset_id` 和 `version` 调用 `context_get`。两端均可使用这三个工具完成验证。

| 现象 | 检查方式 |
| --- | --- |
| Hook 找不到 `origence` | 在启动客户端的同一终端运行 `Get-Command origence -CommandType Application`（Windows）或 `command -v origence`。确认路径指向所选二进制。 |
| MCP 连接失败 | 在客户端的 `/mcp` 核对实际 `/mcp` URL。访问同一宿主的 `/health/ready` 检查服务是否就绪，远程接入同时核对反向代理的 Host 配置。 |
| 缺少 API Key | 用带有 workspace key 的环境重新启动客户端。 |
| `401` 或 `authentication required` | 请管理员核对 key 所属 workspace、有效性及撤销状态。 |
| 显示两个 Origence 服务 | 检查历史手工注册，停用重复入口后再试。 |

### 验证默认 Run Hook

用具有写权限的 key 启动客户端并完成一轮提示，再在同一工作区用该逻辑 key 检查私有 Run。
对 MCP 和 Hook 使用同一 `OC_API_KEY`、API 宿主和工作区。Hook 从 `OC_SERVER_URL` 和可选的
`OC_WORKSPACE_ID` 读取目标，省略工作区时使用该 Key 的默认工作区。Hook 只记录受支持事件的结构化元数据，跳过缺少可靠标识的事件。
会话正文、原始 transcript、工具参数和原始输出均不采集。

安装后无需另写项目 Hook 配置或启动发送器。每次 Hook 用至多两秒确认身份并尝试上报，
已入队的记录可在同项目、工作区和凭据的后续 Hook 重试。在身份确认前无法连接时，当前观察会跳过。
队列位于客户端的插件数据目录，Windows 的目录权限由客户端和用户环境管理。
已安装本机二进制时，可在启动前设置 `OC_PLUGIN_RUN_HOOK=0` 停止上报。
继续使用原有项目配置时，关闭插件 Hook 并沿用下文的独立发送器。

### 按项目开启私有问答增量归档

功能默认关闭。用本机 Origence 插件运行 Codex 或 Claude Code 时，将 `OC_TURN_SYNC_PROJECT` 设为项目根目录的绝对路径，
提供有写权限的 `OC_API_KEY`，并沿用 MCP 的 `OC_SERVER_URL` 和 `OC_WORKSPACE_ID`。
Git 项目可从仓库子目录启动。此开关独立于 `OC_PLUGIN_RUN_HOOK`。

插件在本地过滤 `<private>…</private>`、当前 Origence 凭据和常见凭据，只暂存启用后能配对的主会话问答。
原始会话文件、工具参数和输出不采集。首次暂存前需在线确认归档归属，后续可离线排队。
重试限于同一项目、服务、工作区和凭据，Windows 使用插件数据目录继承的 ACL。
移除变量并重启可暂停，已暂存的问答会在同一配置再次启用时重试。

同一工作区和逻辑 key 可通过 `context_bundle` 的私有 `session` 路线召回。
同步问答的证据使用状态为 `unknown`。见[归档契约](API.md#会话归档与幂等反馈)。

## 手工 MCP 接入

1. 使用项目所属 workspace 的个人 API key，通过启动环境提供 `OC_API_KEY`。
   不要把密钥写进配置文件、提交到项目或贴进对话。
2. 在示例中填写客户端能访问的宿主。Claude Code 可从 `OC_SERVER_URL` 读取宿主，Codex 需要填写带
   `/mcp` 的完整 URL。
3. 按客户端放置配置：
   - **Claude Code**：将 [JSON 示例](../examples/claude-code.mcp.json) 放到项目 `.mcp.json`，
     或把 `mcpServers.origence` 合并进现有配置。
   - **Codex**：将 [TOML 示例](../examples/codex.mcp.toml) 的 `[mcp_servers.origence]` 配置放入
     个人 Codex 配置或受信任项目的 `.codex/config.toml`，保留 `bearer_token_env_var = "OC_API_KEY"`。

在客户端用 `/mcp` 检查连接。调用 `context_session` 核对 `workspace_id`。如需选择其他有权访问的工作区，
可在启动前设置 `OC_WORKSPACE_ID`。两份示例均将该值映射为 HTTP 请求头。

显式归档可交给 `context_turn_archive`：它需要 writer 权限，并使用
一个由调用方生成、重试时保持不变的 turn UUID。另一个客户端在同一 workspace 使用同一逻辑 key，调用
`context_session` 会得到同一个平台归档，再用 `context_turn_get` 按 turn UUID
读取（需 writer 权限）。给两个客户端分别签发独立 key 时，它们的归档互不可见。手工 MCP 接入只处理
调用方提交的轮次，客户端的工作现场由各自宿主维护。
检索内容是外部证据，不是可直接执行的指令。

双客户端真实任务的验收进度见 [P0 验收 #91](https://github.com/TheLudlows/Origence/issues/91)。

## 用 Codex 非交互验证只读检索

在专用测试项目中，若已授权 Codex 使用 reader key 自动搜索和读取资料，可在该项目的
Codex MCP 配置中加入以下两项。使用 `codex exec` 且
`approval_policy = "never"` 时，需要交互审批的工具调用会在到达 Origence 前失败。

```toml
[mcp_servers.origence.tools.context_search]
approval_mode = "approve"

[mcp_servers.origence.tools.context_get]
approval_mode = "approve"
```

让 Codex 搜索一份已发布资料，再用命中的 `asset_id` 和 `version` 读取正文。
在 `codex exec --json` 的输出中核对两次 `mcp_tool_call` 的完成状态，并核对答案引用的版本。
如果搜索的完成事件显示 `failed` 且带有审批错误，先检查本次会话是否加载了上述工具配置。

## 记录私有 Trace

使用 `--server-url` 连接 HTTP 宿主后，可为任务建立并读取私有 Run。创建、准备和上报需要 writer 权限。

1. 用 `context_run_create` 提交稳定的 `run_id` 和脱敏任务摘要。
2. 用 `context_run_prepare` 准备上下文，服务端把精确引用和准备回执写入 Run。
3. 用 `context_run_report` 明确上报观察到的 Skill 加载、工具、结果和采集缺口。重试保留原 `event_id` 和请求内容。
4. 用 `context_run_get` 分页查看当前逻辑 key 的 Run 轨迹。

服务端回执记录上下文准备，客户端上报保留报告方，未采集活动保持未知。外部核验使用针对精确 Run 前缀的 HTTP 接口。详见 [Run 契约](contracts/runs.zh-CN.md)和[外部核验契约](contracts/run-verification.zh-CN.md)。

Claude Code 和 Codex 使用相同的 MCP 接口。排查一次跨客户端任务时，可按下表核对记录。

| 要核对的动作 | 在哪里核对 | 记录能证明什么 |
| --- | --- | --- |
| 显式归档与读回 | 两端的 `context_turn_archive`、`context_turn_get` 调用，以及按 turn ID 读取的服务端问答 | 指定问答已保存，同一逻辑 key 的另一端可以读回。 |
| 创建 Run、准备上下文 | `context_run_create`、`context_run_prepare` 回执和 `context_run_get` 事件 | Run 的归属、服务端准备的内容或失败原因。 |
| 使用上下文、加载 Skill、产生任务结果 | 客户端观测和 `context_run_report` 的报告方、事件与覆盖范围 | 客户端声明了哪些动作。缺少记录的步骤保持未知。 |
| 业务结果复核 | 与固定 Run 前缀关联的核验记录及其依据 | 谁转录了什么结论，以及是否已有独立签认。 |

### 复跑双客户端归档验收

安装已配置模型访问权限的 Claude Code 和 Codex CLI，并准备当前版本的 Origence 和固定版本的
AnySeek 可执行文件。在仓库根目录运行以下命令，将四个大写占位符替换为对应路径或模型名。

```sh
python tools/anyseek_smoke.py --anyseek-binary ANYSEEK_BINARY --run python tools/live_archive_smoke.py --binary ORIGENCE_BINARY --claude-model CLAUDE_MODEL --codex-model CODEX_MODEL --report archive-result.json
```

Claude Code 以 `--bare` 运行，从环境获取 Anthropic 凭据和端点。若已在个人设置中配置，
可在命令末尾加 `--claude-settings PATH`，从该 JSON 的 `env` 读取这组设置。
Codex 使用 OpenAI provider，可用 `--codex-credential-store` 选择已有登录凭据的存储方式。
每次运行会调用真实模型，在新的临时 workspace 中完成三次客户端会话。三次调用共享四分钟等待预算，
单次最多等待 85 秒。报告写入一个新文件，仅含模型选择、工具名、耗时和验收结果。

## 按项目启用私有 Run Hook

这是自管配置方式。每个项目使用一个由自己管理的 [Origence profile](../examples/client-hook.profile.json)，
示例 profile 初始关闭。
在项目外的私有目录保存副本。Unix 上目录设为 `0700`、文件设为 `0600`，Windows 上使用仅当前用户可读写的目录和 ACL。
Windows 版本不会创建、收紧或检查 DACL；已有宽权限目录或文件不会被程序拒绝。
这是调用方管理的前置条件，不是程序强制的权限保证。
Windows CI 的 `tools/test_hook_acl.ps1` 仅在独立临时目录显式建立私有 DACL，
使用合成 Hook 输入检查队列、临时文件及锁文件在失败重试/新进程后仍继承该权限。
宽权限负例只验证测试中的 ACL 检查器，不运行 Hook，也不代表产品会拒绝宽权限。
该检查不验证其他身份实际访问、管理员特权、真实客户端安装、事件覆盖或断电持久性。
填入本机的项目绝对路径、服务 origin、逻辑 key UUID 和该项目专用的凭据环境变量名。
`GET /v1/session` 或同一凭据的 MCP `context_session` 返回逻辑 `key_id`，轮换前后相同。
将该项目的 writer 凭据通过启动环境交给客户端和下述发送器。profile 与客户端 Hook 示例均只填环境变量名或 profile 路径。
在 `clients` 中选 `codex`、`claude_code` 或两者，然后将 `enabled` 设为 `true`。

- **Codex**：将 [Hook 示例](../examples/codex.hooks.json) 中所选事件的处理器合并到目标项目的
  `.codex/hooks.json`。已有内联 Hook 时沿用现有配置格式，避免同层重复声明。
  替换可执行文件和 profile 的绝对路径，按 Hook shell 的规则处理空格。
  先按 Codex 本身的流程信任项目，再在 `/hooks` 审查并信任所选 Hook 定义。
  [Codex 官方 Hook 文档](https://learn.chatgpt.com/docs/hooks)
- **Claude Code**：将 [Hook 示例](../examples/claude-code.hooks.json) 中所选事件的处理器合并到
  目标项目的 `.claude/settings.local.json`。替换 `command` 和 `args` 中的绝对路径。
  Windows 指向 `origence.exe` 并按 JSON 规则转义路径。在 `/hooks` 核对加载来源和处理器。
  [Claude Code 官方 Hook 文档](https://code.claude.com/docs/en/hooks)

仅追加对应处理器，保留现有配置和客户端的信任及管理策略。Hook 会核对实际进程工作目录和事件工作目录。
两者须属于该 profile 的项目。普通子目录可用，独立嵌套 Git 项目和项目外目录会被跳过。
启用 linked worktree 时为其设置专用 profile，并核对客户端实际加载的 Hook 配置。

在单独终端或本机用户级服务中为每个 profile 启动发送器。它可先于客户端启动，也可以在离线后重新启动。

```sh
origence client-hook --profile /absolute/private/project.json flush --watch
```

去掉 `--watch` 发送至多 16 条，便于手动运行或由本机调度器调用。Hook 进程只读一次有上限的 stdin，
以短锁写入最多 256 条待发元数据，随后结束。队列满、锁忙、本地写入失败、输入超限或缺少关联标识时，
Hook 保持客户端原流程，可能在 Hook 诊断输出中留下固定的跳过原因。Hook 不读取 transcript，也不保存
会话内容、路径、工具参数、原始输出、凭据或原始事件 JSON。待发队列和去重记录存放在 profile 同目录下
的 `<profile 文件名>.queue/`，只包含派生 ID、首个本地观察时间和明确允许的结构化元数据。

发送器使用现有私有 Run API，每批先用会话接口确认凭据对应的逻辑 key。网络或确认回执异常会保留原
`event_id` 和原负载重试。队列按项目、服务 origin、逻辑 key、凭据环境变量名和当时的凭据隔离。
`held` 是当前配置无法发送的记录数量，`blocked` 表示认证、授权、逻辑 key 或网络问题，
`discarded` 表示服务端明确拒绝或报告身份冲突。权限撤销及凭据轮换后，旧队列保持隔离。
轮换后的新事件仍关联同一逻辑 key 上的同一 Run。观察时已用旧凭据入队的事件可能永久缺失。
关闭 profile 或移出相应 `clients` 条目会在发送器下次读取配置时生效，在途批次仍使用其原凭据，
可停止发送器来结束该批次。需要清理滞留记录时，在项目 Hook 和发送器停用后移走整个私有队列目录。
已确认的记录保留最近 1024 个本地事件 ID。超过该窗口的重复通知仍使用相同 ID，由服务端的幂等约束
拒绝不同负载。Run 事件序号反映服务端确认顺序，时间为本地观察时间。

### 事件和关联边界

由应用直接驱动的任务可用[受管 Run 上报](MANAGED-RUNS.md)记录 Codex App Server 命令完成数据及 Claude Agent SDK 取消结果。

| 客户端 | 关联键 | 可记录的观察 | 覆盖缺口 |
| --- | --- | --- | --- |
| Codex | `session_id` 和 `turn_id` | `UserPromptSubmit`、允许名单内的 `PostToolUse`、`Stop`、主线程 `Interrupt`。Origence MCP 工具只使用结构化结果中实际给出的 `isError` | Bash 与补丁工具状态保留未知，无 Hook 耗时字段。无子代理生命周期、托管工具或异常进程退出来推定终止 |
| Claude Code | `session_id` 和当前用户提示的 UUID `prompt_id` | `UserPromptSubmit`、允许名单内的 `PostToolUse` 和 `PostToolUseFailure`、`Stop`、`StopFailure`，及同时含 `prompt_id` 和 `agent_id` 的 `SubagentStart`、`SubagentStop`。工具耗时及 abort 标记按实际字段记录 | `prompt_id` 的粒度是一条提示，转向或后续提示可使用新 UUID。整轮用户中断没有可靠信号，取消运行中的工具可跳过失败 Hook。`SessionEnd` 无轮次结论。子代理内部工具与后台任务不采集，子代理事件只代表收到时的提示上下文 |

精确工具名单由上面的 Hook 示例和代码同时限定。同一轮多次收到相同生命周期信号时会合并。
子代理同一种生命周期信号也按当前提示和代理 ID 合并，不能据此计算实际启动或停止次数。
`Stop` 表示客户端进入停机检查，其他 Hook 仍可延续该轮。中断和 API 错误均是客户端观察，
业务结果保持未知。没有末尾事件时保留采集缺口。缺少可靠标识时跳过该观察，包括只提供会话 ID 的
Claude Code 版本或事件。

### 显式关联 Origence MCP

如果受信任的调用方已经拥有当前 Hook 事件里的确切标识，可用相同 profile 确定这轮的
`run_id` 和固定 `summary`。Claude Code 使用 `--client claude-code`，并将
`prompt_id` 作为 `--correlation-id`。

```sh
origence client-hook --profile /absolute/private/project.json run-id \
  --client codex --session-id SESSION_ID --correlation-id TURN_ID
```

用相同逻辑 key 调用 MCP `context_run_create`，原样传入两字段，随后用这个 `run_id` 上报或准备上下文。
发送器先到或 MCP 先到都使用同一个私有 Run。现有 Run 工具不从 Codex 或 Claude Code 自动获得
这两个准确的关联标识。当调用方得不到它们时，允许 Hook 独立记录可识别的轮次。需要同轮合并时，
应省略该轮的独立 MCP Run 创建，继续使用无需 Run ID 的检索工具。旧的手动 Run、时间接近的调用和
工具原始参数均不参与关联。显式 Run 创建冲突会按原授权和幂等契约处理。

## 导出已批准的 Skill

获得 workspace 的 tenant/workspace 标识、已发布的 Skill ID 与精确版本后运行：

```sh
origence --server-url https://origence.example skill-export \
  --client codex --project /path/to/project \
  --tenant-id TENANT_ID --workspace-id WORKSPACE_ID SKILL_ID 1
```

Claude Code 改用 `--client claude-code`。程序向宿主重新核对 key、版本和来源，校验包摘要
与目标空间后，将文本写入 Codex 的 `.agents/skills/<name>/` 或 Claude Code 的
`.claude/skills/<name>/`，不会覆盖已有目录。只导出批准发布的文本与参考资料，
不写凭据、执行脚本或配置工具权限。两种客户端按自己的方式识别和加载 Skill；
文件写入不是客户端已加载的证据。

这是显式导出，不是在线受管加载。撤回会阻止后续下载和导出，不能撤回本地副本或已注入的
客户端上下文。把导出的目录留在该 workspace 的专用项目中，不要跨空间复用或提交给未获授权的人。

### Windows Hook 启用前只读权限检查

在停止 Hook 和发送器后，用 PowerShell 7 运行：

```powershell
pwsh -NoProfile -File tools/check_hook_acl.ps1 -PrivateRoot 'C:\Users\me\private-hooks' -ProfilePath 'C:\Users\me\private-hooks\project.json'
```

检查不读取 profile/队列正文或凭据，不启用 Hook，不改文件、配置或 ACL。成功输出
`SUPPORTED_ACL_SNAPSHOT`，不代表 profile 配置有效、服务可达或实际客户端已经接入。
仅支持普通绝对本地 NTFS 路径，拒绝 UNC、设备路径、ADS、歧义路径及路径上的 reparse point。
私有根须隔断继承且只有当前用户的一条 FullControl Allow ACE，并向文件/子目录继承；
到 profile 的目录、profile 以及已有队列/锁/暂存文件必须完整继承这一规则。
owner 仅支持当前用户或本机 Administrators；后者不增加普通用户的 DACL 授权。
额外 ACE（包括额外 SYSTEM/管理员授权）、自定义受保护子 ACL 或未知队列条目均报告不支持，
不自动修复，也不表示这些自定义布局一定不安全。队列不存在时只检查创建前提，不创建队列。

这是窄基线的只读时点检查，不是运行时强制校验或完整 Windows 有效权限审计。
根目录以上的祖先只检查 reparse point，不核验其 ACL；调用方仍须保证祖先位置可信且不能被
其他普通用户替换。检查不防御管理员/备份特权、并发路径替换或之后的权限变化，也不验证
其他用户的实际访问。遇到不支持布局请停止启用并自行核对权限，不要据此自动收紧用户目录。
