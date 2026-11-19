# Synthetic, caller-managed ACL acceptance only. No installed coding client/model.
param([string]$Binary = 'target/debug/origence.exe')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'This check is restricted to GitHub-hosted Windows runners.'
}
$binaryPath = (Resolve-Path $Binary).Path
$root = Join-Path $env:RUNNER_TEMP ('origence-hook-acl-' + [guid]::NewGuid())
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$inherit = [System.Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
$allow = [System.Security.AccessControl.AccessControlType]::Allow
$full = [System.Security.AccessControl.FileSystemRights]::FullControl
$expectedOwner = $sid.Value
$acl = [System.Security.AccessControl.DirectorySecurity]::new()
$acl.SetOwner($sid)
$acl.SetAccessRuleProtection($true, $false)
$acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($sid, $full, $inherit, 'None', $allow))

function Assert-PrivateAcl([string]$Path, [bool]$Inherited = $true) {
    $actual = Get-Acl -LiteralPath $Path
    if ($actual.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne $expectedOwner) {
        throw 'Unexpected fixture owner.'
    }
    $rules = @($actual.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
    if ($rules.Count -ne 1) { throw 'Unexpected fixture ACE count.' }
    $rule = $rules[0]
    if ($rule.IdentityReference.Value -ne $sid.Value -or $rule.AccessControlType -ne $allow -or
        $rule.FileSystemRights -ne $full -or $rule.IsInherited -ne $Inherited -or
        $rule.PropagationFlags -ne [System.Security.AccessControl.PropagationFlags]::None -or
        $actual.AreAccessRulesProtected -eq $Inherited) {
        throw 'Unexpected fixture DACL.'
    }
    $expectedInheritance = if ((Get-Item -LiteralPath $Path).PSIsContainer) { $inherit } else { [System.Security.AccessControl.InheritanceFlags]::None }
    if ($rule.InheritanceFlags -ne $expectedInheritance) { throw 'Unexpected ACE inheritance flags.' }
}

function Invoke-Hook([string]$Action, [string]$InputJson = '') {
    $info = [System.Diagnostics.ProcessStartInfo]::new($binaryPath)
    $info.WorkingDirectory = $project
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($arg in @('client-hook', '--profile', $profile, $Action)) { $info.ArgumentList.Add($arg) }
    if ($Action -eq 'capture') {
        $info.ArgumentList.Add('--client'); $info.ArgumentList.Add('codex')
    }
    $info.Environment['ORIGENCE_ACL_TEST_KEY'] = 'fixture-only-acl-token'
    $process = [System.Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.StandardInput.Write($InputJson)
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(20000)) { $process.Kill($true); throw 'Hook fixture timed out.' }
        if ($process.ExitCode -ne 0) { throw 'Hook fixture exited unsuccessfully.' }
        return @{ Out = $stdout.GetAwaiter().GetResult(); Err = $stderr.GetAwaiter().GetResult() }
    } finally { $process.Dispose() }
}

$listener = $null
$held = $null
try {
    # Create with the specified protected DACL, not create-then-repair inherited permissions.
    [System.IO.FileSystemAclExtensions]::Create([System.IO.DirectoryInfo]::new($root), $acl)
    Assert-PrivateAcl $root $false
    $project = Join-Path $root 'project'
    $profiles = Join-Path $root 'profiles'
    [void][System.IO.Directory]::CreateDirectory($project)
    [void][System.IO.Directory]::CreateDirectory($profiles)
    # Elevated tokens can use Administrators as the default owner. Observe a
    # caller-created child before the app runs; DACL checks remain exact.
    $expectedOwner = (Get-Acl -LiteralPath $profiles).GetOwner([System.Security.Principal.SecurityIdentifier]).Value
    Assert-PrivateAcl $profiles
    # Reserve a loopback port but deliberately never reply. No remote service or real credential.
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $profile = Join-Path $profiles 'project.json'
    @{
        enabled = $true; clients = @('codex'); project = $project
        server_url = "http://127.0.0.1:$($listener.LocalEndpoint.Port)"
        logical_key_id = '550e8400-e29b-41d4-a716-446655440001'
        key_env = 'ORIGENCE_ACL_TEST_KEY'
    } | ConvertTo-Json | Set-Content -LiteralPath $profile -Encoding utf8NoBOM
    Assert-PrivateAcl $profile
    $events = Get-Content (Join-Path $PSScriptRoot '../tests/fixtures/client-hooks/codex.json') -Raw | ConvertFrom-Json -AsHashtable
    $events.start.cwd = $project
    $events.stop.cwd = $project
    $result = Invoke-Hook 'capture' ($events.start | ConvertTo-Json -Depth 20 -Compress)
    if ($result.Err) { throw 'Initial capture unexpectedly skipped.' }
    $queue = Join-Path $profiles 'project.json.queue'
    $state = Join-Path $queue 'queue-v1.json'
    $staging = Join-Path $queue 'queue-v1.tmp'
    foreach ($path in @($queue, $state, (Join-Path $queue 'queue.lock'))) { Assert-PrivateAcl $path }
    $before = [System.IO.File]::ReadAllText($state)
    if (($before | ConvertFrom-Json).pending.Count -ne 1) { throw 'Expected one pending event.' }

    # Refuse delete-sharing so the app's real staging write succeeds but rename cannot replace state.
    $held = [System.IO.File]::Open($state, 'Open', 'Read', 'Read')
    $result = Invoke-Hook 'capture' ($events.stop | ConvertTo-Json -Depth 20 -Compress)
    if (-not $result.Err.Contains('cannot publish hook queue')) { throw 'Expected controlled rename failure.' }
    Assert-PrivateAcl $staging
    Assert-PrivateAcl $state
    if ([System.IO.File]::ReadAllText($state) -ne $before) { throw 'Failed rename altered published state.' }
    $held.Dispose(); $held = $null
    $result = Invoke-Hook 'capture' ($events.stop | ConvertTo-Json -Depth 20 -Compress)
    if ($result.Err -or (Test-Path -LiteralPath $staging)) { throw 'Capture retry did not publish staging.' }
    Assert-PrivateAcl $state
    if ((Get-Content -LiteralPath $state -Raw | ConvertFrom-Json).pending.Count -ne 2) { throw 'Retry lost an event.' }
    $before = [System.IO.File]::ReadAllText($state)
    $result = Invoke-Hook 'flush'
    $report = $result.Out | ConvertFrom-Json
    if ($report.sent -ne 0 -or $report.blocked -ne 'host_unavailable') { throw 'Expected offline sender result.' }
    foreach ($path in @($queue, $state, (Join-Path $queue 'queue.lock'), (Join-Path $queue 'sender.lock'))) { Assert-PrivateAcl $path }
    if ([System.IO.File]::ReadAllText($state) -ne $before) { throw 'Offline send changed pending bytes.' }
    if ($before.Contains('FIXTURE_PRIVATE') -or $before.Contains('fixture-only-acl-token')) { throw 'Unexpected sensitive fixture field in spool.' }

    # Empty detector control only: no profile, Hook invocation, credential or queued data here.
    $control = Join-Path $root 'empty-detector-control'
    [void][System.IO.Directory]::CreateDirectory($control)
    $wide = Get-Acl -LiteralPath $control
    $everyone = [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0')
    $wide.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($everyone, 'ReadAndExecute', $allow))
    Set-Acl -LiteralPath $control -AclObject $wide
    $rejected = $false
    try { Assert-PrivateAcl $control } catch { $rejected = $true }
    if (-not $rejected) { throw 'ACL assertion failed to detect the wider empty control.' }
    Write-Output 'PASS: caller-managed private DACL inherited by synthetic Hook queue, staging and locks across retry/restart; ACL detector rejects empty wider control. Product ACL enforcement, second-user access and real clients NOT_TESTED.'
} finally {
    if ($held) { $held.Dispose() }
    if ($listener) { $listener.Stop() }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
