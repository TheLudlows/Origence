# Windows 原生开发启动

**简体中文** · [English](WINDOWS-DEV.en.md)

在 Windows Terminal 或原生控制台运行，使用 MSVC 构建 Origence。需要控制台时安装 Node.js 24（含 npm）。

## 启动 API 与嵌入式 AnySeek

用 `tools\build.ps1 -Action Build` 构建 Origence。首次启动在数据目录保存 `deployment-admin.key`，可用该凭据通过管理 API 创建工作空间。

```powershell
$env:OC_DATA_DIR = 'C:\dev-data\origence'
.\target\debug\origence.exe serve --local
```

默认 API 地址是 `http://127.0.0.1:8080`。Origence 通过 Rust SDK 打开 `OC_DATA_DIR\anyseek`，由同一进程管理目录恢复、检索和正常关闭。此前使用独立服务绑定的数据按原绑定启动。本地模式使用全新 Origence 目录。

## 同时启动 API 和控制台

从仓库根目录使用部署入口。选择空闲的 API 和控制台端口，并为新开发实例指定数据目录。

```powershell
.\bin\origence-deploy.cmd install --origence .\target\debug\origence.exe --endpoint http://127.0.0.1:8080 --ui-port 3000 --data-dir 'C:\dev-data\origence'
```

打开 `http://127.0.0.1:3000`，用安装器提供的管理员账号登录。结束开发时运行
`.\bin\origence-deploy.cmd uninstall` 停止安装器管理的服务。凭据和卸载行为见[安装说明](INSTALLER.md#本地部署-api-和控制台)。
