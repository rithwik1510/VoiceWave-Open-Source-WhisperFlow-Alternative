import { Easing, interpolate } from "remotion";

/** The app's signature ease (used by the pill capsule + onboarding). */
export const capsuleEase = Easing.bezier(0.22, 1, 0.36, 1);
/** Cinematic camera ease: slow in, long settle. */
export const cameraEase = Easing.bezier(0.3, 0, 0.12, 1);

export function easeOutCubic(x: number): number {
  return 1 - Math.pow(1 - Math.max(0, Math.min(1, x)), 3);
}

/** 0→1 over [start, start+dur] with the capsule ease. */
export function rise(frame: number, start: number, dur: number): number {
  return interpolate(frame, [start, start + dur], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: capsuleEase
  });
}

/** 1→0 over [start, start+dur]. */
export function fall(frame: number, start: number, dur: number): number {
  return 1 - rise(frame, start, dur);
}

/** Deterministic hash noise in [-1, 1]. */
export function noise(i: number): number {
  const x = Math.sin(i * 127.1 + 311.7) * 43758.5453;
  return (x - Math.floor(x)) * 2 - 1;
}

/**
 * Synthetic speech energy in [0, 1]: syllabic ~3 Hz modulation with phrase
 * swells and jitter. `t` in seconds; deterministic, so the pill waveform is
 * identical every render.
 */
export function speechEnergy(t: number, seedOffset = 0): number {
  const syllable = 0.55 + 0.45 * Math.sin(2 * Math.PI * 3.1 * t + seedOffset);
  const phrase = 0.62 + 0.38 * Math.sin(2 * Math.PI * 0.34 * t + 1.4 + seedOffset * 0.7);
  const jitter = 0.16 * noise(Math.floor(t * 22) + seedOffset * 97);
  const v = Math.max(0, syllable * phrase + jitter);
  return Math.min(1, Math.pow(v, 1.35));
}
