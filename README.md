<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/hero-dark.svg">
  <img alt="VoiceWave: private dictation for Windows. Hold Ctrl + Win, speak, and your words appear in any app." src="docs/readme/hero-light.svg" width="100%">
</picture>

<br><br>

<a href="https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/latest"><img alt="Download for Windows" src="docs/readme/download.svg" width="300"></a>

<p>
  <a href="https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative?style=flat-square&label=release&color=1B8EFF"></a>
  <a href="https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/total?style=flat-square&label=downloads&color=0A2A8C"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-0F5FD7?style=flat-square"></a>
  <img alt="Windows 10 and 11" src="https://img.shields.io/badge/Windows-10%20%7C%2011-7ED8FF?style=flat-square&logo=windows&logoColor=white">
</p>

Free and open source · Works offline after a one-time model download · English

</div>

## How it works

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/demo-dark.svg">
  <img alt="Hold Ctrl + Win and speak. VoiceWave types into the app you are using, and spoken commands like new paragraph become formatting." src="docs/readme/demo-light.svg" width="100%">
</picture>

1. **Hold Ctrl + Win and talk.** Double-tap the shortcut to go hands-free; Escape cancels.
2. **Let go.** Your speech is transcribed on your PC: on the graphics card through Vulkan
   when it qualifies, otherwise on the CPU.
3. **The text lands at your cursor**, in whatever app you were using.

## What you get

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/features-dark.svg">
  <img alt="Features: never leaves your PC, fast on your GPU, works in any app, hold or hands-free, spoken commands, dictionary and snippets, on-device AI polish, free and open source." src="docs/readme/features-light.svg" width="100%">
</picture>

## Never leaves your PC

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/privacy-dark.svg">
  <img alt="Audio and text stay on your PC: microphone, on-device speech engine, your app. Cloud speech services are never used." src="docs/readme/privacy-light.svg" width="100%">
</picture>

There is no VoiceWave server and no account. Audio stays in memory and is transcribed on your
machine. VoiceWave goes online for three things only:

- **Model downloads**, once: the speech model at setup, and the AI polish model if you turn it on.
- **Update checks** against GitHub. Updates are signed and verified before they install.
- **Anonymous usage and problem reports**, on by default, which you can turn off during setup or
  in Settings → Diagnostics. [What they send](#anonymous-usage-and-problem-reports)

## AI polish, on your machine

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/readme/polish-dark.svg">
  <img alt="Optional AI polish runs on your PC: it removes filler words and fixes punctuation." src="docs/readme/polish-light.svg" width="100%">
</picture>

Optional and off by default. A small local model (Qwen2.5 1.5B through llama.cpp) tidies fillers,
casing and punctuation, with profiles for Standard, Coding and Writing (Literal skips it). A
fidelity check rejects rewrites that drift from what you said and keeps the plain transcript
instead. It is a one-time download of about 1 GB and needs a CPU with AVX2.

## Install

1. Download `VoiceWave.Local.Core_<version>_x64-setup.exe` from the
   [latest release](https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/releases/latest)
   and run it.
2. **Windows may warn "Windows protected your PC" (Unknown publisher).** The installer is not
   code-signed yet, so SmartScreen does not recognise it. Choose **More info → Run anyway**. Every
   installer is built in public by [GitHub Actions](.github/workflows/release.yml) from this
   repository, and in-app updates are signed and verified before they install.
3. **First launch** checks your microphone and downloads one speech model, chosen for your PC:
   about 488 MB (Small) up to 1.6 GB (Large Turbo). After that, dictation works offline.

Or install with `winget install VoiceWave.LocalCore`.

### Requirements

|                    | Needs                                                                                                                        |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| **Windows**        | 10 or 11, 64-bit                                                                                                             |
| **CPU**            | x64 with SSE4.1. AVX2 is needed for the GPU engine and AI polish.                                                            |
| **GPU** (optional) | A discrete NVIDIA, AMD or Intel graphics card with Vulkan 1.2. PCs with only integrated graphics use the CPU engine for now. |
| **Language**       | English                                                                                                                      |

## Compared with cloud dictation

|                             |           **VoiceWave**           |                       Wispr Flow                        |
| --------------------------- | :-------------------------------: | :-----------------------------------------------------: |
| Price                       |             **Free**              | $15/month for Pro; free tier capped at 2,000 words/week |
| Where speech is transcribed |          **On your PC**           |                    On their servers                     |
| Works offline               | **Yes**, after the model download |                           No                            |
| Account required            |              **No**               |                           Yes                           |
| Open source                 |       **Yes** (Apache-2.0)        |                           No                            |

Wispr Flow details from [wisprflow.ai/pricing](https://wisprflow.ai/pricing), April 2026.

## Anonymous usage and problem reports

On by default for new installs. They show us whether setup and dictation work for you, and what
breaks, so we can fix it. Reports go to [PostHog](https://posthog.com) on its EU Cloud, in a project
set to discard IP addresses. If you shared the older usage count, 0.7.7 asks you once and records
nothing new until you answer. One switch turns everything off, during setup or in
**Settings → Diagnostics**. Details are in the [privacy notice](docs/PRIVACY.md).

Every report carries a random install ID and a random ID for each launch, the app version, Windows
10 or 11, your graphics card maker, whether speech runs on the graphics card or the processor and
which model family, how you installed VoiceWave (winget, a direct download, or unknown), which
version you first installed and how many days ago (estimated for installs older than 0.7.7), and
whether the PC is a virtual machine. On top of that, VoiceWave sends only:

- when the app starts: whether it started with Windows, and whether the last run ended normally,
  crashed, hit an internal error, or was cut off by a restart or an update, what it was doing then
  and how long it had run;
- setup steps, and whether setup was finished or skipped;
- once for each day you dictate: how many dictations were started, worked, came back empty, failed,
  heard no speech, were retried or were hands free; how fast text appeared; how long you spoke; how
  many words were inserted; how often speech ran on the graphics card; how the text got in (typed,
  pasted, left on the clipboard, kept in History only, or held back from an app running as
  administrator); how many went into each kind of app (browser, code editor, terminal, chat, office,
  notes, AI app, or other); and your formatting style;
- whether your first dictation ran on the graphics card or the processor;
- failed dictations by type, with the kind of app when the text couldn't be inserted;
- speech helper crashes by cause, internal errors by area, and graphics card to processor fallbacks;
- model downloads: which model, how they ended, where a failed one stopped, retries, how far they
  got and how long they took;
- failed updates: the step, the kind of error and the version;
- shortcut setup problems;
- your answer to "Where did you hear about VoiceWave?", asked once at the end of setup on a new install;
- the reason you pick in "Something wrong?", and where you opened it.

It never sends audio, transcripts, dictionary or snippet content, window titles, app names, file
paths, device names, error messages, account details, or your IP address. An app's name is turned
into a kind of app on your PC and dropped there. 0.7.7 also still sends the older anonymous usage
count through TelemetryDeck, under the same switch; that stops in the next release.

**Support file.** **Settings → Diagnostics → Save support file** writes a zip with your diagnostics
report and recent logs to your Downloads folder. It never leaves your PC unless you attach it to an
issue yourself.

## What's new in 0.7.0

- **GPU dictation on any vendor.** whisper.cpp runs on discrete NVIDIA, AMD and Intel graphics
  cards through Vulkan, in its own worker process. No CUDA download.
- **Picked per PC.** A one-time check confirms the GPU works before VoiceWave switches to it, and
  dictation steps down to the CPU automatically if the GPU misbehaves.
- **Verified GPU models.** whisper.cpp models are checked against pinned SHA-256 hashes before
  they replace anything.

Full history in the [changelog](CHANGELOG.md).

## Support

Something not working? In the app, **Help → Report a problem** opens a pre-filled GitHub issue
with your app version, Windows version and speech engine. You can also
[open an issue](https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/issues/new/choose)
directly. Logs never contain what you dictated.

## Build from source

<details>
<summary>Tauri 2 · Rust · React 18 · whisper.cpp · faster-whisper</summary>

<br>

| Layer      | What it is                                                                           |
| ---------- | ------------------------------------------------------------------------------------ |
| App shell  | Tauri 2, Rust core (audio capture, hotkeys, insertion, storage)                      |
| Interface  | React 18, TypeScript, Tailwind                                                       |
| GPU speech | whisper.cpp 1.9.5 worker processes (`voicewave-asr-vk.exe`, `voicewave-asr-cpu.exe`) |
| CPU speech | faster-whisper in a bundled Python runtime                                           |
| AI polish  | Qwen2.5 1.5B through llama-cpp-python, on the CPU                                    |

```powershell
npm install
npm run tauri:dev     # desktop app (needs the Rust + Tauri prerequisites)
npm run dev           # interface only, in a browser
npm test              # frontend tests
npm run build         # type-check and build the interface
```

The release build stages the Python runtime and the whisper.cpp workers; see
[`release.yml`](.github/workflows/release.yml) and the scripts under
[`scripts/`](scripts/). More in [CONTRIBUTING.md](CONTRIBUTING.md).

</details>

## License

[Apache-2.0](LICENSE). Contributions welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

Speech models are downloaded separately under their own licenses:

- [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) by NVIDIA, the
  default model, licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
  VoiceWave uses the [GGUF conversion by ggml-org](https://huggingface.co/ggml-org/parakeet-GGUF)
  as published, without changes.
- Whisper by OpenAI (Small and Large Turbo), MIT.
