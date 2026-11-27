# Origence 安装与本地部署

**简体中文** · [English](INSTALLER.en.md)

从源码检出或包含 `bin/`、`tools/`、`ui/` 和插件 Marketplace 的发行目录运行。两个脚本均需 Node.js 24（含 npm）。需要构建 Origence 时使用仓库指定的 Rust 工具链和系统编译环境，也可通过 `--origence <路径>` 复用已有二进制。首次获取依赖需要相应软件源可达。

| 目标 | 入口 | 所需输入 |
| --- | --- | --- |
| 让 Claude Code 或 Codex 接入已有 Origence | `origence-setup` | Agent 类型、运行中的 API 地址、工作区 Key |
| 在本机运行 Origence API 和控制台 | `origence-deploy` | 新工作区名称，或现有 API 地址和工作区 Key |

## 接入 Agent

从仓库根目录运行，对应系统的入口可以直接带安装选项。

```powershell
.\bin\origence-setup.cmd install --agent claude
.\bin\origence-setup.cmd install --agent codex
```

```sh
./bin/origence-setup install --agent claude
./bin/origence-setup install --agent codex
```

`--agent both` 可用同一地址和 Key 一次接好两端。省略 `--agent` 时安装 Claude Code。两端复用各自已安装的 CLI，或通过 npm 获取固定版本。若 CLI 不在 PATH，使用 `--claude` 或 `--codex` 指定路径。[客户端接入细节](CLIENTS.md)

按提示输入 API 地址和工作区 Key，Key 输入不回显。本机 API 通常是 `http://127.0.0.1:8080`，远程地址使用 HTTPS，省略 `/mcp`。可用 `--endpoint` 和 `--token` 传参，自动化优先通过启动环境传入 `OC_SERVER_URL` 和 `OC_API_KEY` 后加 `--from-env`，避免 Key 进入命令行历史。只检索使用 `reader`，Run Hook 上报要求写权限。Hook 仍由本机 Origence 二进制处理。

接入脚本核对 API、Key 所属工作区和 MCP 工具，再安装 Agent 插件，保存客户端与 Hook 所需的用户环境和 PATH。Codex 非默认地址通过官方 CLI 配置，已有冲突的 MCP 配置会报错并要求用户处理。接入脚本只使用已有 API，不需要控制台端口。

完成后重开终端和客户端，运行 `/mcp` 检查连接，调用 `context_session` 核对工作区。在 Codex 的 `/hooks` 按客户端提示审阅和信任已安装的 Hook。已有 Agent 共用一组地址和 Key，更换时用 `--force` 并选择所有已接入的 Agent。

卸载可在重开的终端运行 `origence-setup uninstall`，或从仓库中运行对应平台的入口加 `uninstall`。它撤销本次添加的插件、MCP 覆盖和用户环境修改。服务与控制台由部署入口管理。

## 本地部署 API 和控制台

全新本机实例使用 `--init` 创建工作区。控制台默认使用 `127.0.0.1:3000`，占用时可指定 `--ui-port`。已有工作区时省略 `--init`，输入原 API 地址和 Key。复用已停止的服务时，可用 `--data-dir` 或 `OC_DATA_DIR` 指定原目录。

```powershell
.\bin\origence-deploy.cmd install --init demo --endpoint http://127.0.0.1:8080 --ui-port 3001
```

```sh
./bin/origence-deploy install --init demo --endpoint http://127.0.0.1:8080 --ui-port 3001
```

新部署会签发工作空间 Key 和独立部署管理员 Key，并验证两种控制台登录。工作空间 Key 同时用于 Agent 和控制台。管理员 Key 可查看整个部署并审核全局知识。首次初始化通过交互输出、`--credentials-file <Git 目录之外的新文件>` 或显式加 `--show-secrets` 获取新发凭据。已部署的管理员 Key 可由操作员离线轮换。连接现有角色凭据时继续支持控制台账号登录。加 `--verify-write` 可创建并从控制台读回一条标记明确的测试 Run，重复执行会复用同一条。

重复执行会复用部署。只有显式执行 `--force --init <名称>` 才会重建工作区，现有数据目录先改名备份。重建前应停止使用该目录的外部服务。

从仓库运行 `./bin/origence-deploy uninstall` 或 `.\bin\origence-deploy.cmd uninstall`。它停止部署脚本管理的进程，清理其管理的账号配置，并保留数据库、备份、已导出的凭据文件和 Agent 插件。以前的一键安装记录会按所属范围读取，完整撤销旧安装时分别执行两个卸载命令。日志与私有状态位于 Windows 的 `%LOCALAPPDATA%\Origence\installer` 或 Unix 的 `~/.origence/installer`。

[部署与恢复](OPERATIONS.md) · [Claude Code 与 Codex 接入](CLIENTS.md)
