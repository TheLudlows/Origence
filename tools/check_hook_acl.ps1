# Read-only snapshot of a deliberately narrow caller-managed Windows ACL baseline.
param(
    [Parameter(Mandatory)][string]$PrivateRoot,
    [Parameter(Mandatory)][string]$ProfilePath
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'Windows PowerShell 7 is required.' }

function Local-Path([string]$Value) {
    # Reject UNC/device paths, ADS, ambiguous normalization and wildcard syntax.
    if ($Value -notmatch '^[A-Za-z]:\\' -or $Value.Substring(2) -match '[:*?\[\]]' -or
        $Value -match '/' -or $Value -match '\\(?:\.|\.\.)(?:\\|$)' -or
        $Value -match '[ .](?:\\|$)') { throw 'Unsupported path syntax; use a normal absolute local path.' }
    $path = [System.IO.Path]::GetFullPath($Value).TrimEnd('\')
    $drive = [System.IO.DriveInfo]::new([System.IO.Path]::GetPathRoot($path))
    if ($drive.DriveType -ne 'Fixed' -or $drive.DriveFormat -ne 'NTFS') { throw 'Only fixed local NTFS paths are supported.' }
    return $path
}

function No-Reparse([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw 'Reparse points are unsupported.'
    }
    return $item
}

$root = Local-Path $PrivateRoot
$profile = Local-Path $ProfilePath
if ($root.Length -le 3 -or -not $profile.StartsWith($root + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Profile must be below the specified private root.'
}
# Inspect every existing ancestor before descending into it; do not resolve links.
$parts = $profile.Substring(3).Split('\')
$walk = $profile.Substring(0, 3)
[void](No-Reparse $walk)
foreach ($part in $parts) {
    $walk = Join-Path $walk $part
    [void](No-Reparse $walk)
}
if (-not (No-Reparse $root).PSIsContainer -or (No-Reparse $profile).PSIsContainer) {
    throw 'Expected a private directory and a regular profile file.'
}
$current = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$inherit = [System.Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
$count = 0
function Check-Acl([string]$Path, [bool]$IsRoot = $false) {
    $item = No-Reparse $Path
    $acl = Get-Acl -LiteralPath $Path
    # Administrators is a privileged owner, not an additional DACL grant.
    if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -notin @($current, 'S-1-5-32-544')) {
        throw 'Unsupported owner.'
    }
    $rules = @($acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
    if ($rules.Count -ne 1) { throw 'Expected exactly one current-user allow ACE.' }
    $rule = $rules[0]
    $flags = if ($item.PSIsContainer) { $inherit } else { [System.Security.AccessControl.InheritanceFlags]::None }
    if ($rule.IdentityReference.Value -ne $current -or $rule.AccessControlType -ne 'Allow' -or
        $rule.FileSystemRights -ne [System.Security.AccessControl.FileSystemRights]::FullControl -or
        $rule.InheritanceFlags -ne $flags -or $rule.PropagationFlags -ne 'None' -or
        $rule.IsInherited -eq $IsRoot -or $acl.AreAccessRulesProtected -ne $IsRoot) {
        throw 'ACL does not match the supported private inheritance baseline.'
    }
    $script:count++
}
Check-Acl $root $true
$walk = [System.IO.Path]::GetDirectoryName($profile)
while (-not $walk.Equals($root, [System.StringComparison]::OrdinalIgnoreCase)) {
    Check-Acl $walk
    $walk = [System.IO.Path]::GetDirectoryName($walk)
}
Check-Acl $profile
$queue = $profile + '.queue'
$queueStatus = 'absent'
# Enumerate the already checked parent, so a dangling queue reparse point is
# never mistaken for absence by a target-following existence lookup. Errors stop.
$queueEntries = @(Get-ChildItem -LiteralPath ([System.IO.Path]::GetDirectoryName($profile)) -Force |
    Where-Object { $_.Name.Equals([System.IO.Path]::GetFileName($queue), [System.StringComparison]::OrdinalIgnoreCase) })
if ($queueEntries.Count -gt 1) { throw 'Ambiguous queue entry.' }
$entry = if ($queueEntries.Count -eq 1) { $queueEntries[0] } else { $null }
if ($entry) {
    if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Queue reparse point is unsupported.' }
    if (-not (No-Reparse $queue).PSIsContainer) { throw 'Queue must be a directory.' }
    Check-Acl $queue
    $entries = @(Get-ChildItem -LiteralPath $queue -Force)
    if ($entries.Count -gt 4) { throw 'Unexpected queue entries.' }
    foreach ($file in $entries) {
        if ($file.Name -cnotin @('queue-v1.json', 'queue-v1.tmp', 'queue.lock', 'sender.lock') -or $file.PSIsContainer) {
            throw 'Unexpected queue entry.'
        }
        Check-Acl $file.FullName
    }
    $queueStatus = 'checked'
}
@{ status = 'SUPPORTED_ACL_SNAPSHOT'; checked_objects = $count; queue = $queueStatus
   boundary = 'Read-only DACL snapshot, not runtime enforcement or protection against privileged users, untrusted ancestors or concurrent path/ACL changes.' } | ConvertTo-Json -Compress
