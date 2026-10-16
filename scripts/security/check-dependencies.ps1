param(
  [switch]$Enforce
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$inCi = $env:CI -eq 'true'
$result = 'pass'

function Write-Result([string]$value, [string]$detail) {
  Write-Host ("DEPS_CHECK_RESULT={0}" -f $value)
  Write-Host ("DEPS_CHECK_DETAIL={0}" -f $detail)
}

Push-Location $repoRoot
try {
  $prodAuditRaw = npm audit --omit=dev --json
  if ($LASTEXITCODE -ne 0 -and -not $prodAuditRaw) {
    throw 'npm audit --omit=dev failed to produce JSON output.'
  }
  $prodAudit = $prodAuditRaw | ConvertFrom-Json
  $prodHigh = [int]$prodAudit.metadata.vulnerabilities.high
  $prodCritical = [int]$prodAudit.metadata.vulnerabilities.critical

  if ($prodHigh -gt 0 -or $prodCritical -gt 0) {
    throw "Production dependency vulnerabilities exceed policy (high=$prodHigh critical=$prodCritical)."
  }

  # Dev-only advisories with no patched release anywhere, accepted until the
  # date. Production deps (above) are never allowlisted.
  # GHSA-vfj7-8cjw-p6xm: braces <=3.0.3, reached only via Tailwind 3's build
  # tooling on our own glob patterns; the fix is the Tailwind 4 migration.
  $allowlist = @{ 'GHSA-vfj7-8cjw-p6xm' = [datetime]'2027-01-31' }
  $activeAllowlist = @($allowlist.Keys | Where-Object { (Get-Date) -lt $allowlist[$_] })

  $allAuditRaw = npm audit --json
  $allAudit = $allAuditRaw | ConvertFrom-Json
  # Judge the advisories themselves (package severities roll up their
  # dependencies' advisories, so they would re-count an allowlisted one).
  $advisories = @{}
  foreach ($entry in $allAudit.vulnerabilities.PSObject.Properties) {
    foreach ($via in @($entry.Value.via)) {
      if ($via -is [string]) { continue }
      $advisories[($via.url -split '/')[-1]] = $via.severity
    }
  }
  $blocking = @($advisories.Keys | Where-Object {
      @('high', 'critical') -contains $advisories[$_] -and $activeAllowlist -notcontains $_
    })
  $allowed = @($advisories.Keys | Where-Object { $activeAllowlist -contains $_ })
  if ($allowed.Count -gt 0) {
    Write-Host ("Allowlisted dev advisories: {0}" -f (($allowed | Sort-Object) -join ', '))
  }
  if ($blocking.Count -gt 0) {
    throw "Dependency vulnerabilities exceed policy (high/critical advisories: $(($blocking | Sort-Object) -join ', '))."
  }

  $cargoAudit = Get-Command cargo-audit -ErrorAction SilentlyContinue
  if ($null -eq $cargoAudit) {
    $cargoAudit = Get-Command cargo-audit.exe -ErrorAction SilentlyContinue
  }
  if ($null -eq $cargoAudit) {
    if ($Enforce -and $inCi) {
      throw 'cargo-audit is required for enforced dependency checks. Install with: cargo install cargo-audit'
    }
    $result = 'warn-local-skip'
    Write-Warning 'cargo-audit is not installed; skipping Rust advisory scan outside CI.'
  } else {
    # The Rust crate lives in src-tauri/, so there is no Cargo.lock at the repo root.
    cargo audit -q --file (Join-Path $repoRoot 'src-tauri/Cargo.lock')
    if ($LASTEXITCODE -ne 0) {
      throw 'cargo audit reported Rust advisories.'
    }
  }

  Write-Host 'Dependency security checks passed.'
  Write-Result $result $(if ($result -eq 'pass') { 'npm-audit+cargo-audit' } else { 'npm-audit-only' })
}
catch {
  Write-Result 'fail' $_.Exception.Message
  if ($Enforce) {
    throw
  }
  Write-Warning $_.Exception.Message
}
finally {
  Pop-Location
}
