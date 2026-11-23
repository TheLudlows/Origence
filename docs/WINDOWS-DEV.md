# Windows 原生开发启动

在 Windows Terminal 或原生控制台运行，使用 MSVC 构建的 Origence 和 AnySeek。需要 UI 时再安装 Node.js 24 和 PowerShell 7.4+。

## 启动 API 与本地 AnySeek

先用 `tools\build.ps1 -Action Build` 构建 Origence，再准备[兼容版本的 AnySeek](OPERATIONS.md#anyseek-全文与向量)。以下 `workspace-create` 只在首次初始化时执行一次，将返回的 workspace token 保存到私密凭据提供器。

```powershell
$env:OC_DATA_DIR = 'C:\dev-data\origence'
.\target\debug\origence.exe --offline workspace-create dev
.\target\debug\origence.exe serve --local --anyseek-bin C:\src\anyseek\target\debug\anyseek_server.exe
```

默认 API 地址是 `http://127.0.0.1:8080`，AnySeek 在 `127.0.0.1:8081` 以本地磁盘模式运行。Origence 管理 AnySeek 的启动、探测、凭据与退出，默认在 `OC_DATA_DIR\anyseek` 保存 Catalog。把 `anyseek_server.exe` 放到 `PATH` 可省略 `--anyseek-bin`。首次启动时 Catalog 目录由 AnySeek 创建。

沿用已有 Windows 联启数据时，设置原 `OC_DATA_DIR`，并传入原 Catalog 路径 `--anyseek-data-dir` 和原端口 `--anyseek-port`。端口与原 SQLite 后端绑定须匹配。

## 可选 Node UI

从仓库根目录构建并联启 UI。`-AnySeekExe` 可用 PATH 或 `OC_ANYSEEK_BIN` 代替，`-DataDir` 默认读取 `OC_DATA_DIR`。

```powershell
npm ci --ignore-scripts --prefix ui
npm run build --prefix ui
pwsh -NoProfile -File .\tools\start-dev.ps1 -AnySeekExe C:\src\anyseek\target\debug\anyseek_server.exe -Ui
```

脚本调用 `origence serve --local`，等 `/health/ready` 成功后启动 UI。打开 `http://127.0.0.1:3000`，输入当前用户的 Origence key。脚本禁用模型，支持 `-AnySeekDataDir` 与 `-AnySeekPort` 指向原 Catalog 和端口。需要模型时直接运行 `origence.exe serve --local` 并设置[模型环境变量](OPERATIONS.md#本地-embedding-服务)。

按 Ctrl+C 退出。Origence 先结束 HTTP 服务和 Worker，再终止拥有的 AnySeek 子进程。Windows 上当前 AnySeek 的关闭与重开限制见[运维说明](OPERATIONS.md#origence-本地模式)。脚本默认给组件 30 秒退出时间，可用 `-ShutdownTimeoutSeconds` 调整。
