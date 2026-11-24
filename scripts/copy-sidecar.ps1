param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$manifestDir = Join-Path $root "src-tauri"
$hostLine = rustc -vV | Select-String "^host:"
if (-not $hostLine) {
    throw "rustc -vV 没有给出 host"
}
$triple = ($hostLine.ToString() -split "\s+")[-1]
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $manifestDir "target" }
$source = Join-Path $targetDir (Join-Path $Profile "daodian-shot.exe")
if (-not (Test-Path $source)) {
    throw "找不到 $source，先编译 daodian-shot"
}
$destDir = Join-Path $manifestDir "binaries"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$dest = Join-Path $destDir "daodian-shot-$triple.exe"
Copy-Item -Force $source $dest
Write-Output "copied $source -> $dest"
