import React from "react";
import { interpolate, useCurrentFrame, useVideoConfig } from "remotion";
import { capsuleEase, speechEnergy } from "../lib/anim";
import { blue, cyan, cyanSoft } from "../theme";

export type PillState = "hidden" | "idle" | "listening" | "transcribing" | "inserted";

export type PillTimeline = { frame: number; state: PillState }[];

// Geometry per state, straight from the app's pill.css (before display scale).
const GEOM: Record<PillState, { w: number; h: number; lift: number; wave: number; spin: number; waveW: number }> = {
  hidden: { w: 44, h: 14, lift: 14, wave: 0, spin: 0, waveW: 0 },
  idle: { w: 44, h: 14, lift: 0, wave: 0, spin: 0, waveW: 0 },
  listening: { w: 76, h: 26, lift: -12, wave: 1, spin: 0, waveW: 48 },
  transcribing: { w: 64, h: 22, lift: -12, wave: 0, spin: 1, waveW: 0 },
  inserted: { w: 54, h: 18, lift: -8, wave: 0.45, spin: 0, waveW: 22 }
};

const MORPH_FRAMES = 9;
const BARS = 12;

function tracks(timeline: PillTimeline, pick: (g: (typeof GEOM)["idle"]) => number) {
  const frames: number[] = [];
  const values: number[] = [];
  timeline.forEach((entry, i) => {
    const g = GEOM[entry.state];
    if (i === 0) {
      frames.push(entry.frame);
      values.push(pick(g));
      return;
    }
    frames.push(entry.frame, entry.frame + MORPH_FRAMES);
    values.push(values[values.length - 1], pick(g));
  });
  return { frames, values };
}

/**
 * Frame-driven replica of the app's floating pill (FloatingPill + pill.css):
 * same geometry, gradient bars, capsule spring. `display` scales the whole
 * pill up for the video canvas (the real pill is tiny).
 */
export const Pill: React.FC<{
  timeline: PillTimeline;
  /** Canvas position of the pill's bottom-center. */
  x: number;
  y: number;
  display?: number;
  /** Seed so different scenes get different speech waves. */
  seed?: number;
  /** Fade-in at first timeline frame. */
  appear?: boolean;
}> = ({ timeline, x, y, display = 3, seed = 0, appear = false }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const opts = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: capsuleEase } as const;

  const get = (pick: (g: (typeof GEOM)["idle"]) => number) => {
    const t = tracks(timeline, pick);
    return t.frames.length > 1 ? interpolate(frame, t.frames, t.values, opts) : t.values[0];
  };

  const w = get((g) => g.w) * display;
  const h = get((g) => g.h) * display;
  const lift = get((g) => g.lift) * display;
  const waveOpacity = get((g) => g.wave);
  const waveW = get((g) => g.waveW) * display;
  const spin = get((g) => g.spin);

  const appearAt = timeline[0].frame;
  const appearP = appear
    ? interpolate(frame, [appearAt, appearAt + 10], [0, 1], opts)
    : 1;
  // Signal-Blue ripple: an expanding ring pulse announces the pill.
  const rippleP = appear
    ? interpolate(frame, [appearAt, appearAt + 18], [0, 1], {
        extrapolateLeft: "clamp",
        extrapolateRight: "clamp"
      })
    : 1;
  if (frame < appearAt && appear) return null;

  // Scrolling waveform: each bar shows the speech signal slightly earlier in
  // time, newest on the right — mirrors the app's 50 ms history buffer.
  const t = frame / fps;
  const listening = waveOpacity > 0.5;
  const bars = new Array(BARS).fill(0).map((_, i) => {
    const delay = (BARS - 1 - i) * 0.055;
    const e = listening ? speechEnergy(t - delay, seed) : 0.06;
    return Math.min(1, 0.1 + e * 0.92);
  });

  const spinnerSize = 12 * display * spin;

  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        transform: `translate(-50%, -100%) translateY(${lift}px) scale(${0.9 + 0.1 * appearP})`,
        opacity: appearP
      }}
    >
      {appear && rippleP < 1 && (
        <div
          style={{
            position: "absolute",
            left: "50%",
            top: "50%",
            width: 30 * display,
            height: 30 * display,
            borderRadius: 999,
            border: `${1.5 * display}px solid rgba(27,142,255,${0.75 * (1 - rippleP)})`,
            boxShadow: `0 0 ${18 * display * rippleP}px rgba(27,142,255,${0.35 * (1 - rippleP)})`,
            transform: `translate(-50%, -50%) scale(${0.3 + rippleP * 2.4})`,
            pointerEvents: "none"
          }}
        />
      )}
      <div
        style={{
          width: w,
          height: h,
          borderRadius: 999,
          position: "relative",
          overflow: "hidden",
          background: "linear-gradient(135deg, rgba(24,24,27,0.94), rgba(39,39,42,0.96))",
          boxShadow:
            "0 14px 30px rgba(0,0,0,0.4), inset 0 0 0 1px rgba(228,228,231,0.24), inset 0 -9px 18px rgba(9,9,11,0.3)"
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center"
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 2 * display,
              width: waveW,
              opacity: waveOpacity,
              overflow: "hidden"
            }}
          >
            {bars.map((s, i) => (
              <span
                key={i}
                style={{
                  width: 2 * display,
                  height: 16 * display,
                  borderRadius: 999,
                  flexShrink: 0,
                  transform: `scaleY(${s.toFixed(3)})`,
                  background: `linear-gradient(180deg, ${blue}, ${cyan}, ${cyanSoft})`
                }}
              />
            ))}
          </div>
          {spin > 0.05 && (
            <div
              style={{
                position: "absolute",
                width: spinnerSize,
                height: spinnerSize,
                borderRadius: 999,
                border: `${1.2 * display}px solid rgba(27,142,255,0.3)`,
                borderTopColor: "rgba(27,142,255,0.98)",
                borderRightColor: "rgba(126,216,255,0.96)",
                opacity: spin,
                transform: `rotate(${(frame * 15) % 360}deg)`
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
};
