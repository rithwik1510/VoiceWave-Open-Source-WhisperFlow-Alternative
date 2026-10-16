# Smoke test for the speech workers on any machine, GPU or not (CI runners have none):
#   - voicewave-asr-cpu.exe --list-devices succeeds
#   - voicewave-asr-vk.exe --list-devices succeeds, or fails cleanly without hanging
#   - with -ModelPath: the CPU worker starts, loads the model on CPU, decodes
#     1 s of silence over the JSON protocol, and exits when stdin closes
# Usage: ./scripts/asr-worker/smoke-asr-worker.ps1 [-BinDir <dir>] [-ModelPath <ggml .bin>]
param([string]$BinDir = "", [string]$ModelPath = "")
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($BinDir)) {
  $BinDir = Join-Path $PSScriptRoot "..\..\src-tauri\windows\asr-worker\bin"
}
$cpuExe = (Resolve-Path (Join-Path $BinDir "cpu\voicewave-asr-cpu.exe")).Path
$vkExe = (Resolve-Path (Join-Path $BinDir "vk\voicewave-asr-vk.exe")).Path

function Start-Worker([string]$exe, [string[]]$arguments) {
  $psi = [System.Diagnostics.ProcessStartInfo]::new($exe)
  foreach ($a in $arguments) { $psi.ArgumentList.Add($a) }
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $p = [System.Diagnostics.Process]::Start($psi)
  $p.BeginErrorReadLine()  # drain logs so the worker never blocks on a full stderr pipe
  return $p
}

function Invoke-ListDevices([string]$exe, [int]$timeoutMs) {
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $p = Start-Worker $exe @("--list-devices")
  $line = $p.StandardOutput.ReadLineAsync()
  if (-not $p.WaitForExit($timeoutMs)) {
    $p.Kill()
    throw "$(Split-Path -Leaf $exe) --list-devices hung for more than $timeoutMs ms"
  }
  return [pscustomobject]@{ ExitCode = $p.ExitCode; Line = $line.Result; Ms = $sw.ElapsedMilliseconds }
}

# 1. CPU worker: must succeed.
$cpu = Invoke-ListDevices $cpuExe 10000
if ($cpu.ExitCode -ne 0 -or -not ($cpu.Line | ConvertFrom-Json).ok) {
  throw "voicewave-asr-cpu.exe --list-devices failed (exit $($cpu.ExitCode)): $($cpu.Line)"
}
Write-Host "cpu --list-devices OK in $($cpu.Ms) ms: $($cpu.Line)"

# 2. Vulkan worker: success, or a clean failure (no driver / no device), never a hang.
$vk = Invoke-ListDevices $vkExe 20000
if ($vk.ExitCode -eq 0) {
  $parsed = $vk.Line | ConvertFrom-Json
  if (-not $parsed.ok) { throw "voicewave-asr-vk.exe --list-devices returned ok=false: $($vk.Line)" }
  Write-Host "vk --list-devices OK in $($vk.Ms) ms: $($vk.Line)"
} else {
  Write-Host "vk --list-devices failed cleanly (exit $($vk.ExitCode)) in $($vk.Ms) ms: $($vk.Line)"
}

# 3. Protocol round trip on the CPU worker with a real model.
if ($ModelPath) {
  $p = Start-Worker $cpuExe @()
  try {
    $read = {
      param($proc)
      $t = $proc.StandardOutput.ReadLineAsync()
      if (-not $t.Wait(120000)) { throw "worker did not answer within 120 s" }
      if ($null -eq $t.Result) { throw "worker exited (code $(if ($proc.HasExited) { $proc.ExitCode }))" }
      return $t.Result | ConvertFrom-Json
    }
    $ready = & $read $p
    if (-not $ready.ready) { throw "no ready line" }
    $load = @{ id = "load"; command = "load"; modelPath = (Resolve-Path $ModelPath).Path; device = "cpu"; threads = 2; warmup = $false } | ConvertTo-Json -Compress
    $p.StandardInput.WriteLine($load); $p.StandardInput.Flush()
    $loaded = & $read $p
    if (-not $loaded.ok -or $loaded.backendUsed -ne "cpu") { throw "load failed: $($loaded | ConvertTo-Json -Compress)" }
    $silence = [Convert]::ToBase64String([byte[]]::new(32000))  # 1 s of 16 kHz int16 silence
    $p.StandardInput.WriteLine((@{ id = 2; command = "transcribe"; audioPcm16B64 = $silence } | ConvertTo-Json -Compress))
    $p.StandardInput.Flush()
    $res = & $read $p
    if (-not $res.ok -or $res.id -ne 2) { throw "transcribe failed: $($res | ConvertTo-Json -Compress)" }
    Write-Host "cpu worker load + decode OK (load $($loaded.modelInitMs) ms, decode $($res.decodeComputeMs) ms)"
    $p.StandardInput.Close()
    if (-not $p.WaitForExit(5000)) { throw "worker did not exit after stdin closed" }
  } finally {
    if (-not $p.HasExited) { $p.Kill() }
  }
}
Write-Host "Speech worker smoke test passed."
