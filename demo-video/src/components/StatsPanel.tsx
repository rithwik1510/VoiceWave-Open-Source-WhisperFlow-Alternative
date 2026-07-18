import React from "react";
import { useCurrentFrame } from "remotion";
import { easeOutCubic } from "../lib/anim";
import {
  blue,
  blueDeep,
  blueMid,
  blueWash,
  card,
  cyan,
  fontBody,
  fontDisplay,
  gray,
  grayLight,
  ink,
  line,
  lineSoft
} from "../theme";

// Power-user numbers, internally consistent:
// 72,481 words at 40 WPM typing = 1812 min; speaking at 136 WPM = 533 min;
// saved = 1279 min = 21h 19m.
export const STATS = {
  savedMin: 1279,
  savedMonthMin: 222, // 3h 42m
  speakMin: 533,
  dictations: 2347,
  days: 63,
  wpm: 136,
  best: 191,
  words: 72481,
  today: 1204,
  week: 6832,
  monthDeltaPct: 18
};

const GAUGE_MAX = 200;

function fmtDur(mins: number): string {
  const m = Math.round(mins);
  const h = Math.floor(m / 60);
  const r = m % 60;
  if (h === 0) return `${r}m`;
  return r === 0 ? `${h}h` : `${h}h ${r}m`;
}

function fmtNum(n: number): string {
  return Math.round(n).toLocaleString("en-US");
}

const Kicker: React.FC<{ children: React.ReactNode; icon: React.ReactNode }> = ({ children, icon }) => (
  <p
    style={{
      margin: 0,
      display: "flex",
      alignItems: "center",
      gap: 7,
      fontFamily: fontBody,
      fontSize: 14,
      fontWeight: 700,
      textTransform: "uppercase",
      letterSpacing: "0.16em",
      color: gray
    }}
  >
    {icon}
    {children}
  </p>
);

const ClockIcon = (
  <svg width="15" height="15" viewBox="0 0 15 15">
    <circle cx="7.5" cy="7.5" r="6" fill="none" stroke={blue} strokeWidth="1.6" />
    <path d="M7.5 4.2 v3.5 l2.3 1.4" fill="none" stroke={blue} strokeWidth="1.6" strokeLinecap="round" />
  </svg>
);
const GaugeIcon = (
  <svg width="15" height="15" viewBox="0 0 15 15">
    <path d="M2 11 a5.5 5.5 0 0 1 11 0" fill="none" stroke={blue} strokeWidth="1.6" strokeLinecap="round" />
    <line x1="7.5" y1="11" x2="10" y2="6.5" stroke={blue} strokeWidth="1.6" strokeLinecap="round" />
  </svg>
);
const TypeIcon = (
  <svg width="15" height="15" viewBox="0 0 15 15">
    <path d="M2 3 h11 M7.5 3 v9" fill="none" stroke={blue} strokeWidth="1.8" strokeLinecap="round" />
  </svg>
);

/**
 * The app's Stats hero tier, frame-driven: revolving-ring time-saved card,
 * WPM gauge, words panel. `start` = local frame at which count-ups begin.
 */
export const StatsPanel: React.FC<{ start?: number; width?: number; countDur?: number }> = ({
  start = 20,
  width = 1250,
  countDur = 95
}) => {
  const frame = useCurrentFrame();
  const p = easeOutCubic((frame - start) / countDur);
  const ringAngle = (frame * 1.6) % 360;
  const gaugeFrac = Math.min(1, STATS.wpm / GAUGE_MAX) * p;
  const radius = 84;
  const circumference = Math.PI * radius;

  const cardStyle: React.CSSProperties = {
    borderRadius: 26,
    border: `1px solid ${line}`,
    background: card,
    padding: "26px 30px",
    boxShadow: "0 1px 2px rgba(9,9,11,0.04)"
  };

  return (
    <div style={{ width, fontFamily: fontBody }}>
      {/* Hero: revolving ring around the time-saved card. */}
      <div
        style={{
          borderRadius: 30,
          padding: 2,
          background: `conic-gradient(from ${ringAngle}deg, transparent 0deg, ${blue} 45deg, ${cyan} 95deg, transparent 150deg), ${lineSoft}`
        }}
      >
        <div style={{ borderRadius: 28, background: card, padding: "30px 38px" }}>
          <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 40 }}>
            <div>
              <Kicker icon={ClockIcon}>Time saved vs typing</Kicker>
              <p
                style={{
                  margin: "12px 0 0",
                  fontFamily: fontDisplay,
                  fontSize: 88,
                  lineHeight: 1,
                  letterSpacing: "-0.02em",
                  color: ink
                }}
              >
                {fmtDur(STATS.savedMin * p)}
              </p>
              <p style={{ margin: "12px 0 0", fontSize: 19, color: gray }}>
                {fmtDur(STATS.savedMonthMin)} this month · {fmtDur(STATS.speakMin)} spent speaking in total
              </p>
            </div>
            <div style={{ textAlign: "right" }}>
              <p style={{ margin: 0, fontSize: 14, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.16em", color: gray }}>
                Dictations
              </p>
              <p style={{ margin: "6px 0 0", fontSize: 40, fontWeight: 600, color: ink, fontVariantNumeric: "tabular-nums" }}>
                {fmtNum(STATS.dictations * p)}
              </p>
              <p style={{ margin: "4px 0 0", fontSize: 15, color: grayLight }}>across {STATS.days} active days</p>
            </div>
          </div>
        </div>
      </div>

      <div style={{ display: "flex", gap: 20, marginTop: 20 }}>
        {/* Speaking speed gauge. */}
        <div style={{ ...cardStyle, flex: 1 }}>
          <Kicker icon={GaugeIcon}>Speaking speed</Kicker>
          <div style={{ display: "flex", justifyContent: "center", marginTop: 14 }}>
            <svg viewBox="0 0 220 122" style={{ width: 300 }}>
              <defs>
                <linearGradient id="vw-demo-arc" x1="0%" y1="100%" x2="100%" y2="0%">
                  <stop offset="0%" stopColor={blueDeep} />
                  <stop offset="55%" stopColor={blueMid} />
                  <stop offset="100%" stopColor={blue} />
                </linearGradient>
              </defs>
              <path
                d={`M ${110 - radius} 110 A ${radius} ${radius} 0 0 1 ${110 + radius} 110`}
                fill="none"
                stroke={lineSoft}
                strokeWidth={13}
                strokeLinecap="round"
              />
              <path
                d={`M ${110 - radius} 110 A ${radius} ${radius} 0 0 1 ${110 + radius} 110`}
                fill="none"
                stroke="url(#vw-demo-arc)"
                strokeWidth={13}
                strokeLinecap="round"
                strokeDasharray={circumference}
                strokeDashoffset={circumference * (1 - gaugeFrac)}
              />
              <text x="110" y="92" textAnchor="middle" style={{ font: `600 40px ${fontBody}`, fill: ink }}>
                {Math.round(STATS.wpm * p)}
              </text>
              <text x="110" y="112" textAnchor="middle" style={{ font: `600 11px ${fontBody}`, letterSpacing: "0.14em", fill: grayLight }}>
                WPM
              </text>
            </svg>
          </div>
          <div style={{ display: "flex", justifyContent: "center", gap: 10, marginTop: 14 }}>
            <span
              style={{
                fontSize: 15,
                fontWeight: 700,
                color: blueDeep,
                background: blueWash,
                borderRadius: 999,
                padding: "7px 16px"
              }}
            >
              {(STATS.wpm / 40).toFixed(1)}× faster than typing
            </span>
            <span
              style={{
                fontSize: 15,
                fontWeight: 600,
                color: gray,
                border: `1px solid ${line}`,
                borderRadius: 999,
                padding: "7px 16px"
              }}
            >
              Best: {STATS.best} WPM
            </span>
          </div>
        </div>

        {/* Words dictated. */}
        <div style={{ ...cardStyle, flex: 1.15 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <Kicker icon={TypeIcon}>Words dictated</Kicker>
            <span
              style={{
                display: "flex",
                alignItems: "center",
                gap: 5,
                fontSize: 13.5,
                fontWeight: 700,
                color: blueDeep,
                background: blueWash,
                borderRadius: 999,
                padding: "4px 12px"
              }}
            >
              <svg width="12" height="12" viewBox="0 0 12 12">
                <path d="M1 9 L5 5 L7.5 7.5 L11 3.5 M11 7 V3.5 H7.5" fill="none" stroke={blueDeep} strokeWidth="1.6" strokeLinejoin="round" strokeLinecap="round" />
              </svg>
              {STATS.monthDeltaPct}% vs last month
            </span>
          </div>
          <p
            style={{
              margin: "18px 0 0",
              fontFamily: fontDisplay,
              fontSize: 64,
              lineHeight: 1,
              letterSpacing: "-0.02em",
              color: ink
            }}
          >
            {fmtNum(STATS.words * p)}
          </p>
          <p style={{ margin: "6px 0 0", fontSize: 14, color: grayLight }}>all time</p>
          <div style={{ display: "flex", gap: 14, marginTop: 18 }}>
            {[
              { label: "Today", v: STATS.today },
              { label: "This week", v: STATS.week }
            ].map((tile) => (
              <div key={tile.label} style={{ flex: 1, borderRadius: 18, background: "#FAFAFA", padding: "14px 18px" }}>
                <p style={{ margin: 0, fontSize: 12.5, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.14em", color: grayLight }}>
                  {tile.label}
                </p>
                <p style={{ margin: "6px 0 0", fontSize: 26, fontWeight: 600, color: ink, fontVariantNumeric: "tabular-nums" }}>
                  {fmtNum(tile.v * p)}
                </p>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};
