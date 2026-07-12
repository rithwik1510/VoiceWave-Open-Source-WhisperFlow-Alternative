# Plan 008: Launch demo video — "Your voice, on-device"

Planned 2026-07-06, build started 2026-07-07. A 60-second hero demo video for
the website / GitHub / social, built in **Remotion** (`demo-video/`).

## Why Remotion

The app is React with a complete design system. The video renders the actual
design language (Quiet Ink + Signal Blue tokens, Fraunces/Manrope, the
revolving ring, a pixel-faithful pill replica ported from `pill.css` +
`FloatingPill.tsx`) and drives it with frame-perfect motion — spring camera
pushes, beat-synced cuts, count-ups — that a screen recorder can't produce.
Real app assets used directly: `icon.png`, the actual dictation cue sounds
(`cue_open.wav` / `cue_close.wav`).

## Approved decisions (2026-07-07)

- Music + captions, no voiceover. Score + SFX are **synthesized
  programmatically** (`scripts/make-audio.mjs`, pure-math DSP → WAVs) so the
  soundtrack is deterministic and licensing-clean. 96 BPM minimal
  ambient-pulse arranged to the storyboard, with a full drop at the Wi-Fi-off
  moment.
- Fake app lookalikes, but polished: email compose, dark code editor,
  terminal, notes app (user asked for Terminal / VS Code / Notes styles).
  Windows-11-style chrome + taskbar with a live system tray.
- Stats: aspirational-but-plausible power-user numbers, internally consistent
  (72,481 words · 136 WPM · best 191 · 2,347 dictations · 63 days · 21h 19m
  saved = 72,481/40wpm − 533min speaking). Constants in
  `src/components/StatsPanel.tsx` (`STATS`).
- No full render until approved; review happens in Remotion Studio
  (`npm run studio` in `demo-video/`).

## Storyboard (1800 frames @ 30fps, 1920×1080)

| frames | scene | beat |
|---|---|---|
| 0–120 | S1 Hook | lone caret in email, "You type 40 words a minute." |
| 120–300 | S2 Magic | "You speak 130." — pill pops, waveform, ghost speech, transcribe, words cascade in |
| 300–570 | S3 Montage | 3 beat cuts: code review comment → terminal commit → note. "Dictate into any app on Windows." |
| 570–900 | S4 Quality | ghost "um so basically the uh—" → clean text lands; spoken "bullet point" builds a live list |
| 900–1200 | S5 Offline | camera zooms to tray, Wi-Fi clicked OFF, music drops, dictation keeps working. "No cloud. No account. Nothing ever leaves your machine." |
| 1200–1500 | S6 Stats | real Stats-tab layout, count-ups (riser SFX peaks at frame 1200). "Watch the hours come back." |
| 1500–1710 | S7 Offer | "Free. Open source. Yours." + GitHub chip + "This script was dictated with VoiceWave." |
| 1710–1800 | S8 Close | icon in revolving ring, wordmark, gradient CTA "Download for Windows" |

Each scene is also registered as its own composition (S1…S8) for quick
navigation in Studio.

## Status

- Build: complete pending review (all scenes, camera system, pill replica,
  synthesized score + SFX, captions).
- Review: maintainer to inspect in Remotion Studio and direct changes.
- Final MP4 render: NOT yet done (deliberately — awaiting approval).
- Future: 15s/30s social cuts can reuse the same compositions.
