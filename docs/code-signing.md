# Windows code signing

The installer is currently unsigned, so SmartScreen shows "Windows protected your PC" on first run. Signing is wired into `.github/workflows/release.yml` but **does nothing until you add the six GitHub secrets below**. No signing step runs, no config changes, and the build is exactly the unsigned build it is today.

This is Authenticode signing of the `.exe`. It is separate from the Tauri updater key (`TAURI_SIGNING_PRIVATE_KEY`), which signs `latest.json` payloads for auto-update and stays as is.

## How it works

With all secrets set, the workflow:

1. `cargo install trusted-signing-cli` (a few extra minutes; add `Swatinem/rust-cache` or a cache step if it bothers you).
2. Writes `$RUNNER_TEMP/tauri.signing.conf.json` containing only `bundle.windows.signCommand`.
3. Builds with `npm run tauri:build -- --bundles nsis --config <that file>`. Tauri then runs the command on the app exe and on the NSIS installer.
4. Fails the job unless `Get-AuthenticodeSignature` reports the installer as `Valid`.

`tauri.conf.json` has no `signCommand`, so local builds stay unsigned.

**Updater interplay:** Tauri signs the installer first, then produces the updater `.sig` over the final (signed) installer bytes, so `latest.json` and the signed file agree. The workflow's later rename (spaces to dots) does not change bytes. Confirm once on the first signed release by running an in-app update from the previous version.

**Not signed:** the bundled `python.exe`, DLLs, and `voicewave-asr-*.exe` ship unsigned inside the signed installer. That is normal; if Defender ever flags one, sign them in a step before `tauri:build` with the same CLI.

## Option A: Azure Trusted Signing (preferred)

Cost: about $10/month (Basic, 5,000 signatures). No hardware token, works from GitHub Actions. Microsoft has been renaming the service ("Artifact Signing"), so menu names may differ slightly.

1. **Azure subscription.** Create one (pay-as-you-go) if you do not have it.
2. **Create the account.** Portal, "Trusted Signing Accounts", Create. Pick a region and note the **account name** and its **endpoint** (for example `https://eus.codesigning.azure.net`; it must match the region).
3. **Identity validation.** In the account, "Identity validations", New. Choose Individual or Organization and upload government ID or business documents. Review takes from minutes to a few days. Availability is country-limited (individuals were US/Canada only at last check; organizations need a verifiable business history), so confirm your country is supported before paying for anything.
4. **Certificate profile.** After validation passes, create a "Public Trust" certificate profile and note its **name**.
5. **App registration.** Entra ID, App registrations, New. Note the **tenant ID** and **client ID**. Under Certificates and secrets, create a client secret and copy its **value**.
6. **Permission.** On the Trusted Signing account, Access control (IAM), add role assignment **Trusted Signing Certificate Profile Signer** to that app registration.
7. **GitHub secrets** (repo Settings, Secrets and variables, Actions):

   | Secret                           | Value                                |
   | -------------------------------- | ------------------------------------ |
   | `AZURE_TENANT_ID`                | tenant ID from step 5                |
   | `AZURE_CLIENT_ID`                | client ID from step 5                |
   | `AZURE_CLIENT_SECRET`            | client secret value from step 5      |
   | `AZURE_TRUSTED_SIGNING_ENDPOINT` | endpoint from step 2                 |
   | `AZURE_TRUSTED_SIGNING_ACCOUNT`  | account name from step 2             |
   | `AZURE_TRUSTED_SIGNING_PROFILE`  | certificate profile name from step 4 |

   All six are required; if any is missing the signing steps are skipped and the log says which ones.

## Option B: OV/EV certificate instead

Roughly $200-$500/year. Since 2023 the private key must live on a hardware token or cloud HSM, so a plain `.pfx` in a GitHub secret is not possible. Use a vendor's cloud signing service (SSL.com eSigner, DigiCert KeyLocker, ...) and replace the `signCommand` string in the "Configure code signing" step with the vendor's command, for example `signtool sign /sha1 <thumbprint> /fd sha256 /tr http://timestamp.digicert.com /td sha256 %1` once the vendor's signtool/KSP is installed on the runner. An OV cert still starts with no SmartScreen reputation; EV historically cleared it immediately.

## Verify a signed build

```powershell
signtool verify /pa /v "VoiceWave.Local.Core_<version>_x64-setup.exe"
# or
Get-AuthenticodeSignature "VoiceWave.Local.Core_<version>_x64-setup.exe" | Format-List Status, SignerCertificate
```

Status must be `Valid`, the signer should be your validated identity, and the signature should carry a timestamp (Trusted Signing adds one) so it stays valid after the certificate rotates. Right-click the installer, Properties, "Digital Signatures" shows the same thing.
