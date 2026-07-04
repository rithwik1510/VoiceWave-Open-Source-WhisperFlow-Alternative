param(
  [switch]$DryRun,
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$TauriArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Ensure-GnuRustToolchain {
  $rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
  if (-not (Test-Path $rustup)) {
    throw "rustup not found. Install Rust toolchain first."
  }
  $toolchain = "stable-x86_64-pc-windows-gnu"
  $installed = & $rustup toolchain list | Select-String -Pattern $toolchain -SimpleMatch
  if (-not $installed) {
    & $rustup toolchain install $toolchain
    if ($LASTEXITCODE -ne 0) {
      throw "failed installing $toolchain"
    }
  }
}

function Add-MingwToPathIfAvailable {
  # Explicit override — used by CI, where mingw is installed to a path that is
  # not the local winget package location.
  if (-not [string]::IsNullOrWhiteSpace($env:VOICEWAVE_MINGW_BIN) -and (Test-Path $env:VOICEWAVE_MINGW_BIN)) {
    $env:PATH = "$($env:VOICEWAVE_MINGW_BIN);$env:PATH"
    return
  }

  # Already resolvable on PATH (e.g. a setup-mingw action already prepended it).
  if (Get-Command "gcc.exe" -ErrorAction SilentlyContinue) {
    return
  }

  $candidatePaths = @(
    (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"),
    "C:\msys64\mingw64\bin",
    "C:\mingw64\bin"
  )
  foreach ($candidate in $candidatePaths) {
    if (Test-Path $candidate) {
      $env:PATH = "$candidate;$env:PATH"
      return
    }
  }

  throw "MinGW toolchain not found. Set VOICEWAVE_MINGW_BIN, put gcc.exe on PATH, or install BrechtSanders.WinLibs.POSIX.UCRT via winget."
}

function Prepend-PathEntryIfExists([string]$pathEntry) {
  if (-not (Test-Path $pathEntry)) {
    return
  }

  $entries = $env:PATH -split ";"
  if ($entries -contains $pathEntry) {
    return
  }

  $env:PATH = "$pathEntry;$env:PATH"
}

function Add-VsCmakeToPath {
  $vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { return }
  $installPath = & $vswhere -latest -products * -property installationPath 2>$null
  if ([string]::IsNullOrWhiteSpace($installPath)) { return }
  $cmakeBin = Join-Path $installPath.Trim() "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
  Prepend-PathEntryIfExists $cmakeBin
}

function Add-BundlerToolsToPath {
  $candidatePaths = @(
    "C:\\Program Files (x86)\\NSIS",
    "C:\\Program Files\\NSIS",
    "C:\\Program Files (x86)\\WiX Toolset v3.14\\bin",
    "C:\\Program Files\\WiX Toolset v3.14\\bin"
  )

  foreach ($pathEntry in $candidatePaths) {
    Prepend-PathEntryIfExists $pathEntry
  }
}

function Ensure-NoSpaceTargetDir {
  if ($env:VIBE_SAFE_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = $env:VIBE_SAFE_TARGET_DIR
    return
  }

  $safeRoot = "C:\\voicewave-tauri"
  if (-not (Test-Path $safeRoot)) {
    New-Item -ItemType Directory -Path $safeRoot | Out-Null
  }
  $env:CARGO_TARGET_DIR = Join-Path $safeRoot "target-gnu-build"
}

function Ensure-EmbeddedRuntime([string]$repoRoot) {
  # The bundled CPU faster-whisper runtime must exist BEFORE `tauri build`
  # starts: tauri-build validates the bundle.resources globs at COMPILE time
  # (its build script), which runs before beforeBundleCommand. If the
  # windows/faster-whisper/python tree is missing, the compile fails with
  # "glob pattern ... didn't match any files". So stage it here, up front.
  # No-op when already present; VOICEWAVE_SKIP_EMBEDDED_RUNTIME=1 opts out.
  if (Test-TruthyValue $env:VOICEWAVE_SKIP_EMBEDDED_RUNTIME) {
    Write-Warning "VOICEWAVE_SKIP_EMBEDDED_RUNTIME set; building without the bundled faster-whisper runtime. Fresh installs will not transcribe."
    return
  }
  $runtimePython = Join-Path $repoRoot "src-tauri\windows\faster-whisper\python\python.exe"
  if (Test-Path $runtimePython) {
    return
  }
  Write-Host "Embedded faster-whisper runtime not found; building it now (one-time download + pip install)..."
  & (Join-Path $PSScriptRoot "..\faster_whisper\build-embedded-runtime.ps1")
  if ($LASTEXITCODE -ne 0) { throw "Failed to build embedded faster-whisper runtime" }
  if (-not (Test-Path $runtimePython)) {
    throw "Embedded faster-whisper runtime is still missing at $runtimePython after build."
  }
}

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

function Test-CommandAvailable([string]$commandName) {
  $command = Get-Command $commandName -ErrorAction SilentlyContinue
  return $null -ne $command
}

function Resolve-BundleTargetArgs([string[]]$existingArgs) {
  $explicitBundles = $false
  if ($existingArgs) {
    foreach ($arg in $existingArgs) {
      if ($arg -eq "--bundles" -or $arg -eq "-b") {
        $explicitBundles = $true
        break
      }
    }
  }

  if ($explicitBundles) {
    return @()
  }

  $hasNsis = (Test-CommandAvailable "makensis.exe") -or (Test-CommandAvailable "makensis")
  $hasCandle = (Test-CommandAvailable "candle.exe") -or (Test-CommandAvailable "candle")
  $hasLight = (Test-CommandAvailable "light.exe") -or (Test-CommandAvailable "light")
  $hasWix = $hasCandle -and $hasLight

  if ($hasNsis -and $hasWix) {
    return @()
  }

  if ($hasNsis -and (-not $hasWix)) {
    Write-Warning "WiX bundler tools were not found (candle.exe + light.exe). Building NSIS installer only."
    return @("--bundles", "nsis")
  }

  if ((-not $hasNsis) -and $hasWix) {
    Write-Warning "NSIS bundler tool was not found (makensis.exe). Building MSI installer only."
    return @("--bundles", "msi")
  }

  throw "No supported Windows bundler tools were found. Install NSIS (makensis.exe) and/or WiX Toolset (candle.exe + light.exe), then rerun npm run tauri:build."
}

function Resolve-WhisperFeatureArgs {
  if (Test-TruthyValue $env:VOICEWAVE_DISABLE_CUDA_FEATURE) {
    return @()
  }

  $cudaToolkitDetected = $false
  $forceCuda = Test-TruthyValue $env:VOICEWAVE_FORCE_CUDA_FEATURE
  if ($forceCuda) {
    $cudaToolkitDetected = $true
  }
  elseif (-not [string]::IsNullOrWhiteSpace($env:CUDA_PATH)) {
    $cudaLibPath = Join-Path $env:CUDA_PATH "lib\\x64"
    if (Test-Path $cudaLibPath) {
      $cudaToolkitDetected = $true
    }
  }

  if (-not $cudaToolkitDetected) {
    return @()
  }

  if (-not (Test-CommandAvailable "cl.exe")) {
    if ($forceCuda) {
      throw "VOICEWAVE_FORCE_CUDA_FEATURE is set, but cl.exe was not found in PATH. Install Visual Studio Build Tools or unset VOICEWAVE_FORCE_CUDA_FEATURE."
    }
    Write-Warning "CUDA toolkit detected but cl.exe was not found. Falling back to CPU build for this build."
    return @()
  }

  if (-not [string]::IsNullOrWhiteSpace($env:CUDA_PATH)) {
    $cudaBinPath = Join-Path $env:CUDA_PATH "bin"
    if (Test-Path $cudaBinPath) {
      $env:PATH = "$cudaBinPath;$env:PATH"
    }
  }

  Write-Host "CUDA toolkit detected. Enabling whisper-cuda feature for this Tauri build."
  return @("--features", "whisper-cuda")
}

function Set-UpdaterSigningEnv([string]$repoRoot) {
  # CI (or the caller) can provide the key directly; never override that.
  if (-not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    return
  }

  $keyPath = Join-Path $repoRoot "src-tauri\.tauri\voicewave-updater.key"
  if (-not (Test-Path $keyPath)) {
    Write-Warning "Updater signing key not found at $keyPath. The build will still succeed but will NOT emit signed updater artifacts (.sig). Set TAURI_SIGNING_PRIVATE_KEY or restore the key to enable auto-update signing."
    return
  }

  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyPath -Raw)
  if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    # Local dev key was generated without a password (--ci). Production builds
    # should set both env vars from a secret store instead.
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
  }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-UpdaterSigningEnv $repoRoot
Ensure-GnuRustToolchain
Add-MingwToPathIfAvailable
Add-VsCmakeToPath
Add-BundlerToolsToPath
Ensure-NoSpaceTargetDir
Ensure-EmbeddedRuntime $repoRoot

$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"

$tauriCli = Join-Path $repoRoot "node_modules\.bin\tauri.cmd"
if (-not (Test-Path $tauriCli)) {
  throw "Tauri CLI not found at $tauriCli. Run npm install first."
}

$commandArgs = @("build")
$commandArgs += Resolve-WhisperFeatureArgs
$commandArgs += Resolve-BundleTargetArgs $TauriArgs
if ($TauriArgs) {
  $commandArgs += $TauriArgs
}

if ($DryRun) {
  Write-Host "Repo root: $repoRoot"
  Write-Host "RUSTUP_TOOLCHAIN=$env:RUSTUP_TOOLCHAIN"
  Write-Host "CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR"
  Write-Host ($tauriCli + " " + ($commandArgs -join " "))
  exit 0
}

Push-Location $repoRoot
try {
  & $tauriCli @commandArgs
  if ($LASTEXITCODE -ne 0) {
    throw "tauri build failed"
  }
}
finally {
  Pop-Location
}
