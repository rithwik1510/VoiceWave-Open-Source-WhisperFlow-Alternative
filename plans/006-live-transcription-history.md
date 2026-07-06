# Plan 006: Live transcription history (dashboard + History page)

Planned 2026-07-06, executed same day. Maintainer report: "last 5 dictations
don't appear in the dashboard Today section; History doesn't exist." Both
symptoms are real but the storage layer was never the problem.

## Root causes (verified against code)

1. **Frontend never refreshes history after a dictation.** The backend records
   every mic dictation (`state.rs` `run_dictation_flow` → spawned
   `record_transcript`), but `refreshPhase3Data` (the only thing that loads
   `sessionHistory`) runs at app launch and after manual UI actions only. The
   dashboard TODAY section therefore shows launch-time state forever.
2. **History page is unreachable.** `App.tsx` had a leftover effect redirecting
   `activeNav === "sessions"` → `"home"`, so the fully-built History view below
   it was dead code. (Leftover from a redesign that temporarily removed the nav
   item; the item returned, the redirect didn't leave.)
3. **Only a 140-char preview is stored** — a long dictation can't be recovered
   from history, which kills the rescue use case.
4. **The maintainer's live store had `retentionPolicy: "off"`** (set ~2026-02-19;
   `nextId` 1540 shows ~1.5k records existed before that). Every dictation since
   was silently discarded, and the UI had no way to reveal it: there was a
   `set_history_retention` command but NO getter, so the frontend always showed
   its optimistic "days30" default regardless of the real policy.
5. **Every mic dictation would have produced TWO records** once visible:
   `insert_text` records via `record_insertion` AND `run_dictation_flow`'s
   spawned task records via `record_transcript`.

## Changes

Backend (`src-tauri/src/history/mod.rs`, `src-tauri/src/state.rs`):
- `SessionHistoryRecord.text`: full final transcript (`#[serde(default)]` so
  pre-existing encrypted stores parse; legacy records show preview only).
- `MAX_HISTORY_RECORDS = 200` ring buffer enforced on every append (newest
  kept), on top of the existing time-based retention (Off/7d/30d/Forever).
- Exports prefer full text, fall back to preview for legacy records.
- After the post-dictation history write, emit `voicewave://history-updated`
  to the main window so the UI refreshes without polling.
- New `get_history_retention` command (lib.rs) so the UI reflects the real
  stored policy at startup instead of assuming days30.
- Dedupe: the spawned `record_transcript` is now a rescue write only — it runs
  when `insert_text` errored before persisting, so one dictation = one record
  (with method/success metadata) and a failed insertion still keeps the words.

Frontend (`src/App.tsx`, `src/hooks/useVoiceWave.ts`, `src/types/voicewave.ts`):
- Remove the sessions→home redirect; History nav works again.
- Listen for `voicewave://history-updated` → history-only re-query (limit 50),
  not the heavyweight `refreshPhase3Data`.
- History rows get a Copy action (copies full text, preview fallback).
- "History is off" amber banner on the History page with a one-click
  "Keep 30 days" button; the dashboard TODAY empty state says WHY it's empty
  (history off vs. simply no dictations yet).
- Dashboard TODAY section otherwise unchanged visually — it updates live now.

## Not doing

- No new storage system (encrypted store + retention already existed).
- No cloud sync changes (cloud recent-sentences path untouched).
- No date filtering of the TODAY section (shows most recent 5 regardless of
  day, as before).
