# 009 — Competitive Feature Brainstorm (2026-07)

Research-backed brainstorm: what VoiceWave should build next to win its market.
Based on competitor research (Wispr Flow, SuperWhisper, OpenWhispr, Windows Voice
Access, Dragon), the 2026 local-model landscape (Parakeet), and the vibe-coding wave.

Status: brainstorm / not yet planned. Pick items into numbered plans as they're scheduled.

---

## The strategic frame

VoiceWave's position is stronger than the feature list suggests, because of three market facts:

1. **Wispr Flow has no offline mode at any price.** Every word travels through
   AWS/OpenAI/Anthropic subprocessors, it costs $15/mo, and there's a documented
   trust gap (works in trial, degrades after payment, Trustpilot complaints).
   Their weakness is structural — they can't fix it without rebuilding.
2. **Windows is the underserved platform.** SuperWhisper is Mac-only. Dragon is
   dead for consumers ($699 enterprise-only, Home edition discontinued in 2023).
   Windows Voice Access is a navigation tool with punctuation bugs and a
   vocabulary that doesn't improve first-pass recognition. VoiceWave is the only
   serious local option on the biggest desktop OS.
3. **The real open-source rival is OpenWhispr**, which already ships meeting
   transcription with diarization and AI-agent handoff, cross-platform
   (Mac/Win/Linux). That's the bar for "free and open."

The play is not "catch up to Wispr." It is: make the offline experience *feel*
better than cloud, then take the two adjacent markets nobody owns on Windows
(accessibility orphans, AI-native developers).

---

## Tier 1 — sharpen the core (weeks each, highest leverage)

### 1. Live streaming preview — words appear as you speak
The single biggest felt-quality gap. VoiceWave today is batch: hold key, speak,
release, wait, paste. Wispr streams; Win+H streams. Even chunked partial
transcription rendered into the pill (not inserted until release) would
transform perceived speed. New benchmark apps advertise 80 ms latency with
Parakeet; day one doesn't need that — visible partials from chunked Whisper
turbo close most of the perception gap. This is the feature that makes demos
and comparison videos go VoiceWave's way.

### 2. Hands-free mode (VAD auto-stop / toggle instead of hold)
Key insight: **push-to-talk requires holding a key, and the people who most
need dictation — RSI and accessibility users — are exactly the people who can't
hold keys.** Dragon's death orphaned this entire market on Windows; Talon is
the only refuge and it has a brutal learning curve. A toggle-to-dictate mode
with voice-activity-based auto-stop is cheap (VAD already exists in the
pipeline) and opens a loyal, underserved, evangelistic user base. Free +
offline + accessible is a story nobody else on Windows can tell.

### 3. Per-app style profiles
Wispr's marquee 2026 feature is "casual in Slack, formal in email." The
insertion pipeline *already detects the target app* for the fallback chain —
the signal exists, it just needs routing into the polish prompt with a small
settings UI (per-app: tone + polish on/off). Very high
marketing-value-to-effort ratio.

### 4. Command mode on selected text
"Make this concise," "translate to French," "fix the grammar" on highlighted
text. Wispr gates this behind $15/mo Pro. Every ingredient exists: the
spoken-edit-command parser, the on-device LLM, and the SendInput/clipboard
machinery (capture selection via Ctrl+C, rewrite, paste back). Shipping the
paid competitor's flagship feature free and offline is the loudest possible
message.

---

## Tier 2 — expand the surface (a month-plus each)

### 5. Quick capture / voice notes
Dictating with no text field focused currently has nowhere to go. Let it land
in History as a note (live history was built for this). Wispr made "Flow Notes"
a whole product pillar; here it's mostly wiring a "no target → save to history
+ pill confirmation" path.

### 6. Offline meeting transcription
WASAPI loopback capture (WASAPI/COM is already hand-rolled for the mic guard) +
Whisper on system audio = free, fully offline meeting notes — a category Otter
charges subscriptions for and no cloud tool can offer to privacy-bound users.
V1 without speaker diarization is still valuable; diarization later. Matches
OpenWhispr's biggest feature and beats it on Windows polish.

### 7. The vibe-coding wedge
Developers voice-prompting Cursor/Claude Code is a real 2026 wave (speaking is
3-4x faster than typing prompts, and Cursor's built-in voice only feeds its own
agent box). Terminal insertion for Codex is *already solved* — VoiceWave is
accidentally ahead here. What's missing: a code-aware polish mode (never "fix"
camelCase, preserve backticks and paths) and marketing that says "the dictation
app for AI-native developers on Windows." Small eng, big positioning.

### 8. Self-correcting dictionary
Low-confidence terms are already queued. Close the loop: when the user
immediately corrects a pasted word, promote the correction to the dictionary.
SuperWhisper's #1 complaint is "transcripts need manual cleanup" — an app that
visibly learns from cleanup is the answer to that complaint.

---

## Tier 3 — strategic bets (decide deliberately, don't slot into a sprint)

### 9. Parakeet as a second engine
Parakeet TDT v3 is ~10x faster than Whisper turbo with *better* English WER and
true streaming — but only 25 European languages vs Whisper's 99. Right move:
make the inference layer engine-agnostic and ship both — Parakeet for
speed-critical English, Whisper for language coverage. Also future-proofs
against models leapfrogging each other.

### 10. Regulated-industry positioning
Healthcare, legal, and finance workers are *banned* from cloud dictation tools.
Free + offline + open-source (auditable) is exactly what their IT departments
can approve. Mostly marketing plus small proof features (a "no network calls"
attestation page, portable/no-admin install). Cheap, differentiated, and Wispr
structurally cannot follow.

### 11. Cross-platform (Mac/Linux)
Biggest TAM lever, but the insertion/hotkey layer is deep Win32 (SendInput,
GetAsyncKeyState, COM) — a multi-month rewrite. Honest take: not yet. Windows
is where the vacuum is; win it thoroughly first.

---

## Recommended order

**1 → 2 → 3 → 4**: streaming preview (feel), hands-free mode (new market,
cheap), per-app styles + command mode (deletes Wispr Pro's reason to exist, for
free). That sequence turns "free offline alternative" into "better than the
paid cloud thing, and it's free." Then meeting notes (#6) as the next big swing.

**One non-feature item that beats all of this on ROI: the winget PAT is still
broken** — the website hero advertises `winget install`, and that listing is
stale (since v0.5.0). Distribution bugs cost more users than missing features.

---

## Sources

- [Wispr Flow — What's new](https://wisprflow.ai/whats-new)
- [Wispr vs Superwhisper vs MacWhisper test (spokenly.app)](https://spokenly.app/blog/wispr-flow-vs-superwhisper-vs-macwhisper)
- [Wispr Flow vs Superwhisper (getvoibe.com)](https://www.getvoibe.com/resources/wispr-flow-vs-superwhisper/)
- [OpenWhispr (GitHub)](https://github.com/OpenWhispr/openwhispr)
- [Parakeet vs Whisper benchmarks (spokenly.app)](https://spokenly.app/blog/parakeet-vs-whisper)
- [Northflank open-source STT benchmarks 2026](https://northflank.com/blog/best-open-source-speech-to-text-stt-model-in-2026-benchmarks)
- [Dragon is dead — alternatives (murmur-app.com)](https://murmur-app.com/en/blog/dragon-naturallyspeaking-dead-alternatives)
- [Voice Access limitations (Microsoft Q&A)](https://learn.microsoft.com/en-au/answers/questions/5790297/problems-with-voice-access-for-windows-11-consiste)
- [Speech-to-code (Addy Osmani)](https://addyo.substack.com/p/speech-to-code-vibe-coding-with-voice)
- [Dictate in Cursor (getvoibe.com)](https://www.getvoibe.com/resources/dictate-in-cursor/)
