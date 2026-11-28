# Native development on Windows

**English** · [简体中文](WINDOWS-DEV.md)

Use Windows Terminal or a native console and build Origence with MSVC. Node.js 24 and PowerShell 7.4+ are required if you use the UI.

## Start the API and embedded AnySeek

Build Origence with `tools\build.ps1 -Action Build`. First startup places `deployment-admin.key` in the data directory. Use it to sign in to the console and create a workspace.

```powershell
$env:OC_DATA_DIR = 'C:\dev-data\origence'
.\target\debug\origence.exe serve --local
```

The default API address is `http://127.0.0.1:8080`. Origence opens `OC_DATA_DIR\anyseek` through the Rust SDK. The same process manages directory recovery, retrieval, and graceful shutdown. Start data previously bound to a standalone service using its original binding. Use a fresh Origence directory for local mode.

## Optional Node UI

Build and start the UI from the repository root. `-DataDir` defaults to `OC_DATA_DIR`.

```powershell
npm ci --ignore-scripts --prefix ui
npm run build --prefix ui
pwsh -NoProfile -File .\tools\start-dev.ps1 -Ui
```

The script runs `origence serve --local` and waits for `/health/ready` before starting the UI. Open `http://127.0.0.1:3000` and enter the current user's Origence key. If you need a model, run `origence.exe serve --local` directly and set the [model environment variables](OPERATIONS.en.md#local-embedding-service).

After Ctrl+C, Origence shuts down HTTP and the Worker, then asks the SDK to close the local Catalog. The script allows 30 seconds for shutdown by default. Use `-ShutdownTimeoutSeconds` to adjust it.
