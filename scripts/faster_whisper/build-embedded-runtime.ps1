<#
.SYNOPSIS
  Stage a relocatable, CPU-only Python runtime with faster-whisper pre-installed,
  so it can be bundled INSIDE the VoiceWave installer.

.DESCRIPTION
  The default ASR model (fw-small.en) runs faster-whisper in a Python subprocess.
  Without this, a fresh install has no Python + faster-whisper and cannot
  transcribe (it only works in `npm run tauri:dev`, which points
  VOICEWAVE_FASTER_WHISPER_PYTHON at the dev .venv). This script produces a
  self-contained `python/` tree that the app resolves at runtime via
  <exe_dir>/faster-whisper/python/python.exe (see resolve_python_path /
  python_layout_candidates in src-tauri/src/inference/faster_whisper.rs).

  It uses astral-sh/python-build-standalone (the same relocatable CPython that
  uv/rye ship) rather than the Windows "embeddable" zip, because standalone
  builds install native wheels (ctranslate2, onnxruntime, av) cleanly and run
  from any directory.

.PARAMETER OutputDir
  Directory that will contain the `python/` tree. Defaults to
  src-tauri/windows/faster-whisper (next to worker.py), which is what
  tauri.conf.json bundles.

.PARAMETER PythonVersion
  Major.minor CPython to fetch (default 3.12, matching the dev venv's 3.12.6).

.PARAMETER Clean
  Remove any existing python/ tree first for a from-scratch build.

.EXAMPLE
  ./scripts/faster_whisper/build-embedded-runtime.ps1 -Clean
#>
param(
  [string]$OutputDir,
  [string]$PythonVersion = "3.12",
  [string]$RequirementsFile,
  [switch]$Clean
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
  $OutputDir = Join-Path $repoRoot "src-tauri\windows\faster-whisper"
}
if ([string]::IsNullOrWhiteSpace($RequirementsFile)) {
  $RequirementsFile = Join-Path $PSScriptRoot "embedded-runtime-requirements.txt"
}
if (-not (Test-Path $RequirementsFile)) {
  throw "Requirements file not found: $RequirementsFile"
}

$pythonDir = Join-Path $OutputDir "python"
if ($Clean -and (Test-Path $pythonDir)) {
  Write-Host "Cleaning existing runtime at $pythonDir"
  Remove-Item -Recurse -Force $pythonDir
}
if (Test-Path (Join-Path $pythonDir "python.exe")) {
  Write-Host "Runtime already present at $pythonDir. Use -Clean to rebuild."
  return
}
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

function Resolve-StandaloneAssetUrl([string]$version) {
  # Ask the GitHub API for the newest python-build-standalone release and pick
  # the install_only Windows x64 asset for the requested Python minor version.
  # Pinning by API lookup avoids hard-coding a release tag that later 404s.
  $headers = @{ "User-Agent" = "voicewave-build" }
  if (-not [string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
    $headers["Authorization"] = "Bearer $env:GH_TOKEN"
  }
  $releasesUrl = "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=10"
  $releases = Invoke-RestMethod -Uri $releasesUrl -Headers $headers
  $pattern = "cpython-$([regex]::Escape($version))\.\d+\+\d+-x86_64-pc-windows-msvc-install_only\.tar\.gz$"
  foreach ($release in $releases) {
    $asset = $release.assets |
      Where-Object { $_.name -match $pattern } |
      Sort-Object name -Descending |
      Select-Object -First 1
    if ($asset) {
      return $asset.browser_download_url
    }
  }
  throw "Could not find a python-build-standalone install_only asset for Python $version."
}

$assetUrl = Resolve-StandaloneAssetUrl $PythonVersion
Write-Host "Python runtime source: $assetUrl"

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("vw-pyrt-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
  $archive = Join-Path $work "python.tar.gz"
  Write-Host "Downloading..."
  Invoke-WebRequest -Uri $assetUrl -OutFile $archive

  Write-Host "Extracting..."
  # bsdtar (tar.exe) ships with Windows 10+ and handles .tar.gz. The archive
  # expands to a top-level `python/` directory containing python.exe.
  & tar.exe -xzf $archive -C $work
  if ($LASTEXITCODE -ne 0) { throw "tar extraction failed" }

  $extractedPython = Join-Path $work "python"
  if (-not (Test-Path (Join-Path $extractedPython "python.exe"))) {
    throw "Extracted archive did not contain python/python.exe"
  }

  # $pythonDir may already exist here (e.g. containing only the tracked
  # .gitkeep placeholder from a fresh checkout). If it exists, Move-Item
  # moves the source INSIDE it instead of renaming into place, landing
  # python.exe one directory too deep. Clear it first so the move always
  # renames $extractedPython to become $pythonDir.
  if (Test-Path $pythonDir) {
    Remove-Item -Recurse -Force $pythonDir
  }
  Move-Item -LiteralPath $extractedPython -Destination $pythonDir
}
finally {
  if (Test-Path $work) { Remove-Item -Recurse -Force $work }
}

$py = Join-Path $pythonDir "python.exe"
Write-Host "Upgrading pip..."
& $py -m pip install --upgrade pip --no-warn-script-location
if ($LASTEXITCODE -ne 0) { throw "pip upgrade failed" }

Write-Host "Installing faster-whisper runtime (CPU)..."
& $py -m pip install --no-warn-script-location -r $RequirementsFile
if ($LASTEXITCODE -ne 0) { throw "pip install of runtime requirements failed" }

# llama-cpp-python powers the on-device AI-polish worker (polish_worker.py).
# We install our OWN prebuilt CPU wheel, NOT the stock PyPI one: the official
# wheel is compiled with AVX-512 and crashes with 0xC000001D (illegal
# instruction) on CPUs that lack it. Our wheel (scripts/llm-polish/wheelhouse)
# targets AVX2/FMA/F16C only — broad compatibility across shipped machines.
# --find-links resolves llama-cpp-python from that local wheel; its small pure
# deps (diskcache, jinja2) come from PyPI.
Write-Host "Installing on-device AI-polish runtime (llama-cpp-python, AVX2 CPU wheel)..."
$llamaWheelhouse = Join-Path $repoRoot "scripts\llm-polish\wheelhouse"
$llamaWheel = Get-ChildItem (Join-Path $llamaWheelhouse "llama_cpp_python-*-win_amd64.whl") -ErrorAction SilentlyContinue |
  Select-Object -First 1
if (-not $llamaWheel) {
  throw "AVX2 llama-cpp-python wheel not found in $llamaWheelhouse. It must be committed (or built via scripts/llm-polish/build_llama.bat) so the AI-polish runtime can ship."
}
& $py -m pip install --no-warn-script-location --find-links $llamaWheelhouse $llamaWheel.Name
if ($LASTEXITCODE -ne 0) { throw "pip install of llama-cpp-python failed" }

Write-Host "Trimming runtime to reduce installer size..."
# Byte-caches and the stdlib test suite are dead weight in a shipped runtime.
Get-ChildItem -Path $pythonDir -Recurse -Directory -Filter "__pycache__" -ErrorAction SilentlyContinue |
  ForEach-Object { Remove-Item -Recurse -Force $_.FullName -ErrorAction SilentlyContinue }
$stdlibTest = Join-Path $pythonDir "Lib\test"
if (Test-Path $stdlibTest) { Remove-Item -Recurse -Force $stdlibTest -ErrorAction SilentlyContinue }

Write-Host "Verifying runtime imports..."
$check = 'import faster_whisper, ctranslate2, numpy, av, onnxruntime, tokenizers, huggingface_hub, llama_cpp; from faster_whisper import WhisperModel; from llama_cpp import Llama; print("embedded runtime OK:", faster_whisper.__version__, ctranslate2.__version__, "llama_cpp", llama_cpp.__version__)'
& $py -c $check
if ($LASTEXITCODE -ne 0) { throw "Runtime import self-check failed" }

$sizeMb = [math]::Round(((Get-ChildItem -Recurse -File $pythonDir | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Host ""
Write-Host "Embedded runtime staged at: $pythonDir"
Write-Host "Runtime size: $sizeMb MB"
Write-Host "App will resolve it at <install>/faster-whisper/python/python.exe"
