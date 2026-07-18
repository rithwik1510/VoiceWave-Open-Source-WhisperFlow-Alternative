import React from "react";
import { AbsoluteFill } from "remotion";
import { Camera } from "../components/Camera";
import { Caption } from "../components/Caption";
import { Desktop } from "../components/Desktop";
import { GhostSpeech } from "../components/GhostSpeech";
import { Pill } from "../components/Pill";
import { Caret, TypeOn } from "../components/TypeOn";
import { MailWindow } from "../components/windows/MailWindow";

export const HOOK_MAGIC_DUR = 300;

const BODY = "Quick update before standup — the new build went out this morning. Early numbers look strong, full report by Friday.";

/**
 * Scenes 1+2 (0-10s): the lonely cursor, then the magic moment. One
 * continuous camera move through the caption swap.
 */
export const SceneHookMagic: React.FC = () => {
  return (
    <AbsoluteFill>
      <Camera
        keys={[
          { frame: 0, scale: 1.12, cx: 960, cy: 505 },
          { frame: 115, scale: 1.15, cx: 960, cy: 540 },
          { frame: 138, scale: 1.17, cx: 960, cy: 618 },
          { frame: 255, scale: 1.17, cx: 960, cy: 618 },
          { frame: 300, scale: 1.3, cx: 930, cy: 560 }
        ]}
      >
        <Desktop>
          <MailWindow x={430} y={150}>
            <TypeOn text={BODY} start={255} step={2} caret={false} />
            <Caret height={26} />
          </MailWindow>
          <Pill
            appear
            seed={3}
            x={960}
            y={1078}
            timeline={[
              { frame: 130, state: "idle" },
              { frame: 142, state: "listening" },
              { frame: 235, state: "transcribing" },
              { frame: 253, state: "inserted" },
              { frame: 282, state: "idle" }
            ]}
          />
          {/* Live-speech preview floats where the text will land. */}
          <GhostSpeech
            text="Quick update before standup — the new build went out this morning."
            start={150}
            end={228}
            x={940}
            y={545}
            maxWidth={880}
          />
        </Desktop>
      </Camera>
      {/* Captions live outside the camera so they never scale. */}
      <Caption from={10} to={106} size={48}>
        You type 40 words a minute.
      </Caption>
      <Caption from={118} to={290} size={48} y={874}>
        You speak 130.
      </Caption>
    </AbsoluteFill>
  );
};
