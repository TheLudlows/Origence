param(
    [ValidateSet('Build', 'Test', 'Validate')][string]$Action = 'Validate',
    [int]$Jobs = 1,
    [int]$NativeJobs = 4
)
$ErrorActionPreference = 'Stop'
$buildRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$buildOldPath = $env:PATH
$buildOldProtoc = $env:PROTOC
$buildOldToolchain = $env:RUSTUP_TOOLCHAIN
Push-Location $buildRoot
try {
    # Native deps (Kuzu/LanceDB) must build with the MSVC toolchain. Two layers
    # are pinned, because each is checked by a different layer:
    #   1. VS dev shell -> cl/ml64/CMake/Ninja + INCLUDE/LIB, so cmake and cc-rs
    #      find MSVC instead of a mingw g++/gas earlier on PATH.
    #   2. The MSVC rustc toolchain (`*-pc-windows-msvc`) -> `target.env == "msvc"`,
    #      which is what cc-rs checks to route `.asm` files to ml64 (psm/stacker).
    # A gnu rustc with `CC=cl` compiles Kuzu but breaks psm: cc-rs sees a gnu
    # target (no ml64 routing) yet psm picks `.asm` because the compiler is
    # msvc-like. Forcing the MSVC toolchain makes both layers agree.
    $buildVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $buildVs = & $buildVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $buildVs) { throw 'Install Visual Studio C++ build tools (with CMake and Ninja).' }
    $buildVsDevShell = Join-Path $buildVs 'Common7\Tools\Launch-VsDevShell.ps1'
    if (-not (Test-Path $buildVsDevShell)) { throw "VS dev shell not found: $buildVsDevShell" }
    & $buildVsDevShell -Arch amd64 -HostArch amd64 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "VS dev shell failed (exit $LASTEXITCODE)." }
    # Pin the MSVC rustc toolchain so `target.env == "msvc"`. `rustup toolchain
    # list` (no `--installed` flag) lists installed toolchains; match by exact
    # name to avoid the "(default)" suffix tripping a plain -eq.
    $buildMsvcToolchain = 'stable-x86_64-pc-windows-msvc'
    $buildToolchains = rustup toolchain list 2>$null
    if (-not ($buildToolchains | Where-Object { $_ -match ('(?m)^\s*' + [regex]::Escape($buildMsvcToolchain) + '\b') })) {
        throw "MSVC rustc toolchain not installed: $buildMsvcToolchain (run: rustup toolchain install $buildMsvcToolchain)."
    }
    $env:RUSTUP_TOOLCHAIN = $buildMsvcToolchain
    # Belt and suspenders at the cmake layer: stop a stray mingw g++ on PATH from
    # being probed even if a future toolchain change reintroduces a gnu host.
    $env:CC = 'cl.exe'
    $env:CXX = 'cl.exe'
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
    $env:RUSTUP_TOOLCHAIN = $buildOldToolchain
    Pop-Location
}
