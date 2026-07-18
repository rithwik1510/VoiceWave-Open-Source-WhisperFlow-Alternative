import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Camera } from "../components/Camera";
import { Caption } from "../components/Caption";
import { StatsPanel } from "../components/StatsPanel";
import { capsuleEase } from "../lib/anim";

export const STATS_DUR = 150;

/** Scene 6 (40-50s): the payoff numbers count up inside the real Stats UI. */
export const SceneStats: React.FC = () => {
  const frame = useCurrentFrame();
  const enter = interpolate(frame, [0, 18], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: capsuleEase
  });

  return (
    <AbsoluteFill
      style={{
        background:
          "radial-gradient(1100px 700px at 50% 20%, rgba(27,142,255,0.08), transparent 60%), linear-gradient(180deg, #FBFBFC 0%, #F3F4F7 100%)"
      }}
    >
      <Camera
        keys={[
          { frame: 0, scale: 1.0, cx: 960, cy: 520 },
          { frame: 150, scale: 1.09, cx: 960, cy: 505 }
        ]}
      >
        <div
          style={{
            position: "absolute",
            left: "50%",
            top: 152,
            transform: `translateX(-50%) translateY(${(1 - enter) * 40}px)`,
            opacity: enter,
            filter: `blur(${(1 - enter) * 6}px)`
          }}
        >
          <StatsPanel start={14} countDur={68} />
        </div>
      </Camera>
      <Caption from={92} to={140} size={44} y={972}>
        Watch the hours come back.
      </Caption>
    </AbsoluteFill>
  );
};
