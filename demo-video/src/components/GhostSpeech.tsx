import React from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { capsuleEase } from "../lib/anim";
import { fontBody } from "../theme";

/**
 * "What you're hearing" for muted viewers: faint quoted words appear near the
 * pill while it listens, timed like natural speech, then fade as a group.
 */
export const GhostSpeech: React.FC<{
  text: string;
  start: number;
  end: number;
  x: number;
  y: number;
  /** Frames per word — speech pace, ~3.5 words/sec at 30fps. */
  step?: number;
  size?: number;
  color?: string;
  maxWidth?: number;
}> = ({ text, start, end, x, y, step = 8, size = 26, color = "rgba(113,113,122,0.85)", maxWidth = 760 }) => {
  const frame = useCurrentFrame();
  if (frame < start || frame > end + 14) return null;
  const opts = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: capsuleEase } as const;
  const words = text.split(" ");
  const out = interpolate(frame, [end, end + 12], [0, 1], opts);
  // Closing quote fades in with the last word so it never floats detached
  // at the line end while earlier words are still revealing.
  const lastWordAt = start + (words.length - 1) * step;
  const closeP = interpolate(frame, [lastWordAt, lastWordAt + 6], [0, 1], opts);

  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        transform: "translateX(-50%)",
        width: maxWidth,
        textAlign: "center",
        fontFamily: fontBody,
        fontSize: size,
        fontWeight: 500,
        fontStyle: "italic",
        lineHeight: 1.45,
        color,
        opacity: 1 - out,
        pointerEvents: "none"
      }}
    >
      <span style={{ opacity: 0.6 }}>&ldquo;</span>
      {words.map((w, i) => {
        const at = start + i * step;
        const p = interpolate(frame, [at, at + 6], [0, 1], opts);
        return (
          <span key={i} style={{ opacity: p * 0.95, transition: "none" }}>
            {w + (i < words.length - 1 ? " " : "")}
          </span>
        );
      })}
      <span style={{ opacity: 0.6 * closeP }}>&rdquo;</span>
    </div>
  );
};
