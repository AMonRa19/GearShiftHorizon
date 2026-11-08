$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
Write-Host 'Building FH6 Virtual TCU (Release)...' -ForegroundColor Cyan
cargo build --release
$exe = Join-Path $PSScriptRoot 'target\release\fh6-virtual-tcu-rust.exe'
if (!(Test-Path $exe)) { throw "EXE was not produced: $exe" }
$out = Join-Path $PSScriptRoot 'dist'
$data = Join-Path $out 'data'
New-Item -ItemType Directory -Force -Path $data | Out-Null
foreach ($name in @("tcu_config.json", "tcu_calibration.json")) {
    $old = Join-Path $PSScriptRoot $name
    $new = Join-Path $data $name
    if ((Test-Path $old) -and !(Test-Path $new)) { Copy-Item $old $new }
}
Copy-Item $exe (Join-Path $out 'FH6-Virtual-TCU.exe') -Force
Write-Host "Build complete: $out\FH6-Virtual-TCU.exe" -ForegroundColor Green
Write-Host "Settings and calibration will be saved in: $data" -ForegroundColor Green
Write-Host 'Distribute FH6-Virtual-TCU.exe and the data folder together.'
