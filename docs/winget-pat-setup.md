# Fix winget publishing: replace the WINGET token

Winget auto-publish is down because the `WINGET` repository secret is an invalid personal access token (PAT). The winget listing for `VoiceWave.LocalCore` is stuck at 0.6.1. This takes about five minutes and only the repo owner can do it.

Why a PAT at all: the default `GITHUB_TOKEN` cannot fork `microsoft/winget-pkgs` or open a pull request against it. `winget-releaser` needs a token that belongs to a real account, with a fork of winget-pkgs under that account.

## 1. Create the token

1. Sign in to GitHub as the account that should own the winget fork. This is normally `rithwik1510`, the account that owns this repo. Use the same account every time, because the fork lives under it.
2. Make sure that account has a fork of [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). If it does not, click Fork on that page first.
3. Go to Settings → Developer settings → Personal access tokens → **Tokens (classic)** → Generate new token → Generate new token (classic).
4. Note: `VoiceWave winget publish`.
5. Expiration: 1 year. Add a calendar reminder for 2 weeks before it expires. (A token with no expiry works but is a standing risk.)
6. Scopes: tick **`public_repo`** only. Nothing else is needed.
7. Generate, then copy the token now. GitHub shows it once.

Use a classic token. Fine-grained tokens are scoped to repositories you own and cannot push to a fork and open a pull request against `microsoft/winget-pkgs`, so they do not work here. The workflow preflight also checks for the classic `public_repo` scope.

## 2. Save it as the repo secret

1. Open the VoiceWave repo → Settings → Secrets and variables → Actions.
2. Edit the existing `WINGET` secret (or New repository secret if it is missing). The name must be exactly `WINGET`.
3. Paste the token and save.

## 3. Re-run the publish for the latest release

The 0.7.0 release already exists, so re-submit it with the manual workflow.

In the browser: Actions → "Publish to winget (manual retry)" → Run workflow → `version` = `v0.7.0` (the release tag, with the `v`).

Or from a terminal:

```powershell
gh workflow run winget-release.yml -f version=v0.7.0
```

The input is named `version`, and it takes the release tag. The workflow strips the `v` itself, so winget gets `0.7.0`. Future releases publish on their own through the `publish-winget` job in `release.yml`.

## 4. Confirm it worked

1. In the run, the step "Preflight — validate WINGET PAT" prints `WINGET PAT authenticated as '<login>'` and `WINGET PAT preflight OK.`
2. The "Submit manifest to winget-pkgs" step finishes green.
3. A pull request titled like "New version: VoiceWave.LocalCore version 0.7.0" appears at [microsoft/winget-pkgs/pulls](https://github.com/microsoft/winget-pkgs/pulls) from your account. Microsoft's bots validate it, and it merges after that, usually within a few days.
4. After the merge, give the winget source an hour or so, then run:

   ```powershell
   winget source update
   winget show VoiceWave.LocalCore
   ```

   It should report version 0.7.0.

If the preflight fails with an authentication error, the token was copied wrongly or has expired. If it fails on scopes, the token is missing `public_repo`. Regenerate it and repeat step 2.

## When the token expires

Winget publishing stops again, and the preflight in each release turns the `publish-winget` job red, which emails you. Repeat steps 1 to 3: generate a new classic token with `public_repo`, replace the `WINGET` secret, and re-run the workflow for any release that was missed. Regenerating an expired token (GitHub: Settings → Developer settings → the token → Regenerate token) keeps the same name and scopes.

Until winget is current, keep it out of the main install instructions. The installer from GitHub Releases is the supported route.
