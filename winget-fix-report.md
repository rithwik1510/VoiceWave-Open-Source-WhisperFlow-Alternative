# VoiceWave winget release pipeline — repair report

Status: **Pipeline fixed, manifest built + validated, committed locally.** Not pushed, not
submitted — the public submission is left for you to run (see below).

## What was broken

1. **Invalid `WINGET` PAT (root cause of the 4-version rot).** `release.yml` (job
   `publish-winget`) and `winget-release.yml` both authenticate to microsoft/winget-pkgs with
   the `WINGET` Actions secret. The secret holds an expired/under-scoped classic PAT, so every
   automatic submission since 0.3.1 failed — but nobody was watching the Actions tab, so it
   looked fine. Live winget was stuck at **0.3.1** while GitHub releases reached **0.5.1**.
2. **`v`-prefix bug in `winget-release.yml`.** The manual retry workflow passed the raw release
   tag (`v0.5.1`) straight into the winget-releaser `version:` input, which would publish a
   malformed `v0.5.1` package version (winget versions carry no `v`). `release.yml` already
   stripped the `v` correctly; the retry workflow did not.

## What I fixed (committed locally, not pushed)

- **`.github/workflows/winget-release.yml`** — strip the leading `v` from the tag before using
  it as the winget `version:` (release-tag still uses the raw `v`-tag so the release asset is
  found). Added the loud preflight (below).
- **`.github/workflows/release.yml`** (`publish-winget` job) — added the same loud preflight.
  Its version stripping was already correct.
- **Loud-failure preflight** in both workflows: before the submit step, a PowerShell step
  authenticates the `WINGET` PAT against `api.github.com/user` and asserts it carries
  `public_repo` (or `repo`) scope. Missing / empty / expired / under-scoped token → `throw` →
  the job goes **red** (and GitHub emails on the failed run). This converts the silent rot into
  an obvious, actionable failure with the exact remediation in the log.
- **`winget-manifest/`** — 0.5.1 manifest (version + installer + en-US locale), modeled on the
  live 0.3.1 manifest, schema 1.12.0. `winget validate` → **"Manifest validation succeeded."**

## SHA256 verification (computed myself from the live release asset)

- Asset: `VoiceWave.Local.Core_0.5.1_x64-setup.exe` (size 113,810,515 bytes)
- `Get-FileHash -Algorithm SHA256` → `4432F50D6F637F59308CFE79DB5AB2D392513735067EA2955D5C9FF55BC97BA5`
- GitHub-reported asset digest → identical. **MATCH.** This exact hash is in
  `winget-manifest/VoiceWave.LocalCore.installer.yaml`.

## Your submission command (ready to run — publishes 0.5.1 to winget)

`wingetcreate` regenerates the manifest from the live installer and opens the PR itself, so you
do not even need the staged folder for this path (the folder is the validated reference /
manual fallback). `WINGET_CREATE_GITHUB_TOKEN` can come from your authenticated `gh` CLI:

```powershell
wingetcreate update VoiceWave.LocalCore `
  --urls "https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/download/v0.5.1/VoiceWave.Local.Core_0.5.1_x64-setup.exe" `
  --version 0.5.1 `
  --submit `
  --token $(gh auth token)
```

Notes:

- `gh auth token` must carry `repo`/`public_repo` scope (a normal `gh auth login` does). If the
  submit is rejected for auth, run `gh auth refresh -s public_repo` first, or paste a classic PAT.
- This opens a PR on microsoft/winget-pkgs. It does **not** touch your repo or need CI.
- Manual fallback (uses the staged, already-validated folder):
  `winget validate .\winget-manifest` then submit that folder with `komac submit` or a manual PR.

## Your remaining TODO — restore automatic publishing (fix the `WINGET` secret)

So future releases auto-publish (and the new preflight stays green):

1. Create a **classic** PAT: https://github.com/settings/tokens → _Generate new token (classic)_.
   Scope: check **`public_repo`** (under the `repo` group). Fine-grained PATs are not reliable
   with winget-releaser — use classic. Set a sane expiry and calendar a renewal.
2. Repo → **Settings → Secrets and variables → Actions → New repository secret**
   (`rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative`). Name it exactly **`WINGET`**,
   paste the token. (Update, don't add a second one.)
3. Push these workflow fixes, then either wait for the next release or run the
   **Publish to winget (manual retry)** workflow with input `v0.5.1` to backfill 0.5.1.

## Confirm the listing goes live after submission

- The submit opens a PR on microsoft/winget-pkgs; their Azure validation pipeline runs on it.
  Once a maintainer/bot merges it:
- `gh api repos/microsoft/winget-pkgs/contents/manifests/v/VoiceWave/LocalCore` should list a
  `0.5.1` directory (also browsable at
  https://github.com/microsoft/winget-pkgs/tree/master/manifests/v/VoiceWave/LocalCore).
- After CDN propagation (~30-60 min post-merge): `winget search VoiceWave.LocalCore` shows
  0.5.1, and `winget upgrade VoiceWave.LocalCore` moves 0.3.1 installs forward.
