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

# 3. (Optional) Firebase cloud features
Copy-Item .env.example .env
# Fill VITE_FIREBASE_* keys in .env

# 4. Run dev mode (frontend only)
npm run dev

# 5. Run full desktop app (requires Rust/Tauri setup above)
npm run tauri:dev
```

On first launch, open **Models** and install `fw-small.en` (~466 MB). Dictation will not work until a model is installed.

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
5. Build the Windows installer:
   ```powershell
   npm run tauri:build
   ```
6. Upload the `.exe` to the GitHub release as an asset named `VoiceWave.Local.Core_X.Y.Z_x64-setup.exe`.
7. Update the winget manifest in the `winget-pkgs` upstream repo.
8. Update the download URL on the marketing site (env var on Render).
9. Add an entry to [CHANGELOG.md](CHANGELOG.md).

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
- Model downloads are checksum-verified before activation.

---

## Need Help?

Open an issue or start a Discussion on GitHub. For security issues, do **not** open a public issue — email the maintainer directly (see the security policy in `docs/security/`).
