param(
  [int]$BenchmarkRuns = 20,
  [int]$StabilitySessions = 200,
  [switch]$RunSustained,
  [int]$SustainedMinutes = 30
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

function Ensure-SpaceSafeJunction([string]$sourcePath) {
  $junctionPath = Join-Path $env:TEMP "voicewave-phase-validation-nospace"
  if (Test-Path $junctionPath) {
    $item = Get-Item $junctionPath -ErrorAction SilentlyContinue
    if ($item -and $item.LinkType -eq "Junction") {
      return $junctionPath
    }
    Remove-Item -LiteralPath $junctionPath -Force -Recurse
  }

  New-Item -ItemType Junction -Path $junctionPath -Target $sourcePath | Out-Null
  return $junctionPath
}

$cargoExe = Resolve-CargoPath
Ensure-GnuRustToolchain
Add-MingwToPathIfAvailable
$cargoTargetDir = Join-Path $env:TEMP "voicewave-phase1-target"
$env:CARGO_TARGET_DIR = $cargoTargetDir

Push-Location (Join-Path $PSScriptRoot "..\..\src-tauri")
try {
  & $cargoExe +stable-x86_64-pc-windows-gnu test --no-default-features
  if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

  & $cargoExe +stable-x86_64-pc-windows-gnu run --no-default-features --example phase1_harness -- benchmark --runs $BenchmarkRuns --out "../docs/phase1/phase1-latency-baseline.json"
  if ($LASTEXITCODE -ne 0) { throw "benchmark harness failed" }

  & $cargoExe +stable-x86_64-pc-windows-gnu run --no-default-features --example phase1_harness -- stability --sessions $StabilitySessions --out "../docs/phase1/phase1-stability-200.json"
  if ($LASTEXITCODE -ne 0) { throw "stability harness failed" }

  if ($RunSustained) {
    & $cargoExe +stable-x86_64-pc-windows-gnu run --no-default-features --example phase1_harness -- sustained --minutes $SustainedMinutes --out "../docs/phase1/phase1-sustained-30m.json"
    if ($LASTEXITCODE -ne 0) { throw "sustained harness failed" }
  }
}
finally {
  Pop-Location
}

$spaceSafeRoot = Ensure-SpaceSafeJunction (Resolve-Path (Join-Path $PSScriptRoot "..\..\"))
$desktopManifest = Join-Path $spaceSafeRoot "src-tauri\Cargo.toml"
& $cargoExe +stable-x86_64-pc-windows-gnu test --manifest-path $desktopManifest --no-run
if ($LASTEXITCODE -ne 0) { throw "desktop-feature compile check failed" }

Write-Host "Phase I validation complete."
