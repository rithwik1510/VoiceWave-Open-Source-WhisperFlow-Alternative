import React from "react";
import { Composition } from "remotion";
import { loadFont as loadFraunces } from "@remotion/google-fonts/Fraunces";
import { loadFont as loadManrope } from "@remotion/google-fonts/Manrope";
import { loadFont as loadMono } from "@remotion/google-fonts/JetBrainsMono";
import { Main, MAIN_DUR } from "./Main";
import {
  SHORT_DUR,
  SHORT_H,
  SHORT_RACE_DUR,
  SHORT_SS,
  SHORT_W,
  Short,
  ShortFrame,
  ShortRace
} from "./Short";
import { SS, SuperSample } from "./components/SuperSample";
import { SceneClose, CLOSE_DUR } from "./scenes/SceneClose";
import { SceneHookMagic, HOOK_MAGIC_DUR } from "./scenes/SceneHookMagic";
import { SceneMontage, MONTAGE_DUR } from "./scenes/SceneMontage";
import { SceneOffer, OFFER_DUR } from "./scenes/SceneOffer";
import { SceneOffline, OFFLINE_DUR } from "./scenes/SceneOffline";
import { SceneQuality, QUALITY_DUR } from "./scenes/SceneQuality";
import { SceneStats, STATS_DUR } from "./scenes/SceneStats";
import { FPS, VIDEO_H, VIDEO_W } from "./theme";

loadFraunces();
loadManrope();
loadMono();

// Compositions rasterize at 2x (see SuperSample) so camera moves can't make
// text shimmer; scenes still lay out in 1920x1080 coordinates.
const size = { width: VIDEO_W * SS, height: VIDEO_H * SS, fps: FPS };

const wrap = (C: React.FC): React.FC => {
  const Wrapped: React.FC = () => (
    <SuperSample>
      <C />
    </SuperSample>
  );
  return Wrapped;
};

const wrapShort = (C: React.FC): React.FC => {
  const Wrapped: React.FC = () => (
    <ShortFrame>
      <C />
    </ShortFrame>
  );
  return Wrapped;
};

const MainSS = wrap(Main);
const ShortMain = wrapShort(Short);
const ShortRaceSS = wrapShort(ShortRace);
const S1 = wrap(SceneHookMagic);
const S3 = wrap(SceneMontage);
const S4 = wrap(SceneQuality);
const S5 = wrap(SceneOffline);
const S6 = wrap(SceneStats);
const S7 = wrap(SceneOffer);
const S8 = wrap(SceneClose);

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Composition id="Main" component={MainSS} durationInFrames={MAIN_DUR} {...size} />
      <Composition id="S1-HookMagic" component={S1} durationInFrames={HOOK_MAGIC_DUR} {...size} />
      <Composition id="S3-Montage" component={S3} durationInFrames={MONTAGE_DUR} {...size} />
      <Composition id="S4-Quality" component={S4} durationInFrames={QUALITY_DUR} {...size} />
      <Composition id="S5-Offline" component={S5} durationInFrames={OFFLINE_DUR} {...size} />
      <Composition id="S6-Stats" component={S6} durationInFrames={STATS_DUR} {...size} />
      <Composition id="S7-Offer" component={S7} durationInFrames={OFFER_DUR} {...size} />
      <Composition id="S8-Close" component={S8} durationInFrames={CLOSE_DUR} {...size} />
      <Composition
        id="Short"
        component={ShortMain}
        durationInFrames={SHORT_DUR}
        width={SHORT_W * SHORT_SS}
        height={SHORT_H * SHORT_SS}
        fps={FPS}
      />
      <Composition
        id="Short-Race"
        component={ShortRaceSS}
        durationInFrames={SHORT_RACE_DUR}
        width={SHORT_W * SHORT_SS}
        height={SHORT_H * SHORT_SS}
        fps={FPS}
      />
    </>
  );
};
