param(
  [switch]$Enforce,
  [datetime]$ReleaseCandidateDate = (Get-Date),
  # CI: skip the test/build/security gates that other CI jobs already ran
  # (frontend_quality, rust_quality, dependency_security, secrets_scan, and
  # the job's own Firestore rules step). Local runs keep them.
  [switch]$SkipRepeatedChecks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$checks = New-Object System.Collections.Generic.List[object]
$failedChecks = 0

function Add-CheckResult {
  param(
    [string]$Name,
    [bool]$Passed,
    [string]$Detail
  )

  $status = if ($Passed) { "PASS" } else { "FAIL" }
  $checks.Add([pscustomobject]@{
      Check  = $Name
      Status = $status
      Detail = $Detail
    }) | Out-Null

  if (-not $Passed) {
    $script:failedChecks += 1
  }
}

function Invoke-NpmScript {
  param(
    [string]$ScriptName,
    [string]$Label,
    # Not $Args: that is PowerShell's automatic variable, which silently
    # stays empty and dropped every argument passed here.
    [string[]]$ScriptArgs = @()
  )

  Push-Location $repoRoot
  try {
    if ($ScriptArgs.Count -gt 0) {
      & npm run $ScriptName -- $ScriptArgs
    }
    else {
      npm run $ScriptName
    }
    Add-CheckResult $Label ($LASTEXITCODE -eq 0) "npm run $ScriptName $($ScriptArgs -join ' ')"
  }
  catch {
    Add-CheckResult $Label $false $_.Exception.Message
  }
  finally {
    Pop-Location
  }
}

function Test-LegalComplianceChecklist {
  $relativePath = "docs/testing/legal-compliance-checklist.md"
  $fullPath = Join-Path $repoRoot $relativePath
  if (-not (Test-Path $fullPath)) {
    Add-CheckResult "Legal/compliance checklist present" $false "Missing $relativePath"
    return
  }

  Add-CheckResult "Legal/compliance checklist present" $true $fullPath
  $raw = Get-Content -Path $fullPath -Raw
  $isComplete = $raw -match "(?m)^Status:\s*Complete\s*$"
  Add-CheckResult "Legal/compliance checklist marked complete" $isComplete "Requires `Status: Complete` marker."
}

function Test-RiskRegisterBlockingState {
  $relativePath = "docs/risk/risk-register.md"
  $fullPath = Join-Path $repoRoot $relativePath
  if (-not (Test-Path $fullPath)) {
    Add-CheckResult "Risk register present" $false "Missing $relativePath"
    return
  }

  Add-CheckResult "Risk register present" $true $fullPath

  $lines = Get-Content -Path $fullPath
  $riskRows = @($lines | Where-Object { $_ -match "^\|\s*R-\d+" })
  if ($riskRows.Count -eq 0) {
    Add-CheckResult "Risk rows parsed" $false "No risk rows found."
    return
  }

  Add-CheckResult "Risk rows parsed" $true "Parsed $($riskRows.Count) risk rows."

  $criticalOpen = $false
  $highWithoutEta = 0
  foreach ($row in $riskRows) {
    $parts = @($row -split "\|")
    if ($parts.Count -lt 10) {
      continue
    }

    $severity = $parts[4].Trim()
    $owner = $parts[7].Trim()
    $eta = $parts[8].Trim()
    $status = $parts[9].Trim()

    if (($severity -eq "Critical") -and ($status -eq "Open")) {
      $criticalOpen = $true
    }

    if ($severity -eq "High") {
      if ([string]::IsNullOrWhiteSpace($owner) -or [string]::IsNullOrWhiteSpace($eta)) {
        $highWithoutEta += 1
      }
    }
  }

  Add-CheckResult "No unresolved Critical risks" (-not $criticalOpen) "Critical risks must not be Open."
  Add-CheckResult "All High risks include owner and ETA" ($highWithoutEta -eq 0) "high_missing_owner_or_eta=$highWithoutEta"
}

function Test-RunbookReleaseControls {
  $relativePath = "docs/phase5/beta-ops-runbook.md"
  $fullPath = Join-Path $repoRoot $relativePath
  if (-not (Test-Path $fullPath)) {
    Add-CheckResult "Beta ops runbook present" $false "Missing $relativePath"
    return
  }

  Add-CheckResult "Beta ops runbook present" $true $fullPath
  $raw = Get-Content -Path $fullPath -Raw

  Add-CheckResult "Runbook includes rollout waves" ($raw -match "(?im)^##\s*Rollout Waves") "Requires Rollout Waves section."
  Add-CheckResult "Runbook includes incident thresholds" ($raw -match "(?im)^##\s*Incident Thresholds") "Requires Incident Thresholds section."
  Add-CheckResult "Runbook includes rollback triggers" ($raw -match "(?im)^##\s*Rollback triggers") "Requires Rollback triggers section."
  Add-CheckResult "Runbook includes canary halt rule" ($raw -match "(?im)2 consecutive days") "Requires halt-on-breach criteria."
}

function Test-TabletopDrillArtifact {
  $artifactDir = Join-Path $repoRoot "docs/phase5/artifacts"
  if (-not (Test-Path $artifactDir)) {
    Add-CheckResult "Tabletop drill artifact present" $false "Missing docs/phase5/artifacts directory."
    return
  }

  $drills = @(
    Get-ChildItem -Path $artifactDir -Filter "tabletop-drill-*.md" -File |
      Sort-Object LastWriteTimeUtc -Descending
  )
  if ($drills.Count -eq 0) {
    Add-CheckResult "Tabletop drill artifact present" $false "Expected docs/phase5/artifacts/tabletop-drill-*.md"
    return
  }

  $latest = $drills[0]
  Add-CheckResult "Tabletop drill artifact present" $true $latest.FullName

  $raw = Get-Content -Path $latest.FullName -Raw
  $complete = $raw -match "(?im)^Status:\s*Complete\s*$"
  Add-CheckResult "Tabletop drill marked complete" $complete "Requires `Status: Complete` marker."
}

if ($SkipRepeatedChecks) {
  Invoke-NpmScript -ScriptName "phase4:gate" -Label "Phase 4 technical validation gate" -ScriptArgs @("-SkipValidation")
}
else {
  Invoke-NpmScript -ScriptName "phase4:gate" -Label "Phase 4 technical validation gate"
}

Push-Location $repoRoot
try {
  $phase5ReadinessArgs = @(
    "-ExecutionPolicy", "Bypass",
    "-File", ".\scripts\phase5\run-phase5-readiness.ps1",
    "-ReleaseCandidateDate", $ReleaseCandidateDate.ToString("o"),
    "-RequireFreshArtifacts",
    "-SkipPhase4Prep"
  )
  if ($Enforce) {
    $phase5ReadinessArgs += "-Enforce"
  }
  & powershell @phase5ReadinessArgs
  Add-CheckResult "Phase 5 readiness gate with freshness" ($LASTEXITCODE -eq 0) "Enforced freshness and Phase 4 dependency."
}
catch {
  Add-CheckResult "Phase 5 readiness gate with freshness" $false $_.Exception.Message
}
finally {
  Pop-Location
}

Invoke-NpmScript -ScriptName "phase5:reliability:gate" -Label "Phase 5 reliability gate"
if (-not $SkipRepeatedChecks) {
  Invoke-NpmScript -ScriptName "security:secrets" -Label "Secrets scan gate" -ScriptArgs @("-Enforce")
  Invoke-NpmScript -ScriptName "security:deps" -Label "Dependency security gate" -ScriptArgs @("-Enforce")
  Invoke-NpmScript -ScriptName "security:firestore-rules" -Label "Firestore rules gate" -ScriptArgs @("-Enforce")
  Invoke-NpmScript -ScriptName "quality:frontend:gate" -Label "Frontend quality + coverage gate"
  Invoke-NpmScript -ScriptName "quality:backend:gate" -Label "Backend quality gate"
}
Test-LegalComplianceChecklist
Test-RiskRegisterBlockingState
Test-RunbookReleaseControls
Test-TabletopDrillArtifact

Write-Host ""
Write-Host "Release Gate Report"
Write-Host "-------------------"
$checks | Format-Table -AutoSize | Out-String | Write-Host

if ($failedChecks -gt 0) {
  $message = "Release gate has $failedChecks failed check(s)."
  if ($Enforce) {
    throw $message
  }
  Write-Warning "$message Run with -Enforce to make this gate-blocking."
}
else {
  Write-Host "Release gate checks passed."
}
