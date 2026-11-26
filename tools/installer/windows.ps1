function Write-Json($value) {
    $writer = [IO.StreamWriter]::new([Console]::OpenStandardOutput(), $utf8)
    $writer.WriteLine((ConvertTo-Json -InputObject $value -Compress -Depth 4))
    $writer.Flush()
}

if ($request.action -eq 'protect') {
    $owner = [Security.Principal.WindowsIdentity]::GetCurrent().User
    if ($request.file) {
        $acl = [Security.AccessControl.FileSecurity]::new()
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($owner, 'FullControl', 'Allow')
    } else {
        $acl = [Security.AccessControl.DirectorySecurity]::new()
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($owner, 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow')
    }
    $acl.SetOwner($owner)
    $acl.SetAccessRuleProtection($true, $false)
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $request.path -AclObject $acl
    exit
}
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
try {
    if ($request.action -eq 'machine-path') {
        Write-Json @{ value = [Environment]::GetEnvironmentVariable('Path', 'Machine') }
    } elseif ($request.action -eq 'read') {
        $values = @{}
        foreach ($name in $request.names) {
            $value = $key.GetValue($name, $null, 'DoNotExpandEnvironmentNames')
            $values[$name] = if ($null -ne $value) {
                @{ value = [string]$value; kind = $key.GetValueKind($name).ToString() }
            } else { $null }
        }
        Write-Json $values
    } elseif ($request.action -eq 'write') {
        foreach ($value in $request.values.PSObject.Properties) {
            if ($null -eq $value.Value) { $key.DeleteValue($value.Name, $false) }
            else {
                $kind = [Microsoft.Win32.RegistryValueKind]$value.Value.kind
                $key.SetValue($value.Name, [string]$value.Value.value, $kind)
            }
        }
        Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OrigenceEnvironment {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wparam,
        string lparam, uint flags, uint timeout, out UIntPtr result);
}
'@
        $result = [UIntPtr]::Zero
        [void][OrigenceEnvironment]::SendMessageTimeout([IntPtr]65535, 26, [UIntPtr]::Zero, 'Environment', 2, 1000, [ref]$result)
    } else { throw 'Unknown installer operation' }
} finally { $key.Dispose() }
