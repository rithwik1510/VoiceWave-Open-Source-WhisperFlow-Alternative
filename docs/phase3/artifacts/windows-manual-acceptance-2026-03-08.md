# Windows Manual Acceptance - Rescue Cycle

Date: 2026-03-08  
Scope: VoiceWave rescue baseline validation on target Windows machine

## How To Mark Results

1. For each check row, mark exactly one option as checked (`[x]`) and keep the other option unchecked (`[ ]`).
2. Keep unchecked option as `[ ]` so readiness scripts can parse pass markers cleanly.

## Automated Preconditions

1. `npm run test -- --run` -> pass
2. `npm run build` -> pass
3. `npm run phase3:validate` -> pass

## Runtime Smoke

1. `npm run tauri:dev` launches successfully: [x] pass / [ ] fail
2. Models panel loads and shows catalog rows: [x] pass / [ ] fail
3. Install `tiny.en` succeeds: [x] pass / [ ] fail
4. Install `small.en` succeeds: [x] pass / [ ] fail
5. Switching active model between `tiny.en` and `small.en` succeeds: [x] pass / [ ] fail

## Dictation Acceptance (Core 3 Targets)

1. Notepad short dictation usable (quality + insertion): [x] pass / [ ] fail
2. Notepad medium dictation usable (quality + insertion): [x] pass / [ ] fail
3. VS Code short dictation usable (quality + insertion): [x] pass / [ ] fail
4. VS Code medium dictation usable (quality + insertion): [x] pass / [ ] fail
5. Browser text field/editor short dictation usable: [x] pass / [ ] fail
6. Browser text field/editor medium dictation usable: [x] pass / [ ] fail

## Quality and Safety Checks

1. No inserted transcript contains `[BLANK_AUDIO]`-style artifacts: [x] pass / [ ] fail
2. Low-quality microphone warning appears when applicable: [x] pass / [ ] fail
3. Warning recovery action can switch input device: [x] pass / [ ] fail
4. Warning recovery action can reset VAD to recommended: [x] pass / [ ] fail
5. Cancel/stop remains responsive during live dictation: [x] pass / [ ] fail
