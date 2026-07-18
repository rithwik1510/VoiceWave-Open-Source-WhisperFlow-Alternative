import React from "react";
import { AbsoluteFill } from "remotion";
import { Camera } from "../components/Camera";
import { Caption } from "../components/Caption";
import { Desktop } from "../components/Desktop";
import { GhostSpeech } from "../components/GhostSpeech";
import { Pill } from "../components/Pill";
import { TypeOn } from "../components/TypeOn";
import { NotesWindow } from "../components/windows/NotesWindow";

export const QUALITY_DUR = 330;

/**
 * Scene 4 (19-30s): messy speech becomes clean text, then spoken commands
 * build a bullet list live.
 */
export const SceneQuality: React.FC = () => {
  return (
    <AbsoluteFill>
      <Camera
        keys={[
          { frame: 0, scale: 1.1, cx: 1000, cy: 595 },
          { frame: 140, scale: 1.14, cx: 1005, cy: 610 },
          { frame: 330, scale: 1.17, cx: 1008, cy: 618 }
        ]}
      >
        <Desktop>
          <NotesWindow x={400} y={140} title="Q3 planning">
            <TypeOn text="The report needs three things:" start={134} step={3} />
            <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 8 }}>
              <TypeOn text="•  Budget numbers" start={292} step={2} />
              <TypeOn text="•  Hiring plan" start={300} step={2} />
              <TypeOn text="•  Launch date" start={308} step={2} />
            </div>
          </NotesWindow>
          <Pill
            appear
            seed={41}
            x={960}
            y={1078}
            timeline={[
              { frame: 8, state: "idle" },
              { frame: 16, state: "listening" },
              { frame: 114, state: "transcribing" },
              { frame: 130, state: "inserted" },
              { frame: 158, state: "idle" },
              { frame: 172, state: "listening" },
              { frame: 272, state: "transcribing" },
              { frame: 288, state: "inserted" },
              { frame: 318, state: "idle" }
            ]}
          />
          {/* Live-speech previews float in the empty note body where text will land. */}
          <GhostSpeech
            text="um so basically the uh — the report needs three things"
            start={22}
            end={110}
            x={990}
            y={470}
            step={9}
            maxWidth={800}
          />
          <GhostSpeech
            text="bullet point budget numbers, bullet point hiring plan, bullet point launch date"
            start={178}
            end={266}
            x={990}
            y={585}
            step={7}
            maxWidth={800}
          />
        </Desktop>
      </Camera>
      <Caption from={34} to={128} size={44} y={874}>
        Whisper-grade accuracy.
      </Caption>
      <Caption from={140} to={218} size={44} y={874}>
        Cleaned up before it lands.
      </Caption>
      <Caption from={252} to={322} size={44} y={874}>
        Formatting, by voice.
      </Caption>
    </AbsoluteFill>
  );
};
