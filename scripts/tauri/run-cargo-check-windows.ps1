param(
  [switch]$NoDefaultFeatures,
  [switch]$PreferMsvc,
  [switch]$InstallMsvcComponents,
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$CargoArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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

function Ensure-GnuRustToolchain {
  $rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
  if (-not (Test-Path $rustup)) {
    throw "rustup not found. Install Rust first."
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
  $candidatePaths = @(
    (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"),
    (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\MartinStorsjo.LLVM-MinGW.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\llvm-mingw-20251216-ucrt-x86_64\bin")
  )

  foreach ($candidate in $candidatePaths) {
    if (Test-Path $candidate) {
      Prepend-PathEntryIfExists $candidate
      return
    }
  }

  throw "MinGW toolchain not found. Install WinLibs or LLVM-MinGW via winget."
}

function Ensure-NoSpaceTargetDir {
  if ($env:VIBE_SAFE_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = $env:VIBE_SAFE_TARGET_DIR
    return
  }

  $safeRoot = "C:\voicewave-tauri"
  if (-not (Test-Path $safeRoot)) {
    New-Item -ItemType Directory -Path $safeRoot | Out-Null
  }
  $env:CARGO_TARGET_DIR = Join-Path $safeRoot "target-check"
}

function Resolve-VswherePath {
  $candidate = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $candidate) {
    return $candidate
  }
  return $null
}

function Resolve-VisualStudioInstallPath {
  $vswhere = Resolve-VswherePath
  if (-not $vswhere) {
    return $null
  }
  $installPath = & $vswhere -latest -products * -property installationPath
  if ([string]::IsNullOrWhiteSpace($installPath)) {
    return $null
  }
  return $installPath.Trim()
}

function Resolve-MsvcVersionPath([string]$installPath) {
  if ([string]::IsNullOrWhiteSpace($installPath)) {
    return $null
  }
  $msvcRoot = Join-Path $installPath "VC\Tools\MSVC"
  if (-not (Test-Path $msvcRoot)) {
    return $null
  }
  $versionDir = Get-ChildItem $msvcRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1
  if ($null -eq $versionDir) {
    return $null
  }
  return $versionDir.FullName
}

function Resolve-LatestWindowsSdkVersion {
  $sdkRoot = "C:\Program Files (x86)\Windows Kits\10\Lib"
  if (-not (Test-Path $sdkRoot)) {
    return $null
  }
  $versions = Get-ChildItem $sdkRoot -Directory | Sort-Object Name -Descending
  foreach ($version in $versions) {
    $umKernel32 = Join-Path $version.FullName "um\x64\kernel32.lib"
    $ucrtLib = Join-Path $version.FullName "ucrt\x64\ucrt.lib"
    if ((Test-Path $umKernel32) -and (Test-Path $ucrtLib)) {
      return $version.Name
    }
  }
  return $null
}

function Configure-MsvcEnvironment([string]$installPath) {
  $msvcVersionPath = Resolve-MsvcVersionPath $installPath
  $sdkVersion = Resolve-LatestWindowsSdkVersion
  if (-not $msvcVersionPath -or -not $sdkVersion) {
    return $false
  }

  $binPath = Join-Path $msvcVersionPath "bin\Hostx64\x64"
  $libPath = Join-Path $msvcVersionPath "lib\x64"
  $includePath = Join-Path $msvcVersionPath "include"

  $sdkRoot = "C:\Program Files (x86)\Windows Kits\10"
  $sdkUcrtLib = Join-Path $sdkRoot "Lib\$sdkVersion\ucrt\x64"
  $sdkUmLib = Join-Path $sdkRoot "Lib\$sdkVersion\um\x64"
  $sdkSharedInclude = Join-Path $sdkRoot "Include\$sdkVersion\shared"
  $sdkUcrtInclude = Join-Path $sdkRoot "Include\$sdkVersion\ucrt"
  $sdkUmInclude = Join-Path $sdkRoot "Include\$sdkVersion\um"

  if (
    -not (Test-Path $binPath) -or
    -not (Test-Path $libPath) -or
    -not (Test-Path $sdkUcrtLib) -or
    -not (Test-Path $sdkUmLib)
  ) {
    return $false
  }

  Prepend-PathEntryIfExists $binPath

  $libEntries = @($libPath, $sdkUcrtLib, $sdkUmLib)
  if ([string]::IsNullOrWhiteSpace($env:LIB)) {
    $env:LIB = $libEntries -join ";"
  } else {
    $env:LIB = (($libEntries + @($env:LIB)) -join ";")
  }

  $includeEntries = @($includePath, $sdkSharedInclude, $sdkUcrtInclude, $sdkUmInclude)
  if ([string]::IsNullOrWhiteSpace($env:INCLUDE)) {
    $env:INCLUDE = $includeEntries -join ";"
  } else {
    $env:INCLUDE = (($includeEntries + @($env:INCLUDE)) -join ";")
  }

  return $true
}

function Install-MsvcComponentsIfRequested([string]$installPath) {
  if (-not $InstallMsvcComponents) {
    return
  }
  if ([string]::IsNullOrWhiteSpace($installPath)) {
    throw "No Visual Studio Build Tools installation was found to modify."
  }
  $setup = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe"
  if (-not (Test-Path $setup)) {
    throw "Visual Studio installer executable not found at $setup"
  }

  & $setup modify `
    --installPath $installPath `
    --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    --add Microsoft.VisualStudio.Component.Windows10SDK.19041 `
    --includeRecommended `
    --quiet `
    --norestart `
    --nocache
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Ensure-NoSpaceTargetDir

$installPath = Resolve-VisualStudioInstallPath
Install-MsvcComponentsIfRequested $installPath

$msvcReady = $false
if ($installPath) {
  $msvcReady = Configure-MsvcEnvironment $installPath
}

if ($PreferMsvc -and -not $msvcReady) {
  throw "MSVC toolchain is not ready. Re-run with -InstallMsvcComponents or use GNU fallback."
}

if ($msvcReady) {
  $env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
  Write-Host "Using Rust toolchain: $env:RUSTUP_TOOLCHAIN"
} else {
  Ensure-GnuRustToolchain
  Add-MingwToPathIfAvailable
  $env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
  Write-Warning "MSVC linker prerequisites were not found. Using GNU toolchain fallback for cargo check."
  Write-Host "Using Rust toolchain: $env:RUSTUP_TOOLCHAIN"
}

$cargoManifestPath = Join-Path $repoRoot "src-tauri\Cargo.toml"
if (-not (Test-Path $cargoManifestPath)) {
  throw "Cargo manifest not found: $cargoManifestPath"
}

$commandArgs = @("check", "--manifest-path", $cargoManifestPath)
if ($NoDefaultFeatures) {
  $commandArgs += "--no-default-features"
}
if ($CargoArgs) {
  $commandArgs += $CargoArgs
}

Push-Location $repoRoot
try {
  & cargo @commandArgs
  if ($LASTEXITCODE -ne 0) {
    throw "cargo check failed"
  }
}
finally {
  Pop-Location
}
