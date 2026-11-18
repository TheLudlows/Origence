# Claude Code 与 Codex 接入

两个客户端都通过本地 stdio 进程访问运行中的 Origence HTTP 宿主。客户端不直接访问
SQLite、Blob 或 AnySeek，也不需要它们的凭据。

1. 使用项目所属 workspace 的个人 API key，通过启动环境提供 `OC_API_KEY`。
   不要把密钥写进配置文件、提交到项目或贴进对话。
2. 将示例的 `command` 改为本地 Origence 可执行文件的绝对路径，将
   `--server-url` 改为客户端能访问的宿主地址。Windows 上指向 `origence.exe`。
3. 按客户端放置配置：
   - **Claude Code**：将 [JSON 示例](../examples/claude-code.mcp.json) 放到项目
     `.mcp.json`，或把 `mcpServers.origence` 合并进现有配置。在 Claude Code
     中使用 `/mcp` 检查连接。
   - **Codex**：将 [TOML 示例](../examples/codex.mcp.toml) 中的
     `[mcp_servers.origence]` 合并到个人 Codex 配置，或放入受信任项目的
     `.codex/config.toml`。保留 `env_vars = ["OC_API_KEY"]` 以显式传递凭据；
     `codex mcp list` 只能核对配置是否注册，不能验证实际连接。

用 `context_search` 搜索一份已发布资料，再用 `context_get` 读取指定版本并核对
来源引用。显式归档可交给 `context_turn_archive`：它需要 writer 权限，并使用
一个由调用方生成、重试时保持不变的 turn UUID。另一个客户端使用同一 key，调用
`context_session` 会得到同一个平台归档，再用 `context_turn_get` 按 turn UUID
读取（需 writer 权限）。给两个客户端分别签发独立 key 时，它们的归档互不可见。平台不会自动同步
客户端的本地聊天记录，也不会恢复客户端的工作现场。
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

服务端回执记录上下文准备，客户端上报保留报告方，未采集活动保持未知。外部核验使用针对精确 Run 前缀的 HTTP 接口。详见 [Run 契约](contracts/runs.md)和[外部核验契约](contracts/run-verification.md)。

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
