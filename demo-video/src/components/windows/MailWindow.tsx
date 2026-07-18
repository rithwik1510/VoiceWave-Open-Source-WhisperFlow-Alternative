import React from "react";
import { fontBody, gray, grayLight, ink } from "../../theme";
import { AppDot, WindowFrame } from "./WindowFrame";

/** Clean email compose lookalike (light). Children render in the body. */
export const MailWindow: React.FC<{
  width?: number;
  height?: number;
  x: number;
  y: number;
  children?: React.ReactNode;
}> = ({ width = 1060, height = 660, x, y, children }) => {
  return (
    <WindowFrame title="New message" icon={<AppDot color="#0F5FD7" glyph="✉" />} width={width} height={height} x={x} y={y}>
      <div style={{ padding: "10px 36px 0", fontFamily: fontBody }}>
        {[
          { label: "To", value: "elena@northwind.dev" },
          { label: "Subject", value: "Weekly update" }
        ].map((row) => (
          <div
            key={row.label}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              padding: "13px 0",
              borderBottom: "1px solid #F0F0F2"
            }}
          >
            <span style={{ fontSize: 15, color: grayLight, width: 64 }}>{row.label}</span>
            {row.label === "To" ? (
              <span
                style={{
                  fontSize: 14,
                  fontWeight: 600,
                  color: ink,
                  background: "#F4F4F5",
                  borderRadius: 999,
                  padding: "4px 12px"
                }}
              >
                {row.value}
              </span>
            ) : (
              <span style={{ fontSize: 15, fontWeight: 600, color: ink }}>{row.value}</span>
            )}
          </div>
        ))}
        <div
          style={{
            paddingTop: 26,
            fontSize: 21,
            lineHeight: 1.65,
            color: ink,
            minHeight: 300
          }}
        >
          {children}
        </div>
      </div>
      <div
        style={{
          position: "absolute",
          left: 36,
          bottom: 24,
          display: "flex",
          alignItems: "center",
          gap: 16
        }}
      >
        <div
          style={{
            fontFamily: fontBody,
            fontSize: 14,
            fontWeight: 700,
            color: "#fff",
            background: ink,
            borderRadius: 999,
            padding: "9px 22px"
          }}
        >
          Send
        </div>
        <span style={{ fontFamily: fontBody, fontSize: 13, color: grayLight }}>Draft saved</span>
      </div>
    </WindowFrame>
  );
};

export const mailGray = gray;
