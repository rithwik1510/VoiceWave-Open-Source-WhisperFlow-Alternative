import React from "react";
import { AbsoluteFill, Audio, Sequence, staticFile } from "remotion";
import { Dissolve } from "./components/Transitions";
import { CLOSE_DUR, SceneClose } from "./scenes/SceneClose";
import { HOOK_MAGIC_DUR, SceneHookMagic } from "./scenes/SceneHookMagic";
import { MONTAGE_DUR, SceneMontage } from "./scenes/SceneMontage";
import { OFFER_DUR, SceneOffer } from "./scenes/SceneOffer";
import { OFFLINE_DUR, SceneOffline } from "./scenes/SceneOffline";
import { QUALITY_DUR, SceneQuality } from "./scenes/SceneQuality";
import { STATS_DUR, SceneStats } from "./scenes/SceneStats";

export const MAIN_DUR =
  HOOK_MAGIC_DUR + MONTAGE_DUR + QUALITY_DUR + OFFLINE_DUR + STATS_DUR + OFFER_DUR + CLOSE_DUR; // 1650 (55s)

const AT = {
  montage: HOOK_MAGIC_DUR, // 300
  quality: HOOK_MAGIC_DUR + MONTAGE_DUR, // 570
  offline: HOOK_MAGIC_DUR + MONTAGE_DUR + QUALITY_DUR, // 900
  stats: HOOK_MAGIC_DUR + MONTAGE_DUR + QUALITY_DUR + OFFLINE_DUR, // 1200
  offer: HOOK_MAGIC_DUR + MONTAGE_DUR + QUALITY_DUR + OFFLINE_DUR + STATS_DUR, // 1410
  close: HOOK_MAGIC_DUR + MONTAGE_DUR + QUALITY_DUR + OFFLINE_DUR + STATS_DUR + OFFER_DUR // 1560
};

/** Frames the outgoing scene keeps holding under the incoming dissolve. */
const OVERLAP = 14;

const Sfx: React.FC<{ src: string; at: number; volume?: number; rate?: number }> = ({
  src,
  at,
  volume = 0.5,
  rate = 1
}) => (
  <Sequence from={at}>
    <Audio src={staticFile(`audio/${src}`)} volume={volume} playbackRate={rate} />
  </Sequence>
);

export const Main: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: "#FAFAFA" }}>
      {/* Later siblings stack on top, so each scene dissolves in over the
          previous scene's extended hold. */}
      <Sequence from={0} durationInFrames={HOOK_MAGIC_DUR + OVERLAP}>
        <SceneHookMagic />
      </Sequence>
      <Sequence from={AT.montage} durationInFrames={MONTAGE_DUR + OVERLAP}>
        <Dissolve>
          <SceneMontage />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.quality} durationInFrames={QUALITY_DUR + OVERLAP}>
        <Dissolve>
          <SceneQuality />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.offline} durationInFrames={OFFLINE_DUR + OVERLAP}>
        <Dissolve>
          <SceneOffline />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.stats} durationInFrames={STATS_DUR + OVERLAP}>
        <Dissolve>
          <SceneStats />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.offer} durationInFrames={OFFER_DUR + OVERLAP}>
        <Dissolve>
          <SceneOffer />
        </Dissolve>
      </Sequence>
      <Sequence from={AT.close} durationInFrames={CLOSE_DUR}>
        <Dissolve>
          <SceneClose />
        </Dissolve>
      </Sequence>

      {/* Score */}
      <Audio src={staticFile("audio/music.wav")} volume={1} />

      {/* Scene 1+2: the magic moment. */}
      <Sfx src="pop.wav" at={130} volume={0.55} />
      <Sfx src="cue_open.wav" at={142} volume={0.5} />
      <Sfx src="cue_close.wav" at={235} volume={0.5} />

      {/* Quality: two dictations. */}
      <Sfx src="cue_open.wav" at={AT.quality + 16} volume={0.42} />
      <Sfx src="cue_close.wav" at={AT.quality + 114} volume={0.42} />
      <Sfx src="cue_open.wav" at={AT.quality + 172} volume={0.42} />
      <Sfx src="cue_close.wav" at={AT.quality + 272} volume={0.42} />

      {/* Offline: the click heard round the world. */}
      <Sfx src="click.wav" at={AT.offline + 78} volume={0.8} />
      <Sfx src="thud.wav" at={AT.offline + 80} volume={0.65} />
      <Sfx src="cue_open.wav" at={AT.offline + 152} volume={0.42} />
      <Sfx src="cue_close.wav" at={AT.offline + 238} volume={0.42} />

      {/* Stats: riser peaks exactly as the numbers start counting. */}
      <Sfx src="riser.wav" at={AT.stats - 42} volume={0.45} />
      <Sfx src="pop.wav" at={AT.close + 4} volume={0.4} />
    </AbsoluteFill>
  );
};
