# Smoke test for the speech workers on any machine, GPU or not (CI runners have none):
#   - voicewave-asr-cpu.exe --list-devices succeeds
#   - voicewave-asr-vk.exe --list-devices succeeds, or fails cleanly without hanging
#   - with -ModelPath: the CPU worker starts, loads a whisper model on CPU, decodes
#     1 s of silence over the JSON protocol, and exits when stdin closes
#   - with -ParakeetModelPath: the CPU worker loads the Parakeet model
#     ("engine":"parakeet"), decodes the bundled LibriSpeech clip and must return
#     text containing "furniture", then decodes it again with boostTerms and
#     checks the reply shows the patched logits filter ran and a term matched
# Usage: ./scripts/asr-worker/smoke-asr-worker.ps1 [-BinDir <dir>] [-ModelPath <ggml .bin>]
#          [-ParakeetModelPath <parakeet ggml .bin>] [-ClipPath <16 kHz mono wav>] [-CpuOnly]
# -CpuOnly skips the Vulkan worker (e.g. on a machine where it should not be started).
param(
  [string]$BinDir = "",
  [string]$ModelPath = "",
  [string]$ParakeetModelPath = "",
  [string]$ClipPath = "",
  [switch]$CpuOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($BinDir)) {
  $BinDir = Join-Path $PSScriptRoot "..\..\src-tauri\windows\asr-worker\bin"
}
$cpuExe = (Resolve-Path (Join-Path $BinDir "cpu\voicewave-asr-cpu.exe")).Path
$vkExe = if ($CpuOnly) { $null } else { (Resolve-Path (Join-Path $BinDir "vk\voicewave-asr-vk.exe")).Path }

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
if ($vkExe) {
  $vk = Invoke-ListDevices $vkExe 20000
  if ($vk.ExitCode -eq 0) {
    $parsed = $vk.Line | ConvertFrom-Json
    if (-not $parsed.ok) { throw "voicewave-asr-vk.exe --list-devices returned ok=false: $($vk.Line)" }
    Write-Host "vk --list-devices OK in $($vk.Ms) ms: $($vk.Line)"
  } else {
    Write-Host "vk --list-devices failed cleanly (exit $($vk.ExitCode)) in $($vk.Ms) ms: $($vk.Line)"
  }
}

# 3. Protocol round trips on the CPU worker. Starts a worker, loads the model,
# runs $body against it, then checks the worker exits when stdin closes.
function Send-Request($proc, [hashtable]$request) {
  $proc.StandardInput.WriteLine(($request | ConvertTo-Json -Compress))
  $proc.StandardInput.Flush()
}

function Read-Reply($proc) {
  $t = $proc.StandardOutput.ReadLineAsync()
  if (-not $t.Wait(120000)) { throw "worker did not answer within 120 s" }
  if ($null -eq $t.Result) { throw "worker exited (code $(if ($proc.HasExited) { $proc.ExitCode }))" }
  return $t.Result | ConvertFrom-Json
}

function Invoke-CpuSession([string]$modelPath, [string]$engine, [scriptblock]$body) {
  $p = Start-Worker $cpuExe @()
  try {
    $ready = Read-Reply $p
    if (-not $ready.ready) { throw "no ready line" }
    if ($engine -eq "parakeet" -and $ready.engines -notcontains "parakeet") {
      throw "worker does not advertise the parakeet engine: $($ready | ConvertTo-Json -Compress)"
    }
    $load = @{ id = "load"; command = "load"; modelPath = (Resolve-Path $modelPath).Path; device = "cpu"; threads = 2; warmup = $false }
    if ($engine -eq "parakeet") { $load.engine = "parakeet" }
    Send-Request $p $load
    $loaded = Read-Reply $p
    if (-not $loaded.ok -or $loaded.backendUsed -ne "cpu") { throw "$engine load failed: $($loaded | ConvertTo-Json -Compress)" }
    if ($engine -eq "parakeet" -and $loaded.engine -ne "parakeet") { throw "load answered engine '$($loaded.engine)', expected parakeet" }
    Write-Host "cpu $engine load OK ($($loaded.modelInitMs) ms)"
    & $body $p
    $p.StandardInput.Close()
    if (-not $p.WaitForExit(5000)) { throw "worker did not exit after stdin closed" }
  } finally {
    if (-not $p.HasExited) { $p.Kill() }
  }
}

# 16 kHz mono int16 PCM payload of a wav file, base64 (the worker's audioPcm16B64).
function Get-WavPcmBase64([string]$path) {
  $bytes = [System.IO.File]::ReadAllBytes($path)
  $pos = 12  # skip RIFF header
  while ($pos + 8 -le $bytes.Length) {
    $id = [System.Text.Encoding]::ASCII.GetString($bytes, $pos, 4)
    $size = [BitConverter]::ToUInt32($bytes, $pos + 4)
    if ($id -eq "data") {
      $len = [Math]::Min([int64]$size, $bytes.Length - $pos - 8)
      return [Convert]::ToBase64String($bytes, $pos + 8, [int]$len)
    }
    $pos += 8 + $size + ($size % 2)
  }
  throw "no data chunk in $path"
}

if ($ModelPath) {
  Invoke-CpuSession $ModelPath "whisper" {
    param($p)
    $silence = [Convert]::ToBase64String([byte[]]::new(32000))  # 1 s of 16 kHz int16 silence
    Send-Request $p @{ id = 2; command = "transcribe"; audioPcm16B64 = $silence }
    $res = Read-Reply $p
    if (-not $res.ok -or $res.id -ne 2) { throw "transcribe failed: $($res | ConvertTo-Json -Compress)" }
    Write-Host "cpu whisper decode OK (decode $($res.decodeComputeMs) ms)"
  }
}

# 4. Parakeet: real speech, so an empty or garbled result is a failure, not a pass.
if ($ParakeetModelPath) {
  if ([string]::IsNullOrWhiteSpace($ClipPath)) { $ClipPath = Join-Path $BinDir "..\qualify\short.wav" }
  $clip = Get-WavPcmBase64 (Resolve-Path $ClipPath).Path  # LibriSpeech: "NOW LET'S DUST THE FURNITURE AND PICTURES"
  Invoke-CpuSession $ParakeetModelPath "parakeet" {
    param($p)
    Send-Request $p @{ id = 3; command = "transcribe"; audioPcm16B64 = $clip }
    $plain = Read-Reply $p
    if (-not $plain.ok -or $plain.id -ne 3) { throw "parakeet transcribe failed: $($plain | ConvertTo-Json -Compress)" }
    if ($plain.text -notmatch '(?i)furniture') { throw "parakeet transcript missing 'furniture': '$($plain.text)'" }
    Write-Host "cpu parakeet decode OK (decode $($plain.decodeComputeMs) ms): $($plain.text)"

    # Same clip with dictionary terms: runs the patched logits-filter and token callbacks.
    # "furniture" and "pictures" are not single vocab pieces, so they survive the
    # known-word skip and must match during the decode.
    Send-Request $p @{ id = 4; command = "transcribe"; audioPcm16B64 = $clip; boostTerms = @("furniture", "pictures", "Voicewave") }
    $boosted = Read-Reply $p
    if (-not $boosted.ok -or $boosted.id -ne 4) { throw "parakeet boosted transcribe failed: $($boosted | ConvertTo-Json -Compress)" }
    if ($boosted.text -notmatch '(?i)furniture') { throw "boosted parakeet transcript missing 'furniture': '$($boosted.text)'" }
    # steps counts logits-filter calls (zero if the patch's hook is not wired);
    # completed counts terms matched through the token callback.
    if (-not $boosted.PSObject.Properties["boost"] -or $boosted.boost.steps -lt 1 -or $boosted.boost.completed -lt 1) {
      throw "boosted decode did not run the booster: $($boosted | ConvertTo-Json -Compress)"
    }
    Write-Host "cpu parakeet boosted decode OK (decode $($boosted.decodeComputeMs) ms, boost $($boosted.boost | ConvertTo-Json -Compress)): $($boosted.text)"
  }
}
Write-Host "Speech worker smoke test passed."
