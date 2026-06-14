# Winget Distribution

VoiceWave publishes to the Microsoft Store for Developers (winget) so users can install with:

```powershell
winget install VoiceWave.LocalCore
```

Or by moniker:

```powershell
winget install voicewave
```

## Files in this folder

- `VoiceWave.LocalCore.yaml` — version manifest
- `VoiceWave.LocalCore.installer.yaml` — installer metadata (URL, SHA256, silent flags)
- `VoiceWave.LocalCore.locale.en-US.yaml` — display name, description, tags

These files are **source templates**. The actual published manifests live in [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) under `manifests/v/VoiceWave/LocalCore/<version>/`.

## First-time submission (manual, one time only)

Microsoft reviews new packages more carefully than version bumps, so the first PR is manual.

1. Install wingetcreate:

   ```powershell
   winget install wingetcreate
   ```

2. Fork [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) to your GitHub account.

3. Compute the SHA256 of the release installer:

   ```powershell
   $url = "https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/download/v0.2.0/VoiceWave.Local.Core_0.2.0_x64-setup.exe"
   $tmp = New-TemporaryFile
   Invoke-WebRequest $url -OutFile $tmp
   (Get-FileHash $tmp -Algorithm SHA256).Hash
   ```

4. Paste the SHA256 into `VoiceWave.LocalCore.installer.yaml` (replacing `REPLACE_WITH_SHA256_OF_RELEASE_ASSET`).

5. Validate the manifests locally:

   ```powershell
   winget validate --manifest winget/
   ```

6. Submit the PR:

   ```powershell
   wingetcreate submit --token <your-github-pat> winget/
   ```

7. Wait for the Microsoft automated checks + human review to pass. Usually 1-3 days for first submission.

## Automated releases (after first submission is accepted)

Subsequent version bumps are handled by `.github/workflows/release.yml`. When a new GitHub release is published, that workflow first builds and signs the installer + uploads `latest.json` (for the in-app auto-updater), then its `publish-winget` job — sequenced **after** the installer asset exists — submits the winget update via `winget-releaser`, which:

1. Downloads the release installer asset
2. Computes SHA256
3. Generates updated manifests
4. Opens a PR against `microsoft/winget-pkgs`

> The standalone `.github/workflows/winget-release.yml` is now **manual-only** (`workflow_dispatch`), kept for re-submitting an existing release. It no longer auto-runs on release, to avoid racing the CI build for the installer asset.

### Required repo secret

The workflow needs a GitHub PAT stored as `WINGET`. The default `GITHUB_TOKEN` cannot fork external repos.

1. Create a classic PAT at https://github.com/settings/tokens with scope: `public_repo`.
2. Add it to the repo at Settings -> Secrets and variables -> Actions -> New repository secret, named `WINGET`.

(See `docs/auto-update.md` for the `TAURI_SIGNING_PRIVATE_KEY` secrets the build step needs.)

### Triggering manually

If you need to resubmit a version (or retry after a failed auto-run):

1. Go to Actions -> "Publish to winget (manual retry)" -> Run workflow
2. Enter the release tag (e.g. `v0.2.0`)

## Caveats

- **Installer name must match the regex** `VoiceWave\.Local\.Core_.*_x64-setup\.exe$` in the workflow. If you rename the installer output, update the regex.
- **Code signing is recommended but not required.** Unsigned installers still pass winget review but users may see SmartScreen warnings on first run. Once we have an Authenticode cert, add it to the Tauri bundle config.
- **ProductCode is not set** because the Tauri NSIS installer does not emit a stable ProductCode. Winget falls back to DisplayName matching in Add/Remove Programs, which works for our case.
- **License** in the locale manifest is currently `MIT`. If the repo license is different, update `VoiceWave.LocalCore.locale.en-US.yaml` and commit a `LICENSE` file to the repo root.
