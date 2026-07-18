import React from "react";
import { AbsoluteFill, Img, interpolate, staticFile, useCurrentFrame } from "remotion";
import { capsuleEase } from "../lib/anim";
import { blue, ctaGradient, cyan, fontBody, fontDisplay, glowBlue, grayLight, ink, lineSoft } from "../theme";

export const CLOSE_DUR = 90;

/** Scene 8 (57-60s): logo in the revolving ring, wordmark, gradient CTA. */
export const SceneClose: React.FC = () => {
  const frame = useCurrentFrame();
  const opts = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: capsuleEase } as const;
  const logoP = interpolate(frame, [0, 16], [0, 1], opts);
  const nameP = interpolate(frame, [10, 26], [0, 1], opts);
  const ctaP = interpolate(frame, [24, 40], [0, 1], opts);
  const ringAngle = (frame * 2.2) % 360;

  return (
    <AbsoluteFill
      style={{
        background:
          "radial-gradient(800px 560px at 50% 40%, rgba(27,142,255,0.1), transparent 60%), linear-gradient(180deg, #FBFBFC 0%, #F3F4F7 100%)",
        alignItems: "center",
        justifyContent: "center"
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 28,
          transform: `scale(${1 + 0.03 * (frame / 90)})`
        }}
      >
        <div
          style={{
            borderRadius: 44,
            padding: 3,
            opacity: logoP,
            transform: `scale(${0.85 + 0.15 * logoP})`,
            boxShadow: "0 18px 50px -18px rgba(27,142,255,0.4)",
            background: `conic-gradient(from ${ringAngle}deg, transparent 0deg, ${blue} 55deg, ${cyan} 105deg, transparent 170deg), ${lineSoft}`
          }}
        >
          <div
            style={{
              width: 148,
              height: 148,
              borderRadius: 42,
              background: "#fff",
              display: "grid",
              placeItems: "center"
            }}
          >
            <Img src={staticFile("icon.png")} style={{ width: 108, height: 108, borderRadius: 26 }} />
          </div>
        </div>
        <h1
          style={{
            margin: 0,
            fontFamily: fontDisplay,
            fontSize: 74,
            fontWeight: 600,
            letterSpacing: "-0.02em",
            color: ink,
            opacity: nameP,
            transform: `translateY(${(1 - nameP) * 20}px)`
          }}
        >
          VoiceWave
        </h1>
        <div
          style={{
            opacity: ctaP,
            transform: `translateY(${(1 - ctaP) * 20}px)`,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 16
          }}
        >
          <div
            style={{
              fontFamily: fontBody,
              fontSize: 24,
              fontWeight: 700,
              color: "#fff",
              background: ctaGradient,
              borderRadius: 999,
              padding: "16px 42px",
              boxShadow: `${glowBlue}, inset 0 1px 0 rgba(255,255,255,0.22)`
            }}
          >
            Download for Windows
          </div>
          <p style={{ margin: 0, fontFamily: fontBody, fontSize: 17, color: grayLight }}>
            Windows 10 &amp; 11 · free &amp; open source
          </p>
        </div>
      </div>
    </AbsoluteFill>
  );
};
