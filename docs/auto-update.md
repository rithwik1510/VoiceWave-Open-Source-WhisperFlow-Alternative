# Auto-Update

VoiceWave ships an in-app auto-updater built on
[`tauri-plugin-updater`](https://v2.tauri.app/plugin/updater/). On launch the app
checks the latest GitHub release and, if a newer **signed** build exists, shows a
one-click **Install & Restart** prompt.

## How it works

```
App launch
  └─ UpdatePrompt (src/components/UpdatePrompt.tsx)
       └─ checkForUpdate()  ── GET ──▶ releases/latest/download/latest.json
            ├─ no update / offline → render nothing
            └─ update found → dialog → installPendingUpdate()
                 └─ download .exe ─ verify signature against pubkey ─ run NSIS (passive) ─ relaunch
```

- **Endpoint** (`src-tauri/tauri.conf.json` → `plugins.updater.endpoints`):
  `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`.
  This always resolves to the newest **non-prerelease** release.
- **Signature**: every installer is signed at build time with a minisign-style
  key. The app only installs artifacts whose signature verifies against the
  `pubkey` baked into `tauri.conf.json`. This is independent of (and unrelated to)
  an Authenticode / SmartScreen certificate.
- **Install mode**: `passive` — the NSIS installer shows a progress UI with no
  prompts, then the app relaunches.

## Signing keys

The keypair was generated with the Tauri CLI:

```powershell
npx @tauri-apps/cli signer generate -w src-tauri/.tauri/voicewave-updater.key
```

- `src-tauri/.tauri/voicewave-updater.key` — **private key. Never commit.**
  The folder is gitignored.
- `src-tauri/.tauri/voicewave-updater.key.pub` — public key; its contents are the
  `pubkey` value in `tauri.conf.json`.

> ⚠️ **Security**
> - The committed `pubkey` is safe to share. The private key is **not**.
> - Whoever holds the private key can sign updates that auto-install on every
>   user. Treat it like a release-signing secret.
> - The signing key is password-protected. The private key and its password are
>   stored only as GitHub Actions secrets (`TAURI_SIGNING_PRIVATE_KEY`,
>   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) and in the local gitignored
>   `src-tauri/.tauri/` folder — back both up somewhere safe.
> - **If the private key is lost, auto-update breaks permanently** (you can no
>   longer sign updates the installed base will accept). Back it up.

### Required GitHub Actions secrets

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `src-tauri/.tauri/voicewave-updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The key password (`""` if generated with `--ci`) |
| `WINGET` | Classic PAT with `public_repo` (for the winget PR) |

## Release process

Fully automated via `.github/workflows/release.yml` (triggers on a published,
non-prerelease GitHub Release):

1. Bump the version in **all three**: `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.toml`, `package.json`. It must match the release tag
   (tag `v0.4.0` → version `0.4.0`). The build fails fast if they disagree.
2. Commit, tag, and publish a GitHub Release for `vX.Y.Z` (release notes become
   the update's "What's new").
3. CI (`release.yml`) then:
   - builds + signs the NSIS installer,
   - generates `latest.json` (`scripts/release/generate-latest-json.ps1`),
   - uploads the installer **and** `latest.json` to the release,
   - submits the winget update (`publish-winget` job, after the upload).

### Building / signing locally

`npm run tauri:build` auto-loads `src-tauri/.tauri/voicewave-updater.key` into
`TAURI_SIGNING_PRIVATE_KEY` (see `scripts/tauri/run-tauri-build-windows.ps1`), so
local builds also emit a signed installer + `<installer>.exe.sig`. To produce
`latest.json` from a local build:

```powershell
./scripts/release/generate-latest-json.ps1 `
  -Version 0.4.0 `
  -InstallerPath "C:\vw-target\release\bundle\nsis\VoiceWave.Local.Core_0.4.0_x64-setup.exe" `
  -Notes "What changed in this release."
```

## Files

| File | Role |
| --- | --- |
| `src-tauri/tauri.conf.json` | `plugins.updater` config + `bundle.createUpdaterArtifacts` |
| `src-tauri/src/lib.rs` | Registers `updater` + `process` plugins |
| `src-tauri/capabilities/default.json` | `updater:default`, `process:default` permissions |
| `src/lib/updater.ts` | check / download+install / relaunch wrapper |
| `src/components/UpdatePrompt.tsx` | The launch-time prompt UI |
| `scripts/release/generate-latest-json.ps1` | Builds the update manifest |
| `.github/workflows/release.yml` | Build → sign → latest.json → winget |
