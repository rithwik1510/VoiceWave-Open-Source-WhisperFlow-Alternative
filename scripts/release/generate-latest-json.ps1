<#
.SYNOPSIS
  Generate the `latest.json` update manifest consumed by tauri-plugin-updater.

.DESCRIPTION
  After `npm run tauri:build` produces the signed NSIS installer, it also emits
  a `<installer>.sig` file next to it (because tauri.conf.json sets
  bundle.createUpdaterArtifacts = true and the build was signed). This script
  reads that signature and writes a `latest.json` that points at the release
  download URL for the given version.

  Upload BOTH the `_x64-setup.exe` AND this `latest.json` as assets on the
  GitHub release for tag `v<Version>`. The app's updater endpoint
  (`releases/latest/download/latest.json`) will then serve it automatically.

.EXAMPLE
  ./scripts/release/generate-latest-json.ps1 -Version 0.4.0 `
    -InstallerPath "C:\voicewave-tauri\target-gnu-build\x86_64-pc-windows-gnu\release\bundle\nsis\VoiceWave.Local.Core_0.4.0_x64-setup.exe" `
    -Notes "Bug fixes and the new auto-updater."
#>
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$InstallerPath,
  [string]$Notes = "",
  [string]$Repo = "rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative",
  [string]$OutFile = "latest.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path $InstallerPath)) {
  throw "Installer not found: $InstallerPath"
}

$sigPath = "$InstallerPath.sig"
if (-not (Test-Path $sigPath)) {
  throw "Signature not found: $sigPath`nBuild with the signing key set (TAURI_SIGNING_PRIVATE_KEY) so the .sig is produced."
}

$signature = (Get-Content $sigPath -Raw).Trim()
$fileName = Split-Path $InstallerPath -Leaf
$url = "https://github.com/$Repo/releases/download/v$Version/$fileName"

$manifest = [ordered]@{
  version   = $Version
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = $signature
      url       = $url
    }
  }
}

$json = $manifest | ConvertTo-Json -Depth 6
Set-Content -Path $OutFile -Value $json -Encoding utf8
Write-Host "Wrote $OutFile"
Write-Host "  version : $Version"
Write-Host "  url     : $url"
Write-Host "Next: upload both the installer .exe and $OutFile to the GitHub release for tag v$Version."
