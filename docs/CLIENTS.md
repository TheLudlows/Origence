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

## 记录私有 Trace

使用 `--server-url` 连接 HTTP 宿主后，可为任务建立并读取私有 Run。创建、准备和上报需要 writer 权限。

1. 用 `context_run_create` 提交稳定的 `run_id` 和脱敏任务摘要。
2. 用 `context_run_prepare` 准备上下文，服务端把精确引用和准备回执写入 Run。
3. 用 `context_run_report` 明确上报观察到的 Skill 加载、工具、结果和采集缺口。重试保留原 `event_id` 和请求内容。
4. 用 `context_run_get` 分页查看当前逻辑 key 的 Run 轨迹。

服务端回执记录上下文准备，客户端上报保留报告方，未采集活动保持未知。外部核验使用针对精确 Run 前缀的 HTTP 接口。详见 [Run 契约](contracts/runs.md)和[外部核验契约](contracts/run-verification.md)。

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
