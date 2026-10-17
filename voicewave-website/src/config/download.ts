// Fallback installer URL for the "Download Setup" CTAs, which link the .exe
// directly so the user gets it in one click. The CTAs prefer the newest
// published release's installer from the GitHub API (`useGitHubStats`), so
// this only serves while that loads or when the API is rate-limited.
//
// Bump it on release when convenient; VITE_WINDOWS_DOWNLOAD_URL in the
// Render env overrides it.
const DEFAULT_WINDOWS_DOWNLOAD_URL =
  'https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/download/v0.7.0/VoiceWave.Local.Core_0.7.0_x64-setup.exe'

export const windowsDownloadUrl =
  (import.meta.env.VITE_WINDOWS_DOWNLOAD_URL as string | undefined)?.trim() ||
  DEFAULT_WINDOWS_DOWNLOAD_URL
