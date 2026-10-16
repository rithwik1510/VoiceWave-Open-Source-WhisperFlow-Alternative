"""faster-whisper baseline with the same protocol and output as worker_bench.py.

Loads the model in-process, warms up on the first clip, then per clip runs one
discarded warm decode plus N timed repeats. Run it with the faster-whisper dev
venv (.venv-faster-whisper); CUDA DLLs come from its nvidia-* wheels.

usage: python -I fw_bench.py <model_dir> <cpu|cuda> <compute_type> <threads 0=default> <repeats> <wav>...
"""
import glob, json, os, sys, sysconfig, time, wave

import numpy as np

for d in glob.glob(os.path.join(sysconfig.get_paths()["purelib"], "nvidia", "*", "bin")):
    os.add_dll_directory(d)
    os.environ["PATH"] = d + os.pathsep + os.environ["PATH"]

import psutil  # noqa: E402
from faster_whisper import WhisperModel  # noqa: E402

model_dir, device, ctype, threads, reps = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
wavs = sys.argv[6:]

# Same decode settings as the speech worker: English, beam 5, temperature 0,
# no previous-text conditioning, no VAD.
KW = dict(beam_size=5, best_of=1, language="en", temperature=0.0, condition_on_previous_text=False,
          vad_filter=False, without_timestamps=False)


def read(p):
    with wave.open(p) as w:
        return np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768.0


def run(m, a):
    return "".join(s.text for s in m.transcribe(a, **KW)[0]).strip()


t0 = time.perf_counter()
m = WhisperModel(model_dir, device=device, compute_type=ctype, cpu_threads=threads, local_files_only=True)
load = (time.perf_counter() - t0) * 1000
t = time.perf_counter()
run(m, read(wavs[0]))
warm = (time.perf_counter() - t) * 1000
print(json.dumps({"load_ms": round(load, 1), "warmup_ms": round(warm, 1), "backend": device}), flush=True)
for p in wavs:
    a = read(p)
    times, text = [], ""
    for r in range(reps + 1):
        t = time.perf_counter()
        txt = run(m, a)
        dt = (time.perf_counter() - t) * 1000
        if r == 0:
            text = txt
        else:
            times.append(round(dt, 1))
    print(json.dumps({"wav": p, "times_ms": times, "text": text, "backend": device}), flush=True)
print(json.dumps({"peak_ws_mb": round(psutil.Process().memory_info().peak_wset / 1048576, 1)}), flush=True)
