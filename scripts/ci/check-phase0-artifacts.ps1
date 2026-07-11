$ErrorActionPreference = "Stop"

$requiredFiles = @(
  "README.md",
  "CONTRIBUTING.md",
  "docs/prd/v1-prd.md",
  "docs/phase0-signoff.md",
  "docs/rfc/0001-system-architecture.md",
  "docs/testing/test-strategy.md",
  "docs/testing/hardware-tiers.md",
  "docs/benchmarks/competitive-benchmark-v1.md",
  "docs/risk/risk-register.md",
  "docs/security/threat-model-v1.md",
  "docs/adr/README.md",
  "docs/adr/template.md",
  "docs/adr/0001-phase-0-locked-decisions.md",
  ".github/pull_request_template.md",
  ".github/workflows/ci.yml",
  ".markdownlint-cli2.yaml",
  ".prettierignore",
  ".gitignore"
)

$missing = @()
foreach ($file in $requiredFiles) {
  if (-not (Test-Path $file)) {
    $missing += $file
  }
}

if ($missing.Count -gt 0) {
  Write-Host "Missing required Phase 0 files:"
  $missing | ForEach-Object { Write-Host " - $_" }
  exit 1
}

$contentChecks = @(
  @{ Path = "docs/prd/v1-prd.md"; Pattern = "^# VoiceWave v1 PRD$"; Label = "PRD title" },
  @{ Path = "docs/rfc/0001-system-architecture.md"; Pattern = "^# RFC 0001: VoiceWave v1 System Architecture$"; Label = "RFC title" },
  @{ Path = "docs/testing/test-strategy.md"; Pattern = "^# VoiceWave Test Strategy$"; Label = "Test strategy title" },
  @{ Path = "docs/adr/0001-phase-0-locked-decisions.md"; Pattern = "^## Decision$"; Label = "ADR decision section" }
)

$failedChecks = @()
foreach ($check in $contentChecks) {
  $match = Select-String -Path $check.Path -Pattern $check.Pattern
  if (-not $match) {
    $failedChecks += "$($check.Path) -> $($check.Label)"
  }
}

if ($failedChecks.Count -gt 0) {
  Write-Host "Phase 0 content checks failed:"
  $failedChecks | ForEach-Object { Write-Host " - $_" }
  exit 1
}

if (Test-Path "package.json") {
  if (-not (Test-Path "package-lock.json")) {
    Write-Host "package.json exists but package-lock.json is missing"
    exit 1
  }
  try {
    $pkg = Get-Content "package.json" -Raw | ConvertFrom-Json
    $requiredScripts = @("dev", "build", "test")
    foreach ($scriptName in $requiredScripts) {
      if (-not $pkg.scripts.$scriptName) {
        Write-Host "package.json is missing required script: $scriptName"
        exit 1
      }
    }
  }
  catch {
    Write-Host "Failed to parse package.json"
    exit 1
  }
}

if (Test-Path "src-tauri/Cargo.toml") {
  $cargoHasPackage = Select-String -Path "src-tauri/Cargo.toml" -Pattern "^\[package\]"
  if (-not $cargoHasPackage) {
    Write-Host "src-tauri/Cargo.toml missing [package] section"
    exit 1
  }
}

Write-Host "Phase 0 artifact verification passed."
