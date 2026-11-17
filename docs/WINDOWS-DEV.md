# Windows 原生开发联启（不用 WSL）

支持口径：**PowerShell 7.4+（pwsh）、Windows Terminal/原生 Console 前台运行**，
不支持 Windows PowerShell 5.1、ISE、服务模式或隐藏后台控制台。脚本也可用于 Linux pwsh
的回归，但那不等于 Windows 实测。Node UI 需要 Node.js 24；API/AnySeek 使用已构建
的本机可执行文件。联启不执行 build、npm install、初始化或迁移。

## 常规启动

从 Origence 仓库根目录，先通过当前进程环境/私密凭据提供器设置：

```powershell
$env:MASTER_KEY_SECRET = Read-Host -MaskInput 'AnySeek existing master secret'
$env:OC_ANYSEEK_API_KEY = Read-Host -MaskInput 'Existing ordinary Tenant key (base64)'
$env:OC_ANYSEEK_COLLECTION = 'origence-dev'
$env:OC_DATA_DIR = 'C:\dev-data\origence'
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\start-dev.ps1 -AnySeekExe C:\src\anyseek\target\debug\anyseek_server.exe -AnySeekDataDir C:\dev-data\anyseek -Ui
```

`-ExecutionPolicy Bypass` 仅用于本次已审阅的本地脚本进程，不修改机器/用户策略。
这是复用既有数据与普通 key 的启动命令，不能把新生成的 master secret 用于已有 AnySeek。
秘密不作为参数、不写入脚本、配置、日志或 Git；勿开启 PowerShell transcript。
UI 在浏览器输入当前用户的 Origence key，启动器不接收、不转发该业务 token。
`-Ui` 可省略；`-OrigenceExe` 默认为仓库 `target/debug/origence.exe`，可指定 release 路径。
`-NodeExe` 默认为 PATH 中的 node；`-DataDir` 默认读取 OC_DATA_DIR。

默认地址：AnySeek `127.0.0.1:8081`、API `127.0.0.1:8080`、UI `127.0.0.1:3000`。
可用 `-AnySeekPort/-ApiPort/-UiPort` 改端口；已有 Origence 的后端绑定必须仍匹配，
不能用改端口绕过绑定检查。仅 loopback，无对外监听开关。

AnySeek 必须用当前兼容 pin
`d813b915b7f12e7139ca316adc15699688493196` 构建，或由操作者验证等价合同。
脚本先用普通 key GET `/api/v1/index/{collection}` 验证已授权的 collection 元信息，
不把 `/live` 或 TCP 监听当 ready；Origence 再做完整 schema/analysis/diagnostics
兼容性核验，脚本等 `/health/ready` 返回 `status=ready` 后才启动可选 UI。
ready 不保证模型质量；本开发入口明确设置 `OC_ENABLE_MODELS=false`，用于无模型全文流程。
模型/AML/向量专项配置请用手动启动流程，不会静默继承调用者的 OC_* 配置。

## 一次性初始化（与启动器分开）

仅对**明确选定的全新测试目录、Tenant、key ID 和专用 collection**执行下面操作。
已初始化时跳过，不重复创建或覆盖；已有库不迁移，需遵守 fresh-only 与同版本重开合同。
凭据请保存到自己的私密提供器；以下变量仅留在会话内，关窗即丢失。不要把 token 打印到
共享终端、日志或文档。所有 HTTP 请求指向本机，禁止把示例改为生产地址直接执行。

先准备构建工具与可执行文件：

```powershell
# Origence：Visual Studio C++ Build Tools / MSVC Rust，仓库根目录
.\tools\build.ps1 -Action Build
npm ci --ignore-scripts --prefix ui
npm run build --prefix ui
# AnySeek：另行检出上述 pin，在它的 MSVC Developer PowerShell 中构建
# cargo +1.99.1-x86_64-pc-windows-msvc build --locked --no-default-features --bin anyseek_server -j 2
```

AnySeek 构建命令需要操作者已安装对应 MSVC toolchain；不能以 Linux 构建结果证明
Windows 原生构建通过。也可直接指定可信的该 pin 已构建 exe。

1. 在第一个原生终端中，选择尚不存在的 AnySeek 数据路径并临时以前台初始化（由 AnySeek 创建；不能预建空目录）：

```powershell
if (Test-Path C:\dev-data\anyseek) { throw 'Choose a new AnySeek data path' }
# C:\dev-data 父目录需事先存在；AnySeek 在这次显式初始化中创建 anyseek 子目录。
$env:MASTER_KEY_SECRET = Read-Host -MaskInput 'New private AnySeek master secret'
$env:ANYSEEK_STORAGE_BACKEND = 'local'
& C:\src\anyseek\target\debug\anyseek_server.exe local_ip=127.0.0.1 local_port=8081 index_path=C:\dev-data\anyseek
```

2. 在第二个 pwsh 终端、Origence 仓库根目录执行。提供上一步同一 master secret；
普通 Tenant key 为新生成的 32 随机字节，master 只用于这次管理请求。

```powershell
$env:MASTER_KEY_SECRET = Read-Host -MaskInput 'Same private master secret'
$master = [Convert]::ToBase64String([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($env:MASTER_KEY_SECRET)))
$env:OC_ANYSEEK_API_KEY = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$env:OC_ANYSEEK_COLLECTION = 'origence-dev'
$base = 'http://127.0.0.1:8081'
function New-Operation { @{ epoch = [long][Math]::Floor([DateTimeOffset]::UtcNow.ToUnixTimeSeconds()/86400); nonce = [guid]::NewGuid().ToString('N') } }
function Send-Setup($Path, $Key, $Body) {
    Invoke-RestMethod -Method Post -Uri "$base$Path" -Headers @{apikey=$Key} -ContentType 'application/json' -Body ($Body | ConvertTo-Json -Depth 30) -NoProxy -MaximumRedirection 0 -ErrorAction Stop | Out-Null
}
Send-Setup '/api/v1/tenants' $master @{operation_id=(New-Operation); tenant_id='origence-dev'}
Send-Setup '/api/v1/apikey' $master @{operation_id=(New-Operation); tenant_id='origence-dev'; key_id='dev-app'; apikey_base64=$env:OC_ANYSEEK_API_KEY}
$configText = & .\target\debug\origence.exe anyseek-schema
if ($LASTEXITCODE -ne 0) { throw 'Schema command failed' }
Send-Setup '/api/v1/index' $env:OC_ANYSEEK_API_KEY @{operation_id=(New-Operation); collection_id=$env:OC_ANYSEEK_COLLECTION; config=($configText | ConvertFrom-Json)}
Remove-Variable master
$env:OC_DATA_DIR = 'C:\dev-data\origence'
New-Item -ItemType Directory $env:OC_DATA_DIR -ErrorAction Stop | Out-Null
$workspaceText = & .\target\debug\origence.exe --offline workspace-create dev
if ($LASTEXITCODE -ne 0) { throw 'Workspace initialization failed' }
$workspace = $workspaceText | ConvertFrom-Json
Remove-Variable workspaceText
# $workspace.token 是新 workspace admin key；保存到私密提供器，勿直接输出。
# 同样保存 OC_ANYSEEK_API_KEY 和 MASTER_KEY_SECRET，常规重启复用它们。
```

上述 workspace 管理入口是离线 SQLite 初始化，不启动 Worker。需要更低角色时可独立
用现有 key-create 流程签发，见 [Operations](OPERATIONS.md#初始化与凭据)。
在第一个终端按 Ctrl+C，等待 AnySeek 完全退出；再在第二个终端运行“常规启动”的一条
联启命令（当前会话已持有环境变量）。启动器不会自动启动额外控制台。

## 停止、错误与数据边界

前台 Ctrl+C 由共享 Windows 控制台广播给 pwsh 和子进程。Origence 与固定 pin 的
AnySeek 都支持 Ctrl+C 优雅停止；脚本在 finally 中依次等待 UI、Origence、AnySeek。
每个进程默认等 30 秒（`-ShutdownTimeoutSeconds`），超过后明确提示并只对自己持有的
Process 句柄强停，不按名字/端口杀进程、不杀其他服务、不删除目录或锁文件。

此机制依赖控制台信号传播，**尚未在真实 Windows 验证**。启动失败、信号未传播或
任务耗时超过等待窗口会走强停；强停不能保证 Worker 完成，重启需走既有恢复流程。
直接关闭终端、kill pwsh 或机器掉电不保证 finally 执行，可能留下子进程；这不是
Windows 服务管理器。端口检查不是原子预留，竞争失败会通过子进程退出/ready 超时报告。

每阶段默认 ready 超时 60 秒（`-StartupTimeoutSeconds`；单次 HTTP 最多 2 秒）。
检查全部启用端口无占用、文件/已初始化目录存在后才启动；401/403/404/重定向直接失败。
仅持有锁的现有进程也会导致新进程退出，脚本不会接管它。常规启动不生成目录、key、
Tenant 或 collection，不发管理/写入 HTTP 请求。底层服务正常重开/恢复可能写入其数据。

子进程输出被持续丢弃，不保存完整日志，避免把内部诊断或密钥传到联启终端。
早退报告组件名称与退出码；需要进一步定位时在私密终端单独运行该 exe，按现有脱敏规则
收集诊断。不要把完整环境变量或令牌粘贴到 issue。

## 回归

`tools/test_start_dev.py` 是 opt-in 的 Linux pwsh + 模拟子服务测试，默认跳过，未改变 CI。

```sh
ORIGENCE_TEST_PWSH=/path/to/pwsh python3 -m unittest discover -s tools -p test_start_dev.py -v
```

真实 Windows 待验：MSVC exe 启动、同控制台 Ctrl+C 广播、带空格路径、ready 阶段中断、
提前退出/端口占用、退出后原数据可重开及没有遗留进程。Linux 实测与准确限制见 VALIDATION。
