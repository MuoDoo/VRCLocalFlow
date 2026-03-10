use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use futures_util::StreamExt;
use log::{error, info, warn};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tauri::{AppHandle, Emitter, Manager};
use webrtc_vad::{Vad, SampleRate, VadMode};
use whisper_rs::{FullParams, SamplingStrategy, SystemInfo, WhisperContext, WhisperContextParameters};

const DEFAULT_MODEL_ID: &str = "base";
const MODELS_SUBDIR: &str = "models";
const SAMPLE_RATE: usize = 16_000;
const SILENCE_THRESHOLD: f32 = 0.01;

// — VAD parameters —
const VAD_FRAME_MS: usize = 30;
const VAD_FRAME_SAMPLES: usize = SAMPLE_RATE * VAD_FRAME_MS / 1000; // 480 samples
const MAX_SPEECH_SECONDS: f32 = 15.0;
const MAX_SPEECH_SAMPLES: usize = (SAMPLE_RATE as f32 * MAX_SPEECH_SECONDS) as usize;
const END_OF_SPEECH_MS: usize = 700;
const END_OF_SPEECH_SAMPLES: usize = SAMPLE_RATE * END_OF_SPEECH_MS / 1000;
const MIN_SPEECH_MS: usize = 250;
const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE * MIN_SPEECH_MS / 1000;
const PRE_SPEECH_MS: usize = 300;
const PRE_SPEECH_SAMPLES: usize = SAMPLE_RATE * PRE_SPEECH_MS / 1000;

/// Determine optimal thread count for whisper inference.
/// Uses half the physical cores to leave headroom for the translation thread
/// and avoid cache thrashing from thread oversubscription.
fn optimal_thread_count() -> i32 {
    let physical = num_cpus::get_physical();
    // Use half the physical cores (min 2, max 8).
    // On a 9800X3D (8 cores), this gives 4 threads for Whisper,
    // leaving cores free for CTranslate2 and the rest of the system.
    let threads = (physical / 2).max(2).min(8);
    threads as i32
}

/// Patterns that whisper outputs for non-speech audio — filter these out.
const JUNK_PATTERNS: &[&str] = &[
    "[BLANK_AUDIO]",
    "[MUSIC]",
    "[APPLAUSE]",
    "[LAUGHTER]",
    "(music)",
    "(applause)",
    "[ Silence ]",
    "[no speech]",
    "[silence]",
    "you",       // whisper hallucination on silence
    "Thank you", // common whisper hallucination
];

/// Available whisper models with their metadata.
const WHISPER_MODELS: &[ModelDef] = &[
    ModelDef {
        id: "tiny",
        name: "Tiny",
        filename: "ggml-tiny.bin",
        size_mb: 75,
    },
    ModelDef {
        id: "base",
        name: "Base",
        filename: "ggml-base.bin",
        size_mb: 142,
    },
    ModelDef {
        id: "small",
        name: "Small",
        filename: "ggml-small.bin",
        size_mb: 466,
    },
    ModelDef {
        id: "medium",
        name: "Medium",
        filename: "ggml-medium.bin",
        size_mb: 1500,
    },
    ModelDef {
        id: "large-v3-turbo",
        name: "Large V3 Turbo",
        filename: "ggml-large-v3-turbo.bin",
        size_mb: 1500,
    },
];

struct ModelDef {
    id: &'static str,
    name: &'static str,
    filename: &'static str,
    size_mb: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhisperModelInfo {
    pub id: String,
    pub name: String,
    pub filename: String,
    pub size_mb: u32,
    pub downloaded: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub progress: u32,
}

/// Resolve the models directory, checking resource dir then dev fallbacks.
fn resolve_models_dir(app_handle: &AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let models = resource_dir.join(MODELS_SUBDIR);
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

/// Resolve a model ID (e.g. "small") to its absolute file path.
pub fn resolve_model_path(app_handle: &AppHandle, model_id: &str) -> PathBuf {
    let models_dir = resolve_models_dir(app_handle);
    let filename = WHISPER_MODELS
        .iter()
        .find(|m| m.id == model_id)
        .map(|m| m.filename)
        .unwrap_or_else(|| {
            // Fallback: treat model_id as filename itself
            Box::leak(format!("ggml-{model_id}.bin").into_boxed_str()) as &str
        });
    models_dir.join(filename)
}

#[tauri::command]
pub async fn list_whisper_models(app_handle: AppHandle) -> Result<Vec<WhisperModelInfo>, String> {
    let models_dir = resolve_models_dir(&app_handle);
    let models = WHISPER_MODELS
        .iter()
        .map(|m| {
            let path = models_dir.join(m.filename);
            WhisperModelInfo {
                id: m.id.to_string(),
                name: m.name.to_string(),
                filename: m.filename.to_string(),
                size_mb: m.size_mb,
                downloaded: path.exists(),
            }
        })
        .collect();
    Ok(models)
}

const HF_ORIGIN: &str = "https://huggingface.co";
const HF_MIRROR: &str = "https://hf-mirror.com";
const HF_REPO_PATH: &str = "/ggerganov/whisper.cpp/resolve/main";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Build the list of download endpoints to try in order.
/// Priority: $HF_ENDPOINT env var > HuggingFace origin > HF mirror.
fn download_endpoints() -> Vec<(String, String)> {
    let mut endpoints = Vec::new();
    if let Ok(endpoint) = std::env::var("HF_ENDPOINT") {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        info!("Using HF_ENDPOINT: {endpoint}");
        endpoints.push((endpoint, HF_REPO_PATH.to_string()));
    }
    endpoints.push((HF_ORIGIN.to_string(), HF_REPO_PATH.to_string()));
    endpoints.push((HF_MIRROR.to_string(), HF_REPO_PATH.to_string()));
    endpoints
}

/// Try to connect to an endpoint and return the response.
async fn try_download(
    client: &reqwest::Client,
    base: &str,
    path_prefix: &str,
    filename: &str,
) -> Result<reqwest::Response, String> {
    let url = format!("{base}{path_prefix}/{filename}");
    info!("Trying download from: {url}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("{base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{base}: HTTP {}", resp.status()));
    }
    Ok(resp)
}

#[tauri::command]
pub async fn download_whisper_model(
    model_id: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let model_def = WHISPER_MODELS
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Unknown model: {model_id}"))?;

    let models_dir = resolve_models_dir(&app_handle);
    std::fs::create_dir_all(&models_dir).map_err(|e| format!("Failed to create models dir: {e}"))?;

    let dest = models_dir.join(model_def.filename);
    if dest.exists() {
        return Ok(());
    }

    // Try each endpoint in order (env override > HF origin > HF mirror)
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let endpoints = download_endpoints();
    let mut response: Option<reqwest::Response> = None;
    let mut last_err = String::new();

    for (base, path_prefix) in &endpoints {
        match try_download(&client, base, path_prefix, model_def.filename).await {
            Ok(resp) => {
                info!("Connected to {base}");
                response = Some(resp);
                break;
            }
            Err(e) => {
                warn!("Endpoint failed: {e}");
                last_err = e;
            }
        }
    }

    let response = response.ok_or_else(|| {
        format!("All download endpoints failed. Last error: {last_err}")
    })?;

    let total_size = response.content_length().unwrap_or(0);

    // Download to system temp dir to avoid triggering Tauri dev watcher on resources/
    let tmp_dest = std::env::temp_dir().join(format!("rtvt-download-{}", model_def.filename));

    let mut file = tokio::fs::File::create(&tmp_dest)
        .await
        .map_err(|e| format!("Failed to create temp file: {e}"))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_progress: u32 = 0;

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += chunk.len() as u64;

        let progress = if total_size > 0 {
            ((downloaded as f64 / total_size as f64) * 100.0) as u32
        } else {
            0
        };

        if progress != last_progress {
            last_progress = progress;
            let _ = app_handle.emit(
                "model-download-progress",
                DownloadProgress {
                    model_id: model_id.clone(),
                    progress,
                },
            );
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {e}"))?;
    drop(file);

    // Move temp file to final destination (copy+remove as fallback for cross-device moves)
    if let Err(_) = tokio::fs::rename(&tmp_dest, &dest).await {
        tokio::fs::copy(&tmp_dest, &dest)
            .await
            .map_err(|e| format!("Failed to copy model file: {e}"))?;
        let _ = tokio::fs::remove_file(&tmp_dest).await;
    }

    info!("Model {} downloaded successfully", model_id);
    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model_id,
            progress: 100,
        },
    );

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct AsrResult {
    pub text: String,
    pub language: String,
}

pub struct AsrEngine {
    model_path: String,
    language: String,
    event_name: String,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AsrEngine {
    /// Create a new ASR engine.
    /// `language`: whisper language code, e.g. "en", "zh", or "auto" for auto-detect.
    pub fn new(model_path: Option<String>, language: Option<String>) -> Self {
        Self {
            model_path: model_path.unwrap_or_else(|| format!("resources/models/ggml-{DEFAULT_MODEL_ID}.bin")),
            language: language.unwrap_or_else(|| "auto".to_string()),
            event_name: "asr-result".to_string(),
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    /// Create a new ASR engine with a custom event name for emitting results.
    pub fn with_event_name(model_path: Option<String>, language: Option<String>, event_name: String) -> Self {
        Self {
            model_path: model_path.unwrap_or_else(|| format!("resources/models/ggml-{DEFAULT_MODEL_ID}.bin")),
            language: language.unwrap_or_else(|| "auto".to_string()),
            event_name,
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    pub fn start(&mut self, receiver: Receiver<Vec<f32>>, app_handle: AppHandle) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            warn!("ASR engine is already running");
            return Ok(());
        }

        let model_path = self.model_path.clone();
        let language = self.language.clone();
        let event_name = self.event_name.clone();
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        let handle = thread::Builder::new()
            .name("asr-whisper".into())
            .spawn(move || {
                if let Err(e) =
                    asr_thread(model_path, language, event_name, receiver, app_handle, running.clone())
                {
                    error!("ASR thread error: {e:#}");
                    running.store(false, Ordering::SeqCst);
                }
            })
            .context("Failed to spawn ASR thread")?;

        self.handle = Some(handle);
        info!("ASR engine started (language={})", self.language);
        Ok(())
    }

    pub fn stop(&mut self) {
        if !self.running.load(Ordering::SeqCst) {
            return;
        }
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        info!("ASR engine stopped");
    }
}

impl Drop for AsrEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

// — WebRTC VAD state machine —

enum VadState {
    Idle,
    Speaking,
    TrailingSilence,
}

struct VadProcessor {
    vad: Vad,
    state: VadState,
    speech_buffer: Vec<f32>,
    ring_buffer: VecDeque<f32>,
    speech_sample_count: usize,
    silence_sample_count: usize,
    frame_buffer: Vec<f32>,
}

fn f32_frame_to_i16(frame: &[f32]) -> Vec<i16> {
    frame
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

impl VadProcessor {
    fn new() -> Self {
        let vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        Self {
            vad,
            state: VadState::Idle,
            speech_buffer: Vec::with_capacity(MAX_SPEECH_SAMPLES),
            ring_buffer: VecDeque::with_capacity(PRE_SPEECH_SAMPLES + VAD_FRAME_SAMPLES),
            speech_sample_count: 0,
            silence_sample_count: 0,
            frame_buffer: Vec::with_capacity(VAD_FRAME_SAMPLES * 2),
        }
    }

    /// Process an incoming audio chunk. Returns a speech segment if one is complete.
    fn process_chunk(&mut self, chunk: &[f32]) -> Option<Vec<f32>> {
        self.frame_buffer.extend_from_slice(chunk);

        let mut result = None;

        while self.frame_buffer.len() >= VAD_FRAME_SAMPLES {
            let frame: Vec<f32> = self.frame_buffer.drain(..VAD_FRAME_SAMPLES).collect();
            let i16_frame = f32_frame_to_i16(&frame);
            let is_speech = self.vad.is_voice_segment(&i16_frame).unwrap_or(false);

            if let Some(segment) = self.process_frame(&frame, is_speech) {
                result = Some(segment);
            }
        }

        result
    }

    fn process_frame(&mut self, frame: &[f32], is_speech: bool) -> Option<Vec<f32>> {
        match self.state {
            VadState::Idle => {
                if is_speech {
                    // Prepend ring_buffer contents for pre-speech context
                    self.speech_buffer.clear();
                    self.speech_buffer.extend(self.ring_buffer.iter());
                    self.speech_buffer.extend_from_slice(frame);
                    self.speech_sample_count = frame.len();
                    self.silence_sample_count = 0;
                    self.ring_buffer.clear();
                    self.state = VadState::Speaking;
                } else {
                    // Update ring buffer (FIFO, keep last PRE_SPEECH_SAMPLES)
                    self.ring_buffer.extend(frame.iter());
                    while self.ring_buffer.len() > PRE_SPEECH_SAMPLES {
                        self.ring_buffer.pop_front();
                    }
                }
                None
            }
            VadState::Speaking => {
                self.speech_buffer.extend_from_slice(frame);
                if is_speech {
                    self.speech_sample_count += frame.len();
                } else {
                    self.silence_sample_count = frame.len();
                    self.state = VadState::TrailingSilence;
                }
                // Check max length
                if self.speech_buffer.len() >= MAX_SPEECH_SAMPLES {
                    return Some(self.emit_and_reset());
                }
                None
            }
            VadState::TrailingSilence => {
                self.speech_buffer.extend_from_slice(frame);

                if is_speech {
                    self.speech_sample_count += frame.len();
                    self.silence_sample_count = 0;
                    self.state = VadState::Speaking;
                } else {
                    self.silence_sample_count += frame.len();
                }

                // Check max length
                if self.speech_buffer.len() >= MAX_SPEECH_SAMPLES {
                    return Some(self.emit_and_reset());
                }

                // Check end-of-speech
                if self.silence_sample_count >= END_OF_SPEECH_SAMPLES {
                    if self.speech_sample_count >= MIN_SPEECH_SAMPLES {
                        return Some(self.emit_and_reset());
                    } else {
                        // Too short — discard (click, transient noise)
                        self.reset();
                        return None;
                    }
                }

                None
            }
        }
    }

    fn emit_and_reset(&mut self) -> Vec<f32> {
        let segment = std::mem::take(&mut self.speech_buffer);
        self.reset();
        segment
    }

    fn reset(&mut self) {
        self.speech_buffer.clear();
        self.ring_buffer.clear();
        self.speech_sample_count = 0;
        self.silence_sample_count = 0;
        self.state = VadState::Idle;
    }

    /// Flush any pending speech when the pipeline is stopping or on timeout.
    fn flush(&mut self) -> Option<Vec<f32>> {
        if self.speech_sample_count >= MIN_SPEECH_SAMPLES && !self.speech_buffer.is_empty() {
            Some(self.emit_and_reset())
        } else {
            self.reset();
            None
        }
    }

    /// Flush if currently in Speaking/TrailingSilence state (for recv timeout).
    fn flush_if_speaking(&mut self) -> Option<Vec<f32>> {
        match self.state {
            VadState::Speaking | VadState::TrailingSilence => self.flush(),
            VadState::Idle => None,
        }
    }
}

fn asr_thread(
    model_path: String,
    language: String,
    event_name: String,
    receiver: Receiver<Vec<f32>>,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
) -> Result<()> {
    info!("Loading whisper model from: {model_path}");

    // Log system capabilities for GPU debugging
    let sys_info = SystemInfo::default();
    let sys_info_str = whisper_rs::print_system_info();
    info!("Whisper system info: {sys_info_str}");
    info!(
        "GPU support compiled: cuda={}, blas={}, avx={}, avx2={}, fma={}, f16c={}",
        sys_info.cuda, sys_info.blas, sys_info.avx, sys_info.avx2, sys_info.fma, sys_info.f16c
    );

    let gpu_available = sys_info.cuda;
    if cfg!(feature = "cuda") && !gpu_available {
        warn!("CUDA feature enabled at compile time but CUDA not available at runtime! Check CUDA toolkit installation.");
    }
    if !cfg!(feature = "cuda") && !cfg!(feature = "vulkan") && !cfg!(feature = "metal") {
        info!("No GPU feature enabled at compile time. Running on CPU only.");
    }

    // Configure context with GPU and flash attention support
    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.use_gpu(gpu_available);
    if gpu_available {
        ctx_params.flash_attn(true);
        ctx_params.gpu_device(0);
        info!("Whisper context params: use_gpu=true, flash_attn=true, gpu_device=0");
    } else {
        info!("Whisper context params: use_gpu=false (no GPU backend available)");
    }

    let ctx = WhisperContext::new_with_params(&model_path, ctx_params)
        .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {e}"))?;

    // Pre-create a reusable state to avoid per-inference allocation overhead
    let mut state = ctx
        .create_state()
        .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {e}"))?;

    // Use fewer CPU threads when GPU handles the heavy compute
    let n_threads = if gpu_available { 2 } else { optimal_thread_count() };
    info!(
        "Whisper model loaded successfully (threads={n_threads}, gpu={})",
        if gpu_available { "yes" } else { "no" }
    );

    // Notify frontend about GPU status
    let gpu_status = if gpu_available { "GPU (CUDA)" } else { "CPU" };
    let _ = app_handle.emit(
        "pipeline-status",
        serde_json::json!({
            "status": "running",
            "message": format!("Whisper loaded on {gpu_status}")
        }),
    );

    let mut vad_proc = VadProcessor::new();

    // Closure to run inference on a VAD segment
    let run_inference = |segment: Vec<f32>,
                             state: &mut whisper_rs::WhisperState,
                             language: &str,
                             n_threads: i32,
                             app_handle: &AppHandle,
                             event_name: &str| {
        let trimmed = trim_silence(&segment, SILENCE_THRESHOLD);
        if trimmed.len() < SAMPLE_RATE / 10 || rms_energy(trimmed) < SILENCE_THRESHOLD {
            return;
        }
        // Whisper requires >= 1s of audio — pad with silence (use 1.1s for margin)
        const MIN_WHISPER_SAMPLES: usize = SAMPLE_RATE * 11 / 10;
        let padded: Vec<f32>;
        let audio_for_inference = if trimmed.len() < MIN_WHISPER_SAMPLES {
            padded = {
                let mut buf = trimmed.to_vec();
                buf.resize(MIN_WHISPER_SAMPLES, 0.0);
                buf
            };
            &padded
        } else {
            trimmed
        };
        let audio_duration_ms = (trimmed.len() as f32 / SAMPLE_RATE as f32) * 1000.0;
        let infer_start = std::time::Instant::now();
        match recognize_with_state(state, audio_for_inference, language, n_threads) {
            Ok(result) => {
                let infer_ms = infer_start.elapsed().as_millis();
                let rtf = infer_ms as f32 / audio_duration_ms;
                info!(
                    "Whisper inference: {infer_ms}ms for {audio_duration_ms:.0}ms audio (RTF={rtf:.2})"
                );
                if !result.text.is_empty() && !is_junk(&result.text) {
                    if let Err(e) = app_handle.emit(event_name, &result) {
                        error!("Failed to emit ASR result: {e}");
                    }
                }
            }
            Err(e) => {
                error!("Recognition error: {e:#}");
            }
        }
    };

    while running.load(Ordering::SeqCst) {
        match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(chunk) => {
                if let Some(segment) = vad_proc.process_chunk(&chunk) {
                    run_inference(segment, &mut state, &language, n_threads, &app_handle, &event_name);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if let Some(segment) = vad_proc.flush_if_speaking() {
                    run_inference(segment, &mut state, &language, n_threads, &app_handle, &event_name);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("Audio channel disconnected, stopping ASR");
                break;
            }
        }
    }

    // Process any remaining audio when pipeline stops
    if let Some(segment) = vad_proc.flush() {
        run_inference(segment, &mut state, &language, n_threads, &app_handle, &event_name);
    }

    Ok(())
}

/// Check if the text is a known whisper junk/hallucination pattern.
fn is_junk(text: &str) -> bool {
    let trimmed = text.trim();
    JUNK_PATTERNS
        .iter()
        .any(|p| trimmed.eq_ignore_ascii_case(p))
}

fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Trim leading and trailing silence from audio to reduce inference time.
/// Returns a slice of the original buffer without the silent edges.
fn trim_silence(audio: &[f32], threshold: f32) -> &[f32] {
    if audio.is_empty() {
        return audio;
    }

    // Use a small window (10ms at 16kHz = 160 samples) for edge detection
    let window = 160;
    let threshold_sq = threshold * threshold;

    // Find first non-silent window
    let start = (0..audio.len())
        .step_by(window)
        .find(|&i| {
            let end = (i + window).min(audio.len());
            let chunk = &audio[i..end];
            let energy: f32 = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            energy >= threshold_sq
        })
        .unwrap_or(0);

    // Find last non-silent window
    let end = (0..audio.len())
        .rev()
        .step_by(window)
        .find(|&i| {
            let chunk_start = i.saturating_sub(window);
            let chunk = &audio[chunk_start..=i.min(audio.len() - 1)];
            let energy: f32 = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            energy >= threshold_sq
        })
        .map(|i| (i + 1).min(audio.len()))
        .unwrap_or(audio.len());

    if start >= end {
        return audio;
    }

    // Add a small pad (50ms) at edges to avoid cutting speech onset/offset
    let pad = SAMPLE_RATE / 20; // 50ms
    let start = start.saturating_sub(pad);
    let end = (end + pad).min(audio.len());

    &audio[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_junk ---

    #[test]
    fn test_is_junk_exact_patterns() {
        assert!(is_junk("[BLANK_AUDIO]"));
        assert!(is_junk("[MUSIC]"));
        assert!(is_junk("[APPLAUSE]"));
        assert!(is_junk("[LAUGHTER]"));
        assert!(is_junk("(music)"));
        assert!(is_junk("(applause)"));
        assert!(is_junk("[ Silence ]"));
        assert!(is_junk("[no speech]"));
        assert!(is_junk("[silence]"));
        assert!(is_junk("you"));
        assert!(is_junk("Thank you"));
    }

    #[test]
    fn test_is_junk_case_insensitive() {
        assert!(is_junk("thank you"));
        assert!(is_junk("THANK YOU"));
        assert!(is_junk("YOU"));
        assert!(is_junk("[blank_audio]"));
    }

    #[test]
    fn test_is_junk_trims_whitespace() {
        assert!(is_junk("  you  "));
        assert!(is_junk("\tThank you\n"));
    }

    #[test]
    fn test_is_junk_partial_match_is_not_junk() {
        assert!(!is_junk("Thank you for watching"));
        assert!(!is_junk("you are welcome"));
        assert!(!is_junk("[BLANK_AUDIO] blah"));
    }

    #[test]
    fn test_is_junk_real_speech_is_not_junk() {
        assert!(!is_junk("Hello, how are you?"));
        assert!(!is_junk("今天天气不错"));
    }

    // --- rms_energy ---

    #[test]
    fn test_rms_energy_empty() {
        assert_eq!(rms_energy(&[]), 0.0);
    }

    #[test]
    fn test_rms_energy_all_zeros() {
        assert_eq!(rms_energy(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn test_rms_energy_all_ones() {
        assert!((rms_energy(&[1.0, 1.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_rms_energy_mixed_signs() {
        // RMS is always non-negative; sign of samples doesn't matter
        assert!((rms_energy(&[-1.0, 1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_rms_energy_half_amplitude() {
        assert!((rms_energy(&[0.5, 0.5]) - 0.5).abs() < 1e-6);
    }

    // --- f32_frame_to_i16 ---

    #[test]
    fn test_f32_to_i16_zero() {
        assert_eq!(f32_frame_to_i16(&[0.0]), vec![0i16]);
    }

    #[test]
    fn test_f32_to_i16_full_scale() {
        assert_eq!(f32_frame_to_i16(&[1.0]), vec![i16::MAX]);
        assert_eq!(f32_frame_to_i16(&[-1.0]), vec![-i16::MAX]);
    }

    #[test]
    fn test_f32_to_i16_clamps_overflow() {
        assert_eq!(f32_frame_to_i16(&[2.0]), vec![i16::MAX]);
        assert_eq!(f32_frame_to_i16(&[-2.0]), vec![-i16::MAX]);
    }

    #[test]
    fn test_f32_to_i16_multiple_samples() {
        let result = f32_frame_to_i16(&[0.0, 1.0, -1.0]);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], 0);
        assert_eq!(result[1], i16::MAX);
        assert_eq!(result[2], -i16::MAX);
    }

    // --- trim_silence ---

    #[test]
    fn test_trim_silence_empty() {
        let audio: Vec<f32> = vec![];
        let result = trim_silence(&audio, 0.01);
        assert!(result.is_empty());
    }

    #[test]
    fn test_trim_silence_all_silence_returns_unchanged() {
        // No non-silent window found: start=0, end=len — returned as-is.
        let audio = vec![0.0f32; 480];
        let result = trim_silence(&audio, 0.01);
        assert_eq!(result.len(), audio.len());
    }

    #[test]
    fn test_trim_silence_all_speech_returns_unchanged() {
        let audio = vec![1.0f32; 800];
        let result = trim_silence(&audio, 0.01);
        assert!(!result.is_empty());
        assert!(result.len() <= audio.len());
    }

    #[test]
    fn test_trim_silence_narrows_long_silence() {
        // Layout (window = 160 samples):
        //   [0..1600]    silence (10 windows)
        //   [1600..1760] loud speech (amplitude 1.0)
        //   [1760..3360] silence (10 windows)
        // After trim: raw range [1600..1760], ±800 sample pad → [800..2560],
        // which is narrower than the full 3360-sample input.
        let mut audio = vec![0.0f32; 3360];
        for s in &mut audio[1600..1760] {
            *s = 1.0;
        }
        let result = trim_silence(&audio, 0.01);
        assert!(result.len() < audio.len(), "should trim some silence");
        // Verify the speech region is contained in the returned slice.
        let start_idx = (result.as_ptr() as usize - audio.as_ptr() as usize)
            / std::mem::size_of::<f32>();
        let end_idx = start_idx + result.len();
        assert!(start_idx <= 1600, "speech onset must be included");
        assert!(end_idx >= 1760, "speech offset must be included");
    }

    // --- optimal_thread_count ---

    #[test]
    fn test_optimal_thread_count_in_range() {
        let t = optimal_thread_count();
        assert!(t >= 2, "should use at least 2 threads");
        assert!(t <= 8, "should use at most 8 threads");
    }
}

/// Run inference using a pre-allocated WhisperState (avoids per-call allocation).
fn recognize_with_state(
    state: &mut whisper_rs::WhisperState,
    audio: &[f32],
    language: &str,
    n_threads: i32,
) -> Result<AsrResult> {
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    // Set language: use explicit language or auto-detect
    if language == "auto" {
        params.set_language(Some("auto"));
    } else {
        params.set_language(Some(language));
    }

    // Set thread count for optimal CPU utilization
    params.set_n_threads(n_threads);

    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_suppress_blank(true);
    // Suppress non-speech tokens to reduce hallucinations
    params.set_suppress_non_speech_tokens(true);

    state
        .full(params, audio)
        .map_err(|e| anyhow::anyhow!("Whisper inference failed: {e}"))?;

    let num_segments = state.full_n_segments().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut text = String::new();
    for i in 0..num_segments {
        if let Ok(seg_text) = state.full_get_segment_text(i) {
            text.push_str(&seg_text);
        }
    }

    let language = state
        .full_lang_id_from_state()
        .ok()
        .and_then(|id| whisper_rs::get_lang_str(id).map(|s| s.to_string()))
        .unwrap_or_else(|| "en".to_string());

    Ok(AsrResult {
        text: text.trim().to_string(),
        language,
    })
}
