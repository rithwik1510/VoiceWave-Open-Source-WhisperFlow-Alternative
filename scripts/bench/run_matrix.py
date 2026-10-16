"""Plan 019 Phase 0 benchmark matrix: speed, quality, idle and residency runs.

Each config runs as a subprocess; its stdout is saved as <out>/<name>.jsonl
(plus a final {"rc": ..} line) and stderr as <out>/<name>.stderr.txt.
Power state (AC / battery) is recorded in <out>/env.json. Summarize with
analyze.py. Audio, models and results stay outside the repo.

usage: python -I run_matrix.py --ggml-dir DIR --clips DIR --qclips DIR [--out DIR]
         [--worker-dir src-tauri/windows/asr-worker/bin] [--fw-python PY] [--fw-small DIR] [--fw-turbo DIR]
         [--repeats 3] [--idle-seconds 60] [--filter REGEX]
"""
import argparse, glob, json, os, re, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


def clips(folder, pattern="*.wav"):
    return sorted(glob.glob(os.path.join(folder, pattern)))


def power_state():
    ps = ("Get-CimInstance Win32_Battery | Select-Object Name,BatteryStatus,EstimatedChargeRemaining | "
          "ConvertTo-Json -Compress")
    out = subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True).stdout.strip()
    bat = json.loads(out) if out else None
    # Win32_Battery.BatteryStatus: 1 = discharging (on battery), 2 = on AC.
    status = bat.get("BatteryStatus") if isinstance(bat, dict) else None
    return {"battery": bat, "on_ac": status != 1 if status is not None else None}


def find_fw_model(repo_id):
    hub = os.path.join(os.environ.get("APPDATA", ""), "voicewave", "localcore", "data", "faster-whisper-cache", "hub")
    snaps = glob.glob(os.path.join(hub, "models--" + repo_id.replace("/", "--"), "snapshots", "*"))
    return snaps[0] if snaps else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ggml-dir", required=True)
    ap.add_argument("--clips", required=True, help="speed set (2/5/10/30 s buckets) from fetch_speed_set.py")
    ap.add_argument("--qclips", required=True, help="quality set from fetch_quality_set.py")
    ap.add_argument("--out", default=os.path.join(tempfile.gettempdir(), "voicewave-bench", "results"))
    ap.add_argument("--worker-dir", default=os.path.join(REPO, "src-tauri", "windows", "asr-worker", "bin"))
    ap.add_argument("--fw-python", default=os.path.join(REPO, ".venv-faster-whisper", "Scripts", "python.exe"))
    ap.add_argument("--fw-small", default=find_fw_model("Systran/faster-whisper-small.en"))
    ap.add_argument("--fw-turbo", default=find_fw_model("mobiuslabsgmbh/faster-whisper-large-v3-turbo"))
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--idle-seconds", type=float, default=60)
    ap.add_argument("--filter", default=".")
    a = ap.parse_args()

    os.makedirs(a.out, exist_ok=True)
    env = {"time": time.strftime("%Y-%m-%d %H:%M:%S"), **power_state()}
    json.dump(env, open(os.path.join(a.out, "env.json"), "w"), indent=1)
    print("power:", env)

    vk = os.path.join(a.worker_dir, "vk", "voicewave-asr-vk.exe")
    cpu = os.path.join(a.worker_dir, "cpu", "voicewave-asr-cpu.exe")
    small = os.path.join(a.ggml_dir, "ggml-small.en.bin")
    turbo = os.path.join(a.ggml_dir, "ggml-large-v3-turbo.bin")
    speed = clips(a.clips)
    quality = clips(a.qclips)
    idle_wavs = clips(a.clips, "10s-*.wav")[:1] + clips(a.clips, "2s-*.wav")[:1]
    py = sys.executable
    wb = [py, "-I", os.path.join(HERE, "worker_bench.py")]
    fwb = [a.fw_python, "-I", os.path.join(HERE, "fw_bench.py")]
    R = str(a.repeats)

    def wk(exe, model, device, fa, reps, wavs, *extra):
        return wb + ["--worker", exe, "--model", model, "--device", device, "--threads", "8",
                     "--flash-attn", str(fa), "--repeats", reps, *extra, *wavs]

    configs = [
        # Speed (spike comparison, within 10%).
        ("wk-vk-nv-small", wk(vk, small, "discrete", 1, R, speed, "--vram")),
        ("wk-vk-nv-turbo", wk(vk, turbo, "discrete", 1, R, speed, "--vram")),
        ("wk-vk-igpu-small", wk(vk, small, "integrated", 0, R, speed)),
        ("wk-cpu-t8-small", wk(cpu, small, "cpu", 0, R, speed)),
        # Production warm-up path: does the silent warm-up absorb the slow first decode?
        ("warm-vk-igpu-small", wk(vk, small, "integrated", 0, "1", idle_wavs, "--silent-warmup")),
        ("warm-vk-nv-turbo", wk(vk, turbo, "discrete", 1, "1", idle_wavs, "--silent-warmup")),
        # After-idle decode + VRAM held while resident.
        ("idle-vk-nv-turbo", wk(vk, turbo, "discrete", 1, "1", idle_wavs, "--vram", "--silent-warmup",
                                "--idle-seconds", str(a.idle_seconds), *sum([["--idle-wav", w] for w in idle_wavs], []))),
        # Quality (>= 100 clips, text only).
        ("q-wk-vk-nv-small", wk(vk, small, "discrete", 1, "0", quality)),
        ("q-wk-vk-nv-turbo", wk(vk, turbo, "discrete", 1, "0", quality)),
        ("q-wk-cpu-t8-small", wk(cpu, small, "cpu", 0, "0", quality)),
    ]
    if a.fw_small:
        configs.append(("q-fw-cpu-int8-small", fwb + [a.fw_small, "cpu", "int8", "0", "0", *quality]))
    if a.fw_turbo:
        configs.append(("q-fw-cuda-f16-turbo", fwb + [a.fw_turbo, "cuda", "float16", "0", "0", *quality]))

    filt = re.compile(a.filter)
    for name, cmd in configs:
        if not filt.search(name):
            continue
        t0 = time.time()
        p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
        with open(os.path.join(a.out, name + ".jsonl"), "w", encoding="utf-8") as f:
            f.write(p.stdout)
            f.write(json.dumps({"rc": p.returncode, "wall_s": round(time.time() - t0, 1)}) + "\n")
        with open(os.path.join(a.out, name + ".stderr.txt"), "w", encoding="utf-8") as f:
            f.write(p.stderr)
        print(name, "rc", p.returncode, "wall", round(time.time() - t0, 1), flush=True)


if __name__ == "__main__":
    main()
