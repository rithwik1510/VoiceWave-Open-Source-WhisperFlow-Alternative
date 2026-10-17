# Plan 020: Every Dictation Lands

## Status

**Implemented on branch `feat/lands-every-time` (2026-10-17); owner desktop
test pending.** Written and built in one pass against `main` at 89a6e00
(v0.7.0 + Phase A). The decisions below were made without the owner and are
marked as overridable. Nothing was run against a live desktop: no scripted
dictation, no synthetic keystrokes. The manual checklist at the end is the
real gate.

## Goal

Text from a finished dictation ends up where the user meant it, and when it
can't, the user is told the truth and keeps their words (or their audio).

Category research (Wispr Flow, Handy, Superwhisper, Dragon forums) puts "text
doesn't land" and "long dictations get cut off or lost" among the top daily
complaints. VoiceWave had five concrete gaps.

### Non-goals

- No "review before send" UI (see Ideas).
- No clipboard restore after paste. It was removed on purpose (stale pastes
  into slow targets, wrong text on a second Ctrl+V; `insertion/mod.rs:518-533`)
  and stays removed.
- No engine changes, no `state.rs` split.

## Evidence (main @ 89a6e00)

1. **Terminals only get the clipboard.** `direct_insert` and `clipboard_paste`
   both refuse when `foreground_is_terminal` is true
   (`insertion/mod.rs:641-650`, `:678-685`), so Windows Terminal, PowerShell,
   cmd, Git Bash (and Claude Code / Codex CLI inside them) always end in
   "Copied, press Ctrl+V". The `preferClipboardOnlyForTerminals` setting
   (`settings/mod.rs:198`) has no reader; it is dead.
2. **Failed inserts can look successful.** SendInput return counts *are*
   checked (`insertion/mod.rs:786`, `:861`). The hole is UIPI: when the
   foreground app runs at a higher integrity level (an app started as
   administrator), Windows drops injected input and, per the SendInput docs,
   neither the return value nor GetLastError says so. The ladder then reports
   Direct success. Separately, a clipboard-only result sets the HUD to
   `Inserted` (`state.rs:2487-2490`) even though nothing reached the app.
3. **A failed decode loses the dictation.** Decode errors and timeouts return
   `Err` (`state.rs:4416-4458`); the audio buffer is dropped with the flow and
   the pill shows a raw error (`state.rs:1954-1965`). There is nothing to retry.
4. **Hands-free gives up after 2 s.**
   `silence_timeout_ms = clamp(0.22 * max_capture, 700, 2000)`
   (`state.rs:3789-3790`) is both the hands-free trailing-silence end and the
   initial "start speaking" wait (`state.rs:3943`). Worse, the main hands-free
   entry (double-tap Ctrl+Win) only flips `control_mode` on the session
   (`state.rs:5347-5350`); capture options were fixed at press time, so a
   double-tapped session is hold-to-talk capture with a 2 s start window and
   the 120 s `maxUtteranceMs` cap (clamped 5-180 s, `state.rs:591-592`).
   Long audio then meets fixed deadlines: the faster-whisper worker waits a
   flat 75 s per request (`faster_whisper.rs:182`, `:649-653`) and the outer
   decode timeout tops out at 240 s (`state.rs:606-608`).
5. **No spoken submit.** Structural commands cover new line / paragraph /
   bullet only (`transcript/mod.rs:573-581`); there is no way to send a
   dictated prompt in a terminal or chat.

## Design

### 1. Terminal insertion that works

The engine, not the backend, now decides the terminal path (one place,
`InsertionEngine::insert_text`). The foreground process executable picks a
paste chord; the backend's terminal refusals are gone.

| Terminal family (exe) | Chord | Why |
|---|---|---|
| Windows Terminal (`windowsterminal.exe`, `openconsole.exe`) | Ctrl+Shift+V | WT binds Ctrl+V, Ctrl+Shift+V and Shift+Insert to paste by default. Ctrl+V is the binding users most often remove (so vim/readline get ^V); Ctrl+Shift+V has been a default since the first release and is rarely touched. |
| Classic console (`conhost.exe`, `cmd.exe`, `powershell.exe`, `pwsh.exe`) | Ctrl+V | Console windows report their client shell as the owning process. conhost pastes on Ctrl+V with "Ctrl key shortcuts" on (the Windows 10+ default) and PSReadLine binds Ctrl+V to its own paste. |
| mintty (Git Bash, MSYS2, Cygwin) | Shift+Insert | Ctrl+V reaches bash as `quoted-insert`; Ctrl+Shift+V is off by default. Shift+Insert is mintty's paste. |
| alacritty, wezterm, kitty, hyper, tabby, warp | Ctrl+Shift+V | Their default clipboard paste. Shift+Insert in alacritty/wezterm pastes the *selection* buffer, not the clipboard. |

Order for a recognized terminal: `ClipboardPaste(chord)` then `ClipboardOnly`.
Direct typing (KEYEVENTF_UNICODE) stays off for terminals; ConPTY translation
makes it unreliable. Insert is sent with KEYEVENTF_EXTENDEDKEY and real scan
codes, otherwise it reads as Numpad 0.

When the process can't be resolved and only the window title looks like a
terminal, the old behavior stays: clipboard only (we don't know which chord is
safe).

**Safety: dictated text must never run a command by itself.**

- Without bracketed paste, every line break in pasted text is an Enter.
  Bracketed paste only helps when the program inside the terminal has turned
  it on (DECSET 2004), and VoiceWave can't see that from outside: cmd.exe never
  does, PSReadLine depends on version, Claude Code and Codex do. Windows
  Terminal also pops a "multiple lines" confirmation when the app hasn't
  enabled it.
- So for terminals, `terminal_safe_text` collapses every run of control
  characters (CR, LF, tab, escape, NEL, U+2028/2029) into one space and trims
  trailing whitespace. Tabs matter (they trigger shell completion); escape
  matters (an embedded `ESC[201~` would end a bracketed paste early).
- Result: a terminal paste is always one line with nothing after it. The only
  way to submit is the explicit spoken command (section 2), which is a
  separate Enter keystroke sent on purpose.
- Cost: "new line" and multi-line code blocks become spaces in terminals. This
  is the conservative trade; see Decisions.

The clipboard-only fallback and its pill ("Copied to clipboard, press Ctrl+V")
are unchanged. The clipboard is not restored after paste.

### 2. Spoken submit

`split_trailing_submit_command` (transcript module) strips a trailing
**"press enter"** or **"hit enter"** and returns a submit flag. It runs on the
ASR text before formatting, only when the existing `spokenEditCommands`
toggle is on (default on), so every profile including Literal respects it.

The phrase counts only when it is:

- the whole utterance (dictate a prompt, read it, then say "press enter" on its
  own), or
- its own sentence: preceded by `.`, `!` or `?` as Whisper writes after a
  pause ("Fix the login test. Press enter.").

"To continue, press enter" and "then press enter" stay text. "Send it" was
rejected: "Looks good, send it." is a normal chat message.

After the text lands via Direct or ClipboardPaste, the flow waits briefly
(250 ms after a paste, because Windows Terminal reads the clipboard
asynchronously; 40 ms after typing), checks that the foreground window is the
one that received the text, and sends one Enter. If focus moved, it doesn't
press and the pill says so. If the text only reached the clipboard (elevated
app, paste failed), no Enter is sent and the pill adds "then Enter". The
security-dialog guard applies to Enter too. History records the text without
the command.

### 3. Honest failure detection

- Before any keystroke, the engine compares the foreground process's
  integrity level with VoiceWave's (`OpenProcessToken` + `TokenIntegrityLevel`;
  UIPI blocks input exactly when the target's level is higher). If the token
  can't be opened (access denied) while VoiceWave runs below High, that is
  treated as "higher" too. An elevated target goes straight to clipboard-only,
  and the result carries `blockedByElevation`.
- The pill then says: **"This app runs as administrator"** / "VoiceWave can't
  type into it, so your text is on the clipboard. Press Ctrl+V."
- SendInput return counts were already checked on every call; kept.
- A clipboard-only result now moves the HUD to Idle with its message instead
  of `Inserted`. Only text that reached the app shows as inserted.

### 4. Never lose a dictation

- After capture, the merged samples go into an in-memory slot
  (`retained_capture`, `Arc<Vec<f32>>`, never written to disk) together with
  the foreground window the user dictated into.
- The slot belongs to one session. When the session's flow returns `Ok`
  (text delivered, nothing heard, cancelled), the slot is cleared. When it
  returns `Err` (decode failed, timed out, worker died, empty result), the
  audio stays and the failure pill gets a **Try again** button. Starting the
  next dictation replaces or clears it. Memory cost is at most ~19 MB (5 min
  of 16 kHz f32).
- Try again calls `retry_last_dictation`, which starts a normal session that
  skips capture and goes straight to transcribing the kept audio, through the
  same pipeline (formatting, polish, snippets, insertion, history). Esc
  cancels it like any dictation. A failed retry keeps the audio and offers the
  button again.
- Clicking the pill can activate VoiceWave's own window. Before inserting, a
  retry refocuses the original window, but only when a VoiceWave window owns
  the foreground; if the user deliberately clicked into another app, the text
  goes there.
- The retry notice stays up to 30 s (other notices 12 s) and hover pauses it.

### 5. Hands-free that waits while you think

- Each session carries a shared `hands_free` flag. It is set at start for the
  hands-free shortcut and the pill's Start hands-free, and when a double-tap
  locks a hold-to-talk session. The capture loop reads it on every pass, so a
  mid-capture lock takes effect immediately.
- While the flag is set, capture uses hands-free limits instead of the
  hold-to-talk ones: **ends on the second press (or Esc cancels)**, a
  **30 s** safety silence timeout (also the "start speaking" wait), and a
  **5 min** cap. A pill notice explains when the cap was reached.
- Hold-to-talk is unchanged: `maxUtteranceMs` cap and the existing short
  start window.
- Deadlines scale with audio: the faster-whisper per-request timeout becomes
  `max(75 s, 30 s + 2 x audio)`, and the outer decode timeout ceiling rises
  from 240 s to 900 s (formula unchanged: `max(45 s, 4 s + 7 x audio)`). The
  whisper.cpp worker already scaled with audio length.
- Copy updated: "Press the shortcut again when you're done" replaces "Pause
  when you're done".

## Decisions (owner may override)

1. **Terminal newlines collapse to spaces**, always, even where bracketed paste
   would make them safe. We can't detect bracketed paste from outside, and one
   accidental Enter in a shell is worse than a lost line break.
2. **Windows Terminal uses Ctrl+Shift+V**, not Ctrl+V, because Ctrl+V is the
   binding power users unbind.
3. **Submit phrases: "press enter" and "hit enter" only**, as the whole
   utterance or their own sentence. No "send it".
4. **Submit rides the existing Spoken edit commands toggle**; no new setting.
5. **Hands-free: 30 s silence timeout, 5 min cap.** Wispr Flow caps around
   6 min; 5 min keeps CPU decode time (turbo runs ~12 s per 30 s window on CPU)
   and memory bounded.
6. **Audio is kept only after a failure**, cleared by the next dictation,
   never on disk. No tray item: a tray click leaves the taskbar focused, so a
   retry from there would type into the taskbar.
7. **Unreadable token = elevated** (when VoiceWave itself isn't elevated). A
   process run as a different user at the same level would also go
   clipboard-only with the administrator copy. Rare, and the text still
   arrives on the clipboard.
8. **`preferClipboardOnlyForTerminals` stays dead.** It defaults to `true` in
   every saved settings file, so wiring it would switch terminal paste off for
   everyone. Delete it in a later cleanup.

## Open questions for the owner

1. Is collapsing newlines in terminals acceptable, or do you want multi-line
   paste for Claude Code/Codex specifically (they enable bracketed paste)? That
   would need per-app trust, since the terminal host can't tell us.
2. Should "send it" (or another short phrase) be added for chat apps, accepting
   some false submits?
3. Hands-free limits: are 30 s of silence and 5 min right for you?
4. Do you want a visible "Retry last dictation" somewhere besides the pill
   (main window Home, for example)?

## Known limits

- A partial Direct failure (SendInput fails mid-text) falls through to paste,
  which could duplicate the typed part. Direct is used only for short text
  (< 48 chars) in non-terminal apps, and UIPI, the realistic cause, is now
  caught up front.
- Windows Terminal shows its "large paste" confirmation above 5 KiB (~800
  words). A long hands-free dictation into WT may hit it; it is a confirm, not
  an execution.
- The VS Code integrated terminal is `code.exe`, so it is treated as an editor
  (Ctrl+V, newlines kept). Unchanged from before.
- Insert/paste chords assume default key bindings per terminal.

## Ideas (not built)

- Review-before-send: hold the transcript in the pill with Insert / Edit /
  Discard. Useful for terminals, but it adds a step to every dictation.
- Per-app "multi-line paste OK" list (Claude Code, Codex) once we trust
  bracketed paste there.
- Detect partial Direct typing and skip the paste fallback.
- Show the retry offer on Home as well as the pill.

## Commits

See `git log feat/lands-every-time`.

## Verification

- `npm run typecheck`-equivalent (`tsc -b`), `npm run test` (vitest),
  `cargo check --all-targets`, and a build. Results are in the hand-off report.
- No unit tests were added (owner rule).

## Manual test checklist (owner)

Run on the installed build from this branch. Use the hold-to-talk hotkey
unless noted.

**Terminals**

1. Windows Terminal + Claude Code: dictate "explain this repository". Text
   appears in the prompt, nothing submits. Then dictate "Fix the failing test.
   Press enter." Text appears and the prompt submits once.
2. Windows Terminal + PowerShell: dictate a sentence, then say "new line" in
   the middle of another. Both land on one line; nothing runs.
3. cmd.exe in a classic console window (run `conhost cmd`): dictate a
   sentence. It pastes; nothing runs.
4. Git Bash (mintty): dictate a sentence. It pastes via Shift+Insert, no `^V`
   artifact.
5. Say only "press enter" in a terminal with text already typed: Enter is
   pressed, nothing else is inserted.
6. Say "To continue, press enter." in Notepad: the sentence is typed as text,
   no Enter.

**Elevation**

7. Open Notepad as administrator, dictate: pill says "This app runs as
   administrator"; Ctrl+V pastes the text. The main window must not say
   "Inserted".
8. Admin Windows Terminal: same pill, no keystrokes typed.

**Regular apps**

9. Notepad: short and long dictations insert as before; "new line" still
   makes a line break.
10. Slack or a browser text box: dictation pastes; "Thanks. Press enter."
    sends the message.
11. Switch windows during the 250 ms after a "press enter" paste (hard to hit;
    optional): the pill says Enter wasn't pressed.

**Hands-free**

12. Double-tap Ctrl+Win, wait 10 s before speaking, then speak with a 15 s
    pause in the middle, then press Ctrl+Win once: everything is transcribed.
13. Hands-free shortcut, speak, then stay silent: it finishes about 30 s after
    you stop.
14. Hands-free for over 2 minutes (read an article aloud): nothing is cut at
    120 s; transcription completes.
15. Esc during hands-free cancels and inserts nothing.

**Retry**

16. Force a decode failure (e.g. set `VOICEWAVE_FINAL_DECODE_TIMEOUT_MS=8000`
    and dictate ~60 s on CPU, or kill the speech worker process mid-decode):
    the pill shows the failure with Try again. Click it: the same words are
    transcribed and inserted into the original app.
17. After a failure, dictate something new successfully, then confirm no retry
    is offered for the old audio.
18. Hold-to-talk: release after a short phrase still inserts quickly (no
    regression from the hands-free changes).
