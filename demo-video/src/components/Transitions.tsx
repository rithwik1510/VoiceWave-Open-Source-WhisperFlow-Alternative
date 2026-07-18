import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { capsuleEase } from "../lib/anim";

/**
 * Invisible transition: the scene dissolves in over its first frames while
 * the previous scene (whose Sequence is extended by the same overlap) is
 * still holding underneath. Cameras are always drifting, so the dissolve
 * rides on motion continuity — the cut is felt, never seen.
 */
export const Dissolve: React.FC<{ dur?: number; children: React.ReactNode }> = ({
  dur = 14,
  children
}) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [0, dur], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: capsuleEase
  });
  return <AbsoluteFill style={{ opacity: p }}>{children}</AbsoluteFill>;
};
