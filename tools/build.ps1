param(
    [ValidateSet('Build', 'Test', 'Validate')][string]$Action = 'Validate',
    [int]$Jobs = 1,
    [int]$NativeJobs = 4
)
$ErrorActionPreference = 'Stop'
$buildRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$buildOldPath = $env:PATH
$buildOldToolchain = $env:RUSTUP_TOOLCHAIN
Push-Location $buildRoot
try {
    # LanceDB and SQLite native dependencies need the MSVC dev shell and
    # MSVC Rust target so cc-rs routes assembly to ml64 (psm/stacker).
    $buildVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $buildVs = & $buildVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $buildVs) { throw 'Install Visual Studio C++ build tools.' }
    $buildVsDevShell = Join-Path $buildVs 'Common7\Tools\Launch-VsDevShell.ps1'
    if (-not (Test-Path $buildVsDevShell)) { throw "VS dev shell not found: $buildVsDevShell" }
    & $buildVsDevShell -Arch amd64 -HostArch amd64 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "VS dev shell failed (exit $LASTEXITCODE)." }
    # Pin the MSVC rustc toolchain so `target.env == "msvc"`. `rustup toolchain
    # list` (no `--installed` flag) lists installed toolchains; match by exact
    # name to avoid the "(default)" suffix tripping a plain -eq.
    $buildMsvcToolchain = '1.98.0-x86_64-pc-windows-msvc'
    $buildToolchains = rustup toolchain list 2>$null
    if (-not ($buildToolchains | Where-Object { $_ -match ('(?m)^\s*' + [regex]::Escape($buildMsvcToolchain) + '\b') })) {
        throw "MSVC rustc toolchain not installed: $buildMsvcToolchain (run: rustup toolchain install $buildMsvcToolchain)."
    }
    $env:RUSTUP_TOOLCHAIN = $buildMsvcToolchain
    $env:CC = 'cl.exe'
    $env:CXX = 'cl.exe'
    # Lance builds protoc from source; only the native build toolchain is needed.
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
        throw 'Install CMake (or the Visual Studio C++ CMake tools component).'
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
    $env:RUSTUP_TOOLCHAIN = $buildOldToolchain
    Pop-Location
}
