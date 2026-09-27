param(
    [ValidateSet('Build', 'Test', 'Validate')][string]$Action = 'Validate',
    [int]$Jobs = 1,
    [int]$NativeJobs = 4
)
$ErrorActionPreference = 'Stop'
$buildRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$buildOldPath = $env:PATH
$buildOldProtoc = $env:PROTOC
Push-Location $buildRoot
try {
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue) -or -not (Get-Command ninja -ErrorAction SilentlyContinue)) {
        $buildVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $buildVs = & $buildVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $buildVs) { throw 'Install Visual Studio C++ build tools, CMake and Ninja.' }
        $env:PATH = "$buildVs/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin;$buildVs/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja;$env:PATH"
    }
    if (-not $env:PROTOC -and -not (Get-Command protoc -ErrorAction SilentlyContinue)) {
        $buildProtoc = & cargo run --manifest-path tools/storage-probe/Cargo.toml --locked --no-default-features --features build-tools --bin probe-protoc --target-dir target/storage-probe -j $Jobs
        if ($LASTEXITCODE -ne 0) { throw 'Cannot locate protoc.' }
        $env:PROTOC = $buildProtoc.Trim()
    }
    if ($Action -eq 'Validate') {
        & cargo fmt --all -- --check
        if ($LASTEXITCODE -ne 0) { throw 'Formatting failed.' }
        & cargo clippy --locked --all-targets -j $NativeJobs -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'Clippy failed.' }
    }
    # Compile native dependencies in parallel, but link executables conservatively.
    & cargo build --locked --lib -j $NativeJobs
    if ($LASTEXITCODE -ne 0) { throw 'Library build failed.' }
    if ($Action -eq 'Build') {
        & cargo build --locked -j $Jobs
    } else {
        & cargo test --locked -j $Jobs --no-fail-fast
    }
    if ($LASTEXITCODE -ne 0) { throw "Cargo $Action failed." }
} finally {
    $env:PATH = $buildOldPath
    $env:PROTOC = $buildOldProtoc
    Pop-Location
}
