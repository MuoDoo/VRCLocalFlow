use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use log::{error, info};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Listener, Manager};

use crate::asr::AsrEngine;
use crate::audio::playback::AudioPlayer;
use crate::translate::nllb::Translator;
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
}

fn default_vrchat_port() -> u16 {
    9000
}

impl PipelineConfig {
    fn source_language(&self) -> Language {
        Language::from_code(&self.source_lang).unwrap_or(Language::En)
    }

    fn target_language(&self) -> Language {
        Language::from_code(&self.target_lang).unwrap_or(Language::Zh)
    }
}

/// Manages the real-time pipeline lifecycle.
pub struct Pipeline {
    running: Arc<AtomicBool>,
    asr_engine: Option<AsrEngine>,
    translate_handle: Option<std::thread::JoinHandle<()>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            asr_engine: None,
            translate_handle: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Start the pipeline: audio capture → ASR → translate → TTS.
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

        // Emit running status
        let _ = app_handle.emit(
            "pipeline-status",
            PipelineStatus {
                status: "running".to_string(),
                message: "Pipeline started".to_string(),
            },
        );

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

        let source = config.source_language();
        let target = config.target_language();

        // Create AudioPlayer for TTS output if a device is configured
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
                            message: format!(
                                "Warning: TTS output device unavailable ({})",
                                e
                            ),
                        },
                    );
                    None
                }
            }
        } else {
            None
        };

        let tts_voice = target.tts_voice().to_string();

        // Resolve model ID to actual path
        let model_path = if config.model_path.is_empty() {
            None
        } else {
            let resolved = crate::asr::resolve_model_path(&app_handle, &config.model_path);
            Some(resolved.to_string_lossy().to_string())
        };

        // Start ASR engine with explicit source language
        let mut asr_engine = AsrEngine::new(model_path, Some(source.whisper_code().to_string()));
        asr_engine
            .start(audio_receiver, app_handle.clone())
            .context("Failed to start ASR engine")?;

        // Start translate+TTS loop
        let running = self.running.clone();

        let ah = app_handle.clone();
        let translate_handle = std::thread::Builder::new()
            .name("pipeline-translate".into())
            .spawn(move || {
                translate_loop(ah, running, source, target, tts_voice, player, scroll_controller);
            })
            .context("Failed to spawn translate thread")?;

        self.asr_engine = Some(asr_engine);
        self.translate_handle = Some(translate_handle);

        info!(
            "Pipeline started: {} → {}",
            source.display_name(),
            target.display_name()
        );
        Ok(())
    }

    pub fn stop(&mut self, app_handle: &AppHandle) {
        if !self.is_running() {
            return;
        }

        self.running.store(false, Ordering::SeqCst);

        // Stop ASR
        if let Some(mut engine) = self.asr_engine.take() {
            engine.stop();
        }

        // Wait for translate thread
        if let Some(handle) = self.translate_handle.take() {
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
        if let Some(mut engine) = self.asr_engine.take() {
            engine.stop();
        }
    }
}

/// Translate loop: listens for asr-result events via Tauri listener,
/// translates them, emits translate-result, and optionally runs TTS.
fn translate_loop(
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
    source: Language,
    target: Language,
    tts_voice: String,
    player: Option<AudioPlayer>,
    scroll_controller: Option<ScrollController>,
) {
    // Resolve and load translation models
    let models_root = resolve_models_root(&app_handle);
    let models_root_abs = models_root.canonicalize().unwrap_or_else(|_| models_root.clone());
    info!("Loading translation models from {:?} (resolved: {:?})", models_root, models_root_abs);

    let translator = match Translator::for_pair(&models_root, source, target) {
        Ok(t) => {
            info!("Translation models loaded successfully");
            let _ = app_handle.emit(
                "pipeline-status",
                PipelineStatus {
                    status: "running".to_string(),
                    message: "Translation models loaded".to_string(),
                },
            );
            Some(t)
        }
        Err(e) => {
            error!("Failed to load translation models: {e:#}. Translation disabled.");
            let _ = app_handle.emit(
                "pipeline-status",
                PipelineStatus {
                    status: "warning".to_string(),
                    message: format!("Translation unavailable: {e}"),
                },
            );
            None
        }
    };

    // Channel to receive ASR results from the Tauri event listener
    let (tx, rx) = crossbeam_channel::unbounded::<String>();

    // Subscribe to asr-result events emitted by the ASR engine
    let tx_clone = tx.clone();
    let _listener_id = app_handle.listen("asr-result", move |event: tauri::Event| {
        // event.payload() is a JSON string like {"text":"hello","language":"en"}
        if let Ok(result) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            let text = result
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !text.is_empty() {
                let _ = tx_clone.send(text);
            }
        }
    });

    let mut segment_counter: u64 = 0;

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(text) => {
                segment_counter += 1;
                info!("Translating segment {segment_counter}: {text}");

                // Re-emit ASR result with segment_id so frontend can pair it
                let _ = app_handle.emit(
                    "asr-segment",
                    AsrSegment {
                        text: text.clone(),
                        segment_id: segment_counter,
                    },
                );

                if let Some(ref translator) = translator {
                    let t_start = std::time::Instant::now();
                    match translator.translate(&text) {
                        Ok(translated) => {
                            info!("Translation result ({}ms): {translated}", t_start.elapsed().as_millis());
                            let _ = app_handle.emit(
                                "translate-result",
                                TranslateResult {
                                    text: translated.clone(),
                                    segment_id: segment_counter,
                                },
                            );

                            if let Err(e) = tts::speak(&translated, &tts_voice, player.as_ref()) {
                                error!("TTS error: {e}");
                            }

                            if let Some(ref controller) = scroll_controller {
                                controller.update(&text, &translated, segment_counter);
                            }
                        }
                        Err(e) => {
                            error!("Translation error: {e:#}");
                        }
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Resolve the models root directory.
/// In dev mode: `src-tauri/resources/models/`
/// In packaged app: resource dir from Tauri.
fn resolve_models_root(app_handle: &AppHandle) -> PathBuf {
    // Try the Tauri resource dir (works in packaged app)
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let models = resource_dir.join("models");
        if models.exists() {
            return models;
        }
    }

    // Dev mode fallback: try relative to CWD
    let dev_path = PathBuf::from("resources/models");
    if dev_path.exists() {
        return dev_path;
    }

    // Try relative to src-tauri
    let src_tauri_path = PathBuf::from("src-tauri/resources/models");
    if src_tauri_path.exists() {
        return src_tauri_path;
    }

    // Last resort
    dev_path
}
