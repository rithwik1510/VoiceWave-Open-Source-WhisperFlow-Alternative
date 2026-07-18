import React from "react";
import { fontMono } from "../../theme";
import { AppDot, WindowFrame } from "./WindowFrame";

/** Windows Terminal lookalike. Children land after `git commit -m "`. */
export const TerminalWindow: React.FC<{
  width?: number;
  height?: number;
  x: number;
  y: number;
  children?: React.ReactNode;
}> = ({ width = 1180, height = 640, x, y, children }) => {
  return (
    <WindowFrame title="Terminal" icon={<AppDot color="#2D2D34" glyph=">_" />} width={width} height={height} x={x} y={y} dark background="#0E0E13">
      <div style={{ padding: "22px 30px", fontFamily: fontMono, fontSize: 17, lineHeight: 1.9 }}>
        <div>
          <span style={{ color: "#6FB4FF" }}>C:\dev\voicewave</span>
          <span style={{ color: "#8A8A96" }}> on </span>
          <span style={{ color: "#9CD08F" }}> main</span>
        </div>
        <div style={{ color: "#D6D6DE" }}>
          <span style={{ color: "#F4BF75" }}>❯</span> git add -A
        </div>
        <div>
          <span style={{ color: "#6FB4FF" }}>C:\dev\voicewave</span>
          <span style={{ color: "#8A8A96" }}> on </span>
          <span style={{ color: "#9CD08F" }}> main</span>
        </div>
        <div style={{ color: "#D6D6DE" }}>
          <span style={{ color: "#F4BF75" }}>❯</span> git commit -m &quot;
          <span style={{ color: "#9CD08F" }}>{children}</span>
          &quot;
        </div>
      </div>
    </WindowFrame>
  );
};
