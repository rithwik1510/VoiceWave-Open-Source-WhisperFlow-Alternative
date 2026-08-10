# Plan 014 — Complete Dark Mode ("Quiet Ink, Night")

Branch: `dark-mode-v1.2`. Status: IMPLEMENTED 2026-08-10 (all 3 phases +
Fable review pass; review fixes: active heatmap range pill gained an inset
`accent-rule` ring for dark-mode edge definition, and dark
`--vw-color-text-hint` was raised `#7d7d86`→`#84848e` to clear 4.5:1 AA on
cards — every other audited pair passed). Gates: tsc clean, vitest 74/74,
cargo 344 lib + integration all green, `vite build` succeeds.

Smoke-test finding (2026-08-10): the shipped app shell and Home screen live in
`src/prototype/` (Layout.tsx, Dashboard.tsx, constants.ts `ONYX_PALETTE`) —
NOT in components/ — and were missed by every migration scope, leaving a
half-light shell with unreadable dark-gray-on-dark card text. Fixed: the
ONYX_PALETTE class strings and both components now use semantic tokens, and
three tokens were added for the Harmonic shell — `--vw-color-shell`
(`#efeff3`/`#060609`), `--vw-color-canvas` (`#ffffff`/`#101014`),
`--vw-recording-fill` (`#000000`/`#2a2a31`). Dark elevation order is
shell < page < canvas < surface < inset. Deliberately literal: the recording
waveform panel (black + white bars, both themes) and the brand accent
hexes/gradients in constants.ts. Visual pass in both themes still pending.

## Product intent

A first-class dark theme for the entire main window, at the fidelity of
Linear/Vercel/Raycast dark modes. The "Quiet Ink + Signal Blue" system is
preserved exactly: quiet zinc neutrals, near-monochrome chrome, and the
VoiceWave blue family as the ONLY brand color — gradient CTAs, blue accents,
and the dictation-state palette stay blue in both themes. Dark mode inverts
the paper, never the brand.

Non-goals: the website is untouched. The floating pill window
(`pill.css` / `FloatingPill.tsx`) is **theme-invariant by design** — it is
already a dark translucent capsule that floats over arbitrary desktop
content; do not add theming to it.

## Architecture

1. **Theme attribute.** `document.documentElement.dataset.theme` is `"light"`
   or `"dark"` (always the *resolved* theme, never `"system"`). All dark
   styling hangs off `:root[data-theme="dark"]` redefining the existing
   `--vw-*` custom properties in `src/styles.css`. No component may branch on
   theme in JS except where a canvas/inline-SVG color cannot come from a CSS
   variable.
2. **Semantic Tailwind tokens.** Extend `tailwind.config.ts` with semantic
   colors mapped to CSS variables so TSX uses `text-ink`, `text-sub`,
   `text-faint`, `bg-surface`, `bg-page`, `bg-inset`, `border-edge`,
   `border-edge-strong`, `divide-hairline`, `text-accent`, etc. Every
   arbitrary-value class (`text-[#09090B]`, `bg-white`, …) in the main-window
   TSX migrates to these. Zero raw neutral hexes may remain in TSX after
   migration (status/amber/red hexes become status tokens, below).
3. **Setting.** `theme: "light" | "dark" | "system"` added to
   `VoiceWaveSettings` (Rust `settings/mod.rs` with
   `#[serde(default)]` → `system`; TS `types/voicewave.ts`). UI: a
   three-option segmented control ("Light / Dark / System") in Settings →
   General, styled like existing segmented controls.
4. **Resolution + no-flash boot.** A tiny module (`src/lib/theme.ts`)
   resolves `system` via `matchMedia("(prefers-color-scheme: dark)")`,
   applies `data-theme`, sets CSS `color-scheme` (`light`/`dark`) so native
   scrollbars/inputs adapt, and subscribes to OS changes while in `system`.
   The last resolved theme is cached in `localStorage("vw-theme")` and applied
   by an inline `<script>` in `index.html` *before* paint; after
   `load_settings` resolves, the real setting reconciles it.
5. **Cross-window/live update.** On settings save the main window re-resolves
   immediately. (Pill excluded per non-goals.)

## Dark token palette (normative)

Values below are the spec. ±1 shade drift is acceptable ONLY if needed to
hit contrast (WCAG AA: 4.5:1 body text, 3:1 large text/UI borders on their
actual background); record any deviation in this file.

| Token | Light (existing) | Dark |
|---|---|---|
| `:root` color / background | `#09090b` / `#fafafa` | `#f4f4f5` / `#0b0b0e` |
| `--vw-ink` | `#18181b` | `#e4e4e7` |
| `--vw-ink-strong` | `#09090b` | `#fafafa` |
| `--vw-ink-hover` | `#27272a` | `#d4d4d8` |
| `--vw-color-text-primary` | `#09090b` | `#f4f4f5` |
| `--vw-color-text-secondary` | `#475569` | `#a3adbb` |
| `--vw-color-text-muted` | `#71717a` | `#8b8b94` |
| `--vw-color-surface` | `#ffffff` | `#17171b` |
| `--vw-color-surface-soft` (page) | `#fafafa` | `#0b0b0e` |
| `--vw-color-surface-subtle` (insets/hover) | `#f4f4f5` | `#212127` |
| `--vw-color-border` | `#e4e4e7` | `#26262c` |
| `--vw-color-border-strong` | `#d4d4d8` | `#34343c` |
| `--vw-color-divider` | `#f1f1f3` | `#1e1e24` |
| `--vw-shadow-card` | `0 1px 2px rgba(9,9,11,0.04)` | `0 1px 2px rgba(0,0,0,0.45)` |
| `--vw-shadow-float` | `0 12px 32px rgba(9,9,11,0.12)` | `0 16px 40px rgba(0,0,0,0.6)` |

Elevation rule in dark: page is darkest, cards are lighter than the page,
hover/inset fills are lighter still. Never rely on shadows alone in dark —
cards keep their 1px border.

**Ink inversion.** In light mode "ink" (near-black) is the action color:
primary buttons, active nav, emphasized text. In dark it inverts to
near-white paper: primary ink buttons become `#e4e4e7` fill with `#09090b`
text, hover `#f4f4f5`. The `--vw-ink*` mapping above achieves this wherever
components already use the tokens; migrated components must use the tokens.

**Blue family (unchanged hues, adjusted deployment).**
`--vw-accent-*`, `--vw-cta-gradient*`, `--vw-ai-gradient`, `--vw-glow-blue`
keep their light values in dark — gradient CTAs are self-contained colored
surfaces with white text and read beautifully on dark. Two adjustments:
- `--vw-accent-soft-gradient`: raise alphas ~1.3× (0.13/0.21/0.26/0.23) so
  the wash stays visible on dark surfaces.
- Anywhere `#0032b8` / `#0a2a8c` is used as *text or icon color on a neutral
  surface*, dark must substitute a readable blue: add
  `--vw-accent-text: #0032b8` (light) → `#5cb3ff` (dark) and use it.

**Dictation-state palette (dark).** Same semantic ramp, tuned for dark:

| State | border | accent | accent-soft |
|---|---|---|---|
| idle | `#2e323a` | `#8b98a8` | `#454e5c` |
| listening | `#2f9dff` | `#4db2ff` | `#1d5c96` |
| transcribing | `#4da9ff` | `#8fceff` | `#23528f` |
| inserted | `#4f84e8` | `#bcd3ff` | `#2d69dd` |
| error | `#8b3a3a` | `#f87171` | `#b45309` |

**Status tokens (new, both themes).** The scattered amber/red banner hexes
(`#92400E`, `#FFFBEB`, `#FDE68A`, `#fff1f1`, `#f3c2c2`, `#a94444`,
`#B3261E`, `#fef2f2`, …) become tokens:

| Token | Light | Dark |
|---|---|---|
| `--vw-status-warn-text` | `#92400e` | `#fbbf24` |
| `--vw-status-warn-bg` | `#fffbeb` | `rgba(146, 64, 14, 0.16)` |
| `--vw-status-warn-border` | `#fde68a` | `#78350f` |
| `--vw-status-danger-text` | `#b3261e` | `#f87171` |
| `--vw-status-danger-bg` | `#fff1f1` | `rgba(239, 68, 68, 0.10)` |
| `--vw-status-danger-border` | `#f3c2c2` | `#7f1d1d` |
| `--vw-status-info-bg` | `#eff6ff` | `rgba(27, 142, 255, 0.10)` |

**Streak heatmap (dark ramp).** Empty cell `#1e242e`; filled ramp runs
low→high `#173157 → #1e4f96 → #1b8eff → #7ed8ff` (same brand ramp, dark
floor); today ring and hover states must hit 3:1 against the page.

### Phase 1 addenda (implemented 2026-08-10)

No spec value was changed. The `styles.css` migration needed a handful of
tokens the spec did not name, because several component classes used neutrals
with no existing token and light mode had to stay byte-identical:

| Added token | Light | Dark | Role |
|---|---|---|---|
| `--vw-ink-contrast` | `#fafafa` | `#09090b` | text/marks ON an `--vw-ink` fill (inverts with the ink) |
| `--vw-on-accent` | `#ffffff` | `#ffffff` | text/marks ON a brand-blue fill (never inverts) |
| `--vw-color-text-label` | `#3f3f46` | `#d4d4d8` | badge labels |
| `--vw-color-text-quiet` | `#52525b` | `#a1a1aa` | inactive segmented/tab/rail labels, chips |
| `--vw-color-surface-hover` | `#f8f8f8` | `#22222a` | secondary-button / quick-action hover |
| `--vw-color-row-hover` | `#fcfcfd` | `#1c1c22` | `.vw-interactive-row` hover |
| `--vw-track-quiet` | `#e8e8ee` | `#26262c` | progress/gauge track |
| `--vw-scrim` / `--vw-scrim-strong` | `rgba(9,9,11,.32)` / `.45` | `rgba(0,0,0,.6)` / `.72` | modal + model-gate backdrops |
| `--vw-ring-shell-bg` / `--vw-ring-inner-bg` | existing values | blue-tinted / `rgba(23,23,27,.985)` | brand glow ring |
| `--vw-cta-gradient-active` | existing `:active` gradient | same | brand |
| `--vw-accent-blue-300` | `#9cc9ff` | same | brand (onboarding done-dot) |
| `--vw-onb-canvas` | `#eff0f4` | `#0a0a0d` | onboarding backdrop |
| `--vw-onb-dot-idle` | `#d9d9e0` | `#3a3a44` | onboarding step dot |
| `--vw-onb-key-border` | `#e0e0e6` | `#34343c` | rehearsal key |
| `--vw-onb-dock-bg` / `--vw-onb-dock-track` | `rgba(255,255,255,.9)` / `#e4eaf5` | `rgba(23,23,27,.9)` / `#1d2a3d` | docked download chip |

Phase 2b (components) added two more, for the same reason — a role existed in
the TSX with no token behind it:

| Added token | Light | Dark | Role |
|---|---|---|---|
| `--vw-color-text-hint` | `#a1a1aa` | `#7d7d86` | weakest text tier (unit suffixes, axis/legend labels, footnotes) — one step below `--vw-color-text-muted`; dark value keeps 4.8:1 on `--vw-color-surface` |
| `--vw-status-success` | `#16a34a` | `#4ade80` | success dot / mark ("You're up to date") |

Phase 2a (`App.tsx`) added four more, same reason:

| Added token | Light | Dark | Role |
|---|---|---|---|
| `--vw-accent-rule` | `rgba(27,142,255,.45)` | `rgba(27,142,255,.55)` | brand-blue hairline rule / quote bar on a neutral surface (was `border-[#1B8EFF]/45`; an opacity modifier can't be applied to a `var()` color in Tailwind 3) |
| `--vw-status-warn-text-soft` | `#a16207` | `#d9a441` | secondary detail line inside a warn banner, one step under `--vw-status-warn-text` |
| `--vw-status-info-border` | `#bfdbfe` | `#1e3a8a` | border for info-tinted panels (`--vw-status-info-bg` had no border partner) |
| `--vw-status-success-text` | `#15803d` | `#4ade80` | success as *body text* (needs 4.5:1 in light, which `--vw-status-success` `#16a34a` misses at 3.1:1); `--vw-status-success` stays the dot/mark colour |

Phase 2a Tailwind aliases added on top of Phase 2b's: `accent-rule`,
`state-error` (`--vw-state-error-accent`), `status-success-text`,
`status-warn-text-soft`, `status-info-border`.

Light-mode value shifts in Phase 2a, all sub-perceptual and all sanctioned by
the spec's "these hexes become tokens" rule:

- Danger banners `#a94444` → `--vw-status-danger-text` (`#b3261e`); the auth
  error card `#9B2C2C`/`#FFF5F5`/`#FED7D7` and the snippet sync error
  `#991B1B` collapse into the same danger triplet.
- Auth info card `#1E40AF` → `--vw-accent-text` (`#0032b8`), per the
  accent-text rule; its `#F5FAFF` sibling panel folds into
  `--vw-status-info-bg` (`#eff6ff`).
- History copy-confirm check `#16A34A` → `--vw-status-success` (unchanged) and
  the model status dot `#10B981` → the same token.
- Model-gate subtitle `#64748B` → `--vw-color-text-muted` (`#71717a`), which is
  the token every other subtitle in the file already used.
- Inset panels inside cards/modals (`#fafafa`) → `--vw-color-surface-subtle`
  (`#f4f4f5`), matching the Phase 2b decision for the same role.

Phase 2b also added Tailwind aliases for CSS variables that already existed:
`ink-hover`, `ink-contrast`, `label`, `quiet`, `hint`, `track`, `accent`
(`--vw-accent-blue-600`), `accent-deep` (`--vw-accent-navy-900`), `on-accent`.

Light-mode value shifts in Phase 2b, all sanctioned by the spec's "these
hexes become tokens" rule and all sub-perceptual:

- Stats month-delta chip `#e8f4ff`/`#0a2a8c` → `--vw-status-info-bg` (`#eff6ff`)
  / `--vw-accent-text` (`#0032b8`), per the accent-text rule.
- Inset tiles inside white cards (`#fafafa`) → `--vw-color-surface-subtle`
  (`#f4f4f5`). `bg-page` would have been the exact light match but inverts the
  elevation rule in dark (an inset would sit *below* its card), so the role
  wins over the hex.
- Onboarding mic-volume hint `#b45309` → `--vw-status-warn-text` (`#92400e`).
- Update-progress track `#e4e4e7` → `--vw-track-quiet` (`#e8e8ee`).
- Heatmap tooltip fill `#09090b` → `--vw-ink` (`#18181b`), so it inverts to a
  light chip in dark instead of vanishing into the page.

Two intentional light-mode value shifts (both sanctioned by the spec's own
"these hexes become tokens" list, both imperceptible):

- `.vw-btn-danger:hover` `#fef2f2` → `--vw-status-danger-bg` (`#fff1f1`).
- `.vw-onb-dock` text `#0a2a8c` → `--vw-accent-text` (`#0032b8`), per the
  accent-text rule.

`--vw-heat-*` light values are lifted verbatim from `StreakHeatmap.tsx`
(`#e8e8ee`, then `rgba(27,142,255,·)` at 0.25/0.45/0.70, then `#1b8eff`), so
that component is a pure token swap in Phase 2.

Settings placement: there was no "General" section in the settings rail, so
one was added (Palette icon, first item) and it is now the landing section;
Appearance lives there.

## Execution

- Phase 1 (foundation): styles.css dark block + tailwind semantic mapping +
  theme.ts + Rust/TS setting + Settings UI control + boot script.
- Phase 2 (parallel): App.tsx migration; components/* migration
  (StatsSection, StreakHeatmap, Onboarding, UpdatePrompt, UpdateSection,
  Sidebar, StatePill).
- Phase 3: Fable review — full diff, contrast audit, `npm test`,
  `cargo test`, `npm run build`.

Every existing test stays green; light mode must be pixel-identical after
migration (tokens resolve to the same values).
