// Direct asset URL for the latest released installer. The marketing site's
// "Download Setup" CTA hits this directly so the user gets the .exe in one
// click instead of bouncing through the GitHub releases page.
//
// Update this on every release. Or, set VITE_WINDOWS_DOWNLOAD_URL in the
// Render dashboard env to override without a code change.
const DEFAULT_WINDOWS_DOWNLOAD_URL =
  'https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/download/v0.5.7/VoiceWave.Local.Core_0.5.7_x64-setup.exe'

export const windowsDownloadUrl =
  (import.meta.env.VITE_WINDOWS_DOWNLOAD_URL as string | undefined)?.trim() ||
  DEFAULT_WINDOWS_DOWNLOAD_URL
