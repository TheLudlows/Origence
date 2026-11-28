# Claude Code 与 Codex 接入

**简体中文** · [English](CLIENTS.en.md)

Claude Code 和 Codex 通过 Origence 服务端的 Streamable HTTP MCP 检索。可直接注册端点，或安装带自动 Run Hook 的插件。

用 [`origence-setup`](INSTALLER.md#接入-agent) 配置 Claude Code 或 Codex 的插件和 Run Hook，并按项目自动归档会话。部署本地 API 和控制台使用 `origence-deploy`。

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
使用自动 Run Hook 时准备 `writer` Key 和支持 `origence plugin-hook` 的本机可执行文件。管理员在控制台或通过管理 API 签发所需凭据。

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

### 按项目自动归档会话

`origence-setup install` 使用可写 Key 时，默认将 `OC_TURN_SYNC_PROJECT` 设为安装时所在项目的根目录。
手工安装插件时，在客户端的启动环境将它设为项目根目录的绝对路径。提供可写的 `OC_API_KEY`，并沿用 MCP 的
`OC_SERVER_URL` 和 `OC_WORKSPACE_ID`。Git 项目可从仓库子目录启动。此开关独立于 `OC_PLUGIN_RUN_HOOK`。

插件在本地过滤 `<private>…</private>`、当前 Origence 凭据和常见凭据，只暂存启用后能配对的主会话问答。
原始会话文件、工具参数和输出不采集。首次暂存前需在线确认归档归属，后续可离线排队。
重试限于同一项目、服务、工作区和凭据，Windows 使用插件数据目录继承的 ACL。
安装器配置可用 `origence-setup install --no-turn-sync` 暂停（传入原来的 `--agent` 选项），然后重启终端和客户端。
手工配置可清空变量并重启客户端。已暂存的问答会在同一配置再次启用时重试。

已保存的问答可在控制台的「会话」中查看。同一工作区和逻辑 Key 可通过 `context_bundle` 的私有 `session` 路线召回。
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
`context_session` 会得到同一个 Origence 会话，再用 `context_turn_get` 按 turn UUID
读取（需 writer 权限）。给两个客户端分别签发独立 Key 时，它们的会话记录互不可见。手工 MCP 接入只处理
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

## 事件和关联边界

由应用直接驱动的任务可用[受管 Run 上报](MANAGED-RUNS.md)记录 Codex App Server 命令完成数据及 Claude Agent SDK 取消结果。

| 客户端 | 关联键 | 可记录的观察 | 覆盖缺口 |
| --- | --- | --- | --- |
| Codex | `session_id` 和 `turn_id` | `UserPromptSubmit`、允许名单内的 `PostToolUse`、`Stop`、主线程 `Interrupt`。Origence MCP 工具只使用结构化结果中实际给出的 `isError` | Bash 与补丁工具状态保留未知，无 Hook 耗时字段。无子代理生命周期、托管工具或异常进程退出来推定终止 |
| Claude Code | `session_id` 和当前用户提示的 UUID `prompt_id` | `UserPromptSubmit`、允许名单内的 `PostToolUse` 和 `PostToolUseFailure`、`Stop`、`StopFailure`，及同时含 `prompt_id` 和 `agent_id` 的 `SubagentStart`、`SubagentStop`。工具耗时及 abort 标记按实际字段记录 | `prompt_id` 的粒度是一条提示，转向或后续提示可使用新 UUID。整轮用户中断没有可靠信号，取消运行中的工具可跳过失败 Hook。`SessionEnd` 无轮次结论。子代理内部工具与后台任务不采集，子代理事件只代表收到时的提示上下文 |

精确工具名单由插件 Hook 配置和代码限定。同一轮多次收到相同生命周期信号时会合并。
子代理同一种生命周期信号也按当前提示和代理 ID 合并，不能据此计算实际启动或停止次数。
`Stop` 表示客户端进入停机检查，其他 Hook 仍可延续该轮。中断和 API 错误均是客户端观察，
业务结果保持未知。没有末尾事件时保留采集缺口。缺少可靠标识时跳过该观察，包括只提供会话 ID 的
Claude Code 版本或事件。
