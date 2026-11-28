# Native development on Windows

**English** · [简体中文](WINDOWS-DEV.md)

Use Windows Terminal or a native console and build Origence with MSVC. Install Node.js 24 with npm if you use the console.

## Start the API and embedded AnySeek

Build Origence with `tools\build.ps1 -Action Build`. First startup places `deployment-admin.key` in the data directory. Use this credential with the administration API to create a workspace.

```powershell
$env:OC_DATA_DIR = 'C:\dev-data\origence'
.\target\debug\origence.exe serve --local
```

The default API address is `http://127.0.0.1:8080`. Origence opens `OC_DATA_DIR\anyseek` through the Rust SDK. The same process manages directory recovery, retrieval, and graceful shutdown. Start data previously bound to a standalone service using its original binding. Use a fresh Origence directory for local mode.

## Start the API and console together

Run the deployment command from the repository root. Pick free API and console ports and specify a data directory for the new development instance.

```powershell
.\bin\origence-deploy.cmd install --origence .\target\debug\origence.exe --endpoint http://127.0.0.1:8080 --ui-port 3000 --data-dir 'C:\dev-data\origence'
```

Open `http://127.0.0.1:3000` and sign in with the administrator account from the installer.
Run `.\bin\origence-deploy.cmd uninstall` to stop the services managed by the installer. See the [installation guide](INSTALLER.en.md#deploy-the-local-api-and-console) for credentials and uninstall behavior.
