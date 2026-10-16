# Plan 019 gate (b): the speech workers must run on a PC with no VC++
# redistributable and no Vulkan SDK. Fails if:
#   - either worker imports a VC runtime DLL (vcruntime*, msvcp*, vcomp*,
#     concrt*, or the dynamic UCRT: ucrtbase / api-ms-win-crt-*)
#   - the CPU worker imports vulkan-1.dll in any form
#   - the Vulkan worker imports vulkan-1.dll at load time (it must be delay-loaded)
# Usage: ./scripts/asr-worker/check-asr-worker-deps.ps1 [-BinDir src-tauri/windows/asr-worker/bin]
param([string]$BinDir = "")
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($BinDir)) {
  $BinDir = Join-Path $PSScriptRoot "..\..\src-tauri\windows\asr-worker\bin"
}

$dumpbin = (Get-Command dumpbin.exe -ErrorAction SilentlyContinue).Source
if (-not $dumpbin) {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  $dumpbin = Get-ChildItem -Path (Join-Path $vsPath "VC\Tools\MSVC") -Recurse -Filter dumpbin.exe |
    Where-Object { $_.FullName -match 'Hostx64\\x64' } | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $dumpbin) { throw "dumpbin.exe not found" }

# Returns @{ Load = [dlls]; Delay = [dlls] } from `dumpbin /dependents`.
function Get-Dependents([string]$exe) {
  $out = & $dumpbin /nologo /dependents $exe
  if ($LASTEXITCODE -ne 0) { throw "dumpbin failed on $exe" }
  $deps = @{ Load = @(); Delay = @() }
  $section = $null
  foreach ($line in $out) {
    if ($line -match 'has the following delay load dependencies') { $section = "Delay"; continue }
    if ($line -match 'has the following dependencies') { $section = "Load"; continue }
    if ($line -match '^\s*Summary') { $section = $null; continue }
    if ($section -and $line -match '^\s+(\S+\.dll)\s*$') { $deps[$section] += $Matches[1].ToLowerInvariant() }
  }
  return $deps
}

$crt = '^(vcruntime|msvcp|vcomp|concrt|ucrtbase|api-ms-win-crt-)'
$failures = @()
$checked = 0
foreach ($variant in "vk", "cpu") {
  $exe = Join-Path $BinDir "$variant\voicewave-asr-$variant.exe"
  if (-not (Test-Path $exe)) { continue }
  $checked++
  $deps = Get-Dependents $exe
  $all = @($deps.Load) + @($deps.Delay)
  Write-Host "voicewave-asr-$variant.exe ($([math]::Round((Get-Item $exe).Length / 1MB, 1)) MB)"
  Write-Host "  load-time : $($deps.Load -join ', ')"
  Write-Host "  delay-load: $(if ($deps.Delay) { $deps.Delay -join ', ' } else { '(none)' })"
  foreach ($dll in $all) {
    if ($dll -match $crt) { $failures += "voicewave-asr-$variant.exe imports VC runtime DLL $dll" }
  }
  if ($variant -eq "cpu" -and ($all -contains "vulkan-1.dll")) {
    $failures += "voicewave-asr-cpu.exe imports vulkan-1.dll"
  }
  if ($variant -eq "vk" -and ($deps.Load -contains "vulkan-1.dll")) {
    $failures += "voicewave-asr-vk.exe imports vulkan-1.dll at load time (must be delay-loaded)"
  }
}

if ($checked -eq 0) { throw "No speech worker found under $BinDir" }
if ($failures.Count -gt 0) {
  $failures | ForEach-Object { Write-Host "FAIL: $_" -ForegroundColor Red }
  exit 1
}
Write-Host "Speech worker dependency check passed ($checked worker(s))."
