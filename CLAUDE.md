# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RTVT (Real-Time Voice Translation) is a Windows-first desktop app for VRChat. It captures audio, transcribes via Whisper, translates via NLLB-200, optionally speaks translations, and ships translated text to VRChat's chatbox over OSC. All inference is local — no external APIs.

**Pipeline**: Audio Capture (cpal) → ASR (whisper-rs) → Translation (ct2rs / NLLB-200-distilled-600M) → TTS (SAPI on Windows, `say` on macOS) + VRChat OSC chatbox

**Primary target: Windows.** macOS is supported for development; Linux is best-effort.

## Architecture

The project is a Cargo workspace with two crates:

- **`src-tauri`** — Tauri 2 host process (UI, audio I/O, settings, OSC, lifecycle).
- **`crates/rtvt-engine`** — sidecar inference binary (Whisper + CTranslate2/NLLB). Spawned by the host as a child process, communicates via line-delimited JSON over stdin/stdout.

Three engine variants share the same source, differentiated by Cargo features: `cpu` (default), `cuda`, `vulkan`. The Windows installer bundles all three as `externalBin` (`rtvt-engine-{cpu,cuda,vulkan}-x86_64-pc-windows-msvc.exe`); the host picks one at runtime based on the `backend` setting.

### Frontend (`src/`)

React 18 + TypeScript + Tailwind. Dark theme with subtitle overlay, settings drawer, status bar.

- **`App.tsx`** — pipeline lifecycle, Tauri command invocation (`start_pipeline`, `stop_pipeline`, `list_backends`, etc.).
- **`hooks/useTranslation.ts`** — listens for `asr-segment` and `translate-result` events; pairs them by `segment_id` into a rolling list (max 20).
- **`hooks/useSettings.ts`** — load/save persisted settings via `load_settings` / `save_settings`.
- **`hooks/useAudioDevices.ts`** — enumerate input devices.
- **`components/`** — `SubtitleOverlay`, `SettingsPanel`, `AudioSelector`, `StatusBar`.

Languages currently supported: `en`, `zh`, `ja` (defined in `crates/rtvt-engine/src/lang.rs` and mirrored in the frontend `BASE_LANGUAGES`). NLLB-200 itself supports 200+ languages; expansion is a data-driven change.

### Host backend (`src-tauri/src/`)

- **`audio/capture.rs`** — cpal input capture. Picks 16 kHz f32 if supported, otherwise resamples via rubato. Bounded `crossbeam_channel` (drops oldest on overflow) feeds the engine.
- **`audio/playback.rs`** — cpal output for TTS audio routing (e.g., to a virtual cable for VRChat).
- **`asr/whisper.rs`** — Whisper model registry + downloader (HuggingFace ggml models). The host **does not** run Whisper itself — that lives in the engine.
- **`translate/registry.rs`** — `Language` enum (single source of truth for language metadata: whisper code, NLLB code, display name, TTS voice).
- **`translate/download.rs`** — NLLB model downloader (with size validation).
- **`tts/`** — `windows.rs` (SAPI) is the production path. `macos.rs` uses the `say` command for dev.
- **`vrchat/`** — `osc.rs` sender, `format.rs` chatbox formatter, `scroll.rs` background thread that scrolls long messages within VRChat's chatbox rate limits.
- **`engine/mod.rs`** — sidecar lifecycle: spawn, JSON IPC writer, response reader, CUDA DLL path discovery.
- **`pipeline/realtime.rs`** — orchestrates audio pump → engine → frontend events → TTS → OSC. Manages the `running` flag and emits `pipeline-status`.

### Sidecar engine (`crates/rtvt-engine/src/`)

- **`main.rs`** — JSON command dispatch loop. Handles `init_asr`, `init_translator`, `asr`, `translate`, `shutdown`. Wall-clock VAD flush ensures tail-end speech is emitted even under continuous audio.
- **`asr.rs`** — `WhisperContext` + WebRTC VAD state machine. Trims silence, filters Whisper hallucinations, decides when a speech segment is complete.
- **`translate.rs`** — CTranslate2 NLLB translator with custom SentencePiece tokenizer that prepends NLLB language tokens.
- **`protocol.rs`** — `Request` / `Response` enums shared with the host (kept in sync manually with `src-tauri/src/engine/mod.rs`).
- **`lang.rs`** — pure code-string mapping (`whisper_code`, `nllb_code`). The engine treats languages as opaque strings; the host is the source of truth for metadata.

### IPC

- **Frontend ↔ Host**: `invoke("command_name", args)` returns `Result<T, String>`; events via `app_handle.emit("event", payload)` and `listen("event", cb)` in React hooks.
- **Host ↔ Engine**: each line on stdin is a JSON `Request`; each line on stdout is a JSON `Response`. Audio is sent as base64-encoded little-endian f32 PCM. Stderr is forwarded to the host log.

### Threading

- Tauri main thread: window + command dispatch.
- Audio capture thread: cpal callback (real-time, must not block).
- Audio pump thread (host): drains the bounded audio channel, sends `Asr` requests to engine.
- Engine reader thread (host): reads stdout, dispatches Tauri events, calls TTS + OSC.
- Engine stderr forwarder thread: pipes engine logs into the host logger.
- VRChat scroll thread: rate-limited OSC chatbox sender.
- Engine process: stdin reader thread + main loop with VAD wall-clock flush.

### State

- Host: Tauri managed state with `Mutex<T>` (`CaptureHandle`, `PipelineState`).
- Frontend: React hooks (`useState`, `useRef`). No global store.
- Settings persisted to `app_config_dir()/settings.json`.

## Build & Development

Models download to `src-tauri/resources/models/` (or the resource dir in a bundled app). Everything is fetched at runtime through the Settings panel — no model conversion or Python required.

```bash
make setup          # pnpm install + download models (Whisper base + NLLB)
make dev            # setup + build CPU engine + launch tauri dev
make build-engine   # cargo build -p rtvt-engine (CPU, dev)
make build-engine-cuda    # release CUDA variant (requires CUDA Toolkit)
make build-engine-vulkan  # release Vulkan variant (requires Vulkan SDK)
make build-win      # Windows NSIS installer (run on Windows)
make check          # cargo check (host only)
make check-engine   # cargo check (engine only)
```

CI builds the Windows installer with all three engine variants (`.github/workflows/build-windows.yml`). CUDA failures fail the workflow — there is no `continue-on-error` on engine builds.

### Windows MSVC CRT

All native code links against the static MSVC runtime (`+crt-static` → `/MT`). This eliminates `vcruntime140.dll` dependencies and lets the installer be a single `.exe`. The constraint:

- `RUSTFLAGS="-C target-feature=+crt-static"` for both host and all engine variants.
- `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` + `CMAKE_POLICY_DEFAULT_CMP0091=NEW` so CMake propagates `/MT` to all languages including CUDA.
- **No `/FORCE` or `/NODEFAULTLIB` workarounds.** If the linker complains about CRT mismatch, fix the offending dependency's build script — don't paper over it.

## Key Constraints

- Stable Rust (edition 2021), no nightly features.
- Tauri 2 (`@tauri-apps/api@^2`).
- Vite dev server on port 1420, HMR on 1421.
- All inference local; no telemetry, no external APIs.
- The engine's JSON protocol must stay byte-compatible with `src-tauri/src/engine/mod.rs` — when changing one, update the other in the same commit.
