use std::io::BufReader;
use std::path::PathBuf;
use std::process::ChildStdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::playback::AudioPlayer;
use crate::engine::{
    self, EngineBackend, EngineProcess, EngineResponse, EngineWriter,
};
use crate::translate::registry::Language;
use crate::tts;
use crate::vrchat;
use crate::vrchat::scroll::ScrollController;

#[derive(Debug, Clone, Serialize)]
pub struct PipelineStatus {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslateResult {
    pub text: String,
    pub segment_id: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AsrSegment {
    pub text: String,
    pub segment_id: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineConfig {
    pub device_id: String,
    pub source_lang: String,
    pub target_lang: String,
    pub tts_enabled: bool,
    pub model_path: String,
    #[serde(default)]
    pub tts_output_device: String,
    #[serde(default)]
    pub vrchat_osc_enabled: bool,
    #[serde(default = "default_vrchat_port")]
    pub vrchat_osc_port: u16,
    #[serde(default = "default_backend")]
    pub backend: String,
}

fn default_vrchat_port() -> u16 {
    9000
}

fn default_backend() -> String {
    "cpu".to_string()
}

impl PipelineConfig {
    fn source_language(&self) -> Language {
        Language::from_code(&self.source_lang).unwrap_or(Language::En)
    }

    fn target_language(&self) -> Language {
        Language::from_code(&self.target_lang).unwrap_or(Language::Zh)
    }

    fn engine_backend(&self) -> EngineBackend {
        EngineBackend::from_str(&self.backend).unwrap_or(EngineBackend::Cpu)
    }
}

/// Manages the real-time pipeline lifecycle.
pub struct Pipeline {
    running: Arc<AtomicBool>,
    engine_process: Option<EngineProcess>,
    audio_pump_handle: Option<std::thread::JoinHandle<()>>,
    reader_handle: Option<std::thread::JoinHandle<()>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            engine_process: None,
            audio_pump_handle: None,
            reader_handle: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Start the pipeline: audio capture → engine sidecar (ASR + translate) → TTS.
    pub fn start(
        &mut self,
        config: PipelineConfig,
        audio_receiver: Receiver<Vec<f32>>,
        app_handle: AppHandle,
    ) -> Result<()> {
        if self.is_running() {
            anyhow::bail!("Pipeline is already running");
        }

        self.running.store(true, Ordering::SeqCst);

        let _ = app_handle.emit(
            "pipeline-status",
            PipelineStatus {
                status: "running".to_string(),
                message: "Starting engine...".to_string(),
            },
        );

        // Spawn engine sidecar
        let backend = config.engine_backend();
        let mut engine = EngineProcess::spawn(backend, &app_handle)
            .with_context(|| format!("Failed to spawn {} engine", backend.display_name()))?;

        let writer = engine.writer();
        let stdout = engine.take_stdout().context("Failed to get engine stdout")?;

        // Send init commands to engine
        let model_path = if config.model_path.is_empty() {
            crate::asr::resolve_model_path(&app_handle, "base")
                .to_string_lossy()
                .to_string()
        } else {
            crate::asr::resolve_model_path(&app_handle, &config.model_path)
                .to_string_lossy()
                .to_string()
        };

        let source = config.source_language();
        let target = config.target_language();

        writer
            .send_init_asr(&model_path, source.whisper_code())
            .context("Failed to send init_asr")?;

        let models_root = resolve_models_root(&app_handle);
        let models_root_str = models_root
            .canonicalize()
            .unwrap_or(models_root)
            .to_string_lossy()
            .to_string();

        writer
            .send_init_translator(&models_root_str, &config.source_lang, &config.target_lang)
            .context("Failed to send init_translator")?;

        // Configure TTS
        tts::set_enabled(config.tts_enabled);

        // Configure VRChat OSC
        vrchat::set_enabled(config.vrchat_osc_enabled);
        let scroll_controller = if config.vrchat_osc_enabled {
            info!("VRChat OSC enabled (port {})", config.vrchat_osc_port);
            Some(ScrollController::new(
                config.vrchat_osc_port,
                self.running.clone(),
            ))
        } else {
            None
        };

        // Create AudioPlayer for TTS output
        let player = if config.tts_enabled && !config.tts_output_device.is_empty() {
            match AudioPlayer::new(&config.tts_output_device) {
                Ok(p) => {
                    info!("TTS output device: {}", config.tts_output_device);
                    Some(p)
                }
                Err(e) => {
                    error!("Failed to open TTS output device: {e:#}. TTS audio disabled.");
                    let _ = app_handle.emit(
                        "pipeline-status",
                        PipelineStatus {
                            status: "running".to_string(),
                            message: format!("Warning: TTS output device unavailable ({e})"),
                        },
                    );
                    None
                }
            }
        } else {
            None
        };

        let tts_voice = target.tts_voice().to_string();

        // Spawn audio pump thread: sends audio chunks to engine
        let pump_writer = writer.clone();
        let pump_running = self.running.clone();
        let audio_pump_handle = std::thread::Builder::new()
            .name("audio-pump".into())
            .spawn(move || {
                audio_pump_loop(audio_receiver, pump_writer, pump_running);
            })
            .context("Failed to spawn audio pump thread")?;

        // Spawn response reader thread: reads engine output, dispatches events
        let reader_writer = writer.clone();
        let reader_running = self.running.clone();
        let reader_ah = app_handle.clone();
        let reader_handle = std::thread::Builder::new()
            .name("engine-reader".into())
            .spawn(move || {
                engine_reader_loop(
                    stdout,
                    reader_writer,
                    reader_ah,
                    reader_running,
                    tts_voice,
                    player,
                    scroll_controller,
                );
            })
            .context("Failed to spawn engine reader thread")?;

        self.engine_process = Some(engine);
        self.audio_pump_handle = Some(audio_pump_handle);
        self.reader_handle = Some(reader_handle);

        info!(
            "Pipeline started: {} → {} (backend={})",
            source.display_name(),
            target.display_name(),
            backend.display_name()
        );
        Ok(())
    }

    pub fn stop(&mut self, app_handle: &AppHandle) {
        if !self.is_running() {
            return;
        }

        self.running.store(false, Ordering::SeqCst);

        // Kill engine process (sends shutdown, then force-kills)
        if let Some(mut engine) = self.engine_process.take() {
            engine.kill();
        }

        // Wait for threads to finish
        if let Some(handle) = self.audio_pump_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }

        let _ = app_handle.emit(
            "pipeline-status",
            PipelineStatus {
                status: "stopped".to_string(),
                message: "Pipeline stopped".to_string(),
            },
        );

        info!("Pipeline stopped");
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut engine) = self.engine_process.take() {
            engine.kill();
        }
    }
}

/// Audio pump: reads audio chunks from capture, sends to engine as base64 JSON.
fn audio_pump_loop(
    audio_rx: Receiver<Vec<f32>>,
    writer: EngineWriter,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::SeqCst) {
        match audio_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(chunk) => {
                if let Err(e) = writer.send_asr_audio(&chunk) {
                    if running.load(Ordering::SeqCst) {
                        error!("Failed to send audio to engine: {e}");
                    }
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Audio channel disconnected");
                break;
            }
        }
    }
}

/// Engine reader: reads JSON responses from engine stdout, dispatches Tauri events.
fn engine_reader_loop(
    mut stdout: BufReader<ChildStdout>,
    writer: EngineWriter,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
    tts_voice: String,
    player: Option<AudioPlayer>,
    scroll_controller: Option<ScrollController>,
) {
    let mut segment_counter: u64 = 0;

    while running.load(Ordering::SeqCst) {
        match engine::read_response(&mut stdout) {
            Ok(response) => match response {
                EngineResponse::Ok { .. } => {
                    // Init acknowledgment — update status
                }
                EngineResponse::Error { error } => {
                    warn!("Engine error: {error}");
                    let _ = app_handle.emit(
                        "pipeline-status",
                        PipelineStatus {
                            status: "warning".to_string(),
                            message: format!("Engine: {error}"),
                        },
                    );
                }
                EngineResponse::Capabilities { capabilities } => {
                    info!(
                        "Engine capabilities: gpu={}, vram={}MB",
                        capabilities.gpu, capabilities.vram_mb
                    );
                }
                EngineResponse::AsrResult { asr_result } => {
                    segment_counter += 1;
                    info!(
                        "ASR segment {}: {} (lang={})",
                        segment_counter, asr_result.text, asr_result.language
                    );

                    // Emit ASR segment for frontend
                    let _ = app_handle.emit(
                        "asr-segment",
                        AsrSegment {
                            text: asr_result.text.clone(),
                            segment_id: segment_counter,
                        },
                    );

                    // Send translation request
                    if let Err(e) = writer.send_translate(&asr_result.text) {
                        error!("Failed to send translate command: {e}");
                    }
                }
                EngineResponse::TranslateResult { translate_result } => {
                    info!("Translation: {}", translate_result.text);

                    // Emit translate result for frontend
                    let _ = app_handle.emit(
                        "translate-result",
                        TranslateResult {
                            text: translate_result.text.clone(),
                            segment_id: segment_counter,
                        },
                    );

                    // TTS
                    if let Err(e) =
                        tts::speak(&translate_result.text, &tts_voice, player.as_ref())
                    {
                        error!("TTS error: {e}");
                    }

                    // VRChat OSC
                    if let Some(ref controller) = scroll_controller {
                        // We need the original text for VRChat display.
                        // The reader doesn't have it at this point, so just use translated.
                        controller.update("", &translate_result.text, segment_counter);
                    }
                }
            },
            Err(e) => {
                if running.load(Ordering::SeqCst) {
                    error!("Engine read error: {e}");
                }
                break;
            }
        }
    }
}

/// Resolve the models root directory.
pub fn resolve_models_root(app_handle: &AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let models = resource_dir.join("models");
        if models.exists() {
            return models;
        }
    }
    let dev_path = PathBuf::from("resources/models");
    if dev_path.exists() {
        return dev_path;
    }
    let src_tauri_path = PathBuf::from("src-tauri/resources/models");
    if src_tauri_path.exists() {
        return src_tauri_path;
    }
    dev_path
}
