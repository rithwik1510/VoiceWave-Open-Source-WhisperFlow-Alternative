# Plan 015: Local Meeting Mode

## Status

**Decision draft — not approved for implementation.**

This plan captures the product research, feasibility questions, recommended
scope, and decision gates for a VoiceWave meeting notetaker. Its presence in
the repository does not mean the feature is committed to the roadmap.

## Summary

Wispr Flow Notetaker extends system-wide dictation into a meeting-memory
product. It captures the user's microphone and computer audio without adding a
bot to the call, creates a live transcript, reprocesses the full recording after
the meeting, attributes speakers, generates summaries and action items, and
makes past meetings searchable through chat and MCP.

VoiceWave can build a meaningfully differentiated version:

> **Private, botless meeting notes processed entirely on the user's Windows PC.**

The feature is strategically attractive, but it is not a small extension of
hands-free dictation. Reliable hour-long capture, crash-safe disk spooling,
system-audio capture, transcript reconciliation, retention controls, and
resource isolation are foundational work. The recommended next action is a
bounded technical spike, followed by a separate go/no-go decision.

## What the competitor feature is

Wispr Flow Notetaker is separate from Flow Scratchpad/Notes:

- **Scratchpad/Notes** is a personal editor for quick thoughts, drafts, images,
  and dictated notes.
- **Notetaker** records multi-person meetings and turns them into a reusable
  transcript, summary, action list, and knowledge source.

The currently documented Notetaker experience includes:

1. Botless capture across Zoom, Google Meet, Microsoft Teams, Slack Huddles,
   unscheduled calls, and in-person conversations.
2. A live transcript during the meeting.
3. A higher-quality post-meeting transcription pass over the complete audio.
4. Speaker separation and attempted real-name attribution using calendar
   attendees, personal vocabulary, and conversational context.
5. Topic-grouped summaries, decisions, action items, and ownership.
6. In-meeting catch-up and questions.
7. Search and questions across meeting history with links to exact transcript
   moments.
8. Read-only MCP access for Claude, ChatGPT, Cursor, VS Code, and other tools.
9. Optional pre-meeting briefs using previous notes, Slack, and web sources.

Wispr's capture runs through its desktop app and does not join the call as a
participant. However, Notetaker is not a local-only product: Private Cloud Sync
must be enabled, and meeting data may be stored on-device and/or in Wispr's
cloud. Privacy Mode independently controls model-training use; it does not turn
off cloud synchronization.

At the August 5, 2026 launch, Notetaker was available on Mac, with Windows
announced as forthcoming. Documentation and availability are changing quickly.

## Product thesis

### Why it fits VoiceWave

VoiceWave already owns several required building blocks:

- Windows-native Tauri/Rust runtime.
- Local Whisper/faster-whisper inference.
- Microphone capture, resampling, VAD, and capture lifecycle management.
- Personal dictionary and terminology hints.
- Encrypted local history and retention controls.
- Floating listening/status pill.
- Local post-processing and model management.
- Explicit hold-to-talk and hands-free control modes.

Meeting Mode would extend VoiceWave from **voice input** into **voice memory**.
It also reinforces the project's strongest structural position: users can
process sensitive speech without sending it to a third-party transcription
service.

### Why it may not be worth building

The feature moves VoiceWave into a second product category with different
quality expectations:

- A failed 20-second dictation is inconvenient; a failed 90-minute meeting is
  a serious loss.
- Long-running capture must survive device changes, sleep/wake, low disk space,
  app crashes, and inference failures.
- Speaker diarization and real-name attribution are probabilistic and can
  create confidently incorrect ownership.
- Recording conversations adds consent, compliance, retention, and trust work.
- Continuous transcription can compete with the core dictation experience for
  CPU, GPU, memory, and model-worker capacity.
- A useful meeting library needs a new persistence and search model; the
  current short-dictation history is not sufficient.

This feature should proceed only if VoiceWave wants meeting intelligence to
become a major product pillar rather than a checklist item.

## Proposed product

Working name: **VoiceWave Meeting Mode**.

Primary promise:

> Record a meeting without inviting a bot. Get a searchable transcript,
> decisions, and action items locally on your Windows PC.

### Core user journey

1. The user opens **Meetings** and selects **Start meeting**.
2. VoiceWave shows a recording-consent reminder and the selected microphone and
   system-audio device.
3. The user confirms and VoiceWave begins capturing separate microphone and
   system-audio tracks.
4. A persistent, unmistakable recording pill shows duration, audio health,
   storage status, and Stop/Cancel controls.
5. A rolling transcript appears with timestamps and `You`/`Others` labels.
6. The user can add a private bookmark or note during the meeting.
7. On Stop, VoiceWave flushes the recording and runs a final processing pass.
8. The meeting opens with:
   - Overview
   - Decisions
   - Action items
   - Open questions
   - Timestamped transcript
   - Optional audio playback
9. The user reviews, edits, exports, retains, or deletes the result.

## Goals

1. Capture microphone and Windows system audio without a meeting bot.
2. Keep audio, transcripts, summaries, and indexes local by default.
3. Survive ordinary long-session failures without losing completed audio.
4. Preserve timestamp-level evidence for every transcript and generated result.
5. Keep normal VoiceWave dictation responsive during an active meeting.
6. Make consent, recording state, retention, and deletion explicit.
7. Produce a useful result without requiring speaker-name attribution.

## Non-goals for V1

1. Automatic calendar recording.
2. Silent or background recording without explicit confirmation.
3. Cloud sync, team sharing, or collaborative notes.
4. Slack, email, CRM, or task-manager integrations.
5. Pre-meeting briefs.
6. Named speaker recognition.
7. A promise of perfect speaker diarization.
8. Cross-meeting semantic chat or a remote MCP service.
9. macOS, mobile, or Linux meeting capture.
10. Video or screen recording.

## Current code reality

### Reusable

- `src-tauri/src/audio/mod.rs`: capture lifecycle, resampling, VAD, device
  selection, and audio-quality analysis.
- `src-tauri/src/state.rs`: lifecycle orchestration, cancellation, events,
  inference scheduling, and user-visible recovery.
- `src-tauri/src/inference/`: local Whisper/faster-whisper workers and model
  readiness.
- `src-tauri/src/dictionary/`: terminology hints for names, acronyms, and
  project vocabulary.
- `src-tauri/src/history/`: encryption and retention patterns.
- `src/components/FloatingPill.tsx`: visible capture-state surface.
- `src/App.tsx` and `src/hooks/useVoiceWave.ts`: frontend/core coordination.

### Not reusable as-is

- Dictation capture is short-lived and designed to finish into one insertion;
  meeting capture must stream to disk for hours.
- The active dictation session is a single foreground workflow; a meeting is a
  background session that must coexist with dictation.
- Dictation history is a capped, flat record store; meetings contain large
  timestamped segment collections and optional audio assets.
- The current inference path is effectively a constrained worker resource;
  continuous meeting decode cannot be allowed to delay normal dictation.
- The polish worker is optimized for short text. Meeting summarization needs
  chunking, structured evidence, and a larger factual context.

## Proposed architecture

### 1. Meeting session controller

Add a dedicated `MeetingController` instead of adding more branches to
`VoiceWaveController`.

Responsibilities:

- Start, stop, cancel, pause, and recover meeting sessions.
- Own capture tracks and the chunk manifest.
- Publish meeting lifecycle and health events.
- Schedule live and final transcription.
- Coordinate finalization, summarization, and cleanup.
- Never own global dictation hotkeys or insertion behavior.

Proposed lifecycle:

```text
idle
  -> preparing
  -> recording
  -> stopping
  -> finalizing
  -> ready

Any non-terminal state may enter recoverableError or failed.
```

### 2. Dual-track Windows audio capture

Capture and preserve two independently timestamped streams:

- **Mic track:** primarily the VoiceWave user.
- **Loopback track:** audio played by the active Windows output device.

Implement system audio using WASAPI loopback rather than depending on Stereo
Mix or treating an output device as a CPAL input.

Requirements:

- Shared monotonic session clock.
- Device-native sample-rate capture followed by controlled resampling.
- Periodic drift measurement between microphone and loopback clocks.
- Explicit handling for output-device changes and Bluetooth profile changes.
- Audio-level health reporting for both tracks.
- Echo/duplication detection when remote voices leak into the microphone.

V1 should label the microphone as `You` and loopback speech as `Others`. That is
more reliable than prematurely assigning remote speaker names.

### 3. Crash-safe audio spool

Never retain the full meeting in memory.

Recommended format:

- Small sequential PCM/WAV chunks per track, such as 30–60 seconds each.
- An append-only manifest committed atomically after every completed chunk.
- AES-GCM encryption per chunk with a per-meeting key protected through the
  existing secure-store pattern.
- A durable `recording.lock`/session marker for startup recovery.
- Final cleanup only after transcript persistence succeeds.

The controller must check free disk space before start and periodically during
recording. Low-space behavior must stop safely and preserve completed chunks.

### 4. Live transcription

Live transcription is a convenience layer, not the source of truth.

Proposed behavior:

- Decode rolling 10–20 second speech windows with overlap.
- Store provisional segments with track, start/end times, and confidence.
- Reconcile overlapping text before displaying stable segments.
- Allow live decode to fall behind or pause under resource pressure.
- Give foreground dictation jobs higher priority than meeting live-decode jobs.
- Show `Recording — transcript catching up` rather than risking capture loss.

### 5. Final transcription

After Stop:

1. Validate the chunk manifest and close both tracks.
2. Reconstruct or stream the normalized tracks.
3. Run a complete long-form transcription pass.
4. Reconcile mic and loopback timelines.
5. Replace provisional transcript segments atomically.
6. Record any gaps, corrupt chunks, or low-confidence regions.

The final transcript must remain usable even if summarization or diarization
fails.

### 6. Speaker roadmap

#### V1

- `You`
- `Others`
- Manual participant names and transcript relabeling

#### V2

- Local diarization of the loopback track into `Speaker 1`, `Speaker 2`, etc.
- One-click name assignment across the meeting.
- Optional reusable local speaker profiles only after explicit consent.

Sherpa-onnx is the preferred first spike because it offers ONNX models and Rust
and C APIs. WhisperX/pyannote is a quality reference, but it adds a substantial
Python/PyTorch runtime and model-distribution burden.

No generated action item may silently convert an uncertain speaker label into
a person's name.

### 7. Local meeting summarization

Use evidence-preserving hierarchical summarization:

1. Split the final transcript into topic-sized timestamped chunks.
2. Extract factual candidates from each chunk:
   - Topics
   - Decisions
   - Action items
   - Owners
   - Dates
   - Open questions
3. Merge candidates in a final organization pass.
4. Attach supporting segment IDs/timestamps to every item.
5. Mark inferred or ambiguous ownership instead of inventing certainty.

Meeting processing must have separate prompts, schemas, timeouts, and worker
limits from short-text polish.

### 8. Meeting persistence

Create a separate store rather than extending `SessionHistoryRecord`.

Suggested entities:

```text
Meeting
  id
  title
  started_at_utc_ms
  ended_at_utc_ms
  lifecycle_state
  processing_version
  audio_retention
  transcript_retention
  summary_status
  created_at_utc_ms
  updated_at_utc_ms

MeetingTrack
  meeting_id
  kind: mic | loopback
  device_label
  sample_rate
  chunk_manifest_path

MeetingSegment
  id
  meeting_id
  track
  speaker_id
  start_ms
  end_ms
  text
  confidence
  provisional

MeetingParticipant
  id
  meeting_id
  display_name
  source: manual | diarization | calendar

MeetingInsight
  id
  meeting_id
  kind: overview | decision | actionItem | openQuestion
  text
  owner_participant_id?
  due_at?
  evidence_segment_ids[]
```

The spike must decide between encrypted SQLite fields and encrypted per-meeting
documents. Standard unencrypted SQLite does not match the current privacy
promise; encrypted full-text search also needs an explicit design.

### 9. Frontend surface

Add a top-level **Meetings** destination, not a tab inside Dictation History.

Minimum screens:

1. Meeting library with status, date, duration, title, and search.
2. Recording screen with timer, dual audio meters, transcript, bookmarks, and
   Stop/Cancel.
3. Processing state with recoverable progress.
4. Meeting detail with Summary, Transcript, Audio, and Details tabs.
5. Retention/delete/export controls.

The floating pill must visibly distinguish meeting recording from ordinary
dictation and hands-free mode. Meeting recording should never use an ambiguous
tiny lock indicator alone.

## Privacy, consent, and retention

Required safeguards:

1. Explicit user confirmation before every recording starts in V1.
2. A clear reminder that the user is responsible for required participant
   notice and consent.
3. Persistent recording indication for the full session.
4. No automatic calendar recording in V1.
5. No outbound audio or transcript transport.
6. One action to delete audio, and another to delete the full meeting.
7. Recoverable delete confirmation; permanent erasure after confirmation.
8. Configurable audio and transcript retention.
9. Diagnostics must never include meeting audio or transcript content.
10. Speaker embeddings, if added later, require separate consent and deletion.

Recommended defaults for evaluation:

- Audio: retain for 7 days so users can verify transcripts, then delete.
- Transcript and summary: 30 days, matching current privacy expectations.
- `Never retain audio` and `Delete after successful processing` options.
- Forever only through an explicit user choice.

These defaults require product review before implementation.

## Reliability requirements

A Meeting Mode release is not acceptable unless it passes:

- 2-hour continuous capture without unbounded memory growth.
- Concurrent normal dictation while meeting capture continues.
- Crash/relaunch recovery with completed chunks intact.
- Microphone disconnect/reconnect.
- Default output-device change.
- Sleep/wake handling with an explicit transcript gap.
- Low-disk handling without corrupting existing chunks.
- Inference-worker crash and restart.
- Cancellation during live decode and during finalization.
- Deletion during ready, failed, and recovered states.
- No transcript duplication at chunk boundaries.
- No audio or transcript data in diagnostics or telemetry.

## Performance budgets

Initial targets to validate in the spike:

- Capture memory remains bounded under 100 MB for a 2-hour session.
- Audio chunks are durable within 65 seconds of capture.
- Foreground dictation latency regression during recording is under 10%.
- Live transcript delay is normally under 20 seconds but may degrade safely.
- UI remains responsive during capture and final processing.
- Final processing reports progress and can resume after interruption.

These are proposed budgets, not confirmed capabilities.

## Delivery phases

### Phase 0 — technical spike and go/no-go

Estimated effort: **about 1 focused engineering week**.

Deliverables:

1. WASAPI loopback capture proof on representative Windows devices.
2. Simultaneous mic and loopback capture with aligned timestamps.
3. Two-hour disk-spool soak test.
4. A recorded 30-minute Zoom/Meet/Teams sample transcribed locally.
5. Foreground dictation latency measurement during capture and live decode.
6. Storage-size and final-processing benchmarks on CPU and GPU machines.
7. Written findings and a revised V1 estimate.

Go/no-go thresholds:

- No missing or corrupt audio in the soak test.
- Device capture works on at least Windows 10 and Windows 11 test machines.
- Dictation remains usable while meeting capture runs.
- Final transcription is acceptably faster than, or near, real time on the
  supported hardware floor.
- Storage and model requirements remain acceptable for the product.

No production UI, migrations, or roadmap promise should be made during this
spike.

### Phase 1 — durable local recorder

Estimated effort after a successful spike: **3–5 weeks**.

- Dedicated meeting lifecycle.
- Dual-track capture.
- Encrypted chunk spool and crash recovery.
- Manual Start/Stop UI and recording pill.
- Post-meeting transcription.
- Timestamped `You`/`Others` transcript.
- Audio playback, export, retention, and deletion.

### Phase 2 — useful meeting notes

Estimated effort: **2–4 weeks**.

- Live transcript.
- Structured overview, decisions, action items, and open questions.
- Evidence links and transcript correction.
- Meeting library and search.
- Meeting-type templates: general, 1:1, interview, lecture, stand-up.

### Phase 3 — speaker intelligence and local knowledge

Estimated effort: **4–8 weeks**, strongly dependent on model evaluation.

- Local diarization.
- Manual name assignment and safe label propagation.
- Optional reusable speaker profiles.
- Ask-this-meeting.
- Cross-meeting search.
- Local read-only MCP server.

### Deferred cloud/integration layer

Calendar metadata, Slack context, pre-meeting briefs, cloud sync, team sharing,
and collaboration are separate strategic decisions. They should not be allowed
to delay a strong local meeting product.

## Test strategy

### Unit

- Meeting lifecycle transitions and idempotent Stop/Cancel.
- Chunk manifest atomicity and recovery.
- Audio timestamp alignment and drift correction.
- Transcript overlap reconciliation.
- Retention/deletion policy.
- Summary schema validation and evidence references.
- Speaker-label uncertainty rules.

### Integration

- Synthetic mic and loopback sources with known timestamps.
- Worker crash during live and final transcription.
- Device loss and device change.
- Corrupt/missing chunk recovery.
- Concurrent meeting and dictation scheduling.
- Restart with sessions in every non-terminal lifecycle state.

### Desktop end-to-end

- Zoom, Google Meet, Teams, Slack Huddle, browser playback, and in-person room.
- Headphones, laptop speakers, USB mic, Bluetooth headset, and device switching.
- 15-minute, 60-minute, and 120-minute sessions.
- One speaker, two speakers, overlapping speakers, silence, and background media.
- User verifies summary items by jumping to evidence timestamps.
- Consent reminder, recording indicator, deletion, and retention settings.

## Success metrics

The feature should be judged on trust and reuse, not raw transcript volume.

Proposed beta metrics, collected locally unless separately opted in:

- Successful meeting finalization rate.
- Percentage of meetings recovered after interruption.
- Transcript gap duration.
- Time from Stop to usable transcript.
- Summary items edited or deleted by the user.
- Action items with verified evidence links.
- Meetings reopened, searched, or exported within seven days.
- Dictation latency regression during active meetings.
- Audio/transcript deletion success.

## Major risks and mitigations

### System audio compatibility

**Risk:** WASAPI devices, drivers, Bluetooth modes, and exclusive-mode apps can
behave differently.

**Mitigation:** make Phase 0 a real device matrix, not a single-machine demo;
show track health and fail before recording if loopback is unavailable.

### Resource contention

**Risk:** live meeting decode makes normal dictation slower.

**Mitigation:** prioritize dictation, allow meeting transcript lag, use a
separate worker or scheduler, and benchmark the supported hardware floor.

### Speaker errors

**Risk:** an action item is attributed to the wrong person.

**Mitigation:** V1 uses `You`/`Others`; keep uncertainty visible; require manual
confirmation before promoting inferred speaker names.

### Summary hallucination

**Risk:** the model invents decisions, owners, or dates.

**Mitigation:** structured extraction, evidence segment IDs, uncertainty
labels, and a transcript-first fallback when validation fails.

### Recording trust and legality

**Risk:** botless capture is less visible to other meeting participants.

**Mitigation:** explicit confirmation, persistent recording UI, consent
language, no V1 auto-record, and immediate deletion controls.

### Storage and recovery

**Risk:** long recordings consume disk or become corrupt.

**Mitigation:** bounded chunks, atomic manifest updates, free-space checks,
per-chunk encryption, resumable finalization, and tested partial recovery.

### Product dilution

**Risk:** VoiceWave becomes a mediocre dictation app and a mediocre meeting app.

**Mitigation:** do not proceed unless the team treats Meeting Mode as a major
pillar with an explicit reliability budget and owner.

## Alternatives

### A. Build only Quick Notes

Save dictation into a local notes library when no text target is active.

- Much smaller scope.
- Reuses history and dictation.
- Does not solve meeting capture or multi-speaker transcription.
- Useful foundation even if Meeting Mode is rejected.

### B. Import recordings only

Let users drag an existing audio file into VoiceWave for local transcription
and summarization.

- Avoids live capture, device handling, consent UI, and crash recovery.
- Validates long-form transcription and summary demand.
- Does not provide live transcript or botless meeting recording.

This is the lowest-risk market test and should be considered before Phase 1.

### C. Integrate an external meeting service

- Fastest way to obtain polished speaker labels and summaries.
- Breaks the local-only product promise.
- Introduces usage cost, vendor dependency, accounts, and data governance.
- Not recommended as the default VoiceWave implementation.

### D. Do nothing

Keep VoiceWave focused entirely on high-quality dictation.

- Preserves product clarity and engineering focus.
- Leaves meeting transcription to specialized products.
- Reasonable if current dictation reliability, distribution, and adoption have
  higher expected return than a new product pillar.

## Decision framework

Before approving Phase 0, answer:

1. Do we want VoiceWave to become a meeting-memory product, or remain a focused
   voice-input product?
2. Is fully local Windows meeting capture a sufficiently strong market wedge?
3. Can we support the storage/model footprint without damaging accessibility?
4. Are we prepared to own recording-consent and retention UX?
5. Can we test across a meaningful Windows device matrix?
6. Which demand test comes first: Quick Notes, recording import, or live capture?

Recommended decision sequence:

1. Interview or survey existing VoiceWave users about meeting-note behavior.
2. Prototype **audio-file import** to validate long-form transcript and summary
   quality without changing the capture runtime.
3. If the results are strong, approve the one-week WASAPI Phase 0 spike.
4. Review measured quality, resource cost, and user demand.
5. Approve or reject Meeting Mode V1 as a separate roadmap decision.

## Recommendation

**Do not approve full implementation yet.**

Approve only the decision work and, if user demand is credible, a bounded
technical spike. The strategic opportunity is real: Wispr's product validates
the category, while VoiceWave can offer a Windows-first, botless, local-only
alternative. But the feature is worthwhile only if capture reliability,
dictation coexistence, and local long-form quality survive measurement.

The recommended lowest-risk path is:

> Quick Notes/data model -> audio-file import experiment -> WASAPI capture spike
> -> explicit V1 go/no-go.

## Research sources

- [Wispr Flow Notetaker launch](https://www.reddit.com/r/WisprFlow/comments/1vgk20o/august_5_2026_wispr_flow_notetaker_is_live_plus/)
- [Wispr Flow MCP documentation](https://docs.wisprflow.ai/articles/4759919286-how-to-connect-wispr-flow-to-claude-chatgpt-and-other-ai-tools-mcp)
- [Wispr Flow private cloud sync and privacy controls](https://docs.wisprflow.ai/articles/4709791908-understanding-privacy-mode-and-cloud-sync)
- [Wispr Flow privacy policy](https://wisprflow.ai/privacy-policy)
- [Wispr Flow terms for meeting recording and consent](https://wisprflow.ai/terms-of-service)
- [Wispr Flow system requirements](https://docs.wisprflow.ai/articles/1036674442-supported-devices-and-system-requirements)
- [Wispr Flow Scratchpad documentation](https://docs.wisprflow.ai/articles/9618237082-using-the-scratchpad-to-save-and-edit-notes)
- [Granola botless AI notetaker](https://www.granola.ai/ai-note-taker)
- [Otter speaker identification overview](https://help.otter.ai/hc/en-us/articles/21665587209367-Speaker-Identification-Overview)
- [WhisperX diarization](https://github.com/m-bain/whisperX)
- [pyannote.audio](https://github.com/pyannote/pyannote-audio)
- [sherpa-onnx speaker diarization](https://k2-fsa.github.io/sherpa/onnx/speaker-diarization/index.html)
