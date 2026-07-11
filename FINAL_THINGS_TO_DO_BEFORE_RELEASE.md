# Final Things To Do Before Release

_Last updated: 2026-02-12_

## Purpose

This file tracks the non-optimization, non-code-review work that must be complete before VoiceWave release.

## Remaining Pre-Release Work

1. Release Reliability Gates � Critical

- Keep hard pass/fail gates green with fresh evidence on this branch.
- Include latency, stuck-listening, long-utterance completeness, and insertion success artifacts.

2. Supported App Compatibility Certification � Critical

- Complete and maintain pass matrix for core targets (Notepad, VS Code, browser editors, Slack, Notion, etc.).
- Verify real insertion correctness, not only transcript quality.

3. Installer/Update Trust Path � Critical

- Signed manifest verification.
- Resumable installer/download behavior.
- Tamper quarantine and recovery flow.
- Safe rollback behavior for bad updates/artifacts.

4. Privacy/Security Closure � High

- Ensure encryption-at-rest for retained transcript/history content.
- If deferred, document explicit ADR risk acceptance and closure plan.

5. Diagnostics + Supportability � High

- Keep local redacted diagnostics export stable.
- Ensure triage artifacts are sufficient to debug field issues quickly.

6. Release Operations / Canary Discipline � High

- Define release-blocking regressions.
- Define rollback triggers (latency, stuck listening, insertion failure rates).
- Lock canary monitoring checklist.

7. Battery/Thermal Signoff (30 min) � Medium/High

- Run and record sustained 30-minute dictation evidence on reference hardware.
- Close deferred battery gate before GA.

8. Documentation Truth Sync � Medium

- Keep README + phase docs consistent with current branch evidence.
- Remove or correct stale pass claims immediately.

## Notes

- Current product state is strong for internal/beta use.
- This checklist is intentionally release-focused and should be reviewed before any GA decision.
