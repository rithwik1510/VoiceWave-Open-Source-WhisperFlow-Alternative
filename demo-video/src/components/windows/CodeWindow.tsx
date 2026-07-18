import React from "react";
import { fontBody, fontMono } from "../../theme";
import { AppDot, WindowFrame } from "./WindowFrame";

const T = {
  bg: "#17171C",
  side: "#1D1D23",
  linenum: "#4B4B55",
  text: "#D6D6DE",
  dim: "#8A8A96",
  kw: "#6FB4FF",
  str: "#9CD08F",
  fn: "#E5C07B",
  comment: "#5C6370"
};

const CODE: { num: number; parts: { t: string; c?: string }[] }[] = [
  { num: 41, parts: [{ t: "async fn ", c: T.kw }, { t: "insert_text", c: T.fn }, { t: "(&self, text: &str) {" }] },
  { num: 42, parts: [{ t: "    let ", c: T.kw }, { t: "target = focused_window()?;" }] },
  { num: 43, parts: [{ t: "    // fall back to clipboard paste", c: T.comment }] },
  { num: 44, parts: [{ t: "    self.sender.dispatch(target, text)" }] },
  { num: 45, parts: [{ t: "}" }] }
];

const CODE_AFTER: { num: number; parts: { t: string; c?: string }[] }[] = [
  { num: 46, parts: [{ t: "" }] },
  { num: 47, parts: [{ t: "fn ", c: T.kw }, { t: "rescue_capsule", c: T.fn }, { t: "(&self) -> PillNotice {" }] },
  { num: 48, parts: [{ t: "    PillNotice::with_transcript(self.last_final())" }] },
  { num: 49, parts: [{ t: "}" }] }
];

/** Dark code-editor lookalike (activity strip, file tree, tabs, review box). */
export const CodeWindow: React.FC<{
  width?: number;
  height?: number;
  x: number;
  y: number;
  children?: React.ReactNode;
}> = ({ width = 1280, height = 760, x, y, children }) => {
  return (
    <WindowFrame title="voicewave — review" icon={<AppDot color="#0F5FD7" glyph="{}" />} width={width} height={height} x={x} y={y} dark background={T.bg}>
      <div style={{ display: "flex", height: "100%", fontFamily: fontMono }}>
        {/* Activity strip */}
        <div style={{ width: 52, background: "#121217", display: "flex", flexDirection: "column", alignItems: "center", paddingTop: 16, gap: 20 }}>
          {[0.9, 0.45, 0.45, 0.45].map((o, i) => (
            <svg key={i} width="20" height="20" viewBox="0 0 20 20" style={{ opacity: o }}>
              {i === 0 && <><rect x="2" y="2" width="7" height="16" rx="1.5" fill="#9DA0AC" /><rect x="11" y="2" width="7" height="9" rx="1.5" fill="#9DA0AC" /></>}
              {i === 1 && <circle cx="9" cy="9" r="6" fill="none" stroke="#9DA0AC" strokeWidth="2" />}
              {i === 2 && <path d="M4 3 v10 a3 3 0 0 0 3 3 h9 M13 12 l3 4 -3 4" stroke="#9DA0AC" strokeWidth="2" fill="none" transform="scale(0.8)" />}
              {i === 3 && <><rect x="3" y="3" width="14" height="14" rx="2" fill="none" stroke="#9DA0AC" strokeWidth="2" /><path d="M7 10 l2.5 2.5 L14 8" stroke="#9DA0AC" strokeWidth="2" fill="none" /></>}
            </svg>
          ))}
        </div>
        {/* File tree */}
        <div style={{ width: 210, background: T.side, padding: "14px 0", fontFamily: fontBody, fontSize: 13, color: T.dim }}>
          <p style={{ margin: "0 0 8px 18px", fontSize: 11, fontWeight: 700, letterSpacing: "0.08em", color: "#6E6E78" }}>EXPLORER</p>
          {["src/", "  insertion.rs", "  pipeline.rs", "  review.md", "  state.rs"].map((f, i) => (
            <div key={f} style={{ padding: "4px 18px", whiteSpace: "pre", color: i === 3 ? "#E8E8EE" : T.dim, background: i === 3 ? "rgba(27,142,255,0.14)" : "transparent" }}>
              {f}
            </div>
          ))}
        </div>
        {/* Editor */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", gap: 2, padding: "8px 12px 0", fontFamily: fontBody, fontSize: 13 }}>
            <div style={{ padding: "7px 18px", borderRadius: "8px 8px 0 0", background: T.bg, color: "#E8E8EE", border: "1px solid rgba(255,255,255,0.08)", borderBottom: "none" }}>
              review.md
            </div>
            <div style={{ padding: "7px 18px", color: T.dim }}>insertion.rs</div>
          </div>
          <div style={{ flex: 1, padding: "18px 26px", fontSize: 15.5, lineHeight: 1.75 }}>
            {CODE.map((row) => (
              <div key={row.num} style={{ display: "flex" }}>
                <span style={{ width: 44, color: T.linenum, textAlign: "right", marginRight: 26, flexShrink: 0 }}>{row.num}</span>
                <span>
                  {row.parts.map((p, i) => (
                    <span key={i} style={{ color: p.c ?? T.text, whiteSpace: "pre" }}>{p.t}</span>
                  ))}
                </span>
              </div>
            ))}
            {/* Review comment thread — where dictation lands. */}
            <div
              style={{
                margin: "22px 0 0 70px",
                width: 640,
                borderRadius: 12,
                border: "1px solid rgba(27,142,255,0.35)",
                background: "rgba(27,142,255,0.07)",
                padding: "14px 18px",
                fontFamily: fontBody
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                <div style={{ width: 22, height: 22, borderRadius: 999, background: "linear-gradient(135deg,#0F5FD7,#1B8EFF)", color: "#fff", display: "grid", placeItems: "center", fontSize: 11, fontWeight: 800 }}>R</div>
                <span style={{ fontSize: 13, fontWeight: 700, color: "#E8E8EE" }}>rithwik</span>
                <span style={{ fontSize: 12, color: T.dim }}>reviewing</span>
              </div>
              <div style={{ fontSize: 16.5, lineHeight: 1.6, color: "#E8E8EE", minHeight: 54 }}>{children}</div>
            </div>
            <div style={{ marginTop: 22 }}>
              {CODE_AFTER.map((row) => (
                <div key={row.num} style={{ display: "flex" }}>
                  <span style={{ width: 44, color: T.linenum, textAlign: "right", marginRight: 26, flexShrink: 0 }}>{row.num}</span>
                  <span>
                    {row.parts.map((p, i) => (
                      <span key={i} style={{ color: p.c ?? T.text, whiteSpace: "pre" }}>{p.t}</span>
                    ))}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </WindowFrame>
  );
};
