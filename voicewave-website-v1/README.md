# VoiceWave Website

Marketing site for VoiceWave desktop download distribution.

## Local

```powershell
npm install
npm run dev
```

## Build

```powershell
npm run build
```

## Download URL Wiring

All primary download CTAs are controlled by one environment variable:

`VITE_WINDOWS_DOWNLOAD_URL`

Example:

`VITE_WINDOWS_DOWNLOAD_URL=https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/latest/download/VoiceWave.Local.Core_0.2.1_x64-setup.exe`

See `voicewave-website/.env.example`.

## Render Static Site Settings

1. Root directory: `voicewave-website`
2. Build command: `npm install && npm run build`
3. Publish directory: `dist`
4. Environment variable: `VITE_WINDOWS_DOWNLOAD_URL=<your release asset URL>`
