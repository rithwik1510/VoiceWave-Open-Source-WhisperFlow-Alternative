# Winget distribution

VoiceWave is listed in [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) as `VoiceWave.LocalCore`, so this works:

```powershell
winget install VoiceWave.LocalCore
```

Winget is a secondary install route. The primary route is the installer on GitHub Releases, because the winget listing can lag behind the latest release (see "Current status").

## Current status

Automatic publishing is **broken**. The `WINGET` repository secret holds a personal access token (PAT) that GitHub rejects, so the preflight step fails before anything is submitted. The listing in winget-pkgs is stuck at 0.6.1. Nothing in the app or the installer depends on winget, so releases are unaffected.

To fix it, follow [winget-pat-setup.md](winget-pat-setup.md). Until a new token is saved, publish nothing by hand and do not promote winget as the recommended install.

## How publishing works

There is no manifest in this repo. The workflows call [`vedantmgoyal9/winget-releaser@v2`](https://github.com/vedantmgoyal9/winget-releaser), which:

1. Finds the release asset matching `VoiceWave\.Local\.Core_.*_x64-setup\.exe$`.
2. Downloads it and computes its SHA-256.
3. Copies the latest manifests for `VoiceWave.LocalCore` from upstream winget-pkgs, updates the version, installer URL and hash, and pushes them to a branch of the token owner's fork.
4. Opens a pull request against `microsoft/winget-pkgs`.

Two workflows can run it:

- `.github/workflows/release.yml`, job `publish-winget`. Runs after the installer and `latest.json` are uploaded, on every published release. Its manual input is `tag`.
- `.github/workflows/winget-release.yml` ("Publish to winget (manual retry)"). Manual only, for re-submitting an existing release. Its input is `version`, and the value is the release tag, for example `v0.7.0`. The workflow strips the `v` because winget versions have none.

Both start with a preflight step that calls the GitHub API with the `WINGET` token and fails loudly if the token is missing, expired, or lacks `public_repo`. This exists because publishing used to fail silently across several releases.

## Retrying a release

```powershell
gh workflow run winget-release.yml -f version=v0.7.0
```

Or: Actions → "Publish to winget (manual retry)" → Run workflow → enter the tag.

## Caveats

- The installer name must match the regex `VoiceWave\.Local\.Core_.*_x64-setup\.exe$` in both workflows. If you rename the installer output, update both.
- The installer is not Authenticode-signed yet. Winget accepts unsigned installers, and users see the SmartScreen "Unknown publisher" prompt on first run. The in-app updater is a separate mechanism with its own minisign signature (see [auto-update.md](auto-update.md)).
- The Tauri NSIS installer does not emit a stable ProductCode, so winget matches the installed app by display name in Add/Remove Programs.
- Winget PRs go through Microsoft's automated checks and sometimes a human review. A merge can take from a few hours to a few days.
