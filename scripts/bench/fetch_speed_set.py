"""Speed set: ~5 LibriSpeech test-clean/test-other clips per 2/5/10/30 s bucket, with references.

30 s clips concatenate consecutive utterances of one speaker (0.3 s gaps).
Downloads go to <dl_dir>; clips + manifest.json to <out_dir>. Both must be
outside the repo (audio is never committed). Needs numpy + PyAV (the
faster-whisper dev venv has both).

usage: python -I fetch_speed_set.py <dl_dir> <out_dir>
"""
import json, os, sys, urllib.request, wave

import av
import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
API = "https://datasets-server.huggingface.co/rows?dataset=openslr/librispeech_asr&config={c}&split=test&offset={o}&length=100"


def outside_repo(path):
    p = os.path.abspath(path)
    if os.path.commonpath([p, REPO]) == REPO:
        raise SystemExit(f"{p} is inside the repo; put audio in a temp folder")
    return p


def rows(cfg):
    res = []
    for off in (0, 100, 200, 300):
        with urllib.request.urlopen(API.format(c=cfg, o=off), timeout=60) as r:
            res += [x["row"] for x in json.load(r)["rows"]]
    return res


def decode(path):
    c = av.open(path)
    rs = av.AudioResampler(format="s16", layout="mono", rate=16000)
    chunks = [g.to_ndarray().reshape(-1) for f in c.decode(c.streams.audio[0]) for g in rs.resample(f)]
    chunks += [g.to_ndarray().reshape(-1) for g in rs.resample(None)]
    return np.concatenate(chunks).astype(np.int16)


def write(path, pcm):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(pcm.tobytes())


def flac(dl, cfg, r):
    fp = os.path.join(dl, f"{cfg}-{r['id']}.flac")
    if not os.path.exists(fp):
        urllib.request.urlretrieve(r["audio"][0]["src"], fp)
    return fp


def main():
    dl, out = outside_repo(sys.argv[1]), outside_repo(sys.argv[2])
    os.makedirs(dl, exist_ok=True)
    os.makedirs(out, exist_ok=True)
    # bucket: (min words, max words, min s, max s)
    want = {"2s": (4, 7, 1.4, 2.9), "5s": (11, 16, 4.0, 6.0), "10s": (24, 32, 8.5, 11.5)}
    manifest = []
    for cfg, n_per in (("clean", 3), ("other", 2)):
        rr = rows(cfg)
        got = {k: 0 for k in want}
        for r in rr:
            nw = len(r["text"].split())
            for b, (lo, hi, dlo, dhi) in want.items():
                if got[b] >= n_per or not (lo <= nw <= hi):
                    continue
                pcm = decode(flac(dl, cfg, r))
                d = len(pcm) / 16000
                if dlo <= d <= dhi:
                    name = f"{b}-{cfg}-{r['id']}"
                    write(os.path.join(out, name + ".wav"), pcm)
                    manifest.append({"clip": name, "bucket": b, "set": cfg, "dur": round(d, 2), "ref": r["text"]})
                    got[b] += 1
                break
        by_spk = {}
        for r in rr:
            by_spk.setdefault(r["speaker_id"], []).append(r)
        n30 = 0
        for lst in by_spk.values():
            if n30 >= n_per:
                break
            parts, refs, ids = [], [], []
            for r in lst:
                pcm = decode(flac(dl, cfg, r))
                total = sum(len(p) for p in parts) / 16000
                if total + len(pcm) / 16000 > 31.5:
                    continue
                parts += [pcm, np.zeros(4800, np.int16)]
                refs.append(r["text"])
                ids.append(r["id"])
                if sum(len(p) for p in parts) / 16000 >= 27.5:
                    break
            d = sum(len(p) for p in parts) / 16000
            if 27.5 <= d <= 31.5:
                name = f"30s-{cfg}-{ids[0]}"
                write(os.path.join(out, name + ".wav"), np.concatenate(parts))
                manifest.append({"clip": name, "bucket": "30s", "set": cfg, "dur": round(d, 2), "ref": " ".join(refs)})
                n30 += 1
        print(cfg, got, "30s:", n30)
    json.dump(manifest, open(os.path.join(out, "manifest.json"), "w"), indent=1)
    print(len(manifest), "clips")


if __name__ == "__main__":
    main()
