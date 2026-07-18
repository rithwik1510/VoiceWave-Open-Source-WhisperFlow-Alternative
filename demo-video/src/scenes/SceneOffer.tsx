import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { capsuleEase } from "../lib/anim";
import { fontBody, fontDisplay, gray, grayLight, ink, line } from "../theme";

export const OFFER_DUR = 105;

const Rise: React.FC<{ at: number; children: React.ReactNode }> = ({ at, children }) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [at, at + 16], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: capsuleEase
  });
  return (
    <div
      style={{
        opacity: p,
        transform: `translateY(${(1 - p) * 28}px)`,
        filter: `blur(${(1 - p) * 7}px)`
      }}
    >
      {children}
    </div>
  );
};

/** Scene 7 (50-57s): the offer, understated. A slow push-in and a breathing
 * brand glow keep the frame alive through the hold. */
export const SceneOffer: React.FC = () => {
  const frame = useCurrentFrame();
  const drift = 1 + 0.035 * (frame / 105);
  const breathe = 0.75 + 0.25 * Math.sin((frame / 96) * Math.PI * 2);
  return (
    <AbsoluteFill
      style={{
        background: "linear-gradient(180deg, #FBFBFC 0%, #F4F4F6 100%)",
        alignItems: "center",
        justifyContent: "center"
      }}
    >
      <AbsoluteFill
        style={{
          background: "radial-gradient(900px 600px at 50% 42%, rgba(27,142,255,0.11), transparent 62%)",
          opacity: breathe
        }}
      />
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 26,
          textAlign: "center",
          transform: `scale(${drift})`
        }}
      >
        <Rise at={4}>
          <h1
            style={{
              margin: 0,
              fontFamily: fontDisplay,
              fontSize: 92,
              fontWeight: 600,
              letterSpacing: "-0.02em",
              color: ink
            }}
          >
            Free. Open source. Yours.
          </h1>
        </Rise>
        <Rise at={18}>
          <p style={{ margin: 0, fontFamily: fontBody, fontSize: 28, fontWeight: 500, color: gray }}>
            Powered by Whisper, running locally.
          </p>
        </Rise>
        <Rise at={32}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              fontFamily: fontBody,
              fontSize: 20,
              fontWeight: 600,
              color: ink,
              border: `1px solid ${line}`,
              background: "#fff",
              borderRadius: 999,
              padding: "12px 26px",
              boxShadow: "0 1px 2px rgba(9,9,11,0.05)"
            }}
          >
            <svg width="24" height="24" viewBox="0 0 16 16" fill={ink}>
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
            </svg>
            rithwik1510/VoiceWave
          </div>
        </Rise>
        <Rise at={54}>
          <p style={{ margin: "14px 0 0", fontFamily: fontBody, fontSize: 19, fontStyle: "italic", color: grayLight }}>
            This script was dictated with VoiceWave.
          </p>
        </Rise>
      </div>
    </AbsoluteFill>
  );
};
