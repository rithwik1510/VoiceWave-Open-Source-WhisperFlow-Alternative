import React from "react";
import { fontBody, fontDisplay, grayLight, ink } from "../../theme";
import { AppDot, WindowFrame } from "./WindowFrame";

/** Quiet notes-app lookalike (light, editorial). */
export const NotesWindow: React.FC<{
  width?: number;
  height?: number;
  x: number;
  y: number;
  title?: string;
  children?: React.ReactNode;
}> = ({ width = 1120, height = 700, x, y, title = "Q3 planning", children }) => {
  return (
    <WindowFrame title="Notes" icon={<AppDot color="#E8B04B" glyph="✎" />} width={width} height={height} x={x} y={y}>
      <div style={{ display: "flex", height: "100%" }}>
        <div style={{ width: 240, borderRight: "1px solid #F0F0F2", padding: "18px 0", fontFamily: fontBody }}>
          {[
            { t: title, active: true },
            ...["Reading list", "Ideas", "Standup notes"]
              .filter((t) => t !== title)
              .map((t) => ({ t, active: false }))
          ].map((row) => (
            <div
              key={row.t}
              style={{
                padding: "10px 20px",
                fontSize: 14,
                fontWeight: row.active ? 700 : 500,
                color: row.active ? ink : grayLight,
                background: row.active ? "#F4F7FF" : "transparent",
                borderLeft: row.active ? "3px solid #1B8EFF" : "3px solid transparent"
              }}
            >
              {row.t}
            </div>
          ))}
        </div>
        <div style={{ flex: 1, padding: "30px 46px", minWidth: 0 }}>
          <h2 style={{ margin: 0, fontFamily: fontDisplay, fontSize: 34, fontWeight: 600, color: ink, letterSpacing: "-0.01em" }}>
            {title}
          </h2>
          <p style={{ margin: "6px 0 22px", fontFamily: fontBody, fontSize: 13, color: grayLight }}>
            Edited just now
          </p>
          <div style={{ fontFamily: fontBody, fontSize: 21, lineHeight: 1.7, color: ink }}>{children}</div>
        </div>
      </div>
    </WindowFrame>
  );
};
