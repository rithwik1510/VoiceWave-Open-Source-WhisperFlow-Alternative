"""Quality set: N LibriSpeech test-clean + N test-other utterances as 16 kHz WAVs with references.

Strides through each split for speaker variety. Downloads go to <dl_dir>;
clips + manifest.json to <out_dir>, both outside the repo. Plan 019 needs
>= 100 clips in total (e.g. n_per_set=100 -> 200 clips).

usage: python -I fetch_quality_set.py <dl_dir> <out_dir> <n_per_set>
"""
import json, os, sys, urllib.request, wave

import av
import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
API = "https://datasets-server.huggingface.co/rows?dataset=openslr/librispeech_asr&config={c}&split=test&offset={o}&length={n}"


def outside_repo(path):
    p = os.path.abspath(path)
    if os.path.commonpath([p, REPO]) == REPO:
        raise SystemExit(f"{p} is inside the repo; put audio in a temp folder")
    return p


def main():
    dl, out, n = outside_repo(sys.argv[1]), outside_repo(sys.argv[2]), int(sys.argv[3])
    os.makedirs(dl, exist_ok=True)
    os.makedirs(out, exist_ok=True)
    per_offset = max(1, n // 5)
    man = []
    for cfg in ("clean", "other"):
        rows = []
        for off in (0, 500, 1000, 1500, 2000):
            with urllib.request.urlopen(API.format(c=cfg, o=off, n=per_offset), timeout=60) as r:
                rows += [x["row"] for x in json.load(r)["rows"]]
        for r in rows:
            fp = os.path.join(dl, f"{cfg}-{r['id']}.flac")
            if not os.path.exists(fp):
                urllib.request.urlretrieve(r["audio"][0]["src"], fp)
            c = av.open(fp)
            rs = av.AudioResampler(format="s16", layout="mono", rate=16000)
            chunks = [g.to_ndarray().reshape(-1) for f in c.decode(c.streams.audio[0]) for g in rs.resample(f)]
            chunks += [g.to_ndarray().reshape(-1) for g in rs.resample(None)]
            pcm = np.concatenate(chunks).astype(np.int16)
            name = f"q-{cfg}-{r['id']}"
            with wave.open(os.path.join(out, name + ".wav"), "wb") as w:
                w.setnchannels(1)
                w.setsampwidth(2)
                w.setframerate(16000)
                w.writeframes(pcm.tobytes())
            man.append({"clip": name, "bucket": "q", "set": cfg, "dur": round(len(pcm) / 16000, 2), "ref": r["text"]})
    json.dump(man, open(os.path.join(out, "manifest.json"), "w"), indent=1)
    speakers = len({m["clip"].split("-")[2] for m in man})
    print(len(man), "clips", round(sum(m["dur"] for m in man) / 60, 1), "min", speakers, "speakers")


if __name__ == "__main__":
    main()
