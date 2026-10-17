# Contributing to VoiceWave

Thanks for your interest. This guide covers how to get the project running locally, how to run tests, the release process, and the code style conventions we follow.

---

## Prerequisites

| Tool                      | Version        | Purpose                  |
| ------------------------- | -------------- | ------------------------ |
| Node.js                   | 20+            | Frontend build / scripts |
| Rust                      | stable (1.76+) | Core runtime             |
| Python                    | 3.10–3.12      | faster-whisper worker    |
| Tauri CLI                 | v2             | Desktop shell            |
| Visual Studio Build Tools | 2022           | Windows C++ toolchain    |

Install Tauri CLI:

```powershell
cargo install tauri-cli --version "^2"
```

Install Python dependencies:

```powershell
pip install faster-whisper
```

---

## Local Setup

```powershell
# 1. Clone
git clone https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative.git
cd VoiceWave-Open-Source-WhisperFlow-Alternative

# 2. Install JS dependencies
npm install

# 3. Run dev mode (frontend only)
npm run dev

# 4. Run full desktop app (requires Rust/Tauri setup above)
npm run tauri:dev
```

No speech model is bundled. On first launch, first-run setup downloads one (about 488 MB for Small up to 1.6 GB for Large Turbo, chosen for your PC). Dictation will not work until a model is installed.

> **Cloud sign-in and sync are dormant, developer-only code.** They are compiled out of release builds, and the release workflow injects no `VITE_FIREBASE_*` values, so shipped installers contain no Firebase. You do not need them to work on VoiceWave. If you are developing that code path, copy `.env.example` to `.env` and fill the `VITE_FIREBASE_*` keys.

---

## Running Tests

```powershell
# Frontend unit tests (Vitest)
npm run test -- --run

# Frontend tests with coverage
npm run test:coverage

# Rust library tests
cargo test --manifest-path src-tauri/Cargo.toml

# Full phase validation suite
npm run phase3:validate
npm run phase4:gate
npm run phase5:gate
```

All of the above must pass before a PR is merged.

---

## Release Process

Releases are gated by `npm run release:gate`, which runs phase 4 + phase 5 checks plus risk/compliance validation. Do not bypass it.

**Steps to cut a release:**

1. Confirm `npm run release:gate` passes on a Windows runner with fresh evidence (≤ 7 days old).
2. Bump the version in `src-tauri/tauri.conf.json` and `package.json`.
3. Commit: `chore(release): bump version to X.Y.Z`
4. Tag: `git tag vX.Y.Z && git push origin vX.Y.Z`
5. Publish a GitHub Release for the tag. `.github/workflows/release.yml` builds and signs the installer, then uploads `VoiceWave.Local.Core_X.Y.Z_x64-setup.exe` and `latest.json` to the release (see [docs/auto-update.md](docs/auto-update.md)). To build locally instead, run `npm run tauri:build`.
6. Winget is submitted by the `publish-winget` job in the same workflow. It currently fails because the `WINGET` token is invalid, so there is no manual step. Fix it with [docs/winget-pat-setup.md](docs/winget-pat-setup.md).
7. The marketing site's download buttons follow the newest release automatically. No env var change is needed.
8. Add an entry to [CHANGELOG.md](CHANGELOG.md).

---

## Code Style

**Rust**

- `cargo fmt` before every commit (enforced by CI).
- `cargo clippy -- -D warnings` must be clean.
- Error handling: use `anyhow` for application code, typed errors for library boundaries.
- No `unwrap()` in non-test code — use `?` or explicit error handling.

**TypeScript / React**

- ESLint config is at `eslint.config.mjs` — run `npm run lint` before pushing.
- Components live in `src/components/`, pages in `src/pages/`.
- Tailwind only — no inline styles, no CSS modules.
- No `any` types without a comment explaining why.

**Commits**

Follow Conventional Commits:

```
feat(scope): short description
fix(scope): short description
chore(scope): short description
docs(scope): short description
```

Scope is the subsystem: `audio`, `insertion`, `inference`, `models`, `ui`, `website`, `scripts`, `release`.

---

## Pull Requests

- Keep PRs focused. One logical change per PR.
- Title must follow the commit convention above.
- Include a short description of **what** changed and **why**.
- All CI checks must be green before review is requested.
- For changes touching the audio pipeline or insertion engine, include a manual test note describing what you tested and on what hardware.

---

## Architecture Notes

The primary boundary document is [docs/rfc/0001-system-architecture.md](docs/rfc/0001-system-architecture.md). Read it before touching core runtime modules. Key invariants:

- Audio never leaves the device in the production path.
- The UX state machine must stay deterministic: `idle → listening → transcribing → inserted/error`.
- Insertion fallback order is `direct → clipboard → history` — do not reorder or skip levels.
- Model downloads are verified before activation. The whisper.cpp GGML models are checked against pinned SHA-256 hashes; faster-whisper and AI polish models are checked for completeness and size over HTTPS. Do not describe the latter as checksum-verified.

---

## Need Help?

Open an issue or start a Discussion on GitHub. For security issues, do **not** open a public issue — email the maintainer directly (see the security policy in `docs/security/`).
