import React from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { capsuleEase } from "../lib/anim";
import { fontBody, ink } from "../theme";

/**
 * The editorial caption system: one line at a time, lower third, rises in
 * with a blur (the onboarding step-entrance language) and exits upward.
 */
export const Caption: React.FC<{
  from: number;
  to: number;
  children: React.ReactNode;
  size?: number;
  color?: string;
  y?: number;
  weight?: number;
}> = ({ from, to, children, size = 44, color = ink, y = 930, weight = 600 }) => {
  const frame = useCurrentFrame();
  if (frame < from || frame > to + 12) return null;
  const opts = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: capsuleEase } as const;
  const inP = interpolate(frame, [from, from + 14], [0, 1], opts);
  const outP = interpolate(frame, [to, to + 12], [0, 1], opts);
  const opacity = inP * (1 - outP);
  const rise = (1 - inP) * 26 - outP * 20;
  const blur = (1 - inP) * 8 + outP * 6;

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        top: y,
        display: "flex",
        justifyContent: "center",
        pointerEvents: "none"
      }}
    >
      <div
        style={{
          fontFamily: fontBody,
          fontSize: size,
          fontWeight: weight,
          letterSpacing: "-0.01em",
          color,
          opacity,
          transform: `translateY(${rise}px)`,
          filter: `blur(${blur.toFixed(2)}px)`,
          textAlign: "center",
          whiteSpace: "pre-wrap"
        }}
      >
        {children}
      </div>
    </div>
  );
};
