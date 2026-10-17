<#
.SYNOPSIS
  Strip files the shipped Python runtime never loads, to shrink the installer.

.DESCRIPTION
  Idempotent: safe to run on a fresh, half-pruned, or fully pruned runtime.
  Called from build-embedded-runtime.ps1 (fresh builds) and from
  prepare-tauri-bundle-windows.ps1 (runtimes staged before pruning existed or
  restored from a cache), so the installer is always built from a pruned tree.

  Everything removed here was checked against the only two things that run on
  this interpreter: scripts/faster_whisper/worker.py and
  src-tauri/windows/llm-polish/polish_worker.py. The build script's import
  self-check runs after this and fails the build if a prune broke an import.

.PARAMETER PythonDir
  The staged `python/` tree (contains python.exe).
#>
param(
  [Parameter(Mandatory = $true)][string]$PythonDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$PythonDir = (Resolve-Path $PythonDir).Path
if (-not (Test-Path (Join-Path $PythonDir "python.exe"))) {
  throw "Not a Python runtime (python.exe missing): $PythonDir"
}

function Get-TreeSizeMb([string]$path) {
  $sum = (Get-ChildItem -Recurse -File -Force $path -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  return [math]::Round(($sum / 1MB), 1)
}

function Remove-IfPresent([string]$relativePath) {
  $full = Join-Path $PythonDir $relativePath
  if (Test-Path $full) { Remove-Item -Recurse -Force $full }
}

$beforeMb = Get-TreeSizeMb $PythonDir
$site = "Lib\site-packages"

# Debug symbols and compiler import libraries: only useful when building C
# extensions or debugging CPython itself, never at runtime. The .dll files that
# live beside the llama/ggml .lib files stay (llama_cpp loads them from
# site-packages\llama_cpp\lib).
Get-ChildItem -Path $PythonDir -Recurse -File -Force -Include "*.pdb", "*.lib" |
  ForEach-Object { Remove-Item -Force $_.FullName }
Remove-IfPresent "libs"
Remove-IfPresent "include"

# Byte-caches regenerate on first import; the stdlib test suite and C-API test
# extension modules are dead weight.
@(Get-ChildItem -Path $PythonDir -Recurse -Directory -Force -Filter "__pycache__") |
  ForEach-Object { Remove-Item -Recurse -Force $_.FullName }
Remove-IfPresent "Lib\test"
Remove-IfPresent "Lib\turtledemo"
Get-ChildItem -Path (Join-Path $PythonDir "DLLs") -File -Include "_test*.pyd", "_ctypes_test.pyd", "xxlimited*.pyd" |
  ForEach-Object { Remove-Item -Force $_.FullName }

# GUI and packaging tooling: neither worker (nor ctranslate2, tokenizers,
# huggingface_hub, llama_cpp) imports tkinter, pip, setuptools, or venv. The
# runtime is read-only after install, so nothing ever pip-installs into it.
Remove-IfPresent "tcl"
Remove-IfPresent "Lib\tkinter"
Remove-IfPresent "Lib\idlelib"
Remove-IfPresent "Lib\ensurepip"
Remove-IfPresent "Lib\venv"
Remove-IfPresent "Scripts"
Remove-IfPresent "share"
foreach ($name in "_tkinter.pyd", "tcl86t.dll", "tk86t.dll") {
  Remove-IfPresent "DLLs\$name"
}
# setuptools installs a .pth that imports _distutils_hack at interpreter
# startup; remove the .pth together with the module or every launch errors.
foreach ($name in "pip", "setuptools", "pkg_resources", "_distutils_hack", "distutils-precedence.pth") {
  Remove-IfPresent "$site\$name"
}
Get-ChildItem -Path (Join-Path $PythonDir $site) -Directory -Filter "*.dist-info" |
  Where-Object { $_.Name -match '^(pip|setuptools)-' } |
  ForEach-Object { Remove-Item -Recurse -Force $_.FullName }

# Silero VAD is never used: the app always sends vad_filter=false, and faster-whisper
# imports onnxruntime only inside SileroVADModel.__init__. onnxruntime is a hard
# pip dependency of faster-whisper, so it is installed and then pruned here
# along with the packages only it needs (sympy -> mpmath, flatbuffers, protobuf).
# If VAD is ever enabled, drop this block and re-add onnxruntime to the self-check.
foreach ($name in "onnxruntime", "sympy", "mpmath", "flatbuffers", "google", "isympy.py") {
  Remove-IfPresent "$site\$name"
}
Get-ChildItem -Path (Join-Path $PythonDir $site) -Directory -Filter "*.dist-info" |
  Where-Object { $_.Name -match '^(onnxruntime|sympy|mpmath|flatbuffers|protobuf)-' } |
  ForEach-Object { Remove-Item -Recurse -Force $_.FullName }

# The llama-cpp-python wheel also drops a second copy of its DLLs and the
# CMake/pkg-config build files at site-packages\{bin,lib,include}. llama_cpp
# loads only site-packages\llama_cpp\lib (llama_cpp.py, mtmd_cpp.py).
foreach ($name in "bin", "lib", "include") {
  Remove-IfPresent "$site\$name"
}

# Third-party test suites (numpy/tests, fsspec/tests, ...). Only directories
# named exactly tests/test; importable packages like numpy.testing are kept.
@(Get-ChildItem -Path (Join-Path $PythonDir $site) -Recurse -Directory -Force) |
  Where-Object { $_.Name -in "tests", "test" } |
  ForEach-Object { if (Test-Path $_.FullName) { Remove-Item -Recurse -Force $_.FullName } }

$afterMb = Get-TreeSizeMb $PythonDir
Write-Host "Pruned runtime: $beforeMb MB -> $afterMb MB"
