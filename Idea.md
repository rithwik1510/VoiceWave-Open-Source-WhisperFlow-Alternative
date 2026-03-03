cod# VoiceWave Product Plan v2

## 0) Execution Reset (2026-02-10)

1. v1 speech-to-text engine is whisper.cpp via Rust integration.
2. We are not building or training a custom ASR model.
3. Any mock transcript path is test scaffolding only and cannot be used for phase completion claims.
4. Phase completion requires runtime evidence from the real whisper.cpp path.

## 1) Product Charter

### Mission
Build the most trusted local dictation desktop app for professionals who want fast speech-to-text without sending audio to the cloud.

### Product Statement
VoiceWave is a privacy-first desktop dictation app for Windows and macOS that captures speech locally, transcribes with on-device models, and inserts text reliably into active apps with low latency.

### North Star Outcome
Users can speak and see accurate text appear in their target app in under 2 seconds for typical utterances on supported hardware.

## 2) Product Principles (Non-Negotiable)

1. Privacy by default: no audio leaves device in v1.
2. Reliability over feature count: insertion success and stability beat adding many features.
3. Performance as a feature: low latency is a launch requirement, not a nice-to-have.
4. Explicit permissions: clear rationale and graceful fallback when denied.
5. Simple first run: benchmark once, recommend best model, keep control with the user.
6. Honest UX: communicate limits clearly (supported apps, permission needs, hardware expectations).

## 3) Scope and Non-Goals

### In Scope (v1)
1. Local-only inference path.
2. Windows and macOS support.
3. Global hotkey dictation (push-to-talk + toggle).
4. Clipboard + simulated paste insertion fallback chain.
5. Model download/management with checksum verification.
6. Local settings/history/stats with retention controls.
7. Signed installers and signed auto-updates.
8. English-first quality tuning.

### Current Execution Scope Override (2026-02-10)
1. Active implementation and validation scope is Windows-only.
2. macOS implementation and test execution are deferred until macOS hardware is available.
3. v1 product target remains Windows + macOS; this override is a temporary delivery focus, not a product-direction change.

### Out of Scope (v1)
1. Linux support.
2. Cloud fallback transcription.
3. Enterprise admin controls and SSO.
4. Full multilingual parity.
5. Team collaboration features.
6. Final visual design system decisions.
7. Premium feature bundling and final pricing plan.

## 4) Target Users and Jobs-To-Be-Done

### Primary User Segments
1. Knowledge workers writing in docs, chats, and email.
2. Developers and operators dictating notes and commands.
3. Users with RSI or typing fatigue who need hands-light input.

### Core Jobs
1. "When I need to capture thoughts quickly, let me speak and get clean text immediately."
2. "When I switch apps, keep dictation reliable without setup friction."
3. "When privacy matters, guarantee local processing and transparent controls."

## 5) Success Metrics and Launch Gates

### Product Metrics
1. Insertion success rate: >= 98.0% across supported app matrix.
2. Permission completion rate (first-run): >= 85.0%.
3. Correction rate (edited words / total words): <= 12.0% in beta cohort.
4. 7-day retention of activated users: >= 35.0%.
5. Time-to-first-successful-dictation (TTFSD): <= 3 minutes from install.
6. First-session activation: >= 80.0% users complete at least one successful insertion.

### System Metrics
1. p95 end-to-end latency: <= 900 ms on reference mid-tier device using small.en.
2. Crash-free sessions: >= 99.5%.
3. App cold start: <= 2.5 s on reference hardware.
4. Real-time factor (RTF): <= 0.7 for recommended model.

### Security and Compliance Gates
1. Zero unresolved critical/high security findings.
2. Signed binaries and signed updates verified in CI and pre-release.
3. Model checksum and signature validation enforced.
4. Privacy/legal docs complete and published before GA.

### Commercial Metrics (Post-GA, Deferred for Finalization)
1. Initial placeholder targets exist but are not release blockers for v1 core quality.
2. Final premium packaging and pricing metrics will be set after beta reliability and activation results.
3. Support contact rate remains tracked in beta to size support operations.

## 6) Assumption Register (Bias-Controlled)

| ID | Assumption | Confidence | Validation Method | Kill/Change Trigger |
|---|---|---:|---|---|
| A1 | Local-only can meet acceptable latency on target hardware | Medium | Benchmark by hardware tiers in Phase 1-2 | If >25% beta users fail p95 latency gate |
| A2 | Insertion reliability can reach >=98% in target apps | Medium | App compatibility matrix tests in Phase 3-5 | If two major apps remain <95% after mitigation |
| A3 | Users will grant required OS permissions | Medium | Permission funnel analytics in beta | If completion <70%, redesign onboarding flow |
| A4 | English-first model set is sufficient for launch fit | High | Beta feedback + correction telemetry (opt-in only) | If correction rate remains >18% for target users |
| A5 | Privacy-first positioning is a differentiator | Medium | User interviews + acquisition experiments | If conversion does not improve vs neutral messaging |
| A6 | Users accept local-only v1 without cross-device sync | Medium | Beta interviews + churn reasons in diagnostics flow | If "no sync/mobile" is top-3 churn reason for 2 cycles |
| A7 | Users prefer zero-command natural speech over explicit voice commands | Low | Usability test on edit workflows | If correction loop takes >20 seconds median |

## 7) Architecture Blueprint

### Core Components
1. Desktop shell: Tauri 2 app with Rust core and minimal web UI.
2. Audio pipeline: mic capture, mono conversion, 16 kHz resample, frame buffering, VAD segmentation.
3. Inference engine: whisper.cpp via Rust integration, cancellable decode jobs, streaming partials.
   1. Decoder source is upstream whisper.cpp runtime, not a custom-trained ASR path.
4. Model manager: catalog, download/resume, checksum, storage budget, health checks.
5. Text insertion engine: focused-app insert chain:
   1. Direct paste shortcut simulation.
   2. Clipboard write + user-confirmed paste fallback.
   3. Quick history fallback if insertion blocked.
6. Hotkey manager: global hotkeys, conflict detection, per-app overrides.
7. Persistence layer: SQLite for settings/history/stats; history encryption enabled when history is on.
8. Diagnostics bundle: redacted logs, perf metrics, environment details, no raw audio by default.
9. Update subsystem: signed manifests, staged rollout channels, rollback support.
10. Experience layer: lightweight floating HUD, status sounds, and deterministic state indicators (idle/listening/processing/inserted/error).
11. Command and formatting layer: optional voice commands, snippets, and app-aware style profiles (feature-flagged).

### Frontend Design Placeholder (Intentional)
1. Visual brand direction is deferred; no final aesthetic system is locked in this plan.
2. v1 focuses on functional UX quality: clarity, state visibility, speed, accessibility, and low friction.
3. A dedicated design sprint will define:
   1. Design language and component system.
   2. Typography, color tokens, and motion rules.
   3. Desktop-first and responsive behavior standards.
4. Design decisions must not regress core reliability or latency goals.

### Design Constraints
1. No outbound audio transport in production path.
2. Strict separation between UI thread and inference worker.
3. Every stateful store requires schema versioning and migration tests.
4. Every privileged operation requires explicit permission checks and fallback UX.
5. Audio pipeline and inference must support graceful degradation on thermal or battery constraints.
6. Any AI-assisted rewrite must be user-controllable and transparently labeled.

## 8) Engineering Quality Rules (High-End Company Standard)

### Code Quality
1. Clean architecture boundaries: no UI logic in core domain modules.
2. Mandatory lint/format on CI; no merge if lint fails.
3. Core modules require unit tests before merge.
4. Public interfaces require typed contracts and backward-compat notes.
5. Complex logic requires short rationale comments (not narrative comments).

### Branching and Reviews
1. Protected main branch; no direct commits.
2. Pull request required for all changes.
3. At least one approving reviewer for normal changes; two for security-critical areas.
4. PR template requires:
   1. Problem statement.
   2. Scope and non-scope.
   3. Test evidence.
   4. Risk and rollback plan.

### Definition of Done
1. Feature code complete and reviewed.
2. Unit/integration tests added and passing.
3. Relevant E2E tests passing on supported OS matrix.
4. Docs updated (user docs + internal decision log).
5. Observability and error handling verified.

### Change Management
1. ADR required for architecture-impacting changes.
2. Feature flags for high-risk behavior changes.
3. Migration scripts must be reversible where feasible.
4. Release notes generated for every public build.

## 9) Security, Privacy, and Compliance Baseline

1. Threat model maintained and reviewed per release cycle.
2. Tampered model file detection must fail closed.
3. Update signature verification required before install.
4. Diagnostics export is explicit opt-in, user-triggered, and revocable.
5. History retention controls: Off, 7 days, 30 days, Forever.
6. Legal package before GA:
   1. Privacy Policy.
   2. Terms of Service.
   3. Offline processing disclosure.
   4. Third-party model/license notices.
7. Security incident runbook:
   1. Severity classification.
   2. Owner assignment within 1 hour.
   3. Customer communication template.
   4. Hotfix and rollback protocol.

## 10) Compatibility and Permission Strategy

### Supported Target Apps for v1
1. Browsers: Chrome, Edge, Safari.
2. Editors: VS Code, Notepad/TextEdit.
3. Productivity: Slack, Notion, Google Docs desktop browser usage.

### Compatibility Gate
1. Define "supported" only when insertion success >=98% and no blocker bugs.
2. Publish known limitations list in docs.
3. Maintain an app compatibility matrix in CI/manual QA.

### Permission Funnel
1. Request only needed permissions, at the moment of need.
2. Explain why each permission is needed in plain language.
3. If denied, provide one-click recovery instructions and clipboard-only mode.
4. Capture permission failure reasons through explicit opt-in diagnostics prompts.

### UX Reliability Standards
1. Always show recording state with both visual and optional audio signal.
2. Keep a one-tap "Undo insertion" action for the most recent commit.
3. Preserve transcript in quick history when insertion fails or focus changes.
4. Never block typing; VoiceWave must fail soft and return control immediately.

### Application Layout Blueprint (Structure Only, Style Deferred)
1. Layout pattern: persistent left sidebar with a single primary content canvas.
2. Header behavior: lightweight top row for account/system controls and notifications.
3. Sidebar navigation order for v1:
   1. Home
   2. Sessions
   3. Dictionary
   4. Snippets
   5. Style
   6. Models
   7. Settings
   8. Help
4. Sidebar footer utilities:
   1. Upgrade entry point (placeholder only, no final premium scope locked).
   2. Invite/Referral placeholder (optional).
   3. Settings and Help quick access.
5. Main Home screen content order for v1:
   1. Welcome and current system status.
   2. Quick actions: Start Dictation, Push-to-Talk mode, Active Model selector.
   3. Setup/health card: mic readiness, permissions, model readiness.
   4. Performance chips: streak, words, WPM, insertion success.
   5. Recent sessions panel with copy/retry/open actions.
6. Global state visibility:
   1. Recording state always visible: idle, listening, transcribing, inserted, error.
   2. Permission and model download states must surface in-context, not hidden in Settings.
7. Functional parity goal with established dictation dashboards:
   1. Reuse proven information architecture patterns.
   2. Keep VoiceWave visual identity and interactions distinct.
8. Visual design is intentionally deferred:
   1. Typography, colors, spacing system, and animation language will be decided in dedicated design sprint.
   2. Structural layout and user flows defined here remain the implementation source of truth for v1.

## 11) Model Strategy and Performance Plan

1. All v1 ASR model artifacts must be whisper.cpp-compatible local model files.

### v1 Model Set
1. tiny.en (fallback/low-end).
2. base.en (baseline).
3. small.en (recommended default for many devices).
4. medium.en (recommended for capable hardware).
5. Heavy models remain optional and hidden behind explicit advanced toggle.

### Recommendation Logic
1. First-run benchmark with fixed local samples and optional user sample.
2. Select fastest model meeting quality and latency gates.
3. Allow manual override with clear speed/accuracy labels.

### Distribution and Hosting Policy
1. Model binaries are not stored in the git repository.
2. Models are hosted as versioned release artifacts (CDN/object storage; GitHub Releases acceptable for early stage).
3. VoiceWave ships with a signed model manifest that contains model_id, version, size, sha256, license, and download URL.
4. App checks manifest compatibility with current runtime before enabling install.
5. Browser redirects are not the default install path; installation is handled inside the app UI.

### In-App Installation Lifecycle (One-Click)
1. User opens model picker and selects a recommended or manual model.
2. App shows size, expected speed, and disk-space requirement before download.
3. App downloads in background with pause/resume and progress reporting.
4. On completion, app verifies sha256 checksum and model metadata.
5. If verification passes, app atomically moves model into active cache and marks it available.
6. If verification fails, app quarantines the file, surfaces clear error, and offers retry.
7. If network fails, app resumes partial download from last verified chunk.
8. User can switch, remove, and reinstall models from settings without leaving the app.
9. Manual download link is fallback-only for restricted corporate networks.

### Resource Guardrails
1. Disk quota checks before model download.
2. Download resume and checksum re-verify.
3. Corrupt model auto-quarantine and retry policy.
4. Enforce max cache budget with safe cleanup of least-used inactive models.

## 12) Delivery Plan by Phase (Gate-Based)

### Phase 0 - Program Setup and Spec Freeze
### Goals
1. Finalize v1 scope, architecture, quality bars, and ownership.
2. Establish CI, coding standards, ADR template, and risk register.

### Build Outputs
1. Product requirements document.
2. Architecture RFC and threat model.
3. Test strategy and hardware tier definitions.
4. Competitive benchmark document (Wispr Flow + 3 peers) with parity target map.

### Testing in Phase
1. CI smoke checks for build/lint/test framework.
2. Static analysis baseline.

### Exit Criteria
1. Scope signed off.
2. Quality gates approved.
3. CI baseline green.

### Phase 1 - Core Audio and Inference Foundation
### Goals
1. Build reliable audio capture pipeline and local inference prototype.
2. Produce first end-to-end transcript from local mic input.
3. Replace any mock decode path with real whisper.cpp decode path before phase close.

### Build Outputs
1. Audio capture service with VAD segmentation.
2. Inference worker with whisper.cpp-backed cancellable jobs and partial transcript stream.
3. Basic settings persistence.
4. Minimal floating HUD and state feedback contract.

### Testing in Phase
1. Unit tests: audio transforms, VAD boundaries, worker lifecycle.
2. Integration tests: audio input to transcript output with fixture files.
3. Performance tests: latency and RTF baseline by model.
4. Thermal and battery tests on laptops under 30-minute sustained use.

### Exit Criteria
1. Stable transcript loop on Windows dev devices (current scope override); macOS validation deferred until hardware is available.
2. p95 latency baseline documented.
3. No critical crashes in 200-session internal test.
4. No mock-only decode path remains in production runtime mode.

### Phase 2 - Input and Insertion Reliability
### Goals
1. Implement global hotkeys and insertion fallback chain.
2. Build robust permission handling and denial recovery UX.
3. Validate reliability using real whisper.cpp transcript output path.

### Build Outputs
1. Hotkey manager with conflict detection and rebind flow.
2. Text insertion engine with clipboard fallback and history safety net.
3. Permission orchestration flow and UI.
4. Undo insertion action and consistent state feedback UX.

### Testing in Phase
1. Unit tests: hotkey state machine, insertion decision tree.
2. Integration tests: permission denied/retry flows.
3. E2E tests: dictation into app matrix (top 8 target apps).
4. Chaos tests: focus loss, app switching, and keyboard layout changes mid-session.

### Exit Criteria
1. Insertion success >=95% in internal compatibility matrix.
2. All permission failure paths recover without data loss.
3. No blocker bugs in top 5 target apps.

### Phase 3 - Model Manager, UX Controls, and History
### Goals
1. Ship model catalog/download/checksum system.
2. Add user controls for model selection, retention, and dictionary rules.

### Build Outputs
1. Model manager UI and backend.
2. First-run benchmark and recommendation flow.
3. History/stats module with encryption when enabled.
4. Signed model manifest integration and resumable installer pipeline for whisper.cpp-compatible model artifacts.
5. Personal dictionary auto-learn queue with user approval workflow.

### Testing in Phase
1. Unit tests: checksum verification, retention policies, dictionary transforms.
2. Integration tests: model switch behavior during idle and active states.
3. Security tests: tampered model rejection and quarantine.
4. Reliability tests: interrupted downloads, resume correctness, low-disk and corrupt-file recovery.
5. Accuracy tests: proper nouns and technical terms before/after auto-learn suggestions.

### Exit Criteria
1. Model recommendation flow works across hardware tiers.
2. Corrupt model recovery passes all test cases.
3. History retention controls verified end-to-end.
4. One-click in-app install passes success criteria on Windows and macOS test matrix.

### Current Rebaseline Status (Execution Truth)
1. Phase 0: complete.
2. Phase 1: complete for core runtime path (real whisper.cpp decode active); battery signoff evidence still needs a full >=30 minute capture artifact.
3. Phase 2: implemented and validated in current Windows scope; global OS-level hotkey registration evidence is still tracked as a hardening item.
4. Phase 3: implemented and validated in current Windows scope with model manager/history/dictionary command surfaces and evidence artifacts.
5. Phase 4-6: not started.

### Phase 4 - Hardening, Security, and Release Pipeline
### Goals
1. Harden cross-platform stability and security posture.
2. Complete signed installer and signed updater pipeline.

### Build Outputs
1. Packaging for Windows and macOS.
2. Signed update manifests and staged channel support.
3. Diagnostics export and redaction pipeline.
4. Privacy-safe product analytics spec (all opt-in, no raw audio, no sensitive text capture by default).

### Testing in Phase
1. Security tests: update signature validation, rollback integrity.
2. Performance tests: cold start, p50/p95/p99 latency, memory and CPU.
3. Regression suite: transcript snapshots and schema migration compatibility.
4. Red-team style privacy tests for accidental sensitive text/log leakage.

### Exit Criteria
1. Zero open high/critical security issues.
2. Signed build and update verification green in CI.
3. Release candidate passes full regression on OS matrix.

### Phase 5 - Beta Program and Feedback Loop
### Goals
1. Validate product-market fit signals and operational readiness.
2. Tune accuracy, latency, and insertion reliability using beta findings.

### Build Outputs
1. Beta/stable channel operations.
2. In-app diagnostics export flow and support runbook.
3. Updated compatibility matrix and known-limits documentation.
4. Parity backlog execution for top user-requested gaps (snippets, styles, commands as feature flags).

### Testing in Phase
1. Beta cohort telemetry via explicit diagnostics opt-in only.
2. Structured exploratory QA across app matrix.
3. Weekly reliability review with defect burn-down.
4. Structured usability tests for correction loop speed and cognitive load.

### Exit Criteria
1. Crash-free sessions >=99.5%.
2. Insertion success >=98%.
3. Correction rate <=12% on defined beta tasks.
4. TTFSD <=3 minutes and first-session activation >=80%.

### Phase 6 - GA Launch and Stabilization
### Goals
1. Launch to production with staged rollout and rollback readiness.
2. Maintain quality during scale-up and post-launch support.

### Build Outputs
1. GA installers and release notes.
2. Staged rollout plan (5% -> 25% -> 50% -> 100%).
3. Post-launch incident and escalation playbook.

### Testing in Phase
1. Final release acceptance suite.
2. Canary monitoring and rollback drill.
3. Daily bug triage and patch cadence.

### Exit Criteria
1. GA gates met for two consecutive release candidates.
2. Rollback dry run completed successfully.
3. Support SLAs active and staffed.

## 13) Unified Testing Strategy

### Test Pyramid
1. Unit tests for deterministic business logic and transforms.
2. Integration tests for module interactions and data flow.
3. E2E desktop tests for real user workflows and permissions.
4. Manual exploratory QA focused on app compatibility and UX edge cases.

### Required Test Categories
1. Functional correctness.
2. Performance and resource usage.
3. Accuracy benchmarks (WER/CER on curated corpus).
4. Security and privacy tests.
5. Upgrade/migration compatibility.
6. Failure recovery and chaos-style resilience checks.

### Release-Blocking Conditions
1. Failing security-critical tests.
2. Regression in insertion success or crash-free rate beyond thresholds.
3. Unsatisfied legal/compliance checklist.

## 14) CI/CD and Release Governance

1. CI on every PR: build, lint, unit/integration tests, security scans.
2. Nightly jobs: E2E matrix, performance baselines, compatibility checks.
3. Signed artifact generation only from protected release workflow.
4. Staged rollout with automatic stop rules for crash/regression spikes.
5. Rollback manifests kept for latest stable and one previous stable.
6. Model artifact publish job generates checksums and updates signed model manifest.

## 15) Risk Register (Initial)

1. OS permission friction reduces activation.
   1. Mitigation: permission education UX + clipboard-only fallback.
2. Insertion failures in high-value apps.
   1. Mitigation: app matrix ownership and per-app handling rules.
3. Model performance variance across hardware tiers.
   1. Mitigation: benchmark-driven recommendation + conservative defaults.
4. Update/signing pipeline mistakes.
   1. Mitigation: mandatory signing verification and rollback drills.
5. Scope creep before reliability goals are met.
   1. Mitigation: strict gate criteria and change control board.
6. Competitor feature gap perception (snippets, styles, team workflows).
   1. Mitigation: parity milestone plan and transparent roadmap communication.
7. "Privacy-first" message weakens if diagnostics are unclear.
   1. Mitigation: plain-language data handling controls and trust center docs.
8. Battery drain/thermal discomfort on laptops.
   1. Mitigation: performance governor and model auto-downshift policy.

## 16) Competitive Parity Plan (Wispr-Class Bar)

### Must-Win v1 Capabilities
1. Reliable hold-to-talk and release-to-insert in top productivity apps.
2. Fast insertion with low-latency local inference and clear state feedback.
3. Personal dictionary with easy corrections and export/import.
4. Stable app behavior with no focus theft and strong fallback UX.

### Fast-Follow v1.1 Capabilities (90 days after GA)
1. Snippets and reusable voice shortcuts.
2. App-aware style presets (email/chat/docs/coding).
3. Optional edit commands (undo last sentence, new line, bullet list).
4. Expanded language support tiers with published quality ranges.

### Fast-Follow v1.2 Capabilities (180 days after GA)
1. Cross-device sync (opt-in) for dictionary, snippets, and settings.
2. Team features: shared dictionary/snippets and usage dashboards.
3. Developer mode improvements for code dictation ergonomics.

## 17) Pricing and Premium Strategy Placeholder (Deferred)

1. v1 priority is best-in-class core dictation reliability and user trust.
2. Premium feature set is intentionally deferred until core quality gates are consistently met.
3. Candidate premium areas (to validate later): advanced commands, snippets, style presets, and team features.
4. Pricing model options (monthly, yearly, lifetime) will be decided after beta evidence on usage and retention.

## 18) Open Questions Requiring Decisions (Remaining)

1. For v1.1, do we prioritize snippets first or style presets first?
2. Which acquisition channel is primary for launch: developer communities, productivity creators, or organic search?
3. When should the dedicated frontend design sprint run (before beta or immediately after beta stabilization)?
4. Which premium features have enough proven value to justify paid tiers without weakening the free core experience?
5. Should global OS-level hotkey registration ship as a Phase 4 hard gate or Phase 4.1 release hardening item?

## 19) Operating Cadence and Ownership

1. Weekly product review: metrics, user feedback, and gate status.
2. Weekly engineering review: reliability, defect trends, and blocker removal.
3. Daily beta triage during Phase 5-6 with severity-based SLA.
4. Single-thread owner per phase with explicit backup owner.
5. Escalation policy: unresolved launch-blocking issue must be decisioned within 24 hours.
6. Monthly strategy review: pricing, positioning, and parity roadmap reprioritization.

## 20) Immediate Next Steps (First 10 Working Days)

1. Close Phase 1 battery gate with >=30 minute Windows capture artifact and hardware context.
2. Execute Phase 4 hardening backlog: signed installer/update verification and rollback drill.
3. Complete security/reliability closure for model pipeline and retention encryption evidence.
4. Validate app compatibility matrix with emphasis on global-hotkey and insertion reliability in real apps.
5. Publish updated legal/compliance checklist with owners and target release dates.
6. Freeze Phase 5 beta operations plan and telemetry/diagnostics runbook.

---

This document is the source of truth for v1 execution. Any scope or quality-gate change requires ADR or formal product decision log entry.
