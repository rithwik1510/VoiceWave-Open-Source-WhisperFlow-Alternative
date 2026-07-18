import React from "react";
import { AbsoluteFill, Sequence } from "remotion";
import { Camera } from "../components/Camera";
import { Dissolve } from "../components/Transitions";
import { Caption } from "../components/Caption";
import { Desktop } from "../components/Desktop";
import { Pill } from "../components/Pill";
import { TypeOn } from "../components/TypeOn";
import { CodeWindow } from "../components/windows/CodeWindow";
import { NotesWindow } from "../components/windows/NotesWindow";
import { TerminalWindow } from "../components/windows/TerminalWindow";

export const MONTAGE_DUR = 270;
const SEG = 90;

/**
 * Scene 3 (10-19s): beat-cut montage — code review, terminal, notes. Each
 * segment opens mid-dictation and lands its text before the next cut.
 */
export const SceneMontage: React.FC = () => {
  return (
    <AbsoluteFill>
      {/* Segment 1: code review comment. */}
      <Sequence from={0} durationInFrames={SEG + 6}>
        <Camera
          keys={[
            { frame: 0, scale: 1.06, cx: 965, cy: 590 },
            { frame: SEG, scale: 1.12, cx: 975, cy: 605 }
          ]}
        >
          <Desktop>
            <CodeWindow x={320} y={130} height={700}>
              <TypeOn
                text="Nice catch — let's fall back to clipboard paste when the window loses focus."
                start={50}
                step={2}
              />
            </CodeWindow>
            <Pill
              seed={11}
              x={960}
              y={1078}
              timeline={[
                { frame: 0, state: "listening" },
                { frame: 34, state: "transcribing" },
                { frame: 47, state: "inserted" },
                { frame: 76, state: "idle" }
              ]}
            />
          </Desktop>
        </Camera>
      </Sequence>

      {/* Segment 2: terminal commit message. */}
      <Sequence from={SEG} durationInFrames={SEG + 6}>
        <Dissolve dur={6}>
          <Camera
          keys={[
            { frame: 0, scale: 1.08, cx: 955, cy: 595 },
            { frame: SEG, scale: 1.14, cx: 950, cy: 608 }
          ]}
        >
          <Desktop>
            <TerminalWindow x={370} y={180}>
              <TypeOn text="fix insertion focus race, add paste fallback" start={52} step={2} />
            </TerminalWindow>
            <Pill
              seed={23}
              x={960}
              y={1078}
              timeline={[
                { frame: 0, state: "listening" },
                { frame: 36, state: "transcribing" },
                { frame: 49, state: "inserted" },
                { frame: 78, state: "idle" }
              ]}
            />
          </Desktop>
        </Camera>
        </Dissolve>
      </Sequence>

      {/* Segment 3: a quick note. */}
      <Sequence from={SEG * 2} durationInFrames={SEG}>
        <Dissolve dur={6}>
          <Camera
          keys={[
            { frame: 0, scale: 1.06, cx: 970, cy: 590 },
            { frame: SEG, scale: 1.12, cx: 980, cy: 605 }
          ]}
        >
          <Desktop>
            <NotesWindow x={400} y={150} title="Ideas">
              <TypeOn
                text="Book flights for the offsite — check Tuesday fares before noon."
                start={54}
                step={2}
              />
            </NotesWindow>
            <Pill
              seed={31}
              x={960}
              y={1078}
              timeline={[
                { frame: 0, state: "listening" },
                { frame: 38, state: "transcribing" },
                { frame: 51, state: "inserted" },
                { frame: 80, state: "idle" }
              ]}
            />
          </Desktop>
        </Camera>
        </Dissolve>
      </Sequence>

      <Caption from={8} to={160} size={42} y={874}>
        Dictate into any app on Windows.
      </Caption>
      <Caption from={176} to={258} size={42} y={874}>
        Same voice. Every window.
      </Caption>
    </AbsoluteFill>
  );
};
