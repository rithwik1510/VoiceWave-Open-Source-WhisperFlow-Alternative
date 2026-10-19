# Builds the whisper.cpp speech workers (plan 019) with MSVC + Ninja:
#   <OutDir>\vk\voicewave-asr-vk.exe    Vulkan + CPU
#   <OutDir>\cpu\voicewave-asr-cpu.exe  CPU only
# Each variant gets its own CMake build tree. The Vulkan variant needs the
# Vulkan SDK (VULKAN_SDK, or -VulkanSdk); the CPU variant is configured with
# VULKAN_SDK removed from the environment so nothing Vulkan can leak in.
# The fetched whisper.cpp gets parakeet-logits-filter.patch (plan 021); a
# -WhisperSource checkout must already carry it.
#
# Usage: ./scripts/asr-worker/build-asr-worker.ps1 [-Variant all|vk|cpu] [-BuildRoot <dir>]
#          [-OutDir <dir>] [-VulkanSdk <dir>] [-WhisperSource <local whisper.cpp 1.9.5 dir>] [-Clean]
param(
  [ValidateSet("all", "vk", "cpu")][string]$Variant = "all",
  # Outside the repo by default: thousands of object files do not belong in a synced folder.
  [string]$BuildRoot = (Join-Path ([System.IO.Path]::GetTempPath()) "voicewave-asr-build"),
  [string]$OutDir = "",
  [string]$VulkanSdk = $env:VULKAN_SDK,
  [string]$WhisperSource = "",
  [switch]$Clean
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$sourceDir = Join-Path $repoRoot "src-tauri\windows\asr-worker"
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $sourceDir "bin" }
if ([string]::IsNullOrWhiteSpace($VulkanSdk)) { $VulkanSdk = "C:\VulkanSDK\1.4.341.1" }

function Import-VcVars {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { throw "vswhere.exe not found; install Visual Studio 2022 Build Tools with the C++ workload." }
  $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  if (-not $vsPath) { throw "No Visual Studio installation with the x64 C++ tools was found." }
  $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
  # Run vcvars in cmd and import the resulting environment into this session.
  $envDump = & cmd.exe /c "`"$vcvars`" >nul && set"
  if ($LASTEXITCODE -ne 0) { throw "vcvars64.bat failed" }
  foreach ($line in $envDump) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2]) }
  }
  foreach ($tool in "cl.exe", "cmake.exe", "ninja.exe") {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
      throw "$tool not on PATH after vcvars64 (install the 'C++ CMake tools for Windows' component)."
    }
  }
  Write-Host "MSVC environment: $vsPath"
}

function Build-Variant([string]$name) {
  $vulkan = $name -eq "vk"
  $exeName = "voicewave-asr-$name.exe"
  $buildDir = Join-Path $BuildRoot $name
  if ($Clean -and (Test-Path $buildDir)) { Remove-Item -Recurse -Force $buildDir }

  $savedSdk = $env:VULKAN_SDK
  try {
    if ($vulkan) {
      if (-not (Test-Path (Join-Path $VulkanSdk "Bin\glslc.exe"))) { throw "Vulkan SDK not found at $VulkanSdk (glslc.exe missing)." }
      $env:VULKAN_SDK = $VulkanSdk
    } else {
      Remove-Item Env:VULKAN_SDK -ErrorAction SilentlyContinue
    }
    $configure = @(
      "-S", $sourceDir, "-B", $buildDir, "-G", "Ninja",
      "-DCMAKE_BUILD_TYPE=Release",
      "-DVOICEWAVE_ASR_VULKAN=$(if ($vulkan) { 'ON' } else { 'OFF' })"
    )
    if ($WhisperSource) { $configure += "-DFETCHCONTENT_SOURCE_DIR_WHISPER=$((Resolve-Path $WhisperSource).Path)" }
    Write-Host "== Configuring $exeName"
    & cmake @configure
    if ($LASTEXITCODE -ne 0) { throw "cmake configure failed for $name" }
    Write-Host "== Building $exeName"
    & cmake --build $buildDir --config Release --target "voicewave-asr-$name"
    if ($LASTEXITCODE -ne 0) { throw "cmake build failed for $name" }
  } finally {
    if ($null -ne $savedSdk) { $env:VULKAN_SDK = $savedSdk }
  }

  $built = Get-ChildItem -Path $buildDir -Recurse -Filter $exeName | Select-Object -First 1
  if (-not $built) { throw "$exeName was not produced under $buildDir" }
  $dest = Join-Path $OutDir $name
  New-Item -ItemType Directory -Force -Path $dest | Out-Null
  Copy-Item -Force $built.FullName (Join-Path $dest $exeName)
  $mb = [math]::Round($built.Length / 1MB, 1)
  Write-Host "== $exeName -> $dest ($mb MB)"
}

Import-VcVars
$variants = if ($Variant -eq "all") { @("vk", "cpu") } else { @($Variant) }
foreach ($v in $variants) { Build-Variant $v }

& (Join-Path $PSScriptRoot "check-asr-worker-deps.ps1") -BinDir $OutDir
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
