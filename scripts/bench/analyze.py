"""Summarize run_matrix.py results: latency medians per bucket, load, RAM/VRAM, idle, WER.

usage: python -I analyze.py <results_dir> <manifest.json> [<manifest.json> ...] [--filter REGEX]
"""
import argparse, glob, json, os, re, statistics as st

ap = argparse.ArgumentParser()
ap.add_argument("results")
ap.add_argument("manifests", nargs="+")
ap.add_argument("--filter", default=".")
a = ap.parse_args()

ref = {m["clip"]: m for path in a.manifests for m in json.load(open(path))}
ABBR = {"mr": "mister", "mrs": "missus", "dr": "doctor", "st": "saint"}


def norm(s):
    s = s.lower().replace("-", " ").replace("—", " ")
    s = re.sub(r"[^a-z0-9' ]", " ", s)
    s = re.sub(r"(?<![a-z])'|'(?![a-z])", " ", s)
    return [ABBR.get(w, w) for w in s.split()]


def edits(r, h):
    d = list(range(len(h) + 1))
    for i in range(1, len(r) + 1):
        prev, d[0] = d[0], i
        for j in range(1, len(h) + 1):
            cur = min(d[j] + 1, d[j - 1] + 1, prev + (r[i - 1] != h[j - 1]))
            prev, d[j] = d[j], cur
    return d[len(h)]


B = ["2s", "5s", "10s", "30s"]
print(f"{'config':24} {'backend':>7} {'load':>6} {'warm':>6} " + " ".join(f"{b:>6}" for b in B)
      + f" {'RAMpk':>6} {'VRAMres':>7} {'VRAMpk':>6} {'WER':>6} {'clean':>6} {'other':>6} n")
idle_rows = []
for p in sorted(glob.glob(os.path.join(a.results, "*.jsonl"))):
    name = os.path.basename(p)[:-6]
    if not re.search(a.filter, name):
        continue
    lines = [json.loads(l) for l in open(p, encoding="utf-8") if l.strip()]
    head = next((l for l in lines if "load_ms" in l), {})
    rc = lines[-1].get("rc")
    peak = next((l["peak_ws_mb"] for l in lines if "peak_ws_mb" in l), None)
    vram = next((l for l in lines if "vram_peak_mb" in l), {})
    vram_res = (head["vram_resident_mb"] - head["vram_base_mb"]) if head.get("vram_resident_mb") is not None else None
    vram_pk = (vram["vram_peak_mb"] - vram["vram_base_mb"]) if vram.get("vram_base_mb") is not None else None
    by_b, err, nw, per_set = {}, 0, 0, {}
    clips = [l for l in lines if "times_ms" in l]
    for c in clips:
        m = ref.get(os.path.basename(c["wav"])[:-4])
        if m is None:
            continue
        by_b.setdefault(m["bucket"], []).extend(c["times_ms"])
        r, h = norm(m["ref"]), norm(c["text"])
        e = edits(r, h)
        err += e
        nw += len(r)
        s = per_set.setdefault(m["set"], [0, 0])
        s[0] += e
        s[1] += len(r)
    for l in lines:
        if "idle_ms" in l:
            idle_rows.append((name, l, head.get("vram_base_mb")))
    wer = lambda k: 100 * per_set[k][0] / per_set[k][1] if k in per_set else 0
    med = {b: round(st.median(v)) for b, v in by_b.items() if v}
    print(f"{name:24} {str(head.get('backend', '-')):>7} {head.get('load_ms') or 0:6.0f} {head.get('warmup_ms') or 0:6.0f} "
          + " ".join(f"{med.get(b, 0):6d}" for b in B)
          + f" {peak or 0:6.0f} {str(vram_res if vram_res is not None else '-'):>7} {str(vram_pk if vram_pk is not None else '-'):>6}"
          + f" {100 * err / max(nw, 1):6.2f} {wer('clean'):6.2f} {wer('other'):6.2f} {len(clips)}/{nw}w rc={rc}")

if idle_rows:
    print("\nafter-idle decodes")
    for name, l, base in idle_rows:
        held = l["vram_held_mb"] - base if l.get("vram_held_mb") is not None and base is not None else "-"
        print(f"{name:24} round {l['idle_round']} idle {l['idle_s']:.0f}s {os.path.basename(l['wav']):34} "
              f"after-idle {l['idle_ms']:7.1f} ms  warm {l['warm_ms']:7.1f} ms  VRAM held {held} MB over base")
