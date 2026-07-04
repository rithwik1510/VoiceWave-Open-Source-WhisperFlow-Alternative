Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Test-TruthyValue([string]$value) {
  if ([string]::IsNullOrWhiteSpace($value)) {
    return $false
  }
  switch ($value.Trim().ToLowerInvariant()) {
    "1" { return $true }
    "true" { return $true }
    "yes" { return $true }
    "on" { return $true }
    default { return $false }
  }
}

function Resolve-ExistingPath([string[]]$candidates) {
  foreach ($candidate in $candidates) {
    if ([string]::IsNullOrWhiteSpace($candidate)) {
      continue
    }
    if (Test-Path $candidate) {
      return (Resolve-Path $candidate).Path
    }
  }
  return $null
}

function Resolve-MingwRuntimeDirectory {
  $candidates = @()

  # Explicit override (CI sets this to the MinGW that actually compiled the
  # C++ in this build) takes priority.
  if (-not [string]::IsNullOrWhiteSpace($env:VOICEWAVE_MINGW_BIN)) {
    $candidates += $env:VOICEWAVE_MINGW_BIN
  }

  if (-not [string]::IsNullOrWhiteSpace($env:PATH)) {
    $candidates += ($env:PATH -split ";")
  }

  $candidates += @(
    (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"),
    "C:\msys64\mingw64\bin",
    "C:\mingw64\bin"
  )

  # Return the first directory that actually CONTAINS the runtime DLL. The old
  # logic returned the first existing directory on PATH — which, once NSIS and
  # the VS cmake dir are prepended, is not the MinGW dir. That made the DLL copy
  # fall through to the stale committed src-tauri/windows/libstdc++-6.dll, which
  # is too old for the std::codecvt symbols whisper.cpp needs ("Entry Point Not
  # Found" at app launch).
  foreach ($dir in $candidates) {
    if ([string]::IsNullOrWhiteSpace($dir)) {
      continue
    }
    if (Test-Path (Join-Path $dir "libstdc++-6.dll")) {
      return (Resolve-Path $dir).Path
    }
  }

  return $null
}

function Resolve-DllSourcePath([string]$dllName, [string[]]$releaseRoots, [string]$mingwRuntimeDir, [string]$repoRoot) {
  $candidateDirectories = @()
  if ($mingwRuntimeDir) {
    $candidateDirectories += $mingwRuntimeDir
  }

  foreach ($releaseRoot in $releaseRoots) {
    if (-not (Test-Path $releaseRoot)) {
      continue
    }
    $candidateDirectories += $releaseRoot
  }

  foreach ($directory in $candidateDirectories) {
    $candidatePath = Join-Path $directory $dllName
    if (Test-Path $candidatePath) {
      return $candidatePath
    }
  }

  return $null
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

# Ensure the bundled CPU faster-whisper Python runtime is staged before the
# bundler collects resources. Without this, a fresh install has no Python +
# faster-whisper and cannot transcribe the default fw-small.en model (it would
# only work in `npm run tauri:dev`, which points at the dev .venv). The build
# script is a no-op when the runtime is already present, so repeat local builds
# stay fast; CI and first local builds do the one-time download + pip install.
# Skip only when explicitly disabled (e.g. a deliberately runtime-less build).
if (-not (Test-TruthyValue $env:VOICEWAVE_SKIP_EMBEDDED_RUNTIME)) {
  $runtimePython = Join-Path $repoRoot "src-tauri\windows\faster-whisper\python\python.exe"
  if (-not (Test-Path $runtimePython)) {
    Write-Host "Embedded faster-whisper runtime not found; building it now..."
    & (Join-Path $PSScriptRoot "..\faster_whisper\build-embedded-runtime.ps1")
    if ($LASTEXITCODE -ne 0) { throw "Failed to build embedded faster-whisper runtime" }
  }
  if (-not (Test-Path $runtimePython)) {
    throw "Embedded faster-whisper runtime is still missing at $runtimePython after build."
  }
}

$resourceDir = Join-Path $repoRoot "src-tauri\windows"
$resourceDll = Join-Path $resourceDir "WebView2Loader.dll"
$workerResourceDir = Join-Path $resourceDir "faster-whisper"
$workerSource = Join-Path $repoRoot "scripts\faster_whisper\worker.py"
$workerDestination = Join-Path $workerResourceDir "worker.py"

if (-not (Test-Path $resourceDir)) {
  New-Item -ItemType Directory -Path $resourceDir | Out-Null
}

if (-not (Test-Path $workerResourceDir)) {
  New-Item -ItemType Directory -Path $workerResourceDir | Out-Null
}

if (-not (Test-Path $workerSource)) {
  throw "faster-whisper worker script was not found at $workerSource."
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

Copy-Item -Path $workerSource -Destination $workerDestination -Force
Write-Host "Prepared bundle resource: $workerDestination"

$mingwRuntimeDir = Resolve-MingwRuntimeDirectory
if (-not $mingwRuntimeDir) {
  throw "MinGW runtime directory was not found in PATH or known locations. Cannot package GNU runtime DLLs safely."
}

$requiredRuntimeDlls = @(
  "libstdc++-6.dll",
  "libgcc_s_seh-1.dll",
  "libwinpthread-1.dll"
)

foreach ($dllName in $requiredRuntimeDlls) {
  $dllSource = Resolve-DllSourcePath -dllName $dllName -releaseRoots $releaseRoots -mingwRuntimeDir $mingwRuntimeDir -repoRoot $repoRoot
  if (-not $dllSource) {
    throw "Required runtime DLL '$dllName' was not found in MinGW runtime, release outputs, or faster-whisper venv."
  }

  $resourceDestination = Join-Path $resourceDir $dllName
  Copy-Item -Path $dllSource -Destination $resourceDestination -Force
  Write-Host "Prepared bundle resource: $resourceDestination"

  foreach ($releaseRoot in $releaseRoots) {
    if (-not (Test-Path $releaseRoot)) {
      continue
    }
    $releaseDestination = Join-Path $releaseRoot $dllName
    if ((Resolve-Path $dllSource).Path -eq (Resolve-Path $releaseDestination -ErrorAction SilentlyContinue).Path) {
      continue
    }
    Copy-Item -Path $dllSource -Destination $releaseDestination -Force
  }
}
