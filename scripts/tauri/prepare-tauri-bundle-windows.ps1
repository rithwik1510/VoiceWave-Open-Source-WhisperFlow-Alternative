Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$resourceDir = Join-Path $repoRoot "src-tauri\windows"
$resourceDll = Join-Path $resourceDir "WebView2Loader.dll"

if (-not (Test-Path $resourceDir)) {
  New-Item -ItemType Directory -Path $resourceDir | Out-Null
}

$targetRoot = if ($env:CARGO_TARGET_DIR) {
  $env:CARGO_TARGET_DIR
}
else {
  Join-Path $repoRoot "src-tauri\target"
}

$releaseRoots = @()
if (-not [string]::IsNullOrWhiteSpace($env:CARGO_BUILD_TARGET)) {
  $releaseRoots += Join-Path $targetRoot "$($env:CARGO_BUILD_TARGET)\release"
}
$releaseRoots += Join-Path $targetRoot "release"

$loaderSource = $null
foreach ($releaseRoot in $releaseRoots) {
  if (-not (Test-Path $releaseRoot)) {
    continue
  }

  $rootLoader = Join-Path $releaseRoot "WebView2Loader.dll"
  if (Test-Path $rootLoader) {
    $loaderSource = $rootLoader
    break
  }

  $nestedLoader = Get-ChildItem -Path $releaseRoot -Recurse -Filter "WebView2Loader.dll" -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "webview2-com-sys" -and $_.FullName -match "\\x64\\" } |
    Select-Object -First 1

  if ($null -ne $nestedLoader) {
    $loaderSource = $nestedLoader.FullName
    break
  }
}

if (-not $loaderSource) {
  throw "WebView2Loader.dll was not found in release outputs. Expected under $targetRoot."
}

Copy-Item -Path $loaderSource -Destination $resourceDll -Force
Write-Host "Prepared bundle resource: $resourceDll"

function Resolve-FasterWhisperRuntimeDlls {
  $runtimeDir = Join-Path $repoRoot ".venv-faster-whisper\Lib\site-packages\av.libs"
  if (-not (Test-Path $runtimeDir)) {
    return @()
  }

  $aliases = @(
    @{ Canonical = "libstdc++-6.dll"; Pattern = "libstdc++-6*.dll" },
    @{ Canonical = "libgcc_s_seh-1.dll"; Pattern = "libgcc_s_seh-1*.dll" },
    @{ Canonical = "libwinpthread-1.dll"; Pattern = "libwinpthread-1*.dll" }
  )

  $resolved = @()
  foreach ($alias in $aliases) {
    $match = Get-ChildItem -Path $runtimeDir -Filter $alias.Pattern -File -ErrorAction SilentlyContinue |
      Select-Object -First 1
    if ($null -ne $match) {
      $resolved += [pscustomobject]@{
        SourcePath = $match.FullName
        CanonicalName = $alias.Canonical
      }
    }
  }

  return $resolved
}

$fwRuntimeDlls = Resolve-FasterWhisperRuntimeDlls
if ($fwRuntimeDlls.Count -eq 0) {
  Write-Warning "No Faster-Whisper runtime DLL aliases were resolved from .venv-faster-whisper."
}
else {
  foreach ($dll in $fwRuntimeDlls) {
    $resourceTarget = Join-Path $resourceDir $dll.CanonicalName
    Copy-Item -Path $dll.SourcePath -Destination $resourceTarget -Force
    Write-Host "Prepared runtime alias: $resourceTarget"
  }

  foreach ($releaseRoot in $releaseRoots) {
    if (-not (Test-Path $releaseRoot)) {
      continue
    }
    foreach ($dll in $fwRuntimeDlls) {
      $releaseTarget = Join-Path $releaseRoot $dll.CanonicalName
      Copy-Item -Path $dll.SourcePath -Destination $releaseTarget -Force
      Write-Host "Prepared runtime alias: $releaseTarget"
    }
  }
}
