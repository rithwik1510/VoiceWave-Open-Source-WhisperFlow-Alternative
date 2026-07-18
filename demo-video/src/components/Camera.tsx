import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { cameraEase } from "../lib/anim";
import { VIDEO_H, VIDEO_W } from "../theme";

export type CameraKey = {
  frame: number;
  /** Zoom factor. 1 = full canvas. */
  scale: number;
  /** Canvas point that should sit at screen center. */
  cx: number;
  cy: number;
};

/**
 * Cinematic camera: interpolates focus point + zoom between keyframes with a
 * slow-settle ease, like a Screen Studio move. Children are laid out on the
 * full 1920x1080 canvas; the camera does the rest.
 */
export const Camera: React.FC<{ keys: CameraKey[]; children: React.ReactNode }> = ({
  keys,
  children
}) => {
  const frame = useCurrentFrame();
  const frames = keys.map((k) => k.frame);
  const opts = {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: cameraEase
  } as const;
  const scale = keys.length > 1 ? interpolate(frame, frames, keys.map((k) => k.scale), opts) : keys[0].scale;
  const cx = keys.length > 1 ? interpolate(frame, frames, keys.map((k) => k.cx), opts) : keys[0].cx;
  const cy = keys.length > 1 ? interpolate(frame, frames, keys.map((k) => k.cy), opts) : keys[0].cy;

  return (
    <AbsoluteFill style={{ overflow: "hidden" }}>
      <AbsoluteFill
        style={{
          transformOrigin: "0 0",
          transform: `translate(${VIDEO_W / 2}px, ${VIDEO_H / 2}px) scale(${scale}) translate(${-cx}px, ${-cy}px)`,
          willChange: "transform",
          backfaceVisibility: "hidden"
        }}
      >
        {children}
      </AbsoluteFill>
    </AbsoluteFill>
  );
};
