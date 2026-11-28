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

按提示输入 API 地址和工作区 Key，Key 输入不回显。本机 API 通常是 `http://127.0.0.1:8080`，远程地址使用 HTTPS，省略 `/mcp`。可用 `--endpoint` 和 `--token` 传参，自动化优先通过启动环境传入 `OC_SERVER_URL` 和 `OC_API_KEY` 后加 `--from-env`，避免 Key 进入命令行历史。MCP 检索和 Run Hook 上报使用工作区 Key。Hook 使用插件自带的 Node.js 实现。

接入脚本核对 API、Key 所属工作区和 MCP 工具，再安装 Agent 插件，保存客户端与 Hook 所需的用户环境和 PATH。Codex 非默认地址通过官方 CLI 配置，已有冲突的 MCP 配置会报错并要求用户处理。接入脚本只使用已有 API，负责把 Node.js 放进客户端的 PATH。

Windows 的 Agent 接入仅在用户环境变量 `OC_API_KEY` 中持久化工作空间 Key。默认重装优先读取该用户变量，找不到时使用当前进程环境或提示输入。更新接入配置时，安装器移除旧 Agent 安装记录中的 Key 原文，只保留用于识别已设置值的摘要。卸载清除安装器设置且仍匹配的 Key，保留安装前已有的相同值及后来手工更改的值。主动替换 Key 时会丢弃旧值。

使用可写 Key 接入时，`origence-setup` 默认[按项目自动归档会话](CLIENTS.md#按项目自动归档会话)。首次安装默认选择所在 Git 项目的根目录，非 Git 项目使用当前目录。优先沿用安装器保存的选择，其次使用已有的 `OC_TURN_SYNC_PROJECT`。可用 `--turn-sync-project <目录>` 指定、更换或恢复归档项目，用 `--no-turn-sync` 暂停。两个客户端共用设置，只归档启用后所选项目的新问答。

完成后重开终端和客户端，运行 `/mcp` 检查连接，调用 `context_session` 核对工作区。在 Codex 的 `/hooks` 按客户端提示审阅和信任已安装的 Hook。已有 Agent 共用一组地址和 Key，更换时用 `--force` 并选择所有已接入的 Agent。

卸载可在重开的终端运行 `origence-setup uninstall`，或从仓库中运行对应平台的入口加 `uninstall`。它撤销本次添加的插件、MCP 覆盖和用户环境修改。服务与控制台由部署入口管理。

## 本地部署 API 和控制台

本地安装会启动 API 和控制台。控制台默认使用 `127.0.0.1:3000`，可通过 `--ui-port` 修改。复用本地数据时用 `--data-dir` 或 `OC_DATA_DIR` 指定原目录。

```powershell
.\bin\origence-deploy.cmd install --endpoint http://127.0.0.1:8080 --ui-port 3001
```

```sh
./bin/origence-deploy install --endpoint http://127.0.0.1:8080 --ui-port 3001
```

控制台在安装时取得部署管理员凭据后启用账号密码登录。只提供工作区 Key 时，使用该 Key 登录控制台。

首次启动自动创建部署管理员。安装器为控制台的 `admin` 账号设置随机密码，通过交互输出、`--credentials-file` 或显式 `--show-secrets` 获取登录信息。用管理员账号创建工作空间，再用该空间的 Key 接入 Agent。已有服务可以用工作空间 Key 接入，也可以设置 `ORIGENCE_UI_DEPLOYMENT_ADMIN_KEY` 以管理员身份安装控制台。`--verify-write` 需要提供有写权限的工作空间 Key。

从仓库运行 `./bin/origence-deploy uninstall` 或 `.\bin\origence-deploy.cmd uninstall`。它停止部署脚本管理的进程，清理其管理的账号配置，并保留数据库、备份、已导出的凭据文件和 Agent 插件。以前的一键安装记录会按所属范围读取，完整撤销旧安装时分别执行两个卸载命令。日志与私有状态位于 Windows 的 `%LOCALAPPDATA%\Origence\installer` 或 Unix 的 `~/.origence/installer`。

[部署与恢复](OPERATIONS.md) · [Claude Code 与 Codex 接入](CLIENTS.md)
