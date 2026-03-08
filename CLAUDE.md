# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RTVT (Real-Time Voice Translation) is a desktop app built with **Tauri 2** (Rust backend + React/TypeScript frontend). It captures audio, transcribes via Whisper, translates via Opus-MT, and optionally speaks translations using system TTS. All inference runs locally—no external APIs.

**Pipeline**: Audio Capture (cpal) → ASR (whisper-rs) → Translation (ct2rs/Opus-MT) → TTS (system) → Output

## Build & Development Commands

```bash
make setup          # Install pnpm deps + download all models (first-time)
make dev            # Full setup + launch Tauri dev server
pnpm tauri dev      # Launch Tauri dev (if already set up)
pnpm dev            # Frontend-only Vite dev server (port 1420)
pnpm build          # Frontend production build (tsc + vite)
make check          # cargo check (Rust only, no build artifacts)
make models         # Download Whisper + Opus-MT models
make clean          # Remove converted models
```

Models download to `src-tauri/resources/models/`. Whisper model is `ggml-base.bin` (~148MB). Opus-MT models require Python deps (`make pip-deps`) for conversion.

## Architecture

### Frontend (`src/`)

React 18 + TypeScript + Tailwind CSS. Dark theme UI with subtitle overlay, settings drawer, and status bar.

- **App.tsx**: Main component. Manages pipeline state and settings. Invokes Tauri commands (`start_pipeline`, `stop_pipeline`).
- **hooks/useTranslation.ts**: Listens for `asr-result` and `translate-result` Tauri events. Maintains a rolling list of subtitle entries (max 20), pairing ASR results with translations by segment_id.
- **hooks/useAudioDevices.ts**: Lists audio input devices via Tauri command.
- **components/**: SubtitleOverlay (real-time display), SettingsPanel (config drawer), AudioSelector, StatusBar.

### Backend (`src-tauri/src/`)

Rust modules organized by pipeline stage:

- **audio/capture.rs**: Audio capture via cpal. Enumerates devices, negotiates sample rate (prefers 16kHz for Whisper), resamples via rubato if needed, converts to mono, sends chunks over crossbeam-channel.
- **asr/whisper.rs**: Whisper inference in a dedicated thread. Simple VAD (RMS energy threshold 0.01, triggers on 3s accumulated or 1s silence). Filters hallucination patterns. Emits `asr-result` events.
- **translate/opus_mt.rs**: Loads dual Opus-MT models (en→zh, zh→en) via ct2rs with SentencePiece tokenization. Direction enum: `EnToZh`, `ZhToEn`.
- **tts/**: Platform-dispatched. macOS uses `say` command (voices: Samantha/Tingting). Windows is stubbed for SAPI.
- **pipeline/realtime.rs**: Orchestrates the full pipeline. Spawns audio, ASR, and translate threads. Manages lifecycle via `start()`/`stop()` with Tauri event emission.
- **lib.rs**: Tauri command registration and managed state setup.

### IPC Pattern

- **Frontend → Backend**: `invoke("command_name", args)` (Tauri commands return `Result<T, String>`)
- **Backend → Frontend**: `app_handle.emit("event", data)` (events: `asr-result`, `translate-result`, `pipeline-status`)
- **Frontend event subscriptions**: `listen("event", callback)` in React hooks

### Threading Model

- **Tauri main thread**: Window management + command dispatch
- **Audio thread**: cpal callback (real-time, must not block)
- **ASR thread**: CPU-bound Whisper inference (dedicated thread)
- **Translate thread**: CPU-bound Opus-MT inference + TTS (dedicated thread)
- **Inter-thread**: crossbeam-channel for audio data, Tauri events for ASR/translate results

### State Management

- **Backend**: Tauri managed state with `Mutex<T>` — `CaptureHandle` (audio channel pair), `PipelineState` (pipeline singleton)
- **Frontend**: React hooks only (useState). No Redux or global store.

## Key Constraints

- Stable Rust (edition 2021), no nightly features
- Platform-conditional compilation via `#[cfg(target_os = "...")]` for TTS
- Models loaded at runtime from `src-tauri/resources/models/`, not bundled in binary
- Tauri 2 API (`@tauri-apps/api@^2`)
- Vite dev server on port 1420, HMR on port 1421
