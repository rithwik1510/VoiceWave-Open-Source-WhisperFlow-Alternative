param(
  [int]$Minutes = 30
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-CargoPath {
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargo) {
    return $cargo.Source
  }
  $fallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
  if (Test-Path $fallback) {
    return $fallback
  }
  throw "Cargo not found. Install Rust toolchain first."
}

function Ensure-GnuRustToolchain {
  $rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
  if (-not (Test-Path $rustup)) {
    throw "rustup not found. Install Rust toolchain first."
  }
  $toolchain = "stable-x86_64-pc-windows-gnu"
  $installed = & $rustup toolchain list | Select-String -Pattern $toolchain -SimpleMatch
  if (-not $installed) {
    & $rustup toolchain install $toolchain
    if ($LASTEXITCODE -ne 0) { throw "failed installing $toolchain" }
  }
}

function Add-MingwToPathIfAvailable {
  $wingetMingw = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
  if (Test-Path $wingetMingw) {
    $env:PATH = "$wingetMingw;$env:PATH"
  }
}

function Get-BatteryPercent {
  $battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $battery) { return $null }
  return [int]$battery.EstimatedChargeRemaining
}

function Get-ThermalCelsius {
  $zones = Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue
  if (-not $zones) { return @() }
  return $zones | ForEach-Object {
    [math]::Round(($_.CurrentTemperature / 10.0) - 273.15, 2)
  }
}

$cargoExe = Resolve-CargoPath
Ensure-GnuRustToolchain
Add-MingwToPathIfAvailable
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP "voicewave-phase1-target"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$reportPath = Join-Path $root "docs\phase1\phase1-battery-thermal-windows.json"
$sustainedReportPath = Join-Path $root "docs\phase1\phase1-sustained-30m.json"

$startBattery = Get-BatteryPercent
$startThermal = Get-ThermalCelsius
$startTime = Get-Date

Push-Location (Join-Path $root "src-tauri")
try {
  & $cargoExe +stable-x86_64-pc-windows-gnu run --no-default-features --example phase1_harness -- sustained --minutes $Minutes --out $sustainedReportPath
  if ($LASTEXITCODE -ne 0) { throw "sustained harness run failed" }
}
finally {
  Pop-Location
}

$endTime = Get-Date
$endBattery = Get-BatteryPercent
$endThermal = Get-ThermalCelsius

$batteryDrainPercent = $null
if ($startBattery -ne $null -and $endBattery -ne $null) {
  $batteryDrainPercent = [math]::Max(0, $startBattery - $endBattery)
}

$report = [ordered]@{
  start_time = $startTime.ToString("o")
  end_time = $endTime.ToString("o")
  duration_minutes = [math]::Round(($endTime - $startTime).TotalMinutes, 2)
  start_battery_percent = $startBattery
  end_battery_percent = $endBattery
  battery_drain_percent = $batteryDrainPercent
  start_thermal_celsius = $startThermal
  end_thermal_celsius = $endThermal
  notes = "Thermal sensors are hardware/driver dependent. Empty arrays indicate sensor unavailable."
}

New-Item -ItemType Directory -Path (Split-Path $reportPath -Parent) -Force | Out-Null
$report | ConvertTo-Json -Depth 6 | Set-Content -Path $reportPath -Encoding UTF8

Write-Host "Battery/Thermal report written to $reportPath"
