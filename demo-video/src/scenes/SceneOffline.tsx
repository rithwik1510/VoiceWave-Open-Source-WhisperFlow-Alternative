import React from "react";
import { AbsoluteFill, useCurrentFrame } from "remotion";
import { Camera } from "../components/Camera";
import { Caption } from "../components/Caption";
import { Cursor } from "../components/Cursor";
import { Desktop, WIFI_POS } from "../components/Desktop";
import { GhostSpeech } from "../components/GhostSpeech";
import { Pill } from "../components/Pill";
import { TypeOn } from "../components/TypeOn";
import { NotesWindow } from "../components/windows/NotesWindow";
import { ink } from "../theme";

export const OFFLINE_DUR = 300;
export const WIFI_OFF_AT = 78;

/**
 * Scene 5 (30-40s): the shot competitors can't film. Wi-Fi goes off,
 * dictation keeps working. Music drops with it.
 */
export const SceneOffline: React.FC = () => {
  const frame = useCurrentFrame();
  const wifiOn = frame < WIFI_OFF_AT;

  return (
    <AbsoluteFill>
      <Camera
        keys={[
          { frame: 0, scale: 1.02, cx: 960, cy: 540 },
          { frame: 38, scale: 1.02, cx: 960, cy: 540 },
          { frame: 74, scale: 2.7, cx: WIFI_POS.x, cy: WIFI_POS.y - 26 },
          { frame: 112, scale: 2.7, cx: WIFI_POS.x, cy: WIFI_POS.y - 26 },
          { frame: 150, scale: 1.14, cx: 1000, cy: 610 },
          { frame: 300, scale: 1.18, cx: 1005, cy: 620 }
        ]}
      >
        <Desktop wifiOn={wifiOn}>
          <NotesWindow x={400} y={140} title="Q3 planning">
            {/* Carried over from the previous scene, already on the page. */}
            <span>The report needs three things:</span>
            <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 8 }}>
              <span>•  Budget numbers</span>
              <span>•  Hiring plan</span>
              <span>•  Launch date</span>
            </div>
            <div style={{ marginTop: 14 }}>
              <TypeOn text="Nothing I say ever leaves this laptop." start={252} step={2} />
            </div>
          </NotesWindow>
          <Pill
            seed={53}
            x={960}
            y={1078}
            timeline={[
              { frame: 0, state: "idle" },
              { frame: 152, state: "listening" },
              { frame: 238, state: "transcribing" },
              { frame: 252, state: "inserted" },
              { frame: 282, state: "idle" }
            ]}
          />
          <GhostSpeech
            text="and the best part — nothing I say ever leaves this laptop"
            start={160}
            end={234}
            x={990}
            y={700}
            step={7}
            maxWidth={800}
          />
          <Cursor
            keys={[
              { frame: 14, x: 1150, y: 640 },
              { frame: 66, x: WIFI_POS.x + 4, y: WIFI_POS.y + 4 },
              { frame: 120, x: WIFI_POS.x + 4, y: WIFI_POS.y + 4 },
              { frame: 160, x: 1500, y: 820 }
            ]}
            clickAt={WIFI_OFF_AT}
            to={170}
          />
        </Desktop>
      </Camera>
      <Caption from={84} to={132} size={46} color={ink} y={860}>
        No cloud.
      </Caption>
      <Caption from={140} to={186} size={46} color={ink} y={860}>
        No account.
      </Caption>
      <Caption from={194} to={288} size={46} color={ink} y={874}>
        Nothing ever leaves your machine.
      </Caption>
    </AbsoluteFill>
  );
};
