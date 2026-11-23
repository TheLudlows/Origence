#requires -Version 7.4
[CmdletBinding()]
param(
    [string]$DataDir = $env:OC_DATA_DIR,
    [string]$OrigenceExe = (Join-Path $PSScriptRoot '../target/debug/origence.exe'),
    [switch]$Ui,
    [string]$NodeExe = 'node',
    [ValidateRange(1, 65535)][int]$ApiPort = 8080,
    [ValidateRange(1, 65535)][int]$UiPort = 3000,
    [ValidateRange(1, 600)][int]$StartupTimeoutSeconds = 60,
    [ValidateRange(1, 300)][int]$ShutdownTimeoutSeconds = 30
)
# Origence owns AnySeek. This script adds the optional Node UI in the same console.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$children = [System.Collections.Generic.List[object]]::new()
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.UseProxy = $false
$handler.AllowAutoRedirect = $false
$http = [System.Net.Http.HttpClient]::new($handler)
$http.Timeout = [TimeSpan]::FromSeconds(2)
$http.MaxResponseContentBufferSize = 1048576

function Require-Path([string]$Value, [string]$Kind, [string]$Label) {
    if (-not $Value -or -not (Test-Path -LiteralPath $Value -PathType $Kind)) {
        throw "$Label must already exist; see docs/WINDOWS-DEV.md."
    }
    return (Resolve-Path -LiteralPath $Value).Path
}
function Assert-FreePort([int]$Port) {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try { $listener.Start() } catch { throw "Loopback port $Port is unavailable; no existing service will be stopped or reused." }
    finally { $listener.Stop() }
}
function Assert-Children {
    foreach ($child in $children) {
        if ($child.Process.HasExited) {
            throw "$($child.Name) exited (code $($child.Process.ExitCode)). Child output is suppressed; diagnose separately in a private console."
        }
    }
}
function Start-Owned([string]$Name, [string]$Exe, [string[]]$Arguments, [hashtable]$Environment) {
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Exe
    $info.WorkingDirectory = $root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $false # inherit this console, never request a new one
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    # Do not pass backend/admin/model credentials to either child.
    foreach ($envName in @($info.Environment.Keys)) {
        if ($envName -like 'OC_*' -or $envName -like 'ORIGENCE_*' -or
            $envName -like 'ANYSEEK_*' -or $envName -eq 'MASTER_KEY_SECRET') {
            [void]$info.Environment.Remove($envName)
        }
    }
    foreach ($envName in $Environment.Keys) { $info.Environment[$envName] = $Environment[$envName] }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $info
    try { [void]$process.Start() } catch { $process.Dispose(); throw "$Name could not start; check executable and OS dependencies." }
    $child = [pscustomobject]@{ Name = $Name; Process = $process }
    $children.Add($child)
    # Drain without storing data or invoking a PowerShell callback on another thread.
    $null = $process.StandardOutput.BaseStream.CopyToAsync([System.IO.Stream]::Null)
    $null = $process.StandardError.BaseStream.CopyToAsync([System.IO.Stream]::Null)
}
function Test-Ready([string]$Url, [string]$Kind) {
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $Url)
    $response = $null
    try {
        try { $response = $http.SendAsync($request).GetAwaiter().GetResult() }
        catch { return $false }
        $status = [int]$response.StatusCode
        if ($status -in @(401,403,404) -or ($status -ge 300 -and $status -lt 400)) {
            throw "$Kind readiness rejected (HTTP $status); check service credentials or endpoint."
        }
        if ($status -ne 200) { return $false }
        try {
            $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            if ($Kind -eq 'UI') { return $text.Contains('<html') }
            $body = $text | ConvertFrom-Json
            return $body.status -eq 'ready'
        } catch { return $false }
    } finally {
        if ($null -ne $response) { $response.Dispose() }
        $request.Dispose()
    }
}
function Wait-Ready([string]$Url, [string]$Name) {
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt $StartupTimeoutSeconds) {
        Assert-Children
        if (Test-Ready $Url $Name) { Assert-Children; Write-Host "$Name ready."; return }
        [System.Threading.Thread]::Sleep(200)
    }
    throw "$Name readiness timed out after $StartupTimeoutSeconds seconds (a listening port is insufficient)."
}

try {
    $OrigenceExe = Require-Path $OrigenceExe Leaf 'Origence executable'
    $ports = @($ApiPort)
    if ($Ui) { $ports += $UiPort }
    if (@($ports | Select-Object -Unique).Count -ne $ports.Count) { throw 'Service ports must be distinct.' }
    foreach ($port in $ports) { Assert-FreePort $port }
    if ($Ui) {
        $NodeExe = (Get-Command $NodeExe -CommandType Application -ErrorAction Stop).Source
        $uiEntry = Require-Path (Join-Path $root 'ui/dist/server.js') Leaf 'Built UI entry'
    }
    $arguments = @('serve', '--local', '--bind', "127.0.0.1:$ApiPort")
    if ($DataDir) {
        $arguments += @('--data-dir', $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($DataDir))
    }
    Start-Owned 'Origence' $OrigenceExe $arguments @{ OC_ENABLE_MODELS = 'false'; RUST_LOG = 'error' }
    Wait-Ready "http://127.0.0.1:$ApiPort/health/ready" 'Origence'
    if ($Ui) {
        Start-Owned 'UI' $NodeExe @($uiEntry) @{
            ORIGENCE_API_BASE_URL = "http://127.0.0.1:$ApiPort"; ORIGENCE_UI_HOST = '127.0.0.1'; ORIGENCE_UI_PORT = "$UiPort"
        }
        Wait-Ready "http://127.0.0.1:$UiPort/" 'UI'
    }
    Write-Host "Dev services ready: API http://127.0.0.1:$ApiPort (models disabled). Ctrl+C to stop."
    if ($Ui) { Write-Host "UI http://127.0.0.1:$UiPort" }
    while ($true) { Assert-Children; [System.Threading.Thread]::Sleep(250) }
} finally {
    # Ctrl+C is broadcast by the shared Windows console. Allow owned processes
    # time to finish. On failure/no signal or timeout, terminate only these handles.
    for ($i = $children.Count - 1; $i -ge 0; $i--) {
        $child = $children[$i]
        try {
            if (-not $child.Process.HasExited -and -not $child.Process.WaitForExit($ShutdownTimeoutSeconds * 1000)) {
                Write-Warning "$($child.Name) did not stop within the grace period; forcing this owned process only."
                if (-not $child.Process.HasExited) { $child.Process.Kill() }
                if (-not $child.Process.WaitForExit(5000)) {
                    Write-Warning "$($child.Name) exit is unconfirmed; inspect owned PID $($child.Process.Id) manually."
                }
            }
        } catch {
            Write-Warning "Could not confirm $($child.Name) cleanup; inspect owned PID $($child.Process.Id) manually."
        } finally { $child.Process.Dispose() }
    }
    $http.Dispose()
    $handler.Dispose()
}
