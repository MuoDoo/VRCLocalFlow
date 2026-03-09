# RTVT - Real-Time Voice Translation

[日本語](./README.ja.md) | [中文](./README.zh.md) | English

A desktop application for real-time voice translation, designed for VRChat and other voice communication scenarios. Powered by Tauri 2 (Rust + React/TypeScript), all speech recognition and translation run locally — no external APIs required.

**Pipeline**: Audio Capture → ASR (Whisper) → Translation (NLLB-200) → TTS → VRChat OSC

## Features

- **Real-time voice transcription** — Whisper.cpp with VAD (Voice Activity Detection)
- **Multilingual translation** — NLLB-200-distilled supporting 200+ languages
- **VRChat integration** — Send translated subtitles to VRChat chatbox via OSC
- **100% offline** — All inference runs locally on your machine
- **GPU acceleration** — Optional CUDA / Metal / OpenBLAS support
- **Cross-platform** — Windows (primary), macOS, Linux (partial)

---

## Usage

### System Requirements

| Item | Requirement |
|------|-------------|
| OS | Windows 10/11 (primary), macOS, Linux |
| CPU | 4+ cores recommended |
| RAM | 8 GB+ |
| Storage | ~500 MB (app + models) |
| GPU | Optional (CUDA for acceleration) |

### Installation

Download the latest release from the [Releases](https://github.com/MuoDoo/VRCLocalFlow/releases) page and run the installer.

### Getting Started

1. **Launch the app** — On first launch, download the required models from the Settings panel.
2. **Select audio device** — Choose your microphone or system audio input.
3. **Choose languages** — Set source and target languages in Settings.
4. **Start pipeline** — Click the Start button to begin real-time translation.
5. **VRChat OSC** — Translated text is automatically sent to VRChat chatbox (UDP port 9000).

### Model Management

Models are downloaded on demand through the app's Settings panel:

| Model | Size | Purpose |
|-------|------|---------|
| Whisper tiny | ~75 MB | Fast, lower accuracy ASR |
| Whisper base | ~142 MB | Balanced ASR (default) |
| Whisper small | ~466 MB | Higher accuracy ASR |
| NLLB-200-distilled | ~600 MB | Translation (200+ languages) |

### VRChat Setup

Ensure VRChat OSC is enabled:
1. In VRChat, open the Action Menu.
2. Navigate to **Options → OSC → Enable**.
3. RTVT sends messages to `127.0.0.1:9000` by default.

---

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) (stable, edition 2021)
- [Python 3](https://www.python.org/) (for NLLB model conversion)
- Platform-specific Tauri 2 dependencies — see [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)

### Quick Start

```bash
# Clone the repository
git clone https://github.com/MuoDoo/VRCLocalFlow.git
cd VRCLocalFlow

# First-time setup: install dependencies + download models
make setup

# Launch development server
make dev
```

### Build Commands

| Command | Description |
|---------|-------------|
| `make setup` | Install pnpm deps + download all models |
| `make dev` | Full setup + launch Tauri dev server |
| `make dev-cuda` | Dev mode with CUDA GPU acceleration |
| `pnpm tauri dev` | Launch Tauri dev (if already set up) |
| `pnpm dev` | Frontend-only Vite dev server (port 1420) |
| `pnpm build` | Frontend production build |
| `make check` | Rust type check (no build artifacts) |
| `make models` | Download Whisper + NLLB models |
| `make pip-deps` | Install Python deps for model conversion |
| `make build-win` | Build Windows NSIS installer |
| `make build-win-cuda` | Windows build with CUDA |
| `make build-mac` | Build macOS .dmg (aarch64) |
| `make clean` | Remove downloaded models |

### Project Structure

```
VRCLocalFlow/
├── src/                        # Frontend (React + TypeScript)
│   ├── App.tsx                 # Main component, pipeline state
│   ├── components/             # UI components
│   │   ├── AudioSelector.tsx   # Audio device picker
│   │   ├── SettingsPanel.tsx   # Settings drawer
│   │   ├── StatusBar.tsx       # Pipeline status display
│   │   └── SubtitleOverlay.tsx # Real-time subtitle display
│   └── hooks/                  # React hooks
│       ├── useAudioDevices.ts  # Audio device enumeration
│       ├── useSettings.ts      # Settings persistence
│       └── useTranslation.ts   # ASR/translation event listener
├── src-tauri/                  # Backend (Rust + Tauri 2)
│   ├── src/
│   │   ├── lib.rs              # Tauri commands & managed state
│   │   ├── audio/              # Audio capture & playback (cpal)
│   │   ├── asr/                # Whisper ASR + VAD
│   │   ├── translate/          # NLLB translation (CTranslate2)
│   │   ├── tts/                # Platform TTS (SAPI / macOS)
│   │   ├── pipeline/           # Pipeline orchestration & threading
│   │   └── vrchat/             # VRChat OSC integration
│   └── resources/models/       # Downloaded models (runtime)
├── Makefile                    # Build & setup commands
└── package.json                # Frontend dependencies
```

### Architecture

```
┌─────────────┐    ┌───────────┐    ┌─────────────┐    ┌─────┐
│ Audio Input  │───▶│  Whisper   │───▶│   NLLB-200  │───▶│ TTS │
│   (cpal)    │    │   (ASR)   │    │ (Translate) │    │     │
└─────────────┘    └───────────┘    └──────┬──────┘    └─────┘
                                           │
                                    ┌──────▼──────┐
                                    │ VRChat OSC  │
                                    │  (Chatbox)  │
                                    └─────────────┘
```

- **Threading**: Audio, ASR, and Translation each run on dedicated threads, connected via crossbeam channels.
- **IPC**: Frontend ↔ Backend communication uses Tauri commands (invoke) and events (emit/listen).
- **State**: Backend uses `Mutex<T>` managed state; Frontend uses React hooks.

### Feature Flags (Cargo)

| Feature | Description |
|---------|-------------|
| `cuda` | NVIDIA CUDA GPU acceleration |
| `openblas` | OpenBLAS CPU acceleration |
| `metal` | Apple Metal GPU acceleration |
| `hipblas` | AMD HIP GPU acceleration |

```bash
# Example: build with CUDA
make build-win-cuda
```

---

## License

[MIT](./LICENSE) © 2026 MuoDoo
