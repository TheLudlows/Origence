# Origence 本地接入安装器

**简体中文** · [English](INSTALLER.en.md)

从 Origence 源码检出或包含 `bin/`、`tools/`、`ui/` 和插件 Marketplace 的发行目录运行。Windows 使用 PowerShell，Linux 和 macOS 使用 Bash 或 Zsh。需要 Node.js 24（含 npm）。安装器会复用现有 Origence 与 Claude Code，或者从检出目录构建 Origence、用 npm 安装固定版本的 Claude Code。需要构建 Origence 时准备仓库指定的 Rust 工具链与系统编译环境，也可通过 `--origence` 指定已构建版本。缺少 Node UI 构建时，安装器运行 `npm ci` 并构建。首次获取依赖需要对应软件源可达。

## 安装

有现成 API 和 Key 时，在仓库或发行目录运行安装命令。Key 输入不回显。

```sh
./bin/origence-setup install
```

Windows PowerShell 使用以下入口。

```powershell
.\bin\origence-setup.cmd install
```

后续示例使用 `origence-setup`。首次安装时用上面对应系统的路径替换命令名。

也支持 `origence-setup install --endpoint https://origence.example.com --token '<WORKSPACE_KEY>'`。命令参数可能进入 shell 历史或进程列表。自动化可通过启动环境提供 `OC_SERVER_URL` 和 `OC_API_KEY`，再运行：

```sh
origence-setup install --from-env
```

远程 API 使用 HTTPS。只有本机回环 HTTP 地址能由安装器启动后端或创建工作区。复用已停止的本地实例时，安装器使用已知数据目录、`OC_DATA_DIR` 或检出目录中已有的 `.data/context.db`，也可用 `--data-dir <路径>` 指定原目录。已有实例的 Key 由工作区管理员签发。控制台账号名称为 `admin`，实际工作区权限随提供的 Key。Reader 可登录检索，私有 Run 上报需要 writer、reviewer 或 admin。

全新本地实例可在交互中留空 Key 并输入工作区名称，或直接运行：

```sh
origence-setup install --init demo --endpoint http://127.0.0.1:8080
```

新建工作区的 Key 只在交互初始化时自动显示一次，登录密码在交互成功时显示。非交互使用 `--credentials-file <Git 目录之外的新文件>` 或显式加 `--show-secrets` 取回。凭据文件包含明文密码与 Key，应保存在受保护的位置，勿提交或共享。Unix 私有状态文件权限为 `0600`，Windows 安装器为状态目录和导出文件设置当前用户 ACL。用户级环境变量对以当前用户启动的程序可读。

安装器输出逐步 JSON 结果。它会核对 API 的 `/health/ready` 和 `/v1/whoami`、MCP `initialize`、11 个工具及 `context_session`、控制台 HTTP 首页和真实账号会话，以及 Claude Code 自己报告的 HTTP MCP 连接。工作区 UUID 和逻辑 Key 由服务返回，并在 API、MCP 和控制台三端核对。安装器记录该 UUID，避免遗留的 `OC_WORKSPACE_ID` 把插件指向其他工作区。

## 验证、重复运行与卸载

安装后重开终端和 Claude Code。在 PowerShell 用 `Test-Path Env:OC_API_KEY`，在 Bash 或 Zsh 用 `test -n "$OC_API_KEY"` 核对 Key 已加载，再运行 `origence --version`。在 Claude Code 的 `/mcp` 查看 Origence 连接，用 `/hooks` 检查默认 Run Hook。用输出的地址和账号密码登录（默认 `http://127.0.0.1:3000`）。带写权限运行一轮 Claude Code 后，在该工作区的 Run 页面查看上报。要在安装时真实写入并经控制台会话读回，可加 `--verify-write`。每个工作区和逻辑 Key 只生成一条有明确标签的测试 Run，重复执行复用它。

重复执行相同命令会复用工作区、账号、插件和服务。更换 Key 或已安装的地址时加 `--force`。它会刷新安装配置和控制台密码。只有同时指定 `--force --init <名称>` 才会初始化新工作区；现有数据目录先改名为带时间戳的备份。重建前应停止所有外部 Origence 进程。安装器检查本地监听与已有数据库锁，且只停止自己启动的进程。自管服务在关闭安装终端后继续运行，重新登录或重启机器后可再次运行安装器启动。

重开终端后，可以在任意目录执行卸载。

```sh
origence-setup uninstall
```

若短命令不可用，可从当前检出目录用上文的系统入口执行 `uninstall`。原来的 `node tools/install.mjs --uninstall` 也可继续使用。

安装器的私有状态和服务日志位于 Windows 的 `%LOCALAPPDATA%\Origence\installer` 或 Unix 的
`~/.origence/installer`。卸载撤销所记录的用户环境和 PATH 修改，移除本次安装增加的 Claude 插件和控制台账号，停止安装器启动的进程。保留数据目录、备份和显式导出的凭据文件。在 Windows 使用当前用户注册表，在 Unix 通过 shell 启动文件加载私有环境文件。由用户独立修改的 PATH 条目或启动脚本内容会保留。

| 结果 | 处理方式 |
| --- | --- |
| API 不可达或未就绪 | 核对 endpoint 与本地数据路径，远程检查 HTTPS 证书。安装器启动进程的日志位于私有安装器状态目录。 |
| HTTP 401 或 403 | 确认 Key 的工作区和权限。初次接入已有数据时需要该工作区的有效凭据。 |
| UI 端口被占用 | 使用 `--ui-port <端口>` 选择其他端口。外部运行的控制台不会被改写。 |
| Hook 无运行记录 | 核对新终端中的 `origence --version`、Key 写权限与 `/hooks`。若设置过 `OC_PLUGIN_RUN_HOOK=0`，请去掉该关闭配置。 |
| CLI 插件受策略限制 | 在 Claude Code 审查并启用 Origence 插件，或联系组织管理员。 |

[Claude Code 插件命令](https://code.claude.com/docs/en/plugins/cli-reference) · [客户端接入](CLIENTS.md)
