param(
  [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$Tauri = Join-Path $Root "src-tauri"
$BundleDir = Join-Path $Tauri "target\$Configuration\bundle\msi"

Push-Location $Tauri
cargo tauri build
Pop-Location

if (!(Test-Path $BundleDir)) {
  throw "MSI output directory not found: $BundleDir"
}

$Msi = Get-ChildItem $BundleDir -Filter "*.msi" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($null -eq $Msi) {
  throw "MSI package not found in: $BundleDir"
}

Write-Host "Windows MSI build ready:"
Write-Host $Msi.FullName
