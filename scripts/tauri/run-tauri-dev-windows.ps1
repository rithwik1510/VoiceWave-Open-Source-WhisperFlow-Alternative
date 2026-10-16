param(
  [switch]$DryRun,
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$TauriArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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
  $mingwDir = Resolve-MingwRuntimeDirectory
  if ($mingwDir) {
    $env:PATH = "$mingwDir;$env:PATH"
    return $mingwDir
  }

  throw "MinGW toolchain not found at expected WinLibs path. Install BrechtSanders.WinLibs.POSIX.UCRT via winget."
}

function Resolve-MingwRuntimeDirectory {
  $pathEntries = @()
  if (-not [string]::IsNullOrWhiteSpace($env:PATH)) {
    $pathEntries += ($env:PATH -split ";")
  }

  $knownCandidates = @(
    (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"),
    "C:\msys64\mingw64\bin",
    "C:\mingw64\bin"
  )

  $candidates = @()
  $candidates += $pathEntries
  $candidates += $knownCandidates

  foreach ($candidate in $candidates) {
    if ([string]::IsNullOrWhiteSpace($candidate)) {
      continue
    }
    if (-not (Test-Path $candidate)) {
      continue
    }
    if (Test-Path (Join-Path $candidate "libstdc++-6.dll")) {
      return (Resolve-Path $candidate).Path
    }
  }

  return $null
}

function Ensure-DevRuntimeDlls([string]$repoRoot, [string]$cargoTargetDir, [string]$mingwRuntimeDir) {
  $debugRoots = @(
    (Join-Path $cargoTargetDir "debug")
  )
  if (-not [string]::IsNullOrWhiteSpace($env:CARGO_BUILD_TARGET)) {
    $debugRoots += (Join-Path $cargoTargetDir "$($env:CARGO_BUILD_TARGET)\debug")
  }

  $requiredDlls = @(
    "libstdc++-6.dll",
    "libgcc_s_seh-1.dll",
    "libwinpthread-1.dll"
  )

  foreach ($debugRoot in $debugRoots) {
    if (-not (Test-Path $debugRoot)) {
      New-Item -ItemType Directory -Path $debugRoot -Force | Out-Null
    }

    foreach ($dllName in $requiredDlls) {
      $sourcePath = Join-Path $mingwRuntimeDir $dllName
      if (-not (Test-Path $sourcePath)) {
        throw "Required GNU runtime DLL '$dllName' was not found in $mingwRuntimeDir."
      }
      $destinationPath = Join-Path $debugRoot $dllName
      $shouldCopy = $true
      if (Test-Path $destinationPath) {
        $sourceInfo = Get-Item $sourcePath
        $destinationInfo = Get-Item $destinationPath
        if ($sourceInfo.Length -eq $destinationInfo.Length -and $sourceInfo.LastWriteTimeUtc -eq $destinationInfo.LastWriteTimeUtc) {
          $shouldCopy = $false
        }
      }
      if ($shouldCopy) {
        try {
          Copy-Item -Path $sourcePath -Destination $destinationPath -Force
        }
        catch {
          if (-not (Test-Path $destinationPath)) {
            throw
          }
          Write-Warning "Skipped refreshing $dllName because the existing file is locked by another process."
        }
      }
    }

    $webviewLoader = Join-Path $repoRoot "src-tauri\windows\WebView2Loader.dll"
    if (Test-Path $webviewLoader) {
      $loaderDestination = Join-Path $debugRoot "WebView2Loader.dll"
      try {
        Copy-Item -Path $webviewLoader -Destination $loaderDestination -Force
      }
      catch {
        if (-not (Test-Path $loaderDestination)) {
          throw
        }
        Write-Warning "Skipped refreshing WebView2Loader.dll because the existing file is locked by another process."
      }
    }
  }
}

function Configure-FasterWhisperRuntime([string]$repoRoot) {
  $venvPython = Join-Path $repoRoot ".venv-faster-whisper\\Scripts\\python.exe"
  if (Test-Path $venvPython) {
    $env:VOICEWAVE_FASTER_WHISPER_PYTHON = $venvPython
  }

  $cudaRoot = $env:CUDA_PATH
  if (-not [string]::IsNullOrWhiteSpace($cudaRoot)) {
    $cudaBinCandidates = @(
      (Join-Path $cudaRoot "bin\\x64"),
      (Join-Path $cudaRoot "bin")
    )
    foreach ($candidate in $cudaBinCandidates) {
      if (Test-Path $candidate) {
        $entries = $env:PATH -split ";"
        if ($entries -notcontains $candidate) {
          $env:PATH = "$candidate;$env:PATH"
        }
      }
    }
  }
}

function Stop-StaleViteDevProcesses([string]$repoPath) {
  $escapedRepoPath = [regex]::Escape($repoPath)
  $stale = Get-CimInstance Win32_Process |
    Where-Object {
      $_.CommandLine -and
      $_.CommandLine -match $escapedRepoPath -and
      (
        $_.CommandLine -match "vite(\.js)?" -or
        $_.CommandLine -match "npm-cli\.js run dev" -or
        $_.CommandLine -match "npm run dev"
      )
    } |
    Sort-Object ProcessId -Descending

  foreach ($proc in $stale) {
    try {
      Stop-Process -Id $proc.ProcessId -Force -ErrorAction Stop
      Write-Host "Stopped stale dev process: $($proc.ProcessId) [$($proc.Name)]"
    }
    catch {
      Write-Warning "Failed to stop process $($proc.ProcessId): $($_.Exception.Message)"
    }
  }
}

function Stop-StaleVoiceWaveRuntimeProcesses {
  $stale = Get-CimInstance Win32_Process |
    Where-Object {
      $_.Name -in @("voicewave_core.exe", "cargo.exe", "rustc.exe") -and
      $_.CommandLine -and
      (
        $_.CommandLine -match "voicewave-tauri-target" -or
        $_.CommandLine -match "src-tauri" -or
        $_.CommandLine -match "x86_64-pc-windows-gnu"
      )
    } |
    Sort-Object ProcessId -Descending

  foreach ($proc in $stale) {
    try {
      Stop-Process -Id $proc.ProcessId -Force -ErrorAction Stop
      Write-Host "Stopped stale runtime process: $($proc.ProcessId) [$($proc.Name)]"
    }
    catch {
      Write-Warning "Failed to stop process $($proc.ProcessId): $($_.Exception.Message)"
    }
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
  $env:CARGO_TARGET_DIR = Join-Path $safeRoot "target-gnu"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Ensure-GnuRustToolchain
$mingwRuntimeDir = Add-MingwToPathIfAvailable
Stop-StaleViteDevProcesses $repoRoot
Stop-StaleVoiceWaveRuntimeProcesses
Ensure-NoSpaceTargetDir
Ensure-DevRuntimeDlls -repoRoot $repoRoot -cargoTargetDir $env:CARGO_TARGET_DIR -mingwRuntimeDir $mingwRuntimeDir
Configure-FasterWhisperRuntime -repoRoot $repoRoot

$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"

$tauriCli = Join-Path $repoRoot "node_modules\.bin\tauri.cmd"
if (-not (Test-Path $tauriCli)) {
  throw "Tauri CLI not found at $tauriCli. Run npm install first."
}

$commandArgs = @("dev")
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
    throw "tauri dev failed"
  }
}
finally {
  Pop-Location
}
