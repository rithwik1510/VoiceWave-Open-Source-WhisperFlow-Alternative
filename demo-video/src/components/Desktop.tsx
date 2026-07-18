import React from "react";
import { AbsoluteFill } from "remotion";
import { fontBody } from "../theme";

/**
 * Quiet desktop backdrop: paper gradient with a soft brand-blue radial, plus
 * a Windows-11-style centered taskbar with a live system tray (the Wi-Fi
 * icon there is scene 5's protagonist).
 */
export const Desktop: React.FC<{
  wifiOn?: boolean;
  showTaskbar?: boolean;
  children?: React.ReactNode;
}> = ({ wifiOn = true, showTaskbar = true, children }) => {
  return (
    <AbsoluteFill
      style={{
        background:
          "radial-gradient(1200px 800px at 70% 15%, rgba(27,142,255,0.10), transparent 60%), radial-gradient(900px 700px at 15% 90%, rgba(15,95,215,0.07), transparent 55%), linear-gradient(180deg, #FBFBFC 0%, #F2F3F6 100%)"
      }}
    >
      {children}
      {showTaskbar && (
        <div
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 0,
            height: 56,
            background: "rgba(250,250,251,0.88)",
            borderTop: "1px solid rgba(9,9,11,0.06)",
            backdropFilter: "blur(20px)",
            display: "flex",
            alignItems: "center",
            fontFamily: fontBody
          }}
        >
          {/* Centered launcher icons */}
          <div
            style={{
              position: "absolute",
              left: "50%",
              transform: "translateX(-50%)",
              display: "flex",
              gap: 14,
              alignItems: "center"
            }}
          >
            {/* Start */}
            <svg width="22" height="22" viewBox="0 0 22 22">
              {[0, 1].map((r) =>
                [0, 1].map((c) => (
                  <rect key={`${r}${c}`} x={2 + c * 10} y={2 + r * 10} width="8" height="8" rx="1.5" fill="#3D74E0" />
                ))
              )}
            </svg>
            {[["#4A79D9", "#7EB1FF"], ["#3FA96E", "#7FD8A8"], ["#D9A03F", "#F2CE8B"], ["#8B69D6", "#BBA3EE"]].map(
              ([a, b], i) => (
                <div
                  key={i}
                  style={{
                    width: 24,
                    height: 24,
                    borderRadius: 7,
                    background: `linear-gradient(135deg, ${a}, ${b})`,
                    opacity: 0.9
                  }}
                />
              )
            )}
          </div>
          {/* System tray */}
          <div
            style={{
              position: "absolute",
              right: 18,
              display: "flex",
              alignItems: "center",
              gap: 16,
              color: "#3F3F46"
            }}
          >
            {/* Wi-Fi */}
            <div id="tray-wifi" style={{ position: "relative", width: 20, height: 18 }}>
              <svg width="20" height="18" viewBox="0 0 20 18">
                <path
                  d="M1 6.5 C5.5 2.2 14.5 2.2 19 6.5"
                  fill="none"
                  stroke="#3F3F46"
                  strokeWidth="1.7"
                  strokeLinecap="round"
                  opacity={wifiOn ? 1 : 0.25}
                />
                <path
                  d="M4.2 9.8 C7.5 6.8 12.5 6.8 15.8 9.8"
                  fill="none"
                  stroke="#3F3F46"
                  strokeWidth="1.7"
                  strokeLinecap="round"
                  opacity={wifiOn ? 1 : 0.25}
                />
                <path
                  d="M7.4 13 C9 11.6 11 11.6 12.6 13"
                  fill="none"
                  stroke="#3F3F46"
                  strokeWidth="1.7"
                  strokeLinecap="round"
                  opacity={wifiOn ? 1 : 0.25}
                />
                <circle cx="10" cy="15.6" r="1.5" fill="#3F3F46" opacity={wifiOn ? 1 : 0.35} />
                {!wifiOn && (
                  <line x1="2" y1="16.5" x2="18" y2="1.5" stroke="#3F3F46" strokeWidth="1.8" strokeLinecap="round" />
                )}
              </svg>
            </div>
            {/* Volume */}
            <svg width="18" height="16" viewBox="0 0 18 16">
              <path d="M2 6 h3 l4 -3.5 v11 L5 10 H2 z" fill="#3F3F46" />
              <path d="M12 5 a4.5 4.5 0 0 1 0 6" fill="none" stroke="#3F3F46" strokeWidth="1.6" strokeLinecap="round" />
            </svg>
            {/* Battery */}
            <svg width="24" height="13" viewBox="0 0 24 13">
              <rect x="0.8" y="0.8" width="20" height="11.4" rx="3" fill="none" stroke="#3F3F46" strokeWidth="1.4" />
              <rect x="3" y="3" width="14" height="7" rx="1.5" fill="#3F3F46" />
              <rect x="22" y="4" width="1.8" height="5" rx="0.9" fill="#3F3F46" />
            </svg>
            <div style={{ textAlign: "right", fontSize: 12.5, fontWeight: 600, lineHeight: 1.3 }}>
              <div>9:41 AM</div>
              <div style={{ fontWeight: 500, opacity: 0.75 }}>7/7/2026</div>
            </div>
          </div>
        </div>
      )}
    </AbsoluteFill>
  );
};

/** Canvas coordinates of the tray Wi-Fi icon center (for camera + cursor). */
export const WIFI_POS = { x: 1744, y: 1052 };
