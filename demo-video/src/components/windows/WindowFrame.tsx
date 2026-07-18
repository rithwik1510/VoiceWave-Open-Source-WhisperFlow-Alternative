import React from "react";
import { fontBody, gray, ink, line } from "../../theme";

/**
 * Windows-11-style app window: rounded corners, left icon + title, right
 * caption buttons. Variant controls chrome tone for light/dark apps.
 */
export const WindowFrame: React.FC<{
  title: string;
  icon?: React.ReactNode;
  width: number;
  height: number;
  x: number;
  y: number;
  dark?: boolean;
  background?: string;
  children: React.ReactNode;
  style?: React.CSSProperties;
}> = ({ title, icon, width, height, x, y, dark = false, background, children, style }) => {
  const fg = dark ? "rgba(244,244,245,0.75)" : gray;
  const chrome = dark ? "rgba(255,255,255,0.06)" : "transparent";
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width,
        height,
        borderRadius: 14,
        overflow: "hidden",
        background: background ?? (dark ? "#17171C" : "#FFFFFF"),
        border: `1px solid ${dark ? "rgba(255,255,255,0.1)" : line}`,
        boxShadow: "0 40px 90px -30px rgba(9,9,11,0.35), 0 10px 30px -18px rgba(9,9,11,0.25)",
        fontFamily: fontBody,
        ...style
      }}
    >
      <div
        style={{
          height: 46,
          display: "flex",
          alignItems: "center",
          padding: "0 8px 0 16px",
          background: chrome,
          borderBottom: `1px solid ${dark ? "rgba(255,255,255,0.07)" : "#F0F0F2"}`
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, flex: 1, minWidth: 0 }}>
          {icon}
          <span style={{ fontSize: 15, fontWeight: 600, color: dark ? "rgba(244,244,245,0.85)" : ink }}>
            {title}
          </span>
        </div>
        {/* Caption buttons: minimize, maximize, close. */}
        <div style={{ display: "flex", alignItems: "center" }}>
          {["min", "max", "close"].map((kind) => (
            <div
              key={kind}
              style={{
                width: 46,
                height: 34,
                display: "grid",
                placeItems: "center"
              }}
            >
              <svg width="12" height="12" viewBox="0 0 12 12">
                {kind === "min" && <line x1="1" y1="6" x2="11" y2="6" stroke={fg} strokeWidth="1.2" />}
                {kind === "max" && (
                  <rect x="1.5" y="1.5" width="9" height="9" rx="1.5" fill="none" stroke={fg} strokeWidth="1.2" />
                )}
                {kind === "close" && (
                  <>
                    <line x1="1.5" y1="1.5" x2="10.5" y2="10.5" stroke={fg} strokeWidth="1.2" />
                    <line x1="10.5" y1="1.5" x2="1.5" y2="10.5" stroke={fg} strokeWidth="1.2" />
                  </>
                )}
              </svg>
            </div>
          ))}
        </div>
      </div>
      <div style={{ position: "absolute", inset: "47px 0 0 0" }}>{children}</div>
    </div>
  );
};

/** Small rounded app-icon dot used in window titles. */
export const AppDot: React.FC<{ color: string; glyph?: string }> = ({ color, glyph }) => (
  <div
    style={{
      width: 18,
      height: 18,
      borderRadius: 5,
      background: color,
      display: "grid",
      placeItems: "center",
      color: "#fff",
      fontSize: 10,
      fontWeight: 800,
      flexShrink: 0
    }}
  >
    {glyph ?? ""}
  </div>
);
