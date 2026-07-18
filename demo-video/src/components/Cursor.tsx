import React from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { cameraEase } from "../lib/anim";

export type CursorKey = { frame: number; x: number; y: number };

/** Windows arrow cursor that glides between keyframes; optional click pulse. */
export const Cursor: React.FC<{
  keys: CursorKey[];
  clickAt?: number;
  scale?: number;
  from?: number;
  to?: number;
}> = ({ keys, clickAt, scale = 1.15, from = 0, to = Infinity }) => {
  const frame = useCurrentFrame();
  if (frame < from || frame > to) return null;
  const opts = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: cameraEase } as const;
  const frames = keys.map((k) => k.frame);
  const x = interpolate(frame, frames, keys.map((k) => k.x), opts);
  const y = interpolate(frame, frames, keys.map((k) => k.y), opts);
  const clickP =
    clickAt !== undefined
      ? interpolate(frame, [clickAt, clickAt + 4, clickAt + 10], [0, 1, 0], {
          extrapolateLeft: "clamp",
          extrapolateRight: "clamp"
        })
      : 0;

  return (
    <div style={{ position: "absolute", left: x, top: y, pointerEvents: "none", zIndex: 60 }}>
      {clickAt !== undefined && (
        <div
          style={{
            position: "absolute",
            left: -14,
            top: -14,
            width: 28,
            height: 28,
            borderRadius: 999,
            border: "2px solid rgba(27,142,255,0.8)",
            transform: `scale(${0.4 + clickP * 1.3})`,
            opacity: clickP * 0.9
          }}
        />
      )}
      <svg
        width={17 * scale}
        height={24 * scale}
        viewBox="0 0 17 24"
        style={{ transform: clickP > 0.4 ? "scale(0.92)" : "scale(1)", filter: "drop-shadow(0 2px 4px rgba(9,9,11,0.35))" }}
      >
        <path
          d="M1 1 L1 19 L5.5 15 L8.5 22 L11.5 20.6 L8.6 13.8 L15 13.5 Z"
          fill="#FFFFFF"
          stroke="#09090B"
          strokeWidth="1.3"
          strokeLinejoin="round"
        />
      </svg>
    </div>
  );
};
