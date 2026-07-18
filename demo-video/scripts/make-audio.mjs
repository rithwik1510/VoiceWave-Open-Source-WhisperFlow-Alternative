// Synthesizes the demo video's music bed and SFX as WAV files in public/audio.
// Pure-math DSP (no deps) so the soundtrack is deterministic and licensing-clean.
// Run: node scripts/make-audio.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SR = 44100;
const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "audio");
mkdirSync(OUT, { recursive: true });

// ---------------------------------------------------------------- utilities

function writeWav(name, left, right) {
  const n = left.length;
  const bytesPerSample = 2;
  const channels = 2;
  const dataSize = n * channels * bytesPerSample;
  const buf = Buffer.alloc(44 + dataSize);
  buf.write("RIFF", 0);
  buf.writeUInt32LE(36 + dataSize, 4);
  buf.write("WAVE", 8);
  buf.write("fmt ", 12);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(channels, 22);
  buf.writeUInt32LE(SR, 24);
  buf.writeUInt32LE(SR * channels * bytesPerSample, 28);
  buf.writeUInt16LE(channels * bytesPerSample, 32);
  buf.writeUInt16LE(16, 34);
  buf.write("data", 36);
  buf.writeUInt32LE(dataSize, 40);
  for (let i = 0; i < n; i++) {
    buf.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(left[i] * 32767))), 44 + i * 4);
    buf.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(right[i] * 32767))), 46 + i * 4);
  }
  writeFileSync(join(OUT, name), buf);
  console.log(`wrote ${name} (${(n / SR).toFixed(2)}s)`);
}

// Deterministic noise.
let seed = 1337;
function rand() {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return seed / 4294967296 - 0.5;
}

// Piecewise-linear envelope over [time, value] points.
function env(t, points) {
  if (t <= points[0][0]) return points[0][1];
  for (let i = 1; i < points.length; i++) {
    if (t <= points[i][0]) {
      const [t0, v0] = points[i - 1];
      const [t1, v1] = points[i];
      return v0 + ((t - t0) / (t1 - t0)) * (v1 - v0);
    }
  }
  return points[points.length - 1][1];
}

function normalize(l, r, peakTarget) {
  let peak = 0;
  for (let i = 0; i < l.length; i++) peak = Math.max(peak, Math.abs(l[i]), Math.abs(r[i]));
  const g = peak > 0 ? peakTarget / peak : 1;
  for (let i = 0; i < l.length; i++) {
    l[i] = Math.tanh(l[i] * g * 1.15) / 1.15;
    r[i] = Math.tanh(r[i] * g * 1.15) / 1.15;
  }
}

// -------------------------------------------------------------------- music
// 96 BPM minimal ambient-pulse score arranged to the storyboard:
//   0-4    hook: lone airy pad swells in
//   4-10   magic moment: pulse + chords enter
//   10-19  montage: groove brightens, hats in
//   19-30  quality: gentle plucked arpeggio on top
//   30-34  THE DROP: everything ducks to a sub drone (wifi goes off)
//   34-40  heartbeat: soft halftime kick under near-silence
//   40-50  stats bloom: brightest section, full groove
//   50-57  offer: drums out, warm pads
//   57-61  close: resolved chord, long tail

// Storyboard boundaries (v3, crisp stats/offer): 4/10/19/30/40/45/48.5s,
// video ends at 51.5s.
const BPM = 96;
const BEAT = 60 / BPM;
const BAR = BEAT * 4;
const DUR = 53;
const N = Math.round(SR * DUR);

const A = {
  kick: (t) => env(t, [[0, 0], [3.9, 0], [4, 0.5], [10, 0.62], [29.8, 0.62], [30.1, 0], [33.9, 0], [34, 0.3], [39.9, 0.3], [40, 0.7], [44.8, 0.7], [45.2, 0], [52, 0]]),
  bass: (t) => env(t, [[0, 0], [4, 0.4], [10, 0.55], [30, 0.5], [31, 0.2], [40, 0.6], [45, 0.35], [48.5, 0.25], [50.5, 0]]),
  hat: (t) => env(t, [[0, 0], [9.9, 0], [10, 0.22], [29.8, 0.2], [30.1, 0], [39.9, 0], [40, 0.26], [44.8, 0.26], [45.2, 0.06], [47.5, 0.05], [48.5, 0]]),
  keys: (t) => env(t, [[0, 0.5], [4, 0.68], [10, 0.75], [19, 0.75], [29.8, 0.68], [30.2, 0], [33.4, 0], [33.5, 0.32], [39.9, 0.32], [40, 0.85], [45, 0.58], [48.5, 0.58], [50.5, 0]])
};

// Chord progression, one chord per 2 bars (5s): Am9, Fmaj7, Cmaj9, G6.
const CHORDS = [
  { notes: [110.0, 164.81, 261.63, 493.88], root: 55.0 },
  { notes: [87.31, 130.81, 220.0, 329.63], root: 43.65 },
  { notes: [130.81, 196.0, 329.63, 293.66], root: 65.41 },
  { notes: [98.0, 146.83, 246.94, 329.63], root: 49.0 }
];
const CHORD_LEN = BAR * 2;
function chordAt(t) {
  return CHORDS[Math.floor(t / CHORD_LEN) % CHORDS.length];
}

const mL = new Float64Array(N);
const mR = new Float64Array(N);

// Kick schedule (pattern depends on section) + duck envelope for sidechain.
const kickTimes = [];
for (let bar = 0; bar * BAR < DUR; bar++) {
  const t0 = bar * BAR;
  const beats = t0 >= 40 && t0 < 45 ? [0, 1, 2, 3] : t0 >= 33.5 && t0 < 40 ? [0] : [0, 2];
  for (const b of beats) {
    const t = t0 + b * BEAT;
    if (A.kick(t) > 0.04) kickTimes.push(t);
  }
}
const duck = new Float64Array(N);
for (const k of kickTimes) {
  const s0 = Math.round(k * SR);
  const s1 = Math.min(N, s0 + Math.round(0.28 * SR));
  for (let i = s0; i < s1; i++) {
    const dt = (i - s0) / SR;
    duck[i] = Math.max(duck[i], 0.32 * Math.exp(-dt * 9));
  }
}

// NO sustained pad layer — sustained sine chords read as a church organ.
// The piano (keys + left hand below) is the only harmonic instrument.

// Bass: soft sub hits on beats 1 & 3, following the chord root.
for (let bar = 0; bar * BAR < DUR; bar++) {
  for (const b of [0, 2]) {
    const t0 = bar * BAR + b * BEAT;
    if (t0 >= DUR) continue;
    const gBase = A.bass(t0);
    if (gBase < 0.03) continue;
    const root = chordAt(t0).root;
    const s0 = Math.round(t0 * SR);
    const s1 = Math.min(N, s0 + Math.round(0.7 * SR));
    let ph = 0;
    for (let i = s0; i < s1; i++) {
      const dt = (i - s0) / SR;
      const attack = Math.min(1, dt / 0.02);
      const g = gBase * attack * Math.exp(-dt * 3.2) * (1 - duck[i] * 0.7) * 0.3;
      ph += (2 * Math.PI * root) / SR;
      const v = Math.sin(ph) + 0.12 * Math.sin(2 * ph);
      mL[i] += v * g;
      mR[i] += v * g;
    }
  }
}

// Kicks: soft heartbeat thumps with an exponential pitch drop.
for (const k of kickTimes) {
  const g0 = A.kick(k);
  const s0 = Math.round(k * SR);
  const s1 = Math.min(N, s0 + Math.round(0.32 * SR));
  let ph = 0;
  for (let i = s0; i < s1; i++) {
    const dt = (i - s0) / SR;
    const f = 92 * Math.exp(-dt * 17) + 41;
    ph += (2 * Math.PI * f) / SR;
    const g = g0 * Math.exp(-dt * 11) * 0.5;
    const v = Math.sin(ph) * g;
    mL[i] += v;
    mR[i] += v;
  }
}

// Hats: off-beat filtered noise ticks.
for (let step = 0; step * BEAT * 0.5 < DUR; step++) {
  if (step % 2 === 0) continue; // off-beats only
  const t0 = step * BEAT * 0.5;
  const g0 = A.hat(t0);
  if (g0 < 0.02) continue;
  const s0 = Math.round(t0 * SR);
  const s1 = Math.min(N, s0 + Math.round(0.05 * SR));
  let lp = 0;
  for (let i = s0; i < s1; i++) {
    const dt = (i - s0) / SR;
    const n = rand() * 2;
    lp += 0.35 * (n - lp);
    const hp = n - lp; // crude highpass
    const v = hp * g0 * Math.exp(-dt * 90) * 0.5;
    mL[i] += v * 0.8;
    mR[i] += v * 1.1;
  }
}

// Felt keys: the lead voice. Piano-like additive notes — inharmonic
// partials with fast per-partial decay and a soft felt thump at onset —
// playing broken chords whose density follows the arrangement.
function keyNote(t0, f, vel, pan, dur = 3.0) {
  if (t0 >= DUR || vel <= 0.01) return;
  const s0 = Math.round(t0 * SR);
  const s1 = Math.min(N, s0 + Math.round(dur * SR));
  const phases = [0, 0, 0, 0, 0, 0];
  for (let i = s0; i < s1; i++) {
    const dt = (i - s0) / SR;
    let v = 0;
    for (let n = 1; n <= 6; n++) {
      const fn = f * n * (1 + 0.00038 * n * n);
      phases[n - 1] += (2 * Math.PI * fn) / SR;
      const amp = Math.pow(n, -1.9);
      // Piano-like ring: fundamental sustains, upper partials die fast.
      const decay = Math.exp(-dt * (1.1 + n * 1.3));
      v += Math.sin(phases[n - 1]) * amp * decay;
    }
    const attack = Math.min(1, dt / 0.004);
    const release = Math.min(1, (dur - dt) / 0.18);
    const felt = dt < 0.012 ? rand() * (1 - dt / 0.012) * 0.22 : 0;
    const g = vel * attack * release * (1 - duck[i] * 0.4) * 0.18;
    const out = (v + felt) * g;
    mL[i] += out * (1 - pan);
    mR[i] += out * pan;
  }
}

// Left hand: a soft low root (+third above) rolled at every chord change —
// the sustain that used to come from the pad, now in piano language.
for (let seg = 0; seg * CHORD_LEN < DUR; seg++) {
  const start = seg * CHORD_LEN;
  const chord = CHORDS[seg % CHORDS.length];
  const g = A.keys(start + 0.05);
  if (g < 0.05) continue; // stays silent through the offline drop
  keyNote(start + 0.02, chord.root * 2, g * 0.6, 0.5, 4.6);
  keyNote(start + 0.1, chord.notes[0], g * 0.42, 0.56, 4.6);
}

// Two lonely intro notes under the hook captions, before the pulse starts.
keyNote(0.55, 220.0, 0.5, 0.46);
keyNote(2.3, 164.81, 0.42, 0.56);

let keyIdx = 0;
for (let step = 0; step * BEAT * 0.5 < DUR; step++) {
  const t0 = step * BEAT * 0.5;
  const onBeat = step % 2 === 0;
  const beatInBar = Math.floor(step / 2) % 4;
  let play = false;
  if (t0 >= 4 && t0 < 10) play = onBeat;
  else if (t0 >= 10 && t0 < 30) play = onBeat || beatInBar === 2; // + one 8th pickup
  else if (t0 >= 33.5 && t0 < 40) play = onBeat && beatInBar === 0; // one per bar
  else if (t0 >= 40 && t0 < 45) play = true; // 8ths — the bloom
  else if (t0 >= 45 && t0 < 48.5) play = onBeat;
  if (!play) continue;
  const g0 = A.keys(t0);
  if (g0 < 0.02) continue;
  const chord = chordAt(t0);
  const tone = chord.notes[[0, 2, 1, 3][keyIdx % 4]];
  keyIdx++;
  const f = tone < 180 ? tone * 2 : tone;
  const vel = g0 * (0.85 + 0.3 * (rand() + 0.5) * 0.5) * (onBeat ? 1 : 0.72);
  keyNote(t0, f, vel, keyIdx % 2 === 0 ? 0.42 : 0.58);
}

// Final resolve: a soft rolled C-major-9 under the logo, then silence.
{
  const roll = CHORDS[2].notes;
  roll.forEach((f, i) => {
    keyNote(48.55 + i * 0.07, f < 180 ? f * 2 : f, 0.55 - i * 0.06, 0.42 + i * 0.05);
  });
}

// Room tone: barely-there lowpassed air so the drop never goes digital-dead.
{
  let lpL = 0;
  let lpR = 0;
  const a = 1 - Math.exp((-2 * Math.PI * 700) / SR);
  for (let i = 0; i < N; i++) {
    lpL += a * (rand() * 2 - lpL);
    lpR += a * (rand() * 2 - lpR);
    const fade = env(i / SR, [[0, 0.6], [4, 1], [50.5, 1], [52.7, 0]]);
    mL[i] += lpL * 0.012 * fade;
    mR[i] += lpR * 0.012 * fade;
  }
}

normalize(mL, mR, 0.82);
writeWav("music.wav", mL, mR);

// --------------------------------------------------------------------- sfx

function sfx(dur, fn) {
  const n = Math.round(dur * SR);
  const l = new Float64Array(n);
  const r = new Float64Array(n);
  fn(l, r, n);
  return [l, r];
}

// Whoosh: band-swept noise for camera moves and scene cuts.
{
  const [l, r] = sfx(0.7, (L, R, n) => {
    let lp1 = 0;
    let lp2 = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const p = t / 0.7;
      const fc = 250 + 2500 * p * p;
      const a = 1 - Math.exp((-2 * Math.PI * fc) / SR);
      const x = rand() * 2;
      lp1 += a * (x - lp1);
      lp2 += a * 0.5 * (lp1 - lp2);
      const band = lp1 - lp2;
      const g = Math.pow(Math.sin(Math.PI * Math.min(1, p * 1.08)), 1.6) * 0.8;
      L[i] = band * g * (1 - 0.3 * p);
      R[i] = band * g * (0.7 + 0.3 * p);
    }
  });
  normalize(l, r, 0.6);
  writeWav("whoosh.wav", l, r);
}

// Thock: the transition tap. A deep felted thump with a crisp micro-click —
// the "quality keyboard" sound modern product trailers cut on.
{
  const [l, r] = sfx(0.16, (L, R, n) => {
    let ph = 0;
    let lp = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const f = 118 * Math.exp(-t * 32) + 54;
      ph += (2 * Math.PI * f) / SR;
      const thump = Math.sin(ph) * Math.exp(-t * 24);
      const click = t < 0.008 ? Math.sin(2 * Math.PI * 2900 * t) * Math.exp(-t * 650) * 0.5 : 0;
      const a = 1 - Math.exp((-2 * Math.PI * 1400) / SR);
      lp += a * (rand() * 2 - lp);
      const body = t < 0.03 ? lp * Math.exp(-t * 160) * 0.5 : 0;
      const v = thump + click + body;
      L[i] = v;
      R[i] = v * 0.96;
    }
  });
  normalize(l, r, 0.6);
  writeWav("thock.wav", l, r);
}

// Swish: a very short, soft air movement layered under the thock on cuts.
{
  const [l, r] = sfx(0.2, (L, R, n) => {
    let lp1 = 0;
    let lp2 = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const p = t / 0.2;
      const fc = 700 + 1400 * p;
      const a = 1 - Math.exp((-2 * Math.PI * fc) / SR);
      const x = rand() * 2;
      lp1 += a * (x - lp1);
      lp2 += a * 0.55 * (lp1 - lp2);
      const band = lp1 - lp2;
      const g = Math.pow(Math.sin(Math.PI * p), 2.2);
      L[i] = band * g * (1 - 0.35 * p);
      R[i] = band * g * (0.65 + 0.35 * p);
    }
  });
  normalize(l, r, 0.4);
  writeWav("swish.wav", l, r);
}

// Pop: pill materializes.
{
  const [l, r] = sfx(0.16, (L, R, n) => {
    let ph = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const f = 340 + 320 * Math.min(1, t / 0.05);
      ph += (2 * Math.PI * f) / SR;
      const click = t < 0.004 ? rand() * 1.4 * (1 - t / 0.004) : 0;
      const v = Math.sin(ph) * Math.exp(-t * 34) * 0.9 + click;
      L[i] = v;
      R[i] = v;
    }
  });
  normalize(l, r, 0.5);
  writeWav("pop.wav", l, r);
}

// Click: the wifi toggle.
{
  const [l, r] = sfx(0.06, (L, R, n) => {
    let ph = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      ph += (2 * Math.PI * 1500) / SR;
      const v = Math.sin(ph) * Math.exp(-t * 160) + rand() * Math.exp(-t * 300) * 0.8;
      L[i] = v;
      R[i] = v;
    }
  });
  normalize(l, r, 0.35);
  writeWav("click.wav", l, r);
}

// Thud: the low drop when the world goes offline.
{
  const [l, r] = sfx(0.5, (L, R, n) => {
    let ph = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const f = 72 * Math.exp(-t * 9) + 36;
      ph += (2 * Math.PI * f) / SR;
      const v = Math.sin(ph) * Math.exp(-t * 7);
      L[i] = v;
      R[i] = v;
    }
  });
  normalize(l, r, 0.55);
  writeWav("thud.wav", l, r);
}

// Riser: soft swell into the stats bloom.
{
  const [l, r] = sfx(1.6, (L, R, n) => {
    let lp = 0;
    let ph = 0;
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const p = t / 1.6;
      const a = 1 - Math.exp((-2 * Math.PI * (300 + 3200 * p * p)) / SR);
      lp += a * (rand() * 2 - lp);
      ph += (2 * Math.PI * (180 + 240 * p)) / SR;
      const g = Math.pow(p, 2.2);
      const v = lp * 0.7 * g + Math.sin(ph) * 0.12 * g;
      L[i] = v * (1 - 0.2 * p);
      R[i] = v * (0.8 + 0.2 * p);
    }
  });
  normalize(l, r, 0.45);
  writeWav("riser.wav", l, r);
}
