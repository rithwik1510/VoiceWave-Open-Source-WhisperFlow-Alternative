"""Benchmark one configuration of the production speech worker over its JSON protocol.

Spawns voicewave-asr-{vk,cpu}.exe, picks a device from list_devices, loads the
model, then per clip runs one discarded warm decode plus N timed repeats (the
plan 019 spike protocol). Prints JSON lines on stdout; worker logs pass
through to stderr.

usage: python -I worker_bench.py --worker EXE --model GGML_BIN --device cpu|discrete|integrated
         [--threads 8] [--flash-attn 0|1] [--repeats 3] [--silent-warmup]
         [--idle-seconds 60 --idle-rounds 2 --idle-wav WAV] [--vram] WAV...
"""
import argparse, base64, json, subprocess, threading, time, wave

import psutil


def read_pcm16(path):
    with wave.open(path) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1 and w.getsampwidth() == 2, path
        return w.readframes(w.getnframes())


def vram_used_mb():
    out = subprocess.run(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                         capture_output=True, text=True, check=True)
    return int(out.stdout.strip().splitlines()[0])


class Worker:
    def __init__(self, exe):
        self.p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                                  encoding="utf-8", bufsize=1)
        self.ready = json.loads(self.p.stdout.readline())
        assert self.ready.get("ready"), self.ready
        self.next_id = 0

    def call(self, command, **fields):
        self.next_id += 1
        req = {"id": self.next_id, "command": command, **fields}
        self.p.stdin.write(json.dumps(req) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError(f"worker exited (rc={self.p.wait()}) during {command}")
        res = json.loads(line)
        if res.get("id") != self.next_id:
            raise RuntimeError(f"id mismatch: sent {self.next_id}, got {res}")
        return res

    def close(self):
        try:
            self.call("shutdown")
        except Exception:
            pass
        self.p.wait(timeout=10)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--worker", required=True)
    ap.add_argument("--model", required=True)
    ap.add_argument("--device", choices=["cpu", "discrete", "integrated"], required=True)
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("--flash-attn", type=int, default=0)
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--silent-warmup", action="store_true",
                    help="use the worker's own silent warm-up (production path) instead of warming on clip 1")
    ap.add_argument("--idle-seconds", type=float, default=0)
    ap.add_argument("--idle-rounds", type=int, default=2)
    ap.add_argument("--idle-wav", action="append", default=[])
    ap.add_argument("--vram", action="store_true", help="sample nvidia-smi memory.used")
    ap.add_argument("wavs", nargs="+")
    a = ap.parse_args()

    def out(obj):
        print(json.dumps(obj), flush=True)

    vram_base = vram_used_mb() if a.vram else None
    vram_peak = [vram_base or 0]
    stop = threading.Event()
    if a.vram:
        def poll():
            while not stop.is_set():
                try:
                    vram_peak[0] = max(vram_peak[0], vram_used_mb())
                except Exception:
                    pass
                time.sleep(0.5)
        threading.Thread(target=poll, daemon=True).start()

    w = Worker(a.worker)
    try:
        devices = w.call("list_devices")
        load = {"modelPath": a.model, "threads": a.threads, "flashAttention": bool(a.flash_attn),
                "warmup": a.silent_warmup, "device": "cpu"}
        if a.device != "cpu":
            dev = next((d for d in devices["devices"] if d["type"] == a.device and d.get("ggmlIndex") is not None), None)
            if dev is None:
                raise SystemExit(f"no usable {a.device} GPU in {devices}")
            load["device"] = "gpu"
            if dev.get("luid"):
                load["luid"] = dev["luid"]
            else:
                load["ggmlIndex"] = dev["ggmlIndex"]
        out({"devices": devices["devices"]})
        res = w.call("load", **load)
        if not res.get("ok"):
            raise SystemExit(f"load failed: {res}")
        vram_resident = vram_used_mb() if a.vram else None
        pcm = {p: base64.b64encode(read_pcm16(p)).decode() for p in dict.fromkeys(a.wavs + a.idle_wav)}

        def decode(path):
            t = time.perf_counter()
            r = w.call("transcribe", audioPcm16B64=pcm[path], beamSize=5)
            wall = (time.perf_counter() - t) * 1000
            if not r.get("ok"):
                raise SystemExit(f"transcribe failed: {r}")
            return wall, r

        first_ms, _ = decode(a.wavs[0])  # first real decode after load
        out({"load_ms": res["modelInitMs"], "warmup_ms": res["warmupMs"] if a.silent_warmup else round(first_ms, 1),
             "first_decode_ms": round(first_ms, 1), "backend": res["backendUsed"], "device_name": res["deviceName"],
             "vram_base_mb": vram_base, "vram_resident_mb": vram_resident})
        for path in a.wavs:
            times, text, backend = [], "", None
            for r in range(a.repeats + 1):  # run 0: per-clip warm run, text only
                wall, resp = decode(path)
                if r == 0:
                    text, backend = resp["text"], resp["backendUsed"]
                else:
                    times.append(round(wall, 1))
            out({"wav": path, "times_ms": times, "text": text, "backend": backend})

        for rnd in range(a.idle_rounds if a.idle_seconds else 0):
            time.sleep(a.idle_seconds)
            vram_idle = vram_used_mb() if a.vram else None
            for path in a.idle_wav:
                idle_ms, _ = decode(path)
                warm_ms, _ = decode(path)
                out({"idle_round": rnd + 1, "idle_s": a.idle_seconds, "wav": path, "idle_ms": round(idle_ms, 1),
                     "warm_ms": round(warm_ms, 1), "vram_held_mb": vram_idle})

        out({"peak_ws_mb": round(psutil.Process(w.p.pid).memory_info().peak_wset / 1048576, 1)})
    finally:
        stop.set()
        w.close()
    if a.vram:
        out({"vram_base_mb": vram_base, "vram_peak_mb": vram_peak[0]})


if __name__ == "__main__":
    main()
