import React from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { capsuleEase } from "../lib/anim";

/**
 * Dictated text landing: words cascade in quickly (fade + 5px rise each),
 * the way VoiceWave inserts a finalized transcript — fast, not typed.
 */
export const TypeOn: React.FC<{
  text: string;
  start: number;
  /** Frames between word reveals. */
  step?: number;
  style?: React.CSSProperties;
  caret?: boolean;
  caretColor?: string;
}> = ({ text, start, step = 2, style, caret = false, caretColor = "#1B8EFF" }) => {
  const frame = useCurrentFrame();
  const words = text.split(" ");
  const opts = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: capsuleEase } as const;
  const doneAt = start + words.length * step + 6;

  return (
    <span style={style}>
      {words.map((word, i) => {
        const at = start + i * step;
        const p = interpolate(frame, [at, at + 5], [0, 1], opts);
        if (p <= 0) return null;
        return (
          <span
            key={i}
            style={{
              display: "inline-block",
              opacity: p,
              transform: `translateY(${(1 - p) * 5}px)`,
              whiteSpace: "pre-wrap"
            }}
          >
            {word + (i < words.length - 1 ? " " : "")}
          </span>
        );
      })}
      {caret && frame <= doneAt + 40 && (
        <span
          style={{
            display: "inline-block",
            width: 2.5,
            height: "1.1em",
            verticalAlign: "text-bottom",
            marginLeft: 2,
            background: caretColor,
            opacity: Math.floor(frame / 16) % 2 === 0 ? 1 : 0
          }}
        />
      )}
    </span>
  );
};

/** Blinking caret on its own (the lonely cursor of scene 1). */
export const Caret: React.FC<{ height?: number; color?: string }> = ({
  height = 28,
  color = "#09090B"
}) => {
  const frame = useCurrentFrame();
  return (
    <span
      style={{
        display: "inline-block",
        width: 2.5,
        height,
        background: color,
        opacity: Math.floor(frame / 16) % 2 === 0 ? 1 : 0,
        verticalAlign: "text-bottom"
      }}
    />
  );
};
