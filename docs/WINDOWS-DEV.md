# Windows 原生开发启动

**简体中文** · [English](WINDOWS-DEV.en.md)

在 Windows Terminal 或原生控制台运行，使用 MSVC 构建 Origence。需要 UI 时再安装 Node.js 24 和 PowerShell 7.4+。

## 启动 API 与嵌入式 AnySeek

用 `tools\build.ps1 -Action Build` 构建 Origence。首次启动在数据目录保存 `deployment-admin.key`，用它登录控制台并创建工作空间。

```powershell
$env:OC_DATA_DIR = 'C:\dev-data\origence'
.\target\debug\origence.exe serve --local
```

默认 API 地址是 `http://127.0.0.1:8080`。Origence 通过 Rust SDK 打开 `OC_DATA_DIR\anyseek`，由同一进程管理目录恢复、检索和正常关闭。此前使用独立服务绑定的数据按原绑定启动。本地模式使用全新 Origence 目录。

## 可选 Node UI

从仓库根目录构建并联启 UI。`-DataDir` 默认读取 `OC_DATA_DIR`。

```powershell
npm ci --ignore-scripts --prefix ui
npm run build --prefix ui
pwsh -NoProfile -File .\tools\start-dev.ps1 -Ui
```

脚本调用 `origence serve --local`，等 `/health/ready` 成功后启动 UI。打开 `http://127.0.0.1:3000`，输入当前用户的 Origence key。需要模型时直接运行 `origence.exe serve --local` 并设置[模型环境变量](OPERATIONS.md#本地-embedding-服务)。

按 Ctrl+C 后，Origence 先结束 HTTP 服务和 Worker，再调用 SDK 关闭本地 Catalog。脚本默认给组件 30 秒退出时间，可用 `-ShutdownTimeoutSeconds` 调整。
