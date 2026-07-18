import React from "react";
import { AbsoluteFill, Audio, Img, Sequence, interpolate, staticFile, useCurrentFrame } from "remotion";
import { capsuleEase, easeOutCubic, rise, speechEnergy } from "./lib/anim";
import { Caption } from "./components/Caption";
import { GhostSpeech } from "./components/GhostSpeech";
import { Pill } from "./components/Pill";
import { Dissolve } from "./components/Transitions";
import { TypeOn } from "./components/TypeOn";
import { MailWindow } from "./components/windows/MailWindow";
import {
  blue,
  blueWash,
  card,
  ctaGradient,
  cyan,
  cyanSoft,
  fontBody,
  fontDisplay,
  fontMono,
  glowBlue,
  gray,
  grayLight,
  ink,
  line,
  lineSoft
} from "./theme";

// Vertical cut for Shorts / Reels / TikTok. Scenes lay out on a logical
// 1080x1920 canvas; the composition rasterizes at 2x like the main video.
export const SHORT_W = 1080;
export const SHORT_H = 1920;
export const SHORT_SS = 2;

export const ShortFrame: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <AbsoluteFill style={{ background: "#FAFAFA" }}>
    <div
      style={{
        width: SHORT_W,
        height: SHORT_H,
        transform: `scale(${SHORT_SS})`,
        transformOrigin: "0 0"
      }}
    >
      {children}
    </div>
  </AbsoluteFill>
);

const paperBg =
  "radial-gradient(900px 700px at 75% 12%, rgba(27,142,255,0.10), transparent 60%), radial-gradient(700px 600px at 12% 92%, rgba(15,95,215,0.07), transparent 55%), linear-gradient(180deg, #FBFBFC 0%, #F2F3F6 100%)";

/* ------------------------------------------------------------------ */
/* Scene 1 — the magic moment, reframed for a phone. */
export const SHORT_HOOK_DUR = 320;

const HOOK_BODY = "Quick update — the new build went out this morning. Early numbers look strong.";

export const ShortHook: React.FC = () => {
  const frame = useCurrentFrame();
  const drift = 1 + 0.045 * (frame / SHORT_HOOK_DUR);
  return (
    <AbsoluteFill style={{ background: paperBg }}>
      <div style={{ position: "absolute", inset: 0, transform: `scale(${drift})`, transformOrigin: "50% 46%" }}>
        <MailWindow width={920} height={640} x={80} y={430}>
          <TypeOn text={HOOK_BODY} start={210} step={2} caret />
        </MailWindow>
        <GhostSpeech
          text="Quick update — the new build went out this morning."
          start={70}
          end={182}
          x={540}
          y={1180}
          maxWidth={820}
          size={30}
        />
        <Pill
          appear
          seed={5}
          display={4}
          x={540}
          y={1520}
          timeline={[
            { frame: 30, state: "idle" },
            { frame: 52, state: "listening" },
            { frame: 188, state: "transcribing" },
            { frame: 208, state: "inserted" },
            { frame: 250, state: "idle" }
          ]}
        />
      </div>
      <Caption from={8} to={120} size={54} y={240}>
        Don&rsquo;t type it.
      </Caption>
      <Caption from={132} to={300} size={54} y={240}>
        Say it.
      </Caption>
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------ */
/* Scene 2 — the race: typing vs speaking, counted live. */
export const SHORT_RACE_DUR = 330;

const RACE = {
  typeStart: 26,
  typeDur: 210,
  typeTarget: 50,
  speakStart: 74,
  speakDur: 96,
  speakTarget: 150,
  punchAt: 226
};

const LaneCard: React.FC<{
  label: string;
  value: number;
  target: number;
  accent: boolean;
  appearAt: number;
  children?: React.ReactNode;
}> = ({ label, value, target, accent, appearAt, children }) => {
  const frame = useCurrentFrame();
  const inP = rise(frame, appearAt, 16);
  // The typing lane dims once it's been lapped.
  const lapped = !accent && frame > RACE.speakStart + RACE.speakDur * 0.42;
  const lappedP = accent ? 0 : rise(frame, RACE.speakStart + RACE.speakDur * 0.42, 24);
  return (
    <div
      style={{
        width: 880,
        borderRadius: 40,
        padding: "44px 52px 40px",
        background: card,
        border: `1px solid ${accent ? "rgba(27,142,255,0.35)" : line}`,
        boxShadow: accent
          ? "0 30px 70px -30px rgba(27,142,255,0.45), 0 2px 6px rgba(9,9,11,0.04)"
          : "0 18px 44px -28px rgba(9,9,11,0.25)",
        opacity: inP * (lapped ? 1 - 0.35 * lappedP : 1),
        transform: `translateY(${(1 - inP) * 44}px) scale(${lapped ? 1 - 0.02 * lappedP : 1})`
      }}
    >
      <div
        style={{
          fontFamily: fontMono,
          fontSize: 22,
          fontWeight: 700,
          letterSpacing: "0.18em",
          textTransform: "uppercase",
          color: accent ? blue : grayLight
        }}
      >
        {label}
      </div>
      <div style={{ display: "flex", alignItems: "baseline", gap: 18, marginTop: 6 }}>
        <span
          style={{
            fontFamily: fontDisplay,
            fontVariantNumeric: "tabular-nums",
            fontSize: 168,
            fontWeight: 600,
            lineHeight: 1,
            letterSpacing: "-0.03em",
            color: accent ? ink : gray
          }}
        >
          {Math.round(value)}
        </span>
        <span style={{ fontFamily: fontBody, fontSize: 34, fontWeight: 700, color: grayLight }}>WPM</span>
      </div>
      <div
        style={{
          marginTop: 30,
          height: 54,
          borderRadius: 999,
          background: accent ? blueWash : "#F1F1F4",
          overflow: "hidden",
          position: "relative"
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            width: `${Math.min(100, (value / RACE.speakTarget) * 100)}%`,
            borderRadius: 999,
            background: accent ? ctaGradient : "#C9C9D2",
            boxShadow: accent ? "0 0 34px rgba(27,142,255,0.45)" : "none",
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            paddingRight: 10,
            overflow: "hidden"
          }}
        >
          {children}
        </div>
      </div>
      <div style={{ marginTop: 14, fontFamily: fontBody, fontSize: 22, fontWeight: 600, color: grayLight }}>
        {accent ? "you, talking" : "you, typing"} · ~{target} words a minute
      </div>
    </div>
  );
};

/** Live waveform riding inside the speaking lane's progress bar. */
const LaneWave: React.FC<{ active: boolean }> = ({ active }) => {
  const frame = useCurrentFrame();
  const t = frame / 30;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 5, height: "100%" }}>
      {new Array(9).fill(0).map((_, i) => {
        const e = active ? speechEnergy(t - i * 0.055, 11) : 0.08;
        return (
          <span
            key={i}
            style={{
              width: 5,
              height: 34,
              borderRadius: 999,
              background: `linear-gradient(180deg, #fff, ${cyanSoft})`,
              opacity: 0.9,
              transform: `scaleY(${(0.15 + e * 0.85).toFixed(3)})`
            }}
          />
        );
      })}
    </div>
  );
};

export const ShortRace: React.FC = () => {
  const frame = useCurrentFrame();
  const typeP = easeOutCubic((frame - RACE.typeStart) / RACE.typeDur);
  const speakP = easeOutCubic((frame - RACE.speakStart) / RACE.speakDur);
  const typeVal = Math.max(0, typeP) * RACE.typeTarget;
  const speakVal = Math.max(0, speakP) * RACE.speakTarget;

  const punchP = rise(frame, RACE.punchAt, 18);
  const punchPop = 1 + 0.22 * Math.sin(Math.min(1, Math.max(0, (frame - RACE.punchAt) / 22)) * Math.PI);
  const glow = punchP * (0.75 + 0.25 * Math.sin(frame / 14));

  return (
    <AbsoluteFill style={{ background: paperBg, alignItems: "center" }}>
      <AbsoluteFill
        style={{
          background: "radial-gradient(720px 560px at 50% 78%, rgba(27,142,255,0.16), transparent 65%)",
          opacity: glow
        }}
      />
      <Caption from={4} to={SHORT_RACE_DUR - 16} size={56} y={210} weight={600}>
        Same minute. Two speeds.
      </Caption>

      <div
        style={{
          position: "absolute",
          top: 420,
          left: 0,
          right: 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 56
        }}
      >
        <LaneCard label="Typing" value={typeVal} target={RACE.typeTarget} accent={false} appearAt={10} />
        <LaneCard label="Speaking" value={speakVal} target={RACE.speakTarget} accent appearAt={54}>
          <LaneWave active={frame > RACE.speakStart && frame < RACE.speakStart + RACE.speakDur + 30} />
        </LaneCard>
      </div>

      <div
        style={{
          position: "absolute",
          bottom: 300,
          left: 0,
          right: 0,
          textAlign: "center",
          opacity: punchP,
          transform: `translateY(${(1 - punchP) * 46}px) scale(${punchPop})`
        }}
      >
        <div
          style={{
            fontFamily: fontDisplay,
            fontSize: 128,
            fontWeight: 600,
            letterSpacing: "-0.03em",
            color: ink,
            lineHeight: 1
          }}
        >
          3&times; faster.
        </div>
        <div style={{ marginTop: 18, fontFamily: fontBody, fontSize: 30, fontWeight: 600, color: gray }}>
          Your voice already is.
        </div>
      </div>
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------ */
/* Scene 3 — the offer, vertical. */
export const SHORT_OFFER_DUR = 200;

const Rise: React.FC<{ at: number; children: React.ReactNode }> = ({ at, children }) => {
  const frame = useCurrentFrame();
  const p = rise(frame, at, 16);
  return (
    <div style={{ opacity: p, transform: `translateY(${(1 - p) * 30}px)`, filter: `blur(${(1 - p) * 7}px)` }}>
      {children}
    </div>
  );
};

export const ShortOffer: React.FC = () => {
  const frame = useCurrentFrame();
  const driftS = 1 + 0.04 * (frame / SHORT_OFFER_DUR);
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
          background: "radial-gradient(700px 620px at 50% 44%, rgba(27,142,255,0.12), transparent 62%)",
          opacity: breathe
        }}
      />
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 30,
          textAlign: "center",
          transform: `scale(${driftS})`,
          padding: "0 70px"
        }}
      >
        <Rise at={4}>
          <h1
            style={{
              margin: 0,
              fontFamily: fontDisplay,
              fontSize: 104,
              fontWeight: 600,
              letterSpacing: "-0.02em",
              lineHeight: 1.06,
              color: ink
            }}
          >
            Free.
            <br />
            Open source.
            <br />
            Yours.
          </h1>
        </Rise>
        <Rise at={22}>
          <p style={{ margin: 0, fontFamily: fontBody, fontSize: 32, fontWeight: 500, color: gray }}>
            Whisper, running 100% on your PC.
            <br />
            No cloud. No subscription.
          </p>
        </Rise>
        <Rise at={40}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              fontFamily: fontBody,
              fontSize: 24,
              fontWeight: 600,
              color: ink,
              border: `1px solid ${line}`,
              background: "#fff",
              borderRadius: 999,
              padding: "16px 32px",
              boxShadow: "0 1px 2px rgba(9,9,11,0.05)"
            }}
          >
            <svg width="28" height="28" viewBox="0 0 16 16" fill={ink}>
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
            </svg>
            rithwik1510/VoiceWave
          </div>
        </Rise>
      </div>
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------ */
/* Scene 4 — logo ring, wordmark, CTA. */
export const SHORT_CLOSE_DUR = 180;

export const ShortClose: React.FC = () => {
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
          "radial-gradient(680px 560px at 50% 42%, rgba(27,142,255,0.11), transparent 60%), linear-gradient(180deg, #FBFBFC 0%, #F3F4F7 100%)",
        alignItems: "center",
        justifyContent: "center"
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 34,
          transform: `scale(${1 + 0.035 * (frame / SHORT_CLOSE_DUR)})`
        }}
      >
        <div
          style={{
            borderRadius: 54,
            padding: 4,
            opacity: logoP,
            transform: `scale(${0.85 + 0.15 * logoP})`,
            boxShadow: "0 22px 60px -20px rgba(27,142,255,0.4)",
            background: `conic-gradient(from ${ringAngle}deg, transparent 0deg, ${blue} 55deg, ${cyan} 105deg, transparent 170deg), ${lineSoft}`
          }}
        >
          <div
            style={{
              width: 190,
              height: 190,
              borderRadius: 50,
              background: "#fff",
              display: "grid",
              placeItems: "center"
            }}
          >
            <Img src={staticFile("icon.png")} style={{ width: 138, height: 138, borderRadius: 32 }} />
          </div>
        </div>
        <h1
          style={{
            margin: 0,
            fontFamily: fontDisplay,
            fontSize: 92,
            fontWeight: 600,
            letterSpacing: "-0.02em",
            color: ink,
            opacity: nameP,
            transform: `translateY(${(1 - nameP) * 22}px)`
          }}
        >
          VoiceWave
        </h1>
        <div
          style={{
            opacity: ctaP,
            transform: `translateY(${(1 - ctaP) * 22}px)`,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 18
          }}
        >
          <div
            style={{
              fontFamily: fontBody,
              fontSize: 30,
              fontWeight: 700,
              color: "#fff",
              background: ctaGradient,
              borderRadius: 999,
              padding: "20px 52px",
              boxShadow: `${glowBlue}, inset 0 1px 0 rgba(255,255,255,0.22)`
            }}
          >
            Download for Windows
          </div>
          <p style={{ margin: 0, fontFamily: fontBody, fontSize: 22, color: grayLight }}>
            Windows 10 &amp; 11 · free &amp; open source
          </p>
        </div>
      </div>
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------ */
/* Assembly. */
export const SHORT_DUR = SHORT_HOOK_DUR + SHORT_RACE_DUR + SHORT_OFFER_DUR + SHORT_CLOSE_DUR; // 1030 (~34s)

const AT = {
  race: SHORT_HOOK_DUR,
  offer: SHORT_HOOK_DUR + SHORT_RACE_DUR,
  close: SHORT_HOOK_DUR + SHORT_RACE_DUR + SHORT_OFFER_DUR
};

const OVERLAP = 14;

const Sfx: React.FC<{ src: string; at: number; volume?: number }> = ({ src, at, volume = 0.5 }) => (
  <Sequence from={at}>
    <Audio src={staticFile(`audio/${src}`)} volume={volume} />
  </Sequence>
);

export const Short: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: "#FAFAFA" }}>
      <Sequence from={0} durationInFrames={SHORT_HOOK_DUR + OVERLAP}>
        <ShortHook />
      </Sequence>
      <Sequence from={AT.race} durationInFrames={SHORT_RACE_DUR + OVERLAP}>
        <Dissolve>
          <ShortRace />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.offer} durationInFrames={SHORT_OFFER_DUR + OVERLAP}>
        <Dissolve>
          <ShortOffer />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.close} durationInFrames={SHORT_CLOSE_DUR}>
        <Dissolve>
          <ShortClose />
        </Dissolve>
      </Sequence>

      <Audio src={staticFile("audio/music.wav")} volume={1} />

      {/* Hook: pill appears, listens, inserts. */}
      <Sfx src="pop.wav" at={30} volume={0.55} />
      <Sfx src="cue_open.wav" at={52} volume={0.5} />
      <Sfx src="cue_close.wav" at={188} volume={0.5} />

      {/* Race: riser into the speaking counter, thock on the punchline. */}
      <Sfx src="riser.wav" at={AT.race + RACE.speakStart - 34} volume={0.45} />
      <Sfx src="thock.wav" at={AT.race + RACE.punchAt} volume={0.8} />
      <Sfx src="thud.wav" at={AT.race + RACE.punchAt + 1} volume={0.5} />

      {/* Close: brand pop. */}
      <Sfx src="pop.wav" at={AT.close + 4} volume={0.4} />
    </AbsoluteFill>
  );
};
