import React from "react";
import { AbsoluteFill } from "remotion";
import { VIDEO_H, VIDEO_W } from "../theme";

/** Composition renders at 2x canvas size. */
export const SS = 2;

/**
 * Anti-shimmer supersampling: scenes lay out on the logical 1920x1080 canvas,
 * but the composition rasterizes at 2x. Text is hinted at 4K, so the
 * per-frame subpixel differences the moving camera causes become invisible.
 */
export const SuperSample: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <AbsoluteFill style={{ background: "#FAFAFA" }}>
    <div
      style={{
        width: VIDEO_W,
        height: VIDEO_H,
        transform: `scale(${SS})`,
        transformOrigin: "0 0"
      }}
    >
      {children}
    </div>
  </AbsoluteFill>
);
