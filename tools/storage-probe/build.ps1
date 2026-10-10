param(
    [int]$Jobs = 4,
    [string]$Toolchain = '1.98.1',
    [string]$TargetDirectory = 'target/storage-probe'
)

$ErrorActionPreference = 'Stop'
$probeRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$probeManifest = Join-Path $PSScriptRoot 'Cargo.toml'
$probeTarget = Join-Path $probeRoot $TargetDirectory
$probeOldPath = $env:PATH
$probeOldProtoc = $env:PROTOC

try {
    $probeProtoc = & cargo "+$Toolchain" run --manifest-path $probeManifest --locked --offline --no-default-features --features build-tools --bin probe-protoc --target-dir $probeTarget -j $Jobs
    if ($LASTEXITCODE -ne 0) { throw 'Failed to locate the vendored protoc executable.' }
    $env:PROTOC = $probeProtoc.Trim()
    & cargo "+$Toolchain" build --manifest-path $probeManifest --locked --offline --target-dir $probeTarget -j $Jobs
    if ($LASTEXITCODE -ne 0) { throw 'Rust storage probe build failed.' }
} finally {
    $env:PATH = $probeOldPath
    $env:PROTOC = $probeOldProtoc
}
